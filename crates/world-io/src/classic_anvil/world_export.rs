//! Verified export of a complete Java world folder (M8.10).
//!
//! This module combines two already-verified paths:
//!
//! - [`passthrough_java_world_with_options`] copies every source file byte for byte into a
//!   new folder, validates the source and the copy, and never follows links;
//! - [`write_region_from_overlay_with_policy`] writes a new region file for the dirty chunks
//!   of one region, re-decodes it and verifies every untouched chunk record.
//!
//! The export is assembled in a temporary sibling directory. Only after the staged folder
//! has been reopened, its file manifest compared with the passthrough report, and rebuilt
//! regions re-hashed is the folder renamed to the requested destination. The source world
//! is never written. `session.lock` is the only file deliberately absent from the copy.
//!
//! A single `.mca` is not a world: this module exports `level.dat`, sidecar containers,
//! unknown files and untouched regions together, so the result is a complete save folder.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use minecraft_version::VersionProfile;
use serde::Serialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::passthrough::{
    passthrough_java_world_with_options, PassthroughError, PassthroughOptions, PassthroughReport,
};
use crate::world_codec::world_codec;

use super::overlay::{OverlayError, WorldOverlay};
use super::reader::{region_file_name, AnvilError, JavaWorld, ANVIL_REGION_AXIS_CHUNKS};
use super::region_write::{
    write_region_from_overlay_with_policy, DerivedDataPolicy, RegionWriteError,
};

pub const MAX_EXPORT_REGIONS: usize = 4_096;
const HASH_BUFFER_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldExportOptions {
    /// How to treat derived chunk data in replaced chunks.
    pub derived_data_policy: DerivedDataPolicy,
    /// Limits and chunk validation used by the passthrough copy.
    pub passthrough: PassthroughOptions,
    /// Maximum number of region files that may be rebuilt.
    pub max_regions: usize,
    /// Hash the complete source tree before and after the export.
    ///
    /// Disabling this saves one full read of the source world, but then the report can
    /// only claim the per-file copy checks and the per-region SHA-256 checks, not a
    /// whole-tree comparison.
    pub verify_source_tree: bool,
}

