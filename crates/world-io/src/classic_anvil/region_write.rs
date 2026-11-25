//! Verified region writing for the single supported Java world combination.
//!
//! M8.7 does not expose a general-purpose Anvil writer. It only writes a **new** region
//! file when every one of these conditions already holds:
//!
//! - the world belongs to a verified classic profile;
//! - the dimension is one of the three standard classic dimensions;
//! - the only changed chunk fields are `sections[].block_states` and
//!   `sections[].biomes`;
//! - every source chunk decodes with the same strict reader used for import;
//! - the edited chunk has `Status = minecraft:full`;
//! - the destination file does not exist and is not the source file.
//!
//! Untouched chunk records are copied byte for byte, including their timestamps. A new
//! region container is built in a temporary sibling directory, reopened and verified
//! against the source, and only then renamed into place. The source world is never
//! written.
//!
//! What this module does **not** claim: block entities, entities, ticks, heightmaps,
//! lighting, POI and other derived metadata are preserved as raw data but not recomputed.
//! That limitation is tracked separately in M8.8/M8.9 and is reported in
//! [`RegionWriteReport`].

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use flate2::write::ZlibEncoder;
use flate2::Compression;
use minecraft_version::VersionProfile;
use serde::Serialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::nbt::{self, NbtFile, NbtValue, MAX_NBT_BYTES};
use crate::version_support;
use crate::world_codec::world_codec;

use super::format::{ClassicChunkFormat, DATA_VERSION};
use super::overlay::{verify_preserved_fields, OverlayError, WorldOverlay};
use super::reader::{
    region_file_name, AnvilError, ChunkCompression, JavaWorld, RegionChunk, RegionEntry,
    RegionFile, ANVIL_HEADER_BYTES, ANVIL_REGION_AXIS_CHUNKS, ANVIL_SECTOR_BYTES,
    MAX_REGION_FILE_BYTES,
};

pub const VERIFIED_WRITE_LAYOUT: &str = "classic_anvil";
/// The complete set of chunk fields this writer fills from the overlay.
///
/// `DerivedDataPolicy::VanillaRebuild` may additionally invalidate the fields listed
/// by [`DerivedDataPolicy::invalidated_fields`]; it never writes block data into them.
pub const VERIFIED_WRITE_FIELDS: &[&str] = &["sections[].block_states", "sections[].biomes"];

/// What to do with derived fields that a block/biome write cannot recalculate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum DerivedDataPolicy {
    /// Copy every non-edited field unchanged. This is the backward-compatible default.
    #[default]
    Preserve,
    /// Remove or invalidate heightmaps and light so the vanilla engine rebuilds them on load.
    ///
    /// This does not recompute anything in Rust. It is an explicit hand-off to the
    /// target Minecraft engine and is only valid for a full chunk of the pinned version.
    VanillaRebuild,
}

impl DerivedDataPolicy {
    pub fn action(self) -> &'static str {
        match self {
            DerivedDataPolicy::Preserve => "preserved",
            DerivedDataPolicy::VanillaRebuild => "invalidated_for_vanilla_rebuild",
        }
    }

    pub fn invalidated_fields(self) -> &'static [&'static str] {
        match self {
            DerivedDataPolicy::Preserve => &[],
            DerivedDataPolicy::VanillaRebuild => &[
                "Heightmaps",
                "isLightOn",
                "PostProcessing",
                "sections[].SkyLight",
                "sections[].BlockLight",
            ],
        }
    }
}

/// Published support claim for the write path. It is deliberately small and explicit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifiedWriteSupport {
    pub minecraft_version: String,
    pub data_version: i32,
    pub layout: String,
    pub fields: &'static [&'static str],
    pub derived_data_policies: &'static [&'static str],
    pub recomputes_derived_metadata: bool,
}

impl VerifiedWriteSupport {
    pub fn for_profile(profile: &VersionProfile) -> Self {
        Self {
            minecraft_version: profile.minecraft_version.to_string(),
            data_version: profile.data_version,
            layout: VERIFIED_WRITE_LAYOUT.to_owned(),
            fields: VERIFIED_WRITE_FIELDS,
            derived_data_policies: &["preserve", "vanillaRebuild"],
            recomputes_derived_metadata: false,
        }
    }
}

