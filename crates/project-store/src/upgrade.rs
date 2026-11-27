//! Same-family project upgrade through a verified copy.
//!
//! An upgrade never retargets the original project in place. It first creates a mandatory
//! checkpoint in the source project, then copies the project folder into a staging sibling,
//! rebuilds the copied snapshot for the target profile, reopens and verifies the staged copy,
//! and only then publishes it with an atomic rename. The source manifest and active snapshot
//! are never rewritten.
//!
//! The supported path is deliberately narrow: both profiles must be modern classic Anvil
//! releases with the same layout, chunk family, biome format and dimension bounds, and the
//! target must be newer. Vanilla World projects are refused because their generator provenance
//! binds the original seed, server JAR and DataVersion; regenerating is the honest path there.
//! Imported projects keep their immutable source copy unchanged and only retarget the project
//! snapshot.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use minecraft_version::modern_classic::ModernClassicRelease;
use minecraft_version::{profile_for_id, VersionProfile};
use serde::Serialize;
use world_domain::{BiomeState, BlockRegistry, BlockState, World};

use crate::error::ProjectError;
use crate::manifest::{
    folder_component, normalize_name, now_ms, validate_folder, CreationMode, ProjectComponents,
    ProjectDetail, ProjectManifest, ProjectTarget, SaveKind, SourceKind, MAX_PROJECT_NAME_CHARS,
    PROJECT_FILE,
};
use crate::snapshot::encode_world;
use crate::source::read_source_manifest;
use crate::store::{
    io_error, sha256_hex, snapshot_file, verify_round_trip, write_atomic, write_json, ProjectStore,
    AUTOSAVE_FILE, AUTOSAVE_META, CACHE_DIR, CHECKPOINT_DIR, DATA_DIR, EXPORTS_DIR, SESSION_MARKER,
    SOURCE_DIR,
};

const MAX_UPGRADE_FILES: usize = 100_000;
const CHECKPOINT_LABEL: &str = "Before version upgrade";

/// Exact source and target facts shown before an upgrade starts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpgradeVersion {
    pub profile_id: String,
    pub minecraft_version: String,
    pub data_version: i32,
}

/// One used block state or biome and how many cells use it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpgradeStateCount {
    pub id: String,
    pub count: usize,
}

/// A read-only preflight report. It is produced without modifying the source project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpgradePreview {
    pub source: UpgradeVersion,
    pub target: UpgradeVersion,
    pub dimensions: usize,
    pub chunks: usize,
    pub sections: usize,
    pub blocks: usize,
    pub mapped_blocks: usize,
    pub block_states: usize,
    pub mapped_block_states: usize,
    pub biome_sections: usize,
    pub biome_cells: usize,
    pub mapped_biome_cells: usize,
    pub biome_states: usize,
    pub mapped_biomes: usize,
    pub unknown_block_states: Vec<UpgradeStateCount>,
    pub unknown_biomes: Vec<UpgradeStateCount>,
    pub can_upgrade: bool,
    pub blockers: Vec<String>,
    pub warnings: Vec<String>,
}

/// Options for a same-family upgrade. The target block registry must be the exact catalog
/// pinned for the target client JAR; a curated fallback is not sufficient to claim that every
/// state exists in the target version.
pub struct UpgradeOptions<'a> {
    pub target_profile_id: &'a str,
    pub name: Option<&'a str>,
    pub folder: Option<&'a str>,
    pub components: ProjectComponents,
    pub target_blocks: &'a BlockRegistry,
}

/// What the upgrade actually did, including the mandatory checkpoint and copy verification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpgradeReport {
    pub preview: UpgradePreview,
    pub checkpoint_revision: u64,
    pub checkpoint_file: String,
    pub checkpoint_label: String,
    pub checkpoint_reason: String,
    pub folder: String,
    pub copied_files: usize,
    pub copied_bytes: u64,
    pub snapshot_file: String,
    pub snapshot_sha256: String,
    pub snapshot_bytes: u64,
    pub world_revision: u64,
    pub staged_verified: bool,
    pub source_unchanged: bool,
}

/// A published upgraded project and its report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpgradeOutcome {
    pub project: ProjectDetail,
    pub report: UpgradeReport,
}

#[derive(Debug, Default)]
struct CopyStats {
    files: usize,
    bytes: u64,
}

struct WorldScan {
    chunks: usize,
    sections: usize,
    blocks: usize,
    block_counts: BTreeMap<BlockState, usize>,
    biome_sections: usize,
    biome_cells: usize,
    biome_counts: BTreeMap<BiomeState, usize>,
}

/// Removes the staging tree on every early return. After a successful rename the path no
/// longer exists, so drop is a no-op.
struct StagingProject {
    path: PathBuf,
}

