//! Read-only analysis and editable block import for supported classic Java worlds.
//!
//! This is the format layer behind the `Import World` project mode. It never writes to
//! the selected world:
//!
//! - [`analyze_java_world`] validates `level.dat`, the layout, every region table and every
//!   allocated chunk, then returns counts and explicit blockers;
//! - [`load_java_world`] decodes block states and 4x4x4 biome cells into the sparse
//!   `world_domain::World` used by the editor, under hard chunk/section/block limits.
//!
//! Block entities, entities, ticks, heightmaps, lighting and unknown tags are not
//! represented in the editable world. They stay in the byte-preserving source copy made by
//! [`crate::passthrough`], and the analysis reports them as limitations instead of dropping
//! them silently.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use world_domain::{ChunkPos, DimensionKind, World};

use crate::passthrough::PassthroughError;
use crate::support_matrix::{support_matrix, SupportMatrix};
use crate::version_support;
use crate::world_codec::world_codec;
use crate::world_selection::{classify_world_selection, WorldSelectionKind};
use crate::world_source::{ArchiveReport, WorldSourceKind};

use super::reader::{AnvilError, JavaWorld, LevelDat};

/// Maximum allocated source chunks that can be decoded into one editable project.
pub const MAX_IMPORT_CHUNKS: usize = 20_000;
/// Maximum non-air blocks accepted into the editable project snapshot.
pub const MAX_IMPORT_NON_AIR_BLOCKS: usize = 50_000_000;
/// Maximum non-empty sections accepted into the editable project snapshot.
pub const MAX_IMPORT_SECTIONS: usize = 2_000_000;
/// Layout identifier used by the supported classic Anvil reader.
pub const SUPPORTED_LAYOUT: &str = crate::world_selection::CLASSIC_ANVIL_LAYOUT;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportDimensionAnalysis {
    pub id: String,
    pub kind: DimensionKind,
    pub min_y: i32,
    pub height: i32,
    pub region_files: usize,
    pub chunks: usize,
}

/// Bounded, read-only report for a selected world folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaWorldAnalysis {
    /// The path the user selected (a folder or a `.zip` archive).
    pub root: String,
    pub layout: String,
    pub selection_kind: String,
    pub complete_save: bool,
    pub supported: bool,
    pub data_version: Option<i32>,
    pub version_id: Option<i32>,
    pub version_name: Option<String>,
    pub level_name: Option<String>,
    pub declared_dimensions: Vec<String>,
    pub dimensions: Vec<ImportDimensionAnalysis>,
    pub regions: usize,
    pub chunks: usize,
    pub block_entities: usize,
    pub entities: usize,
    /// Total size of the selected world folder or extracted archive staging folder.
    pub source_bytes: u64,
    /// The original selection path, preserved for archive sources whose
    /// analyzed folder is a temporary extraction.
    pub source_path: String,
    pub source_kind: WorldSourceKind,
    pub archive: Option<ArchiveReport>,
    pub warnings: Vec<String>,
    pub blockers: Vec<String>,
    pub support_matrix: SupportMatrix,
}

/// Counts produced while decoding an imported world into the editable model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaWorldLoadReport {
    pub minecraft_version: String,
    pub data_version: i32,
    pub chunks: usize,
    pub empty_chunks: usize,
    pub non_full_chunks: usize,
    pub sections: usize,
    pub biome_sections: usize,
    pub non_air_blocks: usize,
    pub block_entities: usize,
    pub entities: usize,
    pub dimensions: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldImportOptions {
    pub max_chunks: usize,
    pub max_sections: usize,
    pub max_non_air_blocks: usize,
}