impl Default for WorldExportOptions {
    fn default() -> Self {
        Self {
            derived_data_policy: DerivedDataPolicy::Preserve,
            passthrough: PassthroughOptions::default(),
            max_regions: MAX_EXPORT_REGIONS,
            verify_source_tree: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedRegion {
    pub dimension: String,
    pub region_x: i32,
    pub region_z: i32,
    pub replaced_chunks: usize,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldExportReport {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub files: usize,
    pub bytes: u64,
    pub skipped_files: Vec<String>,
    pub regions: usize,
    pub chunks: usize,
    pub replaced_regions: usize,
    pub replaced_chunks: usize,
    pub exported_regions: Vec<ExportedRegion>,
    pub derived_data_policy: DerivedDataPolicy,
    pub invalidated_fields: &'static [&'static str],
    pub source_tree_sha256_before: Option<String>,
    pub source_tree_sha256_after: Option<String>,
    pub source_tree_verified: bool,
    pub source_unchanged: bool,
    pub copied_files_match_source: bool,
    pub output_revalidated: bool,
}

#[derive(Debug, Error)]
pub enum WorldExportError {
    #[error(transparent)]
    Anvil(#[from] AnvilError),
    #[error(transparent)]
    Overlay(#[from] OverlayError),
    #[error(transparent)]
    Passthrough(#[from] PassthroughError),
    #[error(transparent)]
    RegionWrite(#[from] RegionWriteError),
    #[error("invalid world export request: {0}")]
    InvalidArgument(String),
    #[error("unsafe world export path: {0}")]
    UnsafePath(String),
    #[error("world export destination already exists: {0}")]
    AlreadyExists(String),
    #[error("world export is not verified for Minecraft {0}")]
    UnsupportedVersion(String),
    #[error("world export verification failed: {0}")]
    VerificationFailed(String),
    #[error("world export limit exceeded: {0}")]
    LimitExceeded(String),
    #[error("world export I/O failed: {0}")]
    Io(String),
}

impl WorldExportError {
    pub fn code(&self) -> &'static str {
        match self {
            WorldExportError::Anvil(error) => error.code(),
            WorldExportError::Overlay(error) => error.code(),
            WorldExportError::Passthrough(error) => error.code(),
            WorldExportError::RegionWrite(error) => error.code(),
            WorldExportError::InvalidArgument(_) => "invalid_argument",
            WorldExportError::UnsafePath(_) => "unsafe_path",
            WorldExportError::AlreadyExists(_) => "conflict",
            WorldExportError::UnsupportedVersion(_) => "unsupported_version",
            WorldExportError::VerificationFailed(_) => "verification_failed",
            WorldExportError::LimitExceeded(_) => "limit_exceeded",
            WorldExportError::Io(_) => "io_error",
        }
    }
}

/// Exports a new complete world folder with every dirty overlay chunk applied.
///
/// The source world is opened read-only by the caller's [`WorldOverlay`]. The
/// destination must not exist. The result is published only after the staged folder
/// has been reopened and validated.
pub fn export_java_world(
    overlay: &WorldOverlay,
    destination: impl AsRef<Path>,
) -> Result<WorldExportReport, WorldExportError> {
    export_java_world_with_options(overlay, destination, WorldExportOptions::default())
}

pub fn export_java_world_with_options(
    overlay: &WorldOverlay,
    destination: impl AsRef<Path>,
    options: WorldExportOptions,
) -> Result<WorldExportReport, WorldExportError> {
    world_codec().export_world(overlay, destination.as_ref(), options)
}

pub(super) fn export_world(
    overlay: &WorldOverlay,
    destination: &Path,
    options: WorldExportOptions,
) -> Result<WorldExportReport, WorldExportError> {
    let world = overlay.world();
    ensure_export_supported(&world.profile)?;
    if world.level.data_version != world.profile.data_version {
        return Err(WorldExportError::RegionWrite(
            RegionWriteError::UnsupportedDataVersion {
                found: world.level.data_version,
                supported: vec![world.profile.data_version],
            },
        ));
    }

    let source = fs::canonicalize(&world.root)
        .map_err(|error| io_error(format!("resolving {}", world.root.display()), error))?;
    let destination = resolve_destination(&source, destination)?;
    let parent = destination
        .parent()
        .ok_or_else(|| WorldExportError::InvalidArgument(destination.display().to_string()))?;

    let dirty_regions = dirty_regions(overlay, options.max_regions)?;
    let replaced_chunks = dirty_regions.values().sum::<usize>();
    let before_tree = if options.verify_source_tree {
        Some(source_tree_sha256(&source, &options.passthrough)?)
    } else {
        None
    };

    let staging = StagingExport::create(parent)?;
    let staged_world = staging.path().to_path_buf();
    let passthrough =
        passthrough_java_world_with_options(&source, &staged_world, options.passthrough)?;

    let region_root = staging.root.join("region-writes");
    fs::create_dir_all(&region_root)
        .map_err(|error| io_error(format!("creating {}", region_root.display()), error))?;

    let mut exported_regions = Vec::with_capacity(dirty_regions.len());
    let mut replaced_paths = BTreeMap::new();
    for (index, ((dimension_id, region_x, region_z), chunks)) in dirty_regions.iter().enumerate() {
        let dimension = world.dimension(dimension_id).ok_or_else(|| {
            WorldExportError::InvalidArgument(format!("unknown dimension: {dimension_id}"))
        })?;
        let write_folder = region_root.join(format!("{index:04}"));
        fs::create_dir_all(&write_folder)
            .map_err(|error| io_error(format!("creating {}", write_folder.display()), error))?;
        let write_path = write_folder.join(region_file_name(*region_x, *region_z));
        let write_report = write_region_from_overlay_with_policy(
            overlay,
            dimension_id,
            *region_x,
            *region_z,
            &write_path,
            options.derived_data_policy,
        )?;
        if write_report.replaced != *chunks {
            return Err(WorldExportError::VerificationFailed(format!(
                "region ({region_x}, {region_z}) in {dimension_id} replaced {} chunks, expected {chunks}",
                write_report.replaced
            )));
        }
        let sha256 = hash_file(&write_path)?;

        let canonical_region_folder =
            fs::canonicalize(&dimension.region_folder).map_err(|error| {
                io_error(
                    format!("resolving {}", dimension.region_folder.display()),
                    error,
                )
            })?;
        let relative_folder = canonical_region_folder.strip_prefix(&source).map_err(|_| {
            WorldExportError::UnsafePath(format!(
                "region folder {} is outside the source world",
                dimension.region_folder.display()
            ))
        })?;
        let staged_region_folder = staged_world.join(relative_folder);
        let staged_target = staged_region_folder.join(region_file_name(*region_x, *region_z));
        if !staged_target.starts_with(&staged_world) {
            return Err(WorldExportError::UnsafePath(
                staged_target.display().to_string(),
            ));
        }
        if !staged_target.is_file() {
            return Err(WorldExportError::VerificationFailed(format!(
                "staged region {} is missing",
                staged_target.display()
            )));
        }
        fs::remove_file(&staged_target)
            .map_err(|error| io_error(format!("replacing {}", staged_target.display()), error))?;
        fs::rename(&write_path, &staged_target)
            .map_err(|error| io_error(format!("publishing {}", staged_target.display()), error))?;
        if hash_file(&staged_target)? != sha256 {
            return Err(WorldExportError::VerificationFailed(format!(
                "staged region {} changed while being moved",
                staged_target.display()
            )));
        }
        replaced_paths.insert(
            staged_target
                .strip_prefix(&staged_world)
                .expect("staged region is inside the staged world")
                .to_path_buf(),
            sha256.clone(),
        );
        exported_regions.push(ExportedRegion {
            dimension: dimension_id.clone(),
            region_x: *region_x,
            region_z: *region_z,
            replaced_chunks: write_report.replaced,
            bytes: write_report.bytes,
            sha256,
        });
    }

    validate_staged_manifest(&staged_world, &passthrough, &replaced_paths)?;
    validate_staged_world(&staged_world, &passthrough)?;

    let after_tree = if options.verify_source_tree {
        Some(source_tree_sha256(&source, &options.passthrough)?)
    } else {
        None
    };
    let source_unchanged = match (&before_tree, &after_tree) {
        (Some(before), Some(after)) => before == after,
        _ => true,
    };
    if !source_unchanged {
        return Err(WorldExportError::VerificationFailed(
            "the source world changed while it was being exported".to_owned(),
        ));
    }

    let report = WorldExportReport {
        source,
        destination: destination.clone(),
        files: passthrough.files.len(),
        bytes: passthrough.bytes(),
        skipped_files: passthrough
            .skipped
            .iter()
            .map(|skipped| skipped.relative_path.display().to_string())
            .collect(),
        regions: passthrough.regions,
        chunks: passthrough.chunks,
        replaced_regions: exported_regions.len(),
        replaced_chunks,
        exported_regions,
        derived_data_policy: options.derived_data_policy,
        invalidated_fields: options.derived_data_policy.invalidated_fields(),
        source_tree_sha256_before: before_tree,
        source_tree_sha256_after: after_tree,
        source_tree_verified: options.verify_source_tree,
        source_unchanged,
        copied_files_match_source: true,
        output_revalidated: true,
    };
    staging.publish(&destination)?;
    Ok(report)
}

fn ensure_export_supported(profile: &VersionProfile) -> Result<(), WorldExportError> {
    if profile.capabilities.export {
        return Ok(());
    }
    Err(WorldExportError::UnsupportedVersion(
        profile.minecraft_version.to_string(),
    ))
}

fn dirty_regions(
    overlay: &WorldOverlay,
    max_regions: usize,
) -> Result<BTreeMap<(String, i32, i32), usize>, WorldExportError> {
    let mut regions = BTreeMap::new();
    for key in overlay.dirty_chunks() {
        let region_x = key.x.div_euclid(ANVIL_REGION_AXIS_CHUNKS);
        let region_z = key.z.div_euclid(ANVIL_REGION_AXIS_CHUNKS);
        *regions
            .entry((key.dimension.clone(), region_x, region_z))
            .or_insert(0usize) += 1;
    }
    if regions.len() > max_regions {
        return Err(WorldExportError::LimitExceeded(format!(
            "{} regions need rebuilding, limit is {max_regions}",
            regions.len()
        )));
    }
    Ok(regions)
}

fn resolve_destination(source: &Path, destination: &Path) -> Result<PathBuf, WorldExportError> {
    let name = destination
        .file_name()
        .ok_or_else(|| WorldExportError::InvalidArgument(destination.display().to_string()))?;
    let parent = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent = fs::canonicalize(parent)
        .map_err(|error| io_error(format!("resolving {}", parent.display()), error))?;
    let parent_metadata = fs::symlink_metadata(&parent)
        .map_err(|error| io_error(format!("reading {}", parent.display()), error))?;
    if !parent_metadata.is_dir() {
        return Err(WorldExportError::InvalidArgument(format!(
            "destination parent is not a directory: {}",
            parent.display()
        )));
    }
    let resolved = parent.join(name);
    match fs::symlink_metadata(&resolved) {
        Ok(_) => {
            return Err(WorldExportError::AlreadyExists(
                resolved.display().to_string(),
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(io_error(format!("reading {}", resolved.display()), error)),
    }
    if resolved == source || resolved.starts_with(source) {
        return Err(WorldExportError::UnsafePath(
            "destination must not be inside the source world".to_owned(),
        ));
    }
    if source.starts_with(&resolved) {
        return Err(WorldExportError::UnsafePath(
            "destination must not be an ancestor of the source world".to_owned(),
        ));
    }
    Ok(resolved)
}

fn validate_staged_manifest(
    staged_world: &Path,
    passthrough: &PassthroughReport,
    replaced: &BTreeMap<PathBuf, String>,
) -> Result<(), WorldExportError> {
    let expected = passthrough
        .files
        .iter()
        .map(|file| (file.relative_path.clone(), file.sha256.clone()))
        .collect::<BTreeMap<_, _>>();
    let actual = collect_files(staged_world)?;
    if actual.len() != expected.len() {
        return Err(WorldExportError::VerificationFailed(format!(
            "staged world has {} files, expected {}",
            actual.len(),
            expected.len()
        )));
    }
    for relative in &actual {
        let Some(expected_sha256) = expected.get(relative) else {
            return Err(WorldExportError::VerificationFailed(format!(
                "staged world contains an unexpected file: {}",
                relative.display()
            )));
        };
        let actual_sha256 = hash_file(&staged_world.join(relative))?;
        let wanted = replaced.get(relative).unwrap_or(expected_sha256);
        if &actual_sha256 != wanted {
            return Err(WorldExportError::VerificationFailed(format!(
                "staged file {} does not match its expected SHA-256",
                relative.display()
            )));
        }
    }
    for relative in expected.keys() {
        if actual.binary_search(relative).is_err() {
            return Err(WorldExportError::VerificationFailed(format!(
                "staged world is missing {}",
                relative.display()
            )));
        }
    }
    Ok(())
}

fn validate_staged_world(
    staged_world: &Path,
    passthrough: &PassthroughReport,
) -> Result<(), WorldExportError> {
    let world = JavaWorld::open(staged_world)?;
    if world.level.data_version != world.profile.data_version {
        return Err(WorldExportError::VerificationFailed(format!(
            "staged world has DataVersion {}, expected {}",
            world.level.data_version, world.profile.data_version
        )));
    }
    let mut regions = 0usize;
    let mut chunks = 0usize;
    for dimension in &world.dimensions {
        for (region_x, region_z) in world.region_coordinates(&dimension.id)? {
            let region = world.read_region(&dimension.id, region_x, region_z)?;
            regions += 1;
            let summaries = region.summarize_all()?;
            if summaries.len() != region.entries().len() {
                return Err(WorldExportError::VerificationFailed(format!(
                    "staged region ({region_x}, {region_z}) in {} decoded {} of {} chunks",
                    dimension.id,
                    summaries.len(),
                    region.entries().len()
                )));
            }
            chunks += summaries.len();
        }
    }
    if regions != passthrough.regions || chunks != passthrough.chunks {
        return Err(WorldExportError::VerificationFailed(format!(
            "staged world has {regions} regions / {chunks} chunks, expected {} / {}",
            passthrough.regions, passthrough.chunks
        )));
    }
    Ok(())
}

fn source_tree_sha256(
    root: &Path,
    options: &PassthroughOptions,
) -> Result<String, WorldExportError> {
    let files = collect_files_with_limits(root, options)?;
    let mut hasher = Sha256::new();
    let mut total = 0u64;
    hasher.update((files.len() as u64).to_be_bytes());
    for relative in files {
        let path = root.join(&relative);
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| io_error(format!("reading {}", path.display()), error))?;
        let bytes = metadata.len();
        total = total.checked_add(bytes).ok_or_else(|| {
            WorldExportError::LimitExceeded("source byte count overflow".to_owned())
        })?;
        if total > options.max_total_bytes {
            return Err(WorldExportError::LimitExceeded(format!(
                "source tree is {total} bytes, limit is {}",
                options.max_total_bytes
            )));
        }
        let normalized = relative.to_string_lossy().replace('\\', "/");
        hasher.update((normalized.len() as u64).to_be_bytes());
        hasher.update(normalized.as_bytes());
        hasher.update(bytes.to_be_bytes());
        hash_file_into(&mut hasher, &path)?;
    }
    Ok(hex(hasher.finalize()))
}

fn collect_files(root: &Path) -> Result<Vec<PathBuf>, WorldExportError> {
    collect_files_with_limits(root, &PassthroughOptions::default())
}

fn collect_files_with_limits(
    root: &Path,
    options: &PassthroughOptions,
) -> Result<Vec<PathBuf>, WorldExportError> {
    let mut files = Vec::new();
    let mut total = 0u64;
    let mut stack = vec![PathBuf::new()];
    while let Some(relative) = stack.pop() {
        let directory = root.join(&relative);
        for entry in fs::read_dir(&directory)
            .map_err(|error| io_error(format!("reading {}", directory.display()), error))?
        {
            let entry = entry
                .map_err(|error| io_error(format!("reading {}", directory.display()), error))?;
            let child = relative.join(entry.file_name());
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path)
                .map_err(|error| io_error(format!("reading {}", path.display()), error))?;
            if metadata.file_type().is_symlink() {
                return Err(WorldExportError::UnsafePath(path.display().to_string()));
            }
            if metadata.is_dir() {
                stack.push(child);
                continue;
            }
            if !metadata.is_file() {
                return Err(WorldExportError::UnsafePath(path.display().to_string()));
            }
            if files.len() >= options.max_files {
                return Err(WorldExportError::LimitExceeded(format!(
                    "source tree has more than {} files",
                    options.max_files
                )));
            }
            if metadata.len() > options.max_file_bytes {
                return Err(WorldExportError::LimitExceeded(format!(
                    "{} is {} bytes, per-file limit is {}",
                    path.display(),
                    metadata.len(),
                    options.max_file_bytes
                )));
            }
            total = total.checked_add(metadata.len()).ok_or_else(|| {
                WorldExportError::LimitExceeded("source byte count overflow".to_owned())
            })?;
            if total > options.max_total_bytes {
                return Err(WorldExportError::LimitExceeded(format!(
                    "source tree exceeds {} bytes",
                    options.max_total_bytes
                )));
            }
            files.push(child);
        }
    }
    files.sort();
    Ok(files)
}

fn hash_file(path: &Path) -> Result<String, WorldExportError> {
    let mut hasher = Sha256::new();
    hash_file_into(&mut hasher, path)?;
    Ok(hex(hasher.finalize()))
}

fn hash_file_into(hasher: &mut Sha256, path: &Path) -> Result<(), WorldExportError> {
    let mut file =
        File::open(path).map_err(|error| io_error(format!("opening {}", path.display()), error))?;
    let mut buffer = [0u8; HASH_BUFFER_BYTES];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| io_error(format!("hashing {}", path.display()), error))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(())
}

fn hex(bytes: impl AsRef<[u8]>) -> String {
    let bytes = bytes.as_ref();
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn io_error(context: impl std::fmt::Display, error: std::io::Error) -> WorldExportError {
    WorldExportError::Io(format!("{context}: {error}"))
}

struct StagingExport {
    root: PathBuf,
    staged_world: PathBuf,
    published: bool,
}

impl StagingExport {
    fn create(parent: &Path) -> Result<Self, WorldExportError> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        for attempt in 0..32u32 {
            let root = parent.join(format!(
                ".furybuilder-world-export-{}-{nanos}-{attempt}",
                std::process::id()
            ));
            match fs::create_dir(&root) {
                Ok(()) => {
                    let staged_world = root.join("world");
                    return Ok(Self {
                        root,
                        staged_world,
                        published: false,
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(io_error(format!("creating {}", root.display()), error));
                }
            }
        }
        Err(WorldExportError::Io(
            "could not create a unique export staging directory".to_owned(),
        ))
    }

    fn path(&self) -> &Path {
        &self.staged_world
    }

    fn publish(mut self, destination: &Path) -> Result<(), WorldExportError> {
        match fs::symlink_metadata(destination) {
            Ok(_) => {
                return Err(WorldExportError::AlreadyExists(
                    destination.display().to_string(),
                ))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(io_error(
                    format!("reading {}", destination.display()),
                    error,
                ))
            }
        }
        fs::rename(&self.staged_world, destination)
            .map_err(|error| io_error(format!("publishing {}", destination.display()), error))?;
        self.published = true;
        let _ = fs::remove_dir_all(&self.root);
        Ok(())
    }
}

impl Drop for StagingExport {
    fn drop(&mut self) {
        if !self.published {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_is_gated_by_the_profile_capability() {
        let mut profile = minecraft_version::default_profile();
        profile.capabilities.export = false;
        let error = ensure_export_supported(&profile).expect_err("export must be gated");
        assert_eq!(error.code(), "unsupported_version");

        profile.capabilities.export = true;
        assert!(ensure_export_supported(&profile).is_ok());
    }

    #[test]
    fn tree_collection_is_sorted_and_bounded() {
        let root = std::env::temp_dir().join(format!(
            "furybuilder-export-unit-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0)
        ));
        fs::create_dir_all(root.join("b")).expect("create nested directory");
        fs::write(root.join("z.txt"), b"z").expect("write z");
        fs::write(root.join("b").join("a.txt"), b"a").expect("write a");
        let files = collect_files(&root).expect("collect files");
        assert_eq!(
            files,
            [PathBuf::from("b").join("a.txt"), PathBuf::from("z.txt")]
        );
        let _ = fs::remove_dir_all(root);
    }
}