/// Support claim for the default profile; callers writing an existing world use
/// [`VerifiedWriteSupport::for_profile`] with that world's profile.
pub fn verified_write_support() -> VerifiedWriteSupport {
    VerifiedWriteSupport::for_profile(&version_support::default_version_profile())
}

/// One chunk to replace. The NBT is re-verified against the source before it is written.
#[derive(Debug, Clone, PartialEq)]
pub struct VerifiedChunkReplacement {
    pub x: i32,
    pub z: i32,
    pub nbt: NbtFile,
}

impl VerifiedChunkReplacement {
    pub fn new(x: i32, z: i32, nbt: NbtFile) -> Self {
        Self { x, z, nbt }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegionWriteReport {
    pub support: VerifiedWriteSupport,
    pub dimension: String,
    pub region_x: i32,
    pub region_z: i32,
    pub destination: String,
    pub chunks: usize,
    pub replaced: usize,
    pub preserved: usize,
    pub bytes: u64,
    pub source_region_sha256: String,
    pub source_unchanged: bool,
    pub untouched_records_byte_identical: bool,
    pub output_redecoded: bool,
    pub derived_data_policy: DerivedDataPolicy,
    pub derived_data_action: &'static str,
    pub invalidated_fields: &'static [&'static str],
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RegionWriteError {
    #[error(transparent)]
    Anvil(#[from] AnvilError),
    #[error(transparent)]
    Overlay(#[from] OverlayError),
    #[error("unknown dimension: {0}")]
    UnknownDimension(String),
    #[error("region write combination is not supported: {0}")]
    UnsupportedCombination(String),
    #[error("region DataVersion {found} is not supported; expected {supported:?}")]
    UnsupportedDataVersion { found: i32, supported: Vec<i32> },
    #[error("destination already exists: {0}")]
    AlreadyExists(String),
    #[error("unsafe region write path: {0}")]
    UnsafePath(String),
    #[error("invalid region write request: {0}")]
    InvalidArgument(String),
    #[error("region write verification failed: {0}")]
    VerificationFailed(String),
    #[error("region write limit exceeded: {0}")]
    LimitExceeded(String),
    #[error("region write I/O failed: {0}")]
    Io(String),
}

impl RegionWriteError {
    pub fn code(&self) -> &'static str {
        match self {
            RegionWriteError::Anvil(error) => error.code(),
            RegionWriteError::Overlay(error) => error.code(),
            RegionWriteError::UnknownDimension(_) | RegionWriteError::InvalidArgument(_) => {
                "invalid_argument"
            }
            RegionWriteError::UnsupportedCombination(_) => "unsupported_format",
            RegionWriteError::UnsupportedDataVersion { .. } => "unsupported_version",
            RegionWriteError::AlreadyExists(_) => "conflict",
            RegionWriteError::UnsafePath(_) => "unsafe_path",
            RegionWriteError::VerificationFailed(_) => "verification_failed",
            RegionWriteError::LimitExceeded(_) => "limit_exceeded",
            RegionWriteError::Io(_) => "io_error",
        }
    }
}

/// Writes a new region file from verified chunk replacements.
///
/// `replacements` must contain at most one entry per chunk. Every replacement is checked
/// against the source chunk and the strict chunk decoder before anything is written.
/// Unlisted chunks are copied through with their exact compressed records.
pub fn write_verified_region(
    world: &JavaWorld,
    dimension_id: &str,
    region_x: i32,
    region_z: i32,
    replacements: &[VerifiedChunkReplacement],
    destination: impl AsRef<Path>,
) -> Result<RegionWriteReport, RegionWriteError> {
    write_verified_region_with_policy(
        world,
        dimension_id,
        region_x,
        region_z,
        replacements,
        destination,
        DerivedDataPolicy::Preserve,
    )
}

/// Writes a new region file with an explicit derived-data policy.
///
/// `DerivedDataPolicy::Preserve` is the M8.7 behaviour. `VanillaRebuild` invalidates
/// heightmaps and light only for replaced chunks and leaves every other field checked
/// against an identically invalidated source clone.
pub fn write_verified_region_with_policy(
    world: &JavaWorld,
    dimension_id: &str,
    region_x: i32,
    region_z: i32,
    replacements: &[VerifiedChunkReplacement],
    destination: impl AsRef<Path>,
    policy: DerivedDataPolicy,
) -> Result<RegionWriteReport, RegionWriteError> {
    world_codec().write_region(
        world,
        dimension_id,
        (region_x, region_z),
        replacements,
        destination.as_ref(),
        policy,
    )
}

pub(super) fn write_region(
    world: &JavaWorld,
    dimension_id: &str,
    region_x: i32,
    region_z: i32,
    replacements: &[VerifiedChunkReplacement],
    destination: &Path,
    policy: DerivedDataPolicy,
) -> Result<RegionWriteReport, RegionWriteError> {
    world.level.ensure_supported()?;
    if world.level.data_version != world.profile.data_version {
        return Err(RegionWriteError::UnsupportedDataVersion {
            found: world.level.data_version,
            supported: vec![world.profile.data_version],
        });
    }
    let format = ClassicChunkFormat::for_profile(&world.profile)?;
    let dimension = world
        .dimension(dimension_id)
        .ok_or_else(|| RegionWriteError::UnknownDimension(dimension_id.to_owned()))?;
    if dimension
        .region_folder
        .file_name()
        .and_then(|name| name.to_str())
        != Some("region")
    {
        return Err(RegionWriteError::UnsupportedCombination(
            "only classic region/ folders can be written".to_owned(),
        ));
    }
    let region = world.read_region(dimension_id, region_x, region_z)?;
    let source_metadata = fs::symlink_metadata(region.path())
        .map_err(|error| io_error(format!("reading {}", region.path().display()), error))?;
    if source_metadata.file_type().is_symlink() {
        return Err(RegionWriteError::UnsafePath(
            region.path().display().to_string(),
        ));
    }
    validate_destination(destination, region.path(), region_x, region_z)?;
    let source_region_sha256 = sha256_file(region.path())?;

    let mut pending = BTreeMap::new();
    for replacement in replacements {
        if replacement.x.div_euclid(ANVIL_REGION_AXIS_CHUNKS) != region_x
            || replacement.z.div_euclid(ANVIL_REGION_AXIS_CHUNKS) != region_z
        {
            return Err(RegionWriteError::InvalidArgument(format!(
                "chunk ({}, {}) does not belong to region ({region_x}, {region_z})",
                replacement.x, replacement.z
            )));
        }
        if pending
            .insert((replacement.x, replacement.z), replacement)
            .is_some()
        {
            return Err(RegionWriteError::InvalidArgument(format!(
                "duplicate replacement for chunk ({}, {})",
                replacement.x, replacement.z
            )));
        }
    }

    let source_entries = region.entries().to_vec();
    let source_indices = source_entries
        .iter()
        .map(|entry| entry.local_index)
        .collect::<BTreeSet<_>>();
    if source_indices.len() != source_entries.len() {
        return Err(RegionWriteError::InvalidArgument(
            "source region has duplicate chunk table entries".to_owned(),
        ));
    }
    for (x, z) in pending.keys() {
        if !source_entries
            .iter()
            .any(|entry| entry.x == *x && entry.z == *z)
        {
            return Err(AnvilError::ChunkNotPresent { x: *x, z: *z }.into());
        }
    }

    let mut verified = BTreeMap::new();
    for entry in &source_entries {
        let source = region.read_chunk(entry.x, entry.z)?;
        if let Some(replacement) = pending.get(&(entry.x, entry.z)) {
            if source.summary.status.as_deref() != Some("minecraft:full") {
                return Err(RegionWriteError::UnsupportedCombination(format!(
                    "chunk ({}, {}) has status {:?}; editing requires a fully generated chunk",
                    entry.x, entry.z, source.summary.status
                )));
            }
            let mut nbt = replacement.nbt.clone();
            if policy == DerivedDataPolicy::VanillaRebuild {
                invalidate_derived_fields(&mut nbt, format)?;
            }
            verify_replacement(world, dimension_id, &source, &nbt, policy, format)?;
            verified.insert((entry.x, entry.z), nbt);
        }
    }

    let staging = StagingRegion::new(
        destination
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new(".")),
        region_x,
        region_z,
    )?;
    let bytes_written = build_region_file(&source_entries, &region, &verified, staging.path())?;
    verify_written_region(
        world,
        dimension_id,
        &region,
        staging.path(),
        &verified,
        &source_entries,
        policy,
    )?;
    if sha256_file(region.path())? != source_region_sha256 {
        return Err(RegionWriteError::VerificationFailed(
            "the source region changed while it was being written".to_owned(),
        ));
    }
    staging.publish(destination)?;

    Ok(RegionWriteReport {
        support: VerifiedWriteSupport::for_profile(&world.profile),
        dimension: dimension_id.to_owned(),
        region_x,
        region_z,
        destination: destination.display().to_string(),
        chunks: source_entries.len(),
        replaced: verified.len(),
        preserved: source_entries.len() - verified.len(),
        bytes: bytes_written,
        source_region_sha256,
        source_unchanged: true,
        untouched_records_byte_identical: true,
        output_redecoded: true,
        derived_data_policy: policy,
        derived_data_action: policy.action(),
        invalidated_fields: policy.invalidated_fields(),
    })
}

/// Writes one region using the overlay's dirty chunks for that region.
pub fn write_region_from_overlay(
    overlay: &WorldOverlay,
    dimension_id: &str,
    region_x: i32,
    region_z: i32,
    destination: impl AsRef<Path>,
) -> Result<RegionWriteReport, RegionWriteError> {
    write_region_from_overlay_with_policy(
        overlay,
        dimension_id,
        region_x,
        region_z,
        destination,
        DerivedDataPolicy::Preserve,
    )
}

/// Writes one region from the overlay with an explicit derived-data policy.
pub fn write_region_from_overlay_with_policy(
    overlay: &WorldOverlay,
    dimension_id: &str,
    region_x: i32,
    region_z: i32,
    destination: impl AsRef<Path>,
    policy: DerivedDataPolicy,
) -> Result<RegionWriteReport, RegionWriteError> {
    let mut replacements = Vec::new();
    let mut dirty = 0usize;
    for key in overlay.dirty_chunks() {
        if key.dimension != dimension_id
            || key.x.div_euclid(ANVIL_REGION_AXIS_CHUNKS) != region_x
            || key.z.div_euclid(ANVIL_REGION_AXIS_CHUNKS) != region_z
        {
            continue;
        }
        dirty += 1;
        match overlay.materialize_chunk_nbt(dimension_id, key.x, key.z)? {
            Some(nbt) => replacements.push(VerifiedChunkReplacement::new(key.x, key.z, nbt)),
            None => {
                return Err(RegionWriteError::VerificationFailed(format!(
                    "dirty chunk ({}, {}) produced no materialized NBT",
                    key.x, key.z
                )))
            }
        }
    }
    if dirty != replacements.len() {
        return Err(RegionWriteError::VerificationFailed(
            "not every dirty chunk was materialized".to_owned(),
        ));
    }
    write_verified_region_with_policy(
        overlay.world(),
        dimension_id,
        region_x,
        region_z,
        &replacements,
        destination,
        policy,
    )
}

fn source_with_policy(
    source: &RegionChunk,
    policy: DerivedDataPolicy,
    format: &ClassicChunkFormat,
) -> Result<NbtFile, RegionWriteError> {
    let mut nbt = source.nbt.clone();
    if policy == DerivedDataPolicy::VanillaRebuild {
        invalidate_derived_fields(&mut nbt, format)?;
    }
    Ok(nbt)
}

/// Invalidates derived fields so the vanilla engine rebuilds them on the next load.
///
/// This is an explicit hand-off, not a Rust-side recomputation. It is limited to a
/// fully generated modern chunk: `isLightOn = 0` marks the serialized light as not
/// trustworthy, missing `Heightmaps` are re-primed from section data, and queued
/// `PostProcessing` positions refer to the pre-edit geometry.
fn invalidate_derived_fields(
    nbt: &mut NbtFile,
    format: &ClassicChunkFormat,
) -> Result<(), RegionWriteError> {
    let root = match &mut nbt.root {
        NbtValue::Compound(root) => root,
        _ => {
            return Err(RegionWriteError::VerificationFailed(
                "chunk root is not a compound".to_owned(),
            ))
        }
    };
    root.remove(format.heightmaps);
    root.remove(format.post_processing);
    if let Some(enabled) = root.get_mut(format.light.enabled) {
        *enabled = NbtValue::Byte(0);
    }
    let sections = root.get_mut(format.sections).ok_or_else(|| {
        RegionWriteError::VerificationFailed("chunk has no sections list".to_owned())
    })?;
    let (kind, items) = match sections {
        NbtValue::List { kind, items } => (*kind, items),
        _ => {
            return Err(RegionWriteError::VerificationFailed(
                "chunk sections is not a list".to_owned(),
            ))
        }
    };
    if kind != crate::nbt::TAG_COMPOUND && !items.is_empty() {
        return Err(RegionWriteError::VerificationFailed(
            "chunk sections is not a list of compounds".to_owned(),
        ));
    }
    for item in items {
        let section = match item {
            NbtValue::Compound(section) => section,
            _ => {
                return Err(RegionWriteError::VerificationFailed(
                    "chunk has a non-compound section".to_owned(),
                ))
            }
        };
        section.remove(format.light.sky);
        section.remove(format.light.block);
    }
    Ok(())
}

fn verify_replacement(
    world: &JavaWorld,
    dimension_id: &str,
    source: &RegionChunk,
    replacement: &NbtFile,
    policy: DerivedDataPolicy,
    format: &ClassicChunkFormat,
) -> Result<(), RegionWriteError> {
    let root = replacement.root.as_compound().ok_or_else(|| {
        RegionWriteError::VerificationFailed(format!(
            "chunk ({}, {}) replacement root is not a compound",
            source.x, source.z
        ))
    })?;
    let data_version = match root.get(DATA_VERSION) {
        Some(NbtValue::Int(value)) => *value,
        _ => {
            return Err(RegionWriteError::VerificationFailed(format!(
                "chunk ({}, {}) replacement has no int DataVersion",
                source.x, source.z
            )))
        }
    };
    if data_version != world.profile.data_version {
        return Err(RegionWriteError::UnsupportedDataVersion {
            found: data_version,
            supported: vec![world.profile.data_version],
        });
    }
    match (root.get(format.x_pos), root.get(format.z_pos)) {
        (Some(NbtValue::Int(x)), Some(NbtValue::Int(z))) if *x == source.x && *z == source.z => {}
        _ => {
            return Err(RegionWriteError::VerificationFailed(format!(
                "chunk ({}, {}) replacement xPos/zPos does not match its region slot",
                source.x, source.z
            )))
        }
    }
    let expected_source = source_with_policy(source, policy, format)?;
    verify_preserved_fields(&expected_source, replacement, format)?;
    if replacement == &expected_source {
        return Err(RegionWriteError::InvalidArgument(format!(
            "chunk ({}, {}) replacement is identical to the source chunk",
            source.x, source.z
        )));
    }
    world
        .decode_chunk_nbt(dimension_id, source.x, source.z, replacement)
        .map_err(RegionWriteError::Anvil)?;
    Ok(())
}

fn validate_destination(
    destination: &Path,
    source: &Path,
    region_x: i32,
    region_z: i32,
) -> Result<(), RegionWriteError> {
    let expected = region_file_name(region_x, region_z);
    if destination.file_name().and_then(|name| name.to_str()) != Some(expected.as_str()) {
        return Err(RegionWriteError::InvalidArgument(format!(
            "destination must be named {expected}, got {}",
            destination.display()
        )));
    }
    let parent = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent_metadata = fs::symlink_metadata(parent)
        .map_err(|error| io_error(format!("reading {}", parent.display()), error))?;
    if parent_metadata.file_type().is_symlink() {
        return Err(RegionWriteError::UnsafePath(parent.display().to_string()));
    }
    if !parent_metadata.is_dir() {
        return Err(RegionWriteError::InvalidArgument(format!(
            "destination parent is not a directory: {}",
            parent.display()
        )));
    }
    let parent_canonical = fs::canonicalize(parent)
        .map_err(|error| io_error(format!("resolving {}", parent.display()), error))?;
    let source_canonical = fs::canonicalize(source)
        .map_err(|error| io_error(format!("resolving {}", source.display()), error))?;
    let source_parent = source
        .parent()
        .ok_or_else(|| RegionWriteError::UnsafePath(source.display().to_string()))?;
    let source_parent_canonical = fs::canonicalize(source_parent)
        .map_err(|error| io_error(format!("resolving {}", source_parent.display()), error))?;
    if parent_canonical == source_parent_canonical {
        return Err(RegionWriteError::UnsafePath(
            "destination must not be inside the source region directory".to_owned(),
        ));
    }
    let destination_canonical = parent_canonical.join(expected);
    if destination_canonical == source_canonical {
        return Err(RegionWriteError::UnsafePath(
            "destination would replace the source region".to_owned(),
        ));
    }
    if destination.exists() || fs::symlink_metadata(destination).is_ok() {
        return Err(RegionWriteError::AlreadyExists(
            destination.display().to_string(),
        ));
    }
    Ok(())
}

fn build_region_file(
    entries: &[RegionEntry],
    source: &RegionFile,
    replacements: &BTreeMap<(i32, i32), NbtFile>,
    staging: &Path,
) -> Result<u64, RegionWriteError> {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(staging)
        .map_err(|error| io_error(format!("creating {}", staging.display()), error))?;
    let mut writer = BufWriter::new(file);
    writer
        .write_all(&vec![0u8; ANVIL_HEADER_BYTES])
        .map_err(|error| io_error("writing region header", error))?;

    let mut header = vec![0u8; ANVIL_HEADER_BYTES];
    let max_sectors = MAX_REGION_FILE_BYTES / ANVIL_SECTOR_BYTES;
    let mut next_sector = ANVIL_HEADER_BYTES as u64 / ANVIL_SECTOR_BYTES;
    let timestamp = now_timestamp();
    let mut replaced = 0usize;

    for entry in entries {
        let (record, sectors, record_timestamp) = match replacements.get(&(entry.x, entry.z)) {
            Some(nbt) => {
                replaced += 1;
                let record = encode_chunk_record(nbt)?;
                let sectors = record.len().div_ceil(ANVIL_SECTOR_BYTES as usize);
                (record, sectors, timestamp)
            }
            None => {
                let record = read_record(source.path(), entry)?;
                (record, entry.sector_count as usize, entry.timestamp)
            }
        };
        if sectors == 0 || sectors > u8::MAX as usize {
            return Err(RegionWriteError::LimitExceeded(format!(
                "chunk ({}, {}) needs {sectors} sectors, limit is {}",
                entry.x,
                entry.z,
                u8::MAX
            )));
        }
        if next_sector + sectors as u64 > max_sectors {
            return Err(RegionWriteError::LimitExceeded(format!(
                "region would exceed {MAX_REGION_FILE_BYTES} bytes"
            )));
        }
        let offset = next_sector as u32;
        let index = entry.local_index as usize;
        let location = index * 4;
        header[location] = ((offset >> 16) & 0xff) as u8;
        header[location + 1] = ((offset >> 8) & 0xff) as u8;
        header[location + 2] = (offset & 0xff) as u8;
        header[location + 3] = sectors as u8;
        let timestamp_offset = ANVIL_SECTOR_BYTES as usize + index * 4;
        header[timestamp_offset..timestamp_offset + 4]
            .copy_from_slice(&record_timestamp.to_be_bytes());

        writer.write_all(&record).map_err(|error| {
            io_error(format!("writing chunk ({}, {})", entry.x, entry.z), error)
        })?;
        let padding = sectors * ANVIL_SECTOR_BYTES as usize - record.len();
        if padding > 0 {
            writer
                .write_all(&vec![0u8; padding])
                .map_err(|error| io_error("padding region chunk", error))?;
        }
        next_sector += sectors as u64;
    }

    if replaced != replacements.len() {
        return Err(RegionWriteError::VerificationFailed(format!(
            "{} replacement(s) were not written",
            replacements.len() - replaced
        )));
    }

    writer
        .flush()
        .map_err(|error| io_error("flushing region records", error))?;
    writer
        .seek(SeekFrom::Start(0))
        .map_err(|error| io_error("seeking to region header", error))?;
    writer
        .write_all(&header)
        .map_err(|error| io_error("writing region header", error))?;
    writer
        .flush()
        .map_err(|error| io_error("flushing region header", error))?;
    let file = writer
        .into_inner()
        .map_err(|error| io_error("finishing region file", error.into_error()))?;
    file.sync_all()
        .map_err(|error| io_error("syncing region file", error))?;
    Ok(next_sector * ANVIL_SECTOR_BYTES)
}

fn encode_chunk_record(nbt: &NbtFile) -> Result<Vec<u8>, RegionWriteError> {
    let raw = nbt::write_uncompressed(nbt).map_err(AnvilError::Nbt)?;
    if raw.len() > MAX_NBT_BYTES {
        return Err(RegionWriteError::LimitExceeded(format!(
            "chunk NBT is {} bytes, limit is {MAX_NBT_BYTES}",
            raw.len()
        )));
    }
    let payload = zlib(&raw)?;
    let length = u32::try_from(payload.len() + 1)
        .map_err(|_| RegionWriteError::LimitExceeded("chunk record is too large".to_owned()))?;
    let mut record = vec![0u8; 5 + payload.len()];
    record[0..4].copy_from_slice(&length.to_be_bytes());
    record[4] = ChunkCompression::Zlib.id();
    record[5..].copy_from_slice(&payload);
    Ok(record)
}

fn zlib(bytes: &[u8]) -> Result<Vec<u8>, RegionWriteError> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(bytes)
        .map_err(|error| io_error("compressing chunk", error))?;
    encoder
        .finish()
        .map_err(|error| io_error("finishing chunk compression", error))
}

fn read_record(path: &Path, entry: &RegionEntry) -> Result<Vec<u8>, RegionWriteError> {
    let mut record = vec![0u8; entry.sector_count as usize * ANVIL_SECTOR_BYTES as usize];
    let mut file =
        File::open(path).map_err(|error| io_error(format!("opening {}", path.display()), error))?;
    file.seek(SeekFrom::Start(
        u64::from(entry.offset_sectors) * ANVIL_SECTOR_BYTES,
    ))
    .map_err(|error| io_error(format!("seeking in {}", path.display()), error))?;
    file.read_exact(&mut record)
        .map_err(|error| io_error(format!("reading chunk ({}, {})", entry.x, entry.z), error))?;
    if record.len() < 5 {
        return Err(RegionWriteError::VerificationFailed(format!(
            "chunk ({}, {}) record is shorter than its header",
            entry.x, entry.z
        )));
    }
    let length = u32::from_be_bytes([record[0], record[1], record[2], record[3]]);
    if length == 0 || length as usize > record.len() - 4 {
        return Err(RegionWriteError::VerificationFailed(format!(
            "chunk ({}, {}) record length {length} exceeds its allocated sectors",
            entry.x, entry.z
        )));
    }
    if record[4] & 0x80 != 0 {
        return Err(AnvilError::ExternalChunk {
            x: entry.x,
            z: entry.z,
        }
        .into());
    }
    let _compression = ChunkCompression::from_id(record[4])?;
    Ok(record)
}

fn verify_written_region(
    world: &JavaWorld,
    dimension_id: &str,
    source: &RegionFile,
    written_path: &Path,
    replacements: &BTreeMap<(i32, i32), NbtFile>,
    source_entries: &[RegionEntry],
    policy: DerivedDataPolicy,
) -> Result<(), RegionWriteError> {
    let format = ClassicChunkFormat::for_profile(&world.profile)?;
    let written = RegionFile::open(written_path)?;
    if written.region_coordinates() != source.region_coordinates() {
        return Err(RegionWriteError::VerificationFailed(
            "written region coordinates changed".to_owned(),
        ));
    }
    let written_entries = written.entries();
    if written_entries.len() != source_entries.len() {
        return Err(RegionWriteError::VerificationFailed(format!(
            "written region has {} chunks, source has {}",
            written_entries.len(),
            source_entries.len()
        )));
    }
    let written_by_index = written_entries
        .iter()
        .map(|entry| (entry.local_index, entry))
        .collect::<BTreeMap<_, _>>();
    for source_entry in source_entries {
        let written_entry = written_by_index
            .get(&source_entry.local_index)
            .ok_or_else(|| {
                RegionWriteError::VerificationFailed(format!(
                    "written region lost chunk table entry {}",
                    source_entry.local_index
                ))
            })?;
        if written_entry.x != source_entry.x || written_entry.z != source_entry.z {
            return Err(RegionWriteError::VerificationFailed(format!(
                "written chunk table entry {} changed coordinates",
                source_entry.local_index
            )));
        }
        match replacements.get(&(source_entry.x, source_entry.z)) {
            Some(expected) => {
                let chunk = written.read_chunk(source_entry.x, source_entry.z)?;
                if &chunk.nbt != expected {
                    return Err(RegionWriteError::VerificationFailed(format!(
                        "chunk ({}, {}) does not round-trip through the written region",
                        source_entry.x, source_entry.z
                    )));
                }
                let source_chunk = source.read_chunk(source_entry.x, source_entry.z)?;
                let expected_source = source_with_policy(&source_chunk, policy, format)?;
                verify_preserved_fields(&expected_source, &chunk.nbt, format)?;
                world.decode_chunk_nbt(dimension_id, source_entry.x, source_entry.z, &chunk.nbt)?;
            }
            None => {
                if written_entry.timestamp != source_entry.timestamp {
                    return Err(RegionWriteError::VerificationFailed(format!(
                        "untouched chunk ({}, {}) timestamp changed",
                        source_entry.x, source_entry.z
                    )));
                }
                let expected = read_record(source.path(), source_entry)?;
                let actual = read_record(written.path(), written_entry)?;
                if expected != actual {
                    return Err(RegionWriteError::VerificationFailed(format!(
                        "untouched chunk ({}, {}) is not byte-identical",
                        source_entry.x, source_entry.z
                    )));
                }
            }
        }
    }
    Ok(())
}

fn now_timestamp() -> i32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().min(i32::MAX as u64) as i32)
        .unwrap_or(0)
}