impl Default for WorldImportOptions {
    fn default() -> Self {
        Self {
            max_chunks: MAX_IMPORT_CHUNKS,
            max_sections: MAX_IMPORT_SECTIONS,
            max_non_air_blocks: MAX_IMPORT_NON_AIR_BLOCKS,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum WorldImportError {
    #[error(transparent)]
    Anvil(#[from] AnvilError),
    #[error(transparent)]
    Passthrough(#[from] PassthroughError),
    #[error(transparent)]
    World(#[from] world_domain::WorldError),
    #[error("world import limit exceeded: {0}")]
    LimitExceeded(String),
    #[error("world import verification failed: {0}")]
    VerificationFailed(String),
    #[error("world import I/O failed: {0}")]
    Io(String),
}

impl WorldImportError {
    pub fn code(&self) -> &'static str {
        match self {
            WorldImportError::Anvil(error) => error.code(),
            WorldImportError::Passthrough(error) => error.code(),
            WorldImportError::World(_) => "invalid_argument",
            WorldImportError::LimitExceeded(_) => "limit_exceeded",
            WorldImportError::VerificationFailed(_) => "verification_failed",
            WorldImportError::Io(_) => "io_error",
        }
    }
}

/// Opens the selected folder read-only and reports what the importer can and cannot use.
///
/// Unsupported versions, layouts and malformed chunks are returned as blockers instead of
/// an exception so the wizard can show why creation is disabled. The function never
/// modifies the world.
pub fn analyze_java_world(root: impl AsRef<Path>) -> JavaWorldAnalysis {
    world_codec().inspect(root.as_ref())
}

pub(super) fn inspect_root(root: &Path) -> JavaWorldAnalysis {
    let selection = classify_world_selection(root);
    let mut analysis = JavaWorldAnalysis {
        root: root.display().to_string(),
        layout: selection.layout.clone(),
        selection_kind: selection.kind.as_str().to_owned(),
        complete_save: selection.complete_save,
        supported: false,
        data_version: selection.data_version,
        version_id: None,
        version_name: selection.version_name.clone(),
        level_name: selection.level_name.clone(),
        declared_dimensions: Vec::new(),
        dimensions: Vec::new(),
        regions: 0,
        chunks: 0,
        block_entities: 0,
        entities: 0,
        source_bytes: source_size(root),
        source_path: root.display().to_string(),
        source_kind: WorldSourceKind::Folder,
        archive: None,
        warnings: selection.warnings.clone(),
        blockers: Vec::new(),
        support_matrix: support_matrix(),
    };
    for blocker in &selection.blockers {
        push_unique(&mut analysis.blockers, blocker.clone());
    }

    // A region file, a region directory or an unknown folder is not a complete save.
    // Stop before trying to decode `level.dat` inside a file path.
    if !matches!(selection.kind, WorldSelectionKind::WorldFolder) {
        finish_analysis(&mut analysis);
        return analysis;
    }

    let level = LevelDat::read(root.join("level.dat"));
    match &level {
        Ok(level) => {
            analysis.data_version = Some(level.data_version);
            analysis.version_id = Some(level.version_id);
            analysis.version_name = level.version_name.clone();
            analysis.level_name = level.level_name.clone();
        }
        Err(error) => push_unique(
            &mut analysis.blockers,
            format!("level.dat is missing or unreadable: {error}"),
        ),
    }

    match JavaWorld::open(root) {
        Ok(world) => {
            analysis.layout = SUPPORTED_LAYOUT.to_owned();
            analysis.declared_dimensions = world.declared_dimensions.clone();
            if let Err(error) = inspect_world(&world, &mut analysis) {
                analysis.blockers.push(error.to_string());
            }
        }
        Err(error) => {
            if analysis.layout == "mixed" {
                push_unique(
                    &mut analysis.blockers,
                    "the world mixes classic region/ files with 26.1+ split layout markers"
                        .to_owned(),
                );
            } else if analysis.layout == "split_26_1_plus" {
                push_unique(
                    &mut analysis.blockers,
                    "26.1+ split world layout is not supported".to_owned(),
                );
            }
            analysis.blockers.push(match &error {
                AnvilError::UnsupportedDataVersion { found, supported } => format!(
                    "DataVersion {found} is not supported; this build edits DataVersions {supported:?}"
                ),
                other => other.to_string(),
            });
        }
    }

    if analysis.chunks > MAX_IMPORT_CHUNKS {
        analysis.blockers.push(format!(
            "the world has {} allocated chunks; the editable import limit is {MAX_IMPORT_CHUNKS}",
            analysis.chunks
        ));
    }
    if analysis.chunks == 0 && analysis.blockers.is_empty() {
        analysis
            .warnings
            .push("the world contains no allocated chunks".to_owned());
    }

    finish_analysis(&mut analysis);
    analysis
}

fn push_unique(blockers: &mut Vec<String>, blocker: String) {
    if !blockers.contains(&blocker) {
        blockers.push(blocker);
    }
}

/// Sums regular files below a world folder without following symlinks. This is a size hint
/// for the Review step, not a validation pass; unreadable entries are skipped.
fn source_size(path: &Path) -> u64 {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return 0;
    };
    if metadata.is_file() {
        return metadata.len();
    }
    if !metadata.is_dir() {
        return 0;
    }
    let mut total = 0u64;
    let mut stack = vec![path.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                stack.push(entry.path());
            } else if file_type.is_file() {
                if let Ok(metadata) = entry.metadata() {
                    total = total.saturating_add(metadata.len());
                }
            }
        }
    }
    total
}

fn finish_analysis(analysis: &mut JavaWorldAnalysis) {
    analysis.warnings.push(
        "the selected world is opened read-only and is never modified; importing copies it byte for byte into the project source layer"
            .to_owned(),
    );
    analysis.warnings.push(
        "block states and 4x4x4 biome cells become editable; block entities, entities, ticks, heightmaps, lighting, POI and unknown tags stay in the immutable source copy"
            .to_owned(),
    );
    if analysis.block_entities > 0 || analysis.entities > 0 {
        analysis.warnings.push(format!(
            "{} block entit{} and {} entit{} found; they remain unchanged in the source copy",
            analysis.block_entities,
            if analysis.block_entities == 1 {
                "y"
            } else {
                "ies"
            },
            analysis.entities,
            if analysis.entities == 1 { "y" } else { "ies" },
        ));
    }
    analysis.warnings.push(format!(
        "editable import limits: {MAX_IMPORT_CHUNKS} chunks, {MAX_IMPORT_SECTIONS} sections, {MAX_IMPORT_NON_AIR_BLOCKS} non-air blocks"
    ));
    analysis.supported = analysis.blockers.is_empty()
        && analysis.complete_save
        && analysis.layout == SUPPORTED_LAYOUT;
}

fn inspect_world(world: &JavaWorld, analysis: &mut JavaWorldAnalysis) -> Result<(), AnvilError> {
    for dimension in &world.dimensions {
        let coordinates = world.region_coordinates(&dimension.id)?;
        let mut dimension_chunks = 0usize;
        for (region_x, region_z) in &coordinates {
            let region = world.read_region(&dimension.id, *region_x, *region_z)?;
            dimension_chunks += region.entries().len();
            analysis.chunks += region.entries().len();
            if analysis.chunks > MAX_IMPORT_CHUNKS {
                analysis.blockers.push(format!(
                    "the world has more than {MAX_IMPORT_CHUNKS} allocated chunks"
                ));
                return Ok(());
            }
            for summary in region.summarize_all()? {
                analysis.block_entities += summary.block_entities;
                analysis.entities += summary.entities;
            }
        }
        analysis.regions += coordinates.len();
        analysis.dimensions.push(ImportDimensionAnalysis {
            id: dimension.id.clone(),
            kind: dimension.kind,
            min_y: dimension.min_y,
            height: dimension.height,
            region_files: coordinates.len(),
            chunks: dimension_chunks,
        });
    }
    Ok(())
}

/// Decodes every allocated chunk into the editable sparse world.
///
/// The caller must first check [`analyze_java_world`]; this function repeats the version
/// and layout checks and enforces its own hard limits. It leaves the world revision at the
/// decoded value (zero) for the project creator to bump.
pub fn load_java_world(
    root: impl AsRef<Path>,
) -> Result<(World, JavaWorldLoadReport), WorldImportError> {
    load_java_world_with_options(root, WorldImportOptions::default())
}

pub fn load_java_world_with_options(
    root: impl AsRef<Path>,
    options: WorldImportOptions,
) -> Result<(World, JavaWorldLoadReport), WorldImportError> {
    world_codec().load_world(root.as_ref(), options)
}

pub(super) fn load_world(
    root: &Path,
    options: WorldImportOptions,
) -> Result<(World, JavaWorldLoadReport), WorldImportError> {
    let world = JavaWorld::open(root)?;
    let mut report = JavaWorldLoadReport {
        minecraft_version: world.profile.minecraft_version.to_string(),
        data_version: world.profile.data_version,
        chunks: 0,
        empty_chunks: 0,
        non_full_chunks: 0,
        sections: 0,
        biome_sections: 0,
        non_air_blocks: 0,
        block_entities: 0,
        entities: 0,
        dimensions: Vec::new(),
        warnings: Vec::new(),
    };
    let mut editable = World::new_empty();
    editable.set_version(version_support::world_version(&world.profile));

    for dimension in &world.dimensions {
        report.dimensions.push(dimension.id.clone());
        for (region_x, region_z) in world.region_coordinates(&dimension.id)? {
            let region = world.read_region(&dimension.id, region_x, region_z)?;
            for entry in region.entries() {
                if report.chunks >= options.max_chunks {
                    return Err(WorldImportError::LimitExceeded(format!(
                        "more than {} chunks",
                        options.max_chunks
                    )));
                }
                let chunk = region.read_chunk(entry.x, entry.z)?;
                let decoded = chunk.decode(dimension)?;
                report.chunks += 1;
                report.block_entities += decoded.block_entities;
                report.entities += decoded.entities;
                if decoded.status.as_deref() != Some("minecraft:full") {
                    report.non_full_chunks += 1;
                }
                if decoded
                    .sections
                    .values()
                    .all(|section| section.blocks.is_empty())
                {
                    report.empty_chunks += 1;
                }
                let chunk_pos = ChunkPos::new(decoded.x, decoded.z);
                for (section_y, section) in decoded.sections {
                    report.sections = report.sections.checked_add(1).ok_or_else(|| {
                        WorldImportError::LimitExceeded("section counter overflow".to_owned())
                    })?;
                    if report.sections > options.max_sections {
                        return Err(WorldImportError::LimitExceeded(format!(
                            "more than {} sections",
                            options.max_sections
                        )));
                    }
                    report.non_air_blocks = report
                        .non_air_blocks
                        .checked_add(section.blocks.non_air_count())
                        .ok_or_else(|| {
                            WorldImportError::LimitExceeded("block counter overflow".to_owned())
                        })?;
                    if report.non_air_blocks > options.max_non_air_blocks {
                        return Err(WorldImportError::LimitExceeded(format!(
                            "more than {} non-air blocks",
                            options.max_non_air_blocks
                        )));
                    }
                    let biomes = section.biomes.to_domain().ok_or_else(|| {
                        WorldImportError::VerificationFailed(format!(
                            "section {section_y} in chunk ({}, {}) has biome data that cannot be represented",
                            decoded.x, decoded.z
                        ))
                    })?;
                    report.biome_sections += 1;
                    let mut blocks = section.blocks;
                    blocks.set_biomes(Some(biomes));
                    editable.install_section(&dimension.id, chunk_pos, section_y, Some(blocks))?;
                }
            }
        }
    }

    if report.non_full_chunks > 0 {
        report.warnings.push(format!(
            "{} chunk(s) do not have status minecraft:full; their stored blocks are imported as-is",
            report.non_full_chunks
        ));
    }
    report.warnings.push(
        "block entities, entities, ticks, heightmaps, lighting, POI and unknown tags are preserved only in the immutable source copy; biome cells are editable"
            .to_owned(),
    );
    Ok((editable, report))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_world_reports_a_blocker_instead_of_panicking() {
        let analysis = analyze_java_world(std::env::temp_dir().join("furybuilder-missing-world"));
        assert!(!analysis.supported);
        assert!(!analysis.blockers.is_empty());
    }
}