impl StagingProject {
    fn new(path: PathBuf) -> Self {
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for StagingProject {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

impl ProjectStore {
    /// Preflights a same-family upgrade without writing anything. The report lists every
    /// used block state and biome that is absent from the target version; such a world is
    /// blocked instead of being published with invalid IDs.
    pub fn upgrade_preview(
        &self,
        folder: &str,
        target_profile_id: &str,
        target_blocks: &BlockRegistry,
    ) -> Result<UpgradePreview, ProjectError> {
        let dir = self.project_dir(folder)?;
        let (manifest, world, _) = self.load_saved_world(folder)?;
        let air: BlockState = "minecraft:air"
            .parse()
            .map_err(|error| ProjectError::corrupt(format!("invalid air state: {error}")))?;
        if target_blocks.validate(&air).is_err() {
            return Err(ProjectError::UpgradeUnsupported(
                "the target block catalog is incomplete; build and pin the exact client JAR catalog for the target version"
                    .to_owned(),
            ));
        }
        let source_profile = profile_for_id(&manifest.target.profile_id).ok_or_else(|| {
            ProjectError::UpgradeUnsupported(format!(
                "source profile `{}` is not built into this FuryBuilder",
                manifest.target.profile_id
            ))
        })?;
        let target_profile = profile_for_id(target_profile_id).ok_or_else(|| {
            ProjectError::UpgradeUnsupported(format!(
                "target profile `{target_profile_id}` is not built into this FuryBuilder"
            ))
        })?;
        validate_upgrade_pair(&source_profile, &target_profile)?;
        validate_upgrade_source(&dir, &manifest)?;

        let target_release =
            ModernClassicRelease::for_profile(&target_profile).ok_or_else(|| {
                ProjectError::UpgradeUnsupported(format!(
                    "Minecraft {} is not a verified modern classic release",
                    target_profile.minecraft_version
                ))
            })?;
        let scan = scan_world(&world);
        let unknown_block_states = unknown_states(&scan.block_counts, |state| {
            target_blocks.validate(state).is_ok()
        });
        let unknown_biomes = unknown_states(&scan.biome_counts, |biome| {
            target_release.contains_biome(biome.as_str())
        });

        let unknown_block_cells: usize = unknown_block_states.iter().map(|state| state.count).sum();
        let unknown_biome_cells: usize = unknown_biomes.iter().map(|biome| biome.count).sum();
        let mapped_blocks = scan.blocks.saturating_sub(unknown_block_cells);
        let mapped_block_states = scan
            .block_counts
            .len()
            .saturating_sub(unknown_block_states.len());
        let mapped_biome_cells = scan.biome_cells.saturating_sub(unknown_biome_cells);
        let mapped_biomes = scan.biome_counts.len().saturating_sub(unknown_biomes.len());

        let mut blockers = Vec::new();
        if !unknown_block_states.is_empty() {
            blockers.push(format!(
                "{} block state(s) are not valid in Minecraft {} and would have no mapping",
                unknown_block_states.len(),
                target_profile.minecraft_version
            ));
        }
        if !unknown_biomes.is_empty() {
            blockers.push(format!(
                "{} biome(s) are not in the vanilla Minecraft {} registry",
                unknown_biomes.len(),
                target_profile.minecraft_version
            ));
        }
        let can_upgrade = blockers.is_empty();

        let mut warnings = vec![
            "The upgraded project starts from the saved world snapshot; workspace, scene and parametric editor state are preserved without remapping.".to_owned(),
            "Block and biome IDs are not rewritten. Every used state must already exist in the target version; unknown states block publication instead of being substituted.".to_owned(),
            "The source project is not retargeted. A mandatory checkpoint is created there before the copy is published.".to_owned(),
        ];
        if manifest.source.kind == SourceKind::Imported {
            warnings.push(
                "The immutable imported source copy keeps the original version; only the project snapshot is retargeted."
                    .to_owned(),
            );
        }

        Ok(UpgradePreview {
            source: version_facts(&source_profile),
            target: version_facts(&target_profile),
            dimensions: world.dimensions().len(),
            chunks: scan.chunks,
            sections: scan.sections,
            blocks: scan.blocks,
            mapped_blocks,
            block_states: scan.block_counts.len(),
            mapped_block_states,
            biome_sections: scan.biome_sections,
            biome_cells: scan.biome_cells,
            mapped_biome_cells,
            biome_states: scan.biome_counts.len(),
            mapped_biomes,
            unknown_block_states,
            unknown_biomes,
            can_upgrade,
            blockers,
            warnings,
        })
    }

    /// Runs a same-family upgrade through a staged copy. The source project receives a
    /// mandatory checkpoint; its manifest and active snapshot stay byte-for-byte unchanged.
    pub fn upgrade(
        &self,
        folder: &str,
        options: UpgradeOptions<'_>,
    ) -> Result<UpgradeOutcome, ProjectError> {
        let preview =
            self.upgrade_preview(folder, options.target_profile_id, options.target_blocks)?;
        if !preview.can_upgrade {
            return Err(ProjectError::UpgradeBlocked(preview.blockers.join("; ")));
        }

        let source_dir = self.project_dir(folder)?;
        let (manifest, world, source_bytes) = self.load_saved_world(folder)?;
        let source_profile = profile_for_id(&manifest.target.profile_id).ok_or_else(|| {
            ProjectError::UpgradeUnsupported(format!(
                "source profile `{}` is not built into this FuryBuilder",
                manifest.target.profile_id
            ))
        })?;
        let target_profile = profile_for_id(options.target_profile_id).ok_or_else(|| {
            ProjectError::UpgradeUnsupported(format!(
                "target profile `{}` is not built into this FuryBuilder",
                options.target_profile_id
            ))
        })?;
        validate_upgrade_pair(&source_profile, &target_profile)?;
        validate_upgrade_source(&source_dir, &manifest)?;

        let target = ProjectTarget::from_profile(&target_profile);
        options.components.validate(&target)?;
        let assets = options.components.assets.as_ref().ok_or_else(|| {
            ProjectError::UpgradeUnsupported(
                "the target project needs a verified client asset catalog; install the exact target client JAR first"
                    .to_owned(),
            )
        })?;
        if assets.mode == crate::ComponentMode::Placeholder {
            return Err(ProjectError::UpgradeUnsupported(
                "placeholder assets cannot validate a target block catalog; install the exact target client JAR first"
                    .to_owned(),
            ));
        }
        let name = upgrade_name(&manifest.name, &target_profile, options.name)?;
        let final_folder = self.resolve_upgrade_folder(options.folder, &name)?;
        let source_revision = world.revision();
        let checkpoint_reason = format!(
            "upgrade Minecraft {} to {}",
            source_profile.minecraft_version, target_profile.minecraft_version
        );
        let checkpoint_file = self.checkpoint(
            folder,
            &world,
            manifest.editor_state.clone(),
            Some(CHECKPOINT_LABEL),
            &checkpoint_reason,
        )?;

        let staging = StagingProject::new(self.root().join(format!(
            ".upgrade-{}.furybuilder",
            uuid::Uuid::new_v4().simple()
        )));
        let (mut detail, stats, file, snapshot, revision) = self.build_staged_upgrade(
            &staging,
            &source_dir,
            &manifest,
            &world,
            &target_profile,
            &name,
            options.components,
        )?;

        self.verify_source_unchanged(&source_dir, &manifest, &source_bytes)?;

        let destination = self.root().join(&final_folder);
        if destination.exists() {
            return Err(ProjectError::AlreadyExists(final_folder));
        }
        fs::rename(staging.path(), &destination)
            .map_err(|error| io_error(format!("publishing {}", destination.display()), error))?;
        detail.summary.folder = final_folder.clone();

        Ok(UpgradeOutcome {
            project: detail,
            report: UpgradeReport {
                preview,
                checkpoint_revision: source_revision,
                checkpoint_file,
                checkpoint_label: CHECKPOINT_LABEL.to_owned(),
                checkpoint_reason,
                folder: final_folder,
                copied_files: stats.files,
                copied_bytes: stats.bytes,
                snapshot_file: file,
                snapshot_sha256: sha256_hex(&snapshot),
                snapshot_bytes: snapshot.len() as u64,
                world_revision: revision,
                staged_verified: true,
                source_unchanged: true,
            },
        })
    }

    /// Builds and verifies the complete staged project without touching the source.
    #[allow(clippy::too_many_arguments)]
    fn build_staged_upgrade(
        &self,
        staging: &StagingProject,
        source_dir: &Path,
        source_manifest: &ProjectManifest,
        source_world: &World,
        target_profile: &VersionProfile,
        name: &str,
        components: ProjectComponents,
    ) -> Result<(ProjectDetail, CopyStats, String, Vec<u8>, u64), ProjectError> {
        let stats = copy_project_tree(source_dir, staging.path())?;
        let target_world = retarget_world(source_world, target_profile)?;
        let snapshot = encode_world(&target_world)?;
        let decoded = verify_round_trip(&snapshot)?;
        if decoded.version() != target_world.version()
            || decoded.revision() != target_world.revision()
            || decoded.block_count() != target_world.block_count()
            || decoded.section_count() != target_world.section_count()
            || decoded.chunk_count() != target_world.chunk_count()
        {
            return Err(ProjectError::corrupt(
                "upgraded snapshot failed semantic verification",
            ));
        }
        let file = snapshot_file(target_world.revision());
        let checksum = sha256_hex(&snapshot);
        write_atomic(&staging.path().join(DATA_DIR).join(&file), &snapshot)?;

        let now = now_ms();
        let mut target_manifest = ProjectManifest::new(
            uuid::Uuid::new_v4().to_string(),
            name.to_owned(),
            source_manifest.creation_mode,
            ProjectTarget::from_profile(target_profile),
            target_world.revision(),
            now,
        );
        target_manifest.editor_state = source_manifest.editor_state.clone();
        target_manifest.components = components;
        target_manifest.source = source_manifest.source.clone();
        target_manifest.vanilla = None;
        target_manifest.updated_at_ms = now;
        target_manifest.last_saved_at_ms = now;
        target_manifest.last_save_kind = SaveKind::Manual;
        target_manifest.set_world_payload(
            format!("{DATA_DIR}/{file}"),
            checksum.clone(),
            snapshot.len() as u64,
        );
        target_manifest.validate()?;
        write_json(&staging.path().join(PROJECT_FILE), &target_manifest)?;

        let (detail, opened, recovery) = self.open(
            staging
                .path()
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| ProjectError::InvalidPath(staging.path().display().to_string()))?,
        )?;
        if recovery.is_some() {
            return Err(ProjectError::corrupt(
                "the staged upgrade unexpectedly contains a recovery autosave",
            ));
        }
        if opened.version() != target_world.version()
            || opened.revision() != target_world.revision()
            || opened.block_count() != target_world.block_count()
            || opened.section_count() != target_world.section_count()
            || opened.chunk_count() != target_world.chunk_count()
            || detail.summary.target_profile_id != target_profile.id
        {
            return Err(ProjectError::corrupt(
                "the staged upgrade did not reopen as the target version",
            ));
        }
        Ok((detail, stats, file, snapshot, target_world.revision()))
    }

    fn verify_source_unchanged(
        &self,
        source_dir: &Path,
        expected_manifest: &ProjectManifest,
        expected_snapshot: &[u8],
    ) -> Result<(), ProjectError> {
        let manifest = self.manifest(source_dir)?;
        if &manifest != expected_manifest {
            return Err(ProjectError::corrupt(
                "the source project manifest changed during the upgrade",
            ));
        }
        let path = source_dir.join(&manifest.world_file);
        let bytes = fs::read(&path)
            .map_err(|error| io_error(format!("reading {}", path.display()), error))?;
        if bytes != expected_snapshot {
            return Err(ProjectError::corrupt(
                "the source project snapshot changed during the upgrade",
            ));
        }
        Ok(())
    }

    fn load_saved_world(
        &self,
        folder: &str,
    ) -> Result<(ProjectManifest, World, Vec<u8>), ProjectError> {
        let dir = self.project_dir(folder)?;
        let manifest = self.manifest(&dir)?;
        let path = dir.join(&manifest.world_file);
        let bytes = fs::read(&path)
            .map_err(|error| io_error(format!("reading {}", path.display()), error))?;
        if manifest.world_bytes != bytes.len() as u64 {
            return Err(ProjectError::corrupt(
                "saved snapshot size does not match the project manifest",
            ));
        }
        let actual = sha256_hex(&bytes);
        if actual != manifest.world_checksum {
            return Err(ProjectError::ChecksumMismatch {
                expected: manifest.world_checksum.clone(),
                actual,
            });
        }
        let world = verify_round_trip(&bytes)?;
        if world.version().minecraft_version() != manifest.target.minecraft_version.as_str()
            || world.version().data_version() != manifest.target.data_version
            || world.revision() != manifest.world_revision
        {
            return Err(ProjectError::corrupt(
                "saved snapshot identity does not match the project target",
            ));
        }
        Ok((manifest, world, bytes))
    }

    fn resolve_upgrade_folder(
        &self,
        requested: Option<&str>,
        name: &str,
    ) -> Result<String, ProjectError> {
        if let Some(folder) = requested {
            validate_folder(folder)?;
            if self.root().join(folder).exists() {
                return Err(ProjectError::AlreadyExists(folder.to_owned()));
            }
            return Ok(folder.to_owned());
        }
        let base = folder_component(name);
        let mut folder = base.clone();
        let mut counter = 1;
        while self.root().join(&folder).exists() {
            counter += 1;
            if counter > 999 {
                return Err(ProjectError::AlreadyExists(name.to_owned()));
            }
            let stem = base.trim_end_matches(".furybuilder");
            folder = format!("{stem} ({counter}).furybuilder");
        }
        Ok(folder)
    }
}

fn version_facts(profile: &VersionProfile) -> UpgradeVersion {
    UpgradeVersion {
        profile_id: profile.id.clone(),
        minecraft_version: profile.minecraft_version.to_string(),
        data_version: profile.data_version,
    }
}

fn validate_upgrade_source(dir: &Path, manifest: &ProjectManifest) -> Result<(), ProjectError> {
    if manifest.creation_mode == CreationMode::Vanilla {
        return Err(ProjectError::UpgradeUnsupported(
            "Vanilla World projects bind their generator provenance to the original seed, server JAR and DataVersion; regenerate the world for the target version instead"
                .to_owned(),
        ));
    }
    if manifest.source.kind == SourceKind::Generated && manifest.source.reference.is_some() {
        return Err(ProjectError::UpgradeUnsupported(
            "the project has an immutable generated source tree for its original version; regenerate it for the target version"
                .to_owned(),
        ));
    }
    if manifest.source.kind == SourceKind::Imported {
        let source = read_source_manifest(&dir.join(SOURCE_DIR).join("source.json"))?;
        if source.files.is_empty() {
            return Err(ProjectError::UpgradeUnsupported(
                "the imported project has no immutable source copy to preserve; repair the source reference before upgrading"
                    .to_owned(),
            ));
        }
    }
    Ok(())
}

fn validate_upgrade_pair(
    source: &VersionProfile,
    target: &VersionProfile,
) -> Result<(), ProjectError> {
    if !target.capabilities.write || !target.capabilities.edit {
        return Err(ProjectError::UpgradeUnsupported(format!(
            "the target profile `{}` is not writable",
            target.id
        )));
    }
    if source.family != target.family
        || source.world_layout != target.world_layout
        || source.chunk_family != target.chunk_family
        || source.biome_format != target.biome_format
    {
        return Err(ProjectError::UpgradeUnsupported(format!(
            "Minecraft {} and {} are not in the same world-format family",
            source.minecraft_version, target.minecraft_version
        )));
    }
    if target.data_version <= source.data_version {
        return Err(ProjectError::UpgradeUnsupported(format!(
            "Minecraft {} is not newer than the project target {}",
            target.minecraft_version, source.minecraft_version
        )));
    }
    if ModernClassicRelease::for_profile(source).is_none()
        || ModernClassicRelease::for_profile(target).is_none()
    {
        return Err(ProjectError::UpgradeUnsupported(
            "same-family upgrade is verified only for modern classic releases 1.18-1.21.11"
                .to_owned(),
        ));
    }
    if source.dimension_profiles.len() != target.dimension_profiles.len() {
        return Err(ProjectError::UpgradeUnsupported(
            "source and target profiles declare different dimension sets".to_owned(),
        ));
    }
    for target_dimension in &target.dimension_profiles {
        let source_dimension = source
            .dimension_profiles
            .iter()
            .find(|dimension| dimension.id == target_dimension.id)
            .ok_or_else(|| {
                ProjectError::UpgradeUnsupported(format!(
                    "target dimension `{}` is missing from the source profile",
                    target_dimension.id
                ))
            })?;
        if source_dimension.min_y != target_dimension.min_y
            || source_dimension.height != target_dimension.height
        {
            return Err(ProjectError::UpgradeUnsupported(format!(
                "dimension `{}` has different vertical bounds in the target profile",
                target_dimension.id
            )));
        }
    }
    Ok(())
}

fn retarget_world(source: &World, target: &VersionProfile) -> Result<World, ProjectError> {
    let mut world = World::for_profile(target);
    for (dimension_id, dimension) in source.dimensions() {
        for (chunk_pos, chunk) in dimension.chunks() {
            for (section_y, section) in chunk.sections() {
                world
                    .install_section(
                        dimension_id,
                        *chunk_pos,
                        *section_y,
                        Some(section.clone()),
                    )
                    .map_err(|error| {
                        ProjectError::corrupt(format!(
                            "could not carry section {section_y} of {chunk_pos:?} in `{dimension_id}`: {error}"
                        ))
                    })?;
            }
        }
    }
    world.restore_revision(source.revision().saturating_add(1));
    Ok(world)
}

fn upgrade_name(
    source_name: &str,
    target: &VersionProfile,
    requested: Option<&str>,
) -> Result<String, ProjectError> {
    if let Some(name) = requested {
        return normalize_name(name);
    }
    let suffix = format!(" ({})", target.minecraft_version);
    let max_base = MAX_PROJECT_NAME_CHARS.saturating_sub(suffix.chars().count());
    let base: String = source_name.chars().take(max_base).collect();
    let base = base.trim_end();
    if base.is_empty() {
        return normalize_name(&format!("Upgraded{suffix}"));
    }
    normalize_name(&format!("{base}{suffix}"))
}

fn scan_world(world: &World) -> WorldScan {
    let mut scan = WorldScan {
        chunks: 0,
        sections: 0,
        blocks: 0,
        block_counts: BTreeMap::new(),
        biome_sections: 0,
        biome_cells: 0,
        biome_counts: BTreeMap::new(),
    };
    for dimension in world.dimensions().values() {
        for chunk in dimension.chunks().values() {
            scan.chunks += 1;
            for section in chunk.sections().values() {
                scan.sections += 1;
                for x in 0..16u8 {
                    for y in 0..16u8 {
                        for z in 0..16u8 {
                            if let Some(state) = section.get(x, y, z) {
                                if state.is_air() {
                                    continue;
                                }
                                scan.blocks += 1;
                                *scan.block_counts.entry(state.clone()).or_default() += 1;
                            }
                        }
                    }
                }
                if let Some(biomes) = section.biomes() {
                    scan.biome_sections += 1;
                    for x in 0..16u8 {
                        for y in 0..16u8 {
                            for z in 0..16u8 {
                                if let Some(biome) = biomes.get(x, y, z) {
                                    scan.biome_cells += 1;
                                    *scan.biome_counts.entry(biome.clone()).or_default() += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    scan
}

fn unknown_states<T: Ord + ToString>(
    counts: &BTreeMap<T, usize>,
    is_valid: impl Fn(&T) -> bool,
) -> Vec<UpgradeStateCount> {
    let mut unknown: Vec<UpgradeStateCount> = counts
        .iter()
        .filter(|(value, _)| !is_valid(value))
        .map(|(value, count)| UpgradeStateCount {
            id: value.to_string(),
            count: *count,
        })
        .collect();
    unknown.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.id.cmp(&right.id))
    });
    unknown
}

fn copy_project_tree(source: &Path, destination: &Path) -> Result<CopyStats, ProjectError> {
    let mut stats = CopyStats::default();
    copy_directory(source, destination, true, &mut stats)?;
    Ok(stats)
}

fn copy_directory(
    source: &Path,
    destination: &Path,
    root: bool,
    stats: &mut CopyStats,
) -> Result<(), ProjectError> {
    fs::create_dir_all(destination)
        .map_err(|error| io_error(format!("creating {}", destination.display()), error))?;
    let entries = fs::read_dir(source)
        .map_err(|error| io_error(format!("listing {}", source.display()), error))?;
    for entry in entries {
        let entry =
            entry.map_err(|error| io_error(format!("listing {}", source.display()), error))?;
        let name = entry.file_name().to_string_lossy().to_string();
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| io_error(format!("inspecting {}", path.display()), error))?;
        if metadata.file_type().is_symlink() {
            return Err(ProjectError::UnsafeSource(path.display().to_string()));
        }
        if name.ends_with(".tmp") {
            continue;
        }
        if root
            && (name == SESSION_MARKER
                || name == CACHE_DIR
                || name == CHECKPOINT_DIR
                || name == EXPORTS_DIR
                || name.starts_with("project.json.bak-"))
        {
            continue;
        }
        if metadata.is_dir() {
            if root && name == DATA_DIR {
                copy_data_directory(&path, &destination.join(&name), stats)?;
            } else {
                copy_directory(&path, &destination.join(&name), false, stats)?;
            }
        } else if metadata.is_file() {
            copy_file(&path, &destination.join(&name), metadata.len(), stats)?;
        }
    }
    Ok(())
}

fn copy_data_directory(
    source: &Path,
    destination: &Path,
    stats: &mut CopyStats,
) -> Result<(), ProjectError> {
    fs::create_dir_all(destination)
        .map_err(|error| io_error(format!("creating {}", destination.display()), error))?;
    let entries = fs::read_dir(source)
        .map_err(|error| io_error(format!("listing {}", source.display()), error))?;
    for entry in entries {
        let entry =
            entry.map_err(|error| io_error(format!("listing {}", source.display()), error))?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name == AUTOSAVE_FILE
            || name == AUTOSAVE_META
            || (name.starts_with("world-") && name.ends_with(".fwl"))
        {
            continue;
        }
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| io_error(format!("inspecting {}", path.display()), error))?;
        if metadata.file_type().is_symlink() {
            return Err(ProjectError::UnsafeSource(path.display().to_string()));
        }
        if metadata.is_file() {
            copy_file(&path, &destination.join(&name), metadata.len(), stats)?;
        } else if metadata.is_dir() {
            copy_directory(&path, &destination.join(&name), false, stats)?;
        }
    }
    Ok(())
}

fn copy_file(
    source: &Path,
    destination: &Path,
    bytes: u64,
    stats: &mut CopyStats,
) -> Result<(), ProjectError> {
    if stats.files >= MAX_UPGRADE_FILES {
        return Err(ProjectError::LimitExceeded(format!(
            "upgrade copy exceeds {MAX_UPGRADE_FILES} files"
        )));
    }
    fs::copy(source, destination)
        .map_err(|error| io_error(format!("copying {}", source.display()), error))?;
    stats.files += 1;
    stats.bytes += bytes;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::SourceFile;
    use crate::store::CreateOptions;
    use crate::EditorState;
    use std::sync::atomic::{AtomicU64, Ordering};
    use world_domain::{BiomeSection, BlockPos, BlockSchemaEntry, ChunkPos, Section};

    static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn registry(blocks: &[&str]) -> BlockRegistry {
        BlockRegistry::from_schema(blocks.iter().map(|name| BlockSchemaEntry {
            name: (*name).to_owned(),
            properties: Default::default(),
            values: Default::default(),
            property_values_complete: true,
            default_properties: Default::default(),
        }))
    }

    fn target_components() -> ProjectComponents {
        ProjectComponents {
            assets: Some(crate::AssetsComponent {
                mode: crate::ComponentMode::Managed,
                client_jar_sha1: Some("a".repeat(40)),
                catalog_id: Some("java-test-target".to_owned()),
                catalog_hash: Some("b".repeat(64)),
                pack_stack: Vec::new(),
            }),
            ..ProjectComponents::default()
        }
    }

    fn store() -> (PathBuf, ProjectStore) {
        let root = std::env::temp_dir().join(format!(
            "furybuilder-upgrade-{}-{}-{}",
            std::process::id(),
            now_ms(),
            FIXTURE_COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&root);
        let store = ProjectStore::new(root.join("projects"));
        (root, store)
    }

    fn create_project(
        store: &ProjectStore,
        profile_id: &str,
        name: &str,
        blocks: &[(&str, BlockPos)],
    ) -> String {
        let profile = profile_for_id(profile_id).expect("profile");
        let mut world = crate::build_world(CreationMode::Empty, None, &profile).expect("world");
        for (state, pos) in blocks {
            world
                .set_block(
                    "minecraft:overworld",
                    *pos,
                    &state.parse::<BlockState>().expect("state"),
                )
                .expect("set block");
        }
        world.bump_revision();
        let folder = format!("{name}.furybuilder");
        store
            .create_with_options(
                name,
                CreationMode::Empty,
                EditorState::default(),
                CreateOptions {
                    folder: Some(&folder),
                    profile_id: Some(profile_id),
                    ..CreateOptions::default()
                },
            )
            .expect("create");
        // The generic creator starts from an empty world; save the test blocks as the
        // project snapshot through the normal atomic save path.
        store
            .save(&folder, &world, EditorState::default(), SaveKind::Manual)
            .expect("save");
        folder
    }

    #[test]
    fn previews_and_publishes_a_verified_same_family_copy() {
        let (_root, store) = store();
        let folder = create_project(
            &store,
            "java-classic-1.18.2",
            "Castle",
            &[
                ("minecraft:stone", BlockPos::new(0, 0, 0)),
                ("minecraft:dirt", BlockPos::new(1, 0, 0)),
            ],
        );
        let (_, mut source_world, _) = store.open(&folder).expect("open source");
        source_world
            .set_block(
                "minecraft:the_nether",
                BlockPos::new(0, 64, 0),
                &"minecraft:stone".parse::<BlockState>().expect("state"),
            )
            .expect("set nether block");
        source_world.bump_revision();
        let editor_state = EditorState {
            load_radius: 3,
            ..EditorState::default()
        };
        store
            .save(
                &folder,
                &source_world,
                editor_state.clone(),
                SaveKind::Manual,
            )
            .expect("save");
        store
            .update_editor_state(&folder, editor_state.clone())
            .expect("editor state");

        let source_dir = store.project_dir(&folder).expect("source dir");
        let source_manifest = store.manifest(&source_dir).expect("manifest");
        let before_manifest = fs::read(source_dir.join(PROJECT_FILE)).expect("manifest bytes");
        let before_snapshot =
            fs::read(source_dir.join(&source_manifest.world_file)).expect("snapshot bytes");

        let target = registry(&["minecraft:air", "minecraft:stone", "minecraft:dirt"]);
        let preview = store
            .upgrade_preview(&folder, "java-classic-1.20.6", &target)
            .expect("preview");
        assert!(preview.can_upgrade, "{:?}", preview.blockers);
        assert_eq!(preview.source.minecraft_version, "1.18.2");
        assert_eq!(preview.target.minecraft_version, "1.20.6");
        assert_eq!(preview.blocks, 3);
        assert_eq!(preview.mapped_blocks, 3);
        assert_eq!(preview.block_states, 2);
        assert_eq!(preview.mapped_block_states, 2);
        assert_eq!(preview.unknown_block_states, Vec::new());
        assert_eq!(preview.dimensions, 3);
        assert_eq!(preview.chunks, 2);

        let error = store
            .upgrade(
                &folder,
                UpgradeOptions {
                    target_profile_id: "java-classic-1.20.6",
                    name: Some("Unpinned"),
                    folder: None,
                    components: ProjectComponents::default(),
                    target_blocks: &target,
                },
            )
            .expect_err("target assets are required");
        assert!(matches!(error, ProjectError::UpgradeUnsupported(_)));
        assert!(store.checkpoints(&folder).expect("checkpoints").is_empty());

        let before = store.detail(&folder).expect("source detail");
        let outcome = store
            .upgrade(
                &folder,
                UpgradeOptions {
                    target_profile_id: "java-classic-1.20.6",
                    name: Some("Castle upgraded"),
                    folder: None,
                    components: target_components(),
                    target_blocks: &target,
                },
            )
            .expect("upgrade");
        assert_eq!(outcome.project.summary.name, "Castle upgraded");
        assert_eq!(
            outcome.project.summary.target_profile_id,
            "java-classic-1.20.6"
        );
        assert_eq!(outcome.project.summary.target_minecraft_version, "1.20.6");
        assert_eq!(outcome.project.editor_state, editor_state);
        assert_eq!(outcome.report.preview.blocks, 3);
        assert_eq!(outcome.report.preview.mapped_blocks, 3);
        assert!(outcome.report.staged_verified);
        assert!(outcome.report.source_unchanged);
        assert_eq!(
            outcome.report.checkpoint_revision,
            before.summary.world_revision
        );
        assert_eq!(
            outcome.report.world_revision,
            before.summary.world_revision + 1
        );
        assert_eq!(
            outcome.report.checkpoint_file,
            format!("checkpoint-{:020}.fwl", before.summary.world_revision)
        );
        assert_ne!(
            outcome.project.summary.project_id,
            before.summary.project_id
        );

        let (_, upgraded, _) = store.open(&outcome.report.folder).expect("open upgraded");
        assert_eq!(upgraded.version().minecraft_version(), "1.20.6");
        assert_eq!(upgraded.version().data_version(), 3839);
        assert_eq!(
            upgraded
                .get_block("minecraft:overworld", BlockPos::new(0, 0, 0))
                .expect("block"),
            "minecraft:stone".parse::<BlockState>().expect("state")
        );
        assert_eq!(
            upgraded
                .get_block("minecraft:the_nether", BlockPos::new(0, 64, 0))
                .expect("nether block"),
            "minecraft:stone".parse::<BlockState>().expect("state")
        );
        assert_eq!(upgraded.revision(), before.summary.world_revision + 1);

        let after = store.detail(&folder).expect("source after");
        assert_eq!(after.summary.target_profile_id, "java-classic-1.18.2");
        assert_eq!(after.summary.world_revision, before.summary.world_revision);
        assert_eq!(
            fs::read(source_dir.join(PROJECT_FILE)).expect("manifest after"),
            before_manifest
        );
        assert_eq!(
            fs::read(source_dir.join(&source_manifest.world_file)).expect("snapshot after"),
            before_snapshot
        );
        let checkpoints = store.checkpoints(&folder).expect("checkpoints");
        assert_eq!(checkpoints.len(), 1);
        assert_eq!(checkpoints[0].reason, "upgrade Minecraft 1.18.2 to 1.20.6");
        assert_eq!(checkpoints[0].profile_id, "java-classic-1.18.2");
        assert!(source_dir
            .join(CHECKPOINT_DIR)
            .join(&outcome.report.checkpoint_file)
            .exists());

        let second = store
            .upgrade(
                &outcome.report.folder,
                UpgradeOptions {
                    target_profile_id: "java-classic-1.21.4",
                    name: None,
                    folder: None,
                    components: target_components(),
                    target_blocks: &target,
                },
            )
            .expect("second upgrade");
        assert_eq!(
            second.project.summary.target_profile_id,
            "java-classic-1.21.4"
        );
        assert_eq!(second.project.summary.target_minecraft_version, "1.21.4");
        assert_eq!(second.report.checkpoint_revision, upgraded.revision());
        assert!(second.report.source_unchanged);
    }

    #[test]
    fn unknown_blocks_and_biomes_block_the_upgrade_without_writing() {
        let (_root, store) = store();
        let folder = create_project(
            &store,
            "java-classic-1.18.2",
            "Unknown",
            &[("minecraft:stone", BlockPos::new(0, 0, 0))],
        );
        // Replace the section with one that also carries biome cells so both checks run.
        let profile = profile_for_id("java-classic-1.18.2").expect("profile");
        let mut world = crate::build_world(CreationMode::Empty, None, &profile).expect("world");
        let mut section = Section::empty();
        section
            .set(
                0,
                0,
                0,
                &"minecraft:stone".parse::<BlockState>().expect("state"),
            )
            .expect("set block");
        section
            .set(
                1,
                0,
                0,
                &"example:removed_block"
                    .parse::<BlockState>()
                    .expect("state"),
            )
            .expect("set block");
        section.set_biomes(Some(BiomeSection::uniform(
            "example:biome".parse::<BiomeState>().expect("biome"),
        )));
        world
            .install_section("minecraft:overworld", ChunkPos::new(0, 0), 0, Some(section))
            .expect("install section");
        world.bump_revision();
        store
            .save(&folder, &world, EditorState::default(), SaveKind::Manual)
            .expect("save");

        let target = registry(&["minecraft:air", "minecraft:stone"]);
        let preview = store
            .upgrade_preview(&folder, "java-classic-1.20.6", &target)
            .expect("preview");
        assert!(!preview.can_upgrade);
        assert_eq!(preview.blocks, 2);
        assert_eq!(preview.mapped_blocks, 1);
        assert_eq!(preview.unknown_block_states.len(), 1);
        assert_eq!(preview.unknown_block_states[0].id, "example:removed_block");
        assert_eq!(preview.biome_cells, 64);
        assert_eq!(preview.mapped_biome_cells, 0);
        assert_eq!(preview.unknown_biomes.len(), 1);
        assert_eq!(preview.unknown_biomes[0].id, "example:biome");

        let error = store
            .upgrade(
                &folder,
                UpgradeOptions {
                    target_profile_id: "java-classic-1.20.6",
                    name: None,
                    folder: None,
                    components: target_components(),
                    target_blocks: &target,
                },
            )
            .expect_err("blocked upgrade");
        assert!(matches!(error, ProjectError::UpgradeBlocked(_)));
        let detail = store.detail(&folder).expect("source");
        assert_eq!(detail.summary.target_profile_id, "java-classic-1.18.2");
        assert!(store.checkpoints(&folder).expect("checkpoints").is_empty());
    }

    #[test]
    fn refuses_downgrade_same_version_cross_family_and_vanilla_projects() {
        let (_root, store) = store();
        let folder = create_project(
            &store,
            "java-classic-1.20.6",
            "Downgrade",
            &[("minecraft:stone", BlockPos::new(0, 0, 0))],
        );
        let target = registry(&["minecraft:air", "minecraft:stone"]);
        for target_id in ["java-classic-1.18.2", "java-classic-1.20.6"] {
            let error = store
                .upgrade_preview(&folder, target_id, &target)
                .expect_err("not an upgrade");
            assert!(matches!(error, ProjectError::UpgradeUnsupported(_)));
        }
        let error = store
            .upgrade_preview(&folder, "java-split-26.1", &target)
            .expect_err("cross family");
        assert!(matches!(error, ProjectError::UpgradeUnsupported(_)));
        assert!(profile_for_id("java-split-26.1").is_some());
        let empty = BlockRegistry::from_schema(std::iter::empty());
        let error = store
            .upgrade_preview(&folder, "java-classic-1.21.4", &empty)
            .expect_err("incomplete target catalog");
        assert!(matches!(error, ProjectError::UpgradeUnsupported(_)));

        let vanilla_folder = "Vanilla.furybuilder";
        let profile = profile_for_id("java-classic-1.18.2").expect("profile");
        let mut world = crate::build_world(CreationMode::Empty, None, &profile).expect("world");
        world.bump_revision();
        let provenance = crate::VanillaProvenance {
            seed: "12345".to_owned(),
            preset: "normal".to_owned(),
            center_x: 0,
            center_z: 0,
            radius_chunks: 1,
            generate_structures: true,
            datapacks: Vec::new(),
            minecraft_version: "1.18.2".to_owned(),
            data_version: 2975,
            java_version: "17.0.1".to_owned(),
            server_jar_sha1: "a".repeat(40),
            run_id: "run-1".to_owned(),
            generated_at_ms: 1,
            chunks: 1,
            region_files: 1,
            world_bytes: 1,
        };
        store
            .create_vanilla(
                "Vanilla",
                EditorState::default(),
                &world,
                "world",
                provenance,
                crate::ImportOptions {
                    folder: Some(vanilla_folder),
                    components: ProjectComponents::default(),
                },
            )
            .expect("vanilla project");
        let error = store
            .upgrade_preview(vanilla_folder, "java-classic-1.20.6", &target)
            .expect_err("vanilla provenance");
        assert!(matches!(error, ProjectError::UpgradeUnsupported(_)));
    }

    #[test]
    fn imported_projects_keep_their_immutable_source_copy() {
        let (_root, store) = store();
        let folder = "Imported.furybuilder";
        let profile = profile_for_id("java-classic-1.18.2").expect("profile");
        let mut world = crate::build_world(CreationMode::Empty, None, &profile).expect("world");
        world
            .set_block(
                "minecraft:overworld",
                BlockPos::new(0, 0, 0),
                &"minecraft:stone".parse::<BlockState>().expect("state"),
            )
            .expect("set block");
        world.bump_revision();
        store
            .create_imported(
                "Imported",
                EditorState::default(),
                &world,
                "world",
                crate::ImportOptions {
                    folder: Some(folder),
                    components: ProjectComponents::default(),
                },
            )
            .expect("imported project");

        let dir = store.project_dir(folder).expect("dir");
        let source_dir = dir.join(SOURCE_DIR).join("world");
        fs::create_dir_all(&source_dir).expect("source dir");
        let level_bytes = b"fixture-level";
        fs::write(source_dir.join("level.dat"), level_bytes).expect("source file");
        store
            .record_source_tree(
                folder,
                "world",
                &[SourceFile {
                    relative_path: "world/level.dat".to_owned(),
                    bytes: level_bytes.len() as u64,
                    sha256: sha256_hex(level_bytes),
                }],
            )
            .expect("record source");

        let target = registry(&["minecraft:air", "minecraft:stone"]);
        let outcome = store
            .upgrade(
                folder,
                UpgradeOptions {
                    target_profile_id: "java-classic-1.20.6",
                    name: Some("Imported upgraded"),
                    folder: None,
                    components: target_components(),
                    target_blocks: &target,
                },
            )
            .expect("imported upgrade");
        assert_eq!(outcome.project.source.kind, SourceKind::Imported);
        assert_eq!(outcome.project.source.reference.as_deref(), Some("world"));
        assert!(outcome.project.source.immutable);
        let copied = store
            .project_dir(&outcome.report.folder)
            .expect("copied dir")
            .join(SOURCE_DIR)
            .join("world")
            .join("level.dat");
        assert_eq!(fs::read(copied).expect("copied source"), level_bytes);
        assert_eq!(
            fs::read(source_dir.join("level.dat")).expect("original source"),
            level_bytes
        );
        assert!(outcome.report.source_unchanged);
    }

    #[test]
    fn copy_skips_session_cache_checkpoints_and_autosave() {
        let (_root, store) = store();
        let folder = create_project(
            &store,
            "java-classic-1.18.2",
            "Copy",
            &[("minecraft:stone", BlockPos::new(0, 0, 0))],
        );
        let dir = store.project_dir(&folder).expect("dir");
        fs::write(dir.join(SESSION_MARKER), b"marker").expect("session");
        fs::create_dir_all(dir.join(CACHE_DIR)).expect("cache");
        fs::write(dir.join(CACHE_DIR).join("keep-out"), b"cache").expect("cache file");
        fs::create_dir_all(dir.join(CHECKPOINT_DIR)).expect("changes");
        fs::write(dir.join(CHECKPOINT_DIR).join("checkpoint-1.fwl"), b"old").expect("checkpoint");
        fs::write(dir.join(DATA_DIR).join(AUTOSAVE_FILE), b"autosave").expect("autosave");
        fs::write(dir.join(DATA_DIR).join(AUTOSAVE_META), b"meta").expect("meta");

        let staging = store.root().join("stage.furybuilder");
        let stats = copy_project_tree(&dir, &staging).expect("copy");
        assert!(!staging.join(SESSION_MARKER).exists());
        assert!(!staging.join(CACHE_DIR).exists());
        assert!(!staging.join(CHECKPOINT_DIR).exists());
        assert!(!staging.join(DATA_DIR).join(AUTOSAVE_FILE).exists());
        assert!(!staging.join(DATA_DIR).join(AUTOSAVE_META).exists());
        assert!(staging.join(PROJECT_FILE).exists());
        assert!(staging.join(SOURCE_DIR).join("source.json").exists());
        assert!(stats.files >= 2);
        let _ = fs::remove_dir_all(staging);
    }

    #[test]
    fn a_flat_project_keeps_its_mode_and_blocks() {
        let (_root, store) = store();
        let folder = "Flat.furybuilder";
        store
            .create_with_options(
                "Flat",
                CreationMode::Flat,
                EditorState::default(),
                CreateOptions {
                    folder: Some(folder),
                    profile_id: Some("java-classic-1.18.2"),
                    flat: Some(crate::FlatPreset {
                        base_y: 64,
                        layers: vec![crate::FlatLayer {
                            block: "minecraft:stone".to_owned(),
                            thickness: 2,
                        }],
                        biome: "minecraft:plains".to_owned(),
                    }),
                    ..CreateOptions::default()
                },
            )
            .expect("flat project");
        let target = registry(&["minecraft:air", "minecraft:stone"]);
        let outcome = store
            .upgrade(
                folder,
                UpgradeOptions {
                    target_profile_id: "java-classic-1.21.4",
                    name: None,
                    folder: Some("Flat upgraded.furybuilder"),
                    components: target_components(),
                    target_blocks: &target,
                },
            )
            .expect("flat upgrade");
        assert_eq!(outcome.project.summary.creation_mode, CreationMode::Flat);
        assert_eq!(outcome.project.summary.target_minecraft_version, "1.21.4");
        assert_eq!(outcome.report.folder, "Flat upgraded.furybuilder");
        assert!(outcome.report.source_unchanged);
    }
}