fn sha256_file(path: &Path) -> Result<String, RegionWriteError> {
    let mut file =
        File::open(path).map_err(|error| io_error(format!("opening {}", path.display()), error))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| io_error(format!("hashing {}", path.display()), error))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn io_error(context: impl std::fmt::Display, error: std::io::Error) -> RegionWriteError {
    RegionWriteError::Io(format!("{context}: {error}"))
}

/// Temporary sibling directory that publishes one region file atomically.
struct StagingRegion {
    directory: PathBuf,
    path: PathBuf,
    published: bool,
}

impl StagingRegion {
    fn new(parent: &Path, region_x: i32, region_z: i32) -> Result<Self, RegionWriteError> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        for attempt in 0..32u32 {
            let directory = parent.join(format!(
                ".furybuilder-region-write-{}-{nanos}-{attempt}",
                std::process::id()
            ));
            match fs::create_dir(&directory) {
                Ok(()) => {
                    let path = directory.join(region_file_name(region_x, region_z));
                    return Ok(Self {
                        directory,
                        path,
                        published: false,
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(io_error(format!("creating {}", directory.display()), error))
                }
            }
        }
        Err(RegionWriteError::Io(
            "could not create a unique staging directory".to_owned(),
        ))
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn publish(mut self, destination: &Path) -> Result<(), RegionWriteError> {
        if destination.exists() || fs::symlink_metadata(destination).is_ok() {
            return Err(RegionWriteError::AlreadyExists(
                destination.display().to_string(),
            ));
        }
        fs::rename(&self.path, destination).map_err(|error| {
            io_error(
                format!(
                    "publishing {} to {}",
                    self.path.display(),
                    destination.display()
                ),
                error,
            )
        })?;
        self.published = true;
        Ok(())
    }
}

impl Drop for StagingRegion {
    fn drop(&mut self) {
        if !self.published {
            let _ = fs::remove_file(&self.path);
        }
        let _ = fs::remove_dir(&self.directory);
    }
}
