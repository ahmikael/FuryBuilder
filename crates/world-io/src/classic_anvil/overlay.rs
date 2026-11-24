//! Copy-on-write overlay for edits on top of an imported classic Java world.
//!
//! The source world is opened read-only and never modified. Edits are kept as complete
//! replacement sections per chunk, so a read either returns an untouched source section or
//! an explicitly edited snapshot. No block change is written into a `.mca` file here.
//!
//! The overlay also materialises a patched chunk NBT in memory. That path:
//!
//! - replaces only `sections[].block_states` for the edited sections;
//! - preserves every other chunk tag, every other section tag (including biomes) and
//!   every block entity/entity entry;
//! - re-decodes the patched NBT with the same strict decoder used for import;
//! - compares every non-block-state field with the source before returning it.
//!
//! Persistence is a small, explicit JSON document with a schema version, the source
//! `DataVersion`, palette strings and packed canonical section words as hex strings. The
//! hexadecimal form keeps 64-bit values exact in JavaScript consumers.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use world_domain::{
    BiomeSection as DomainBiomeSection, BiomeState, BlockState, ChunkPos, Section, BIOME_CELLS,
    CHUNK_SIZE, MAX_BIOME_PALETTE, SECTION_SIZE,
};

use crate::nbt::{NbtFile, NbtValue, TAG_COMPOUND, TAG_STRING};
use crate::world_codec::world_codec;

use super::chunk::{
    BiomeSection as JavaBiomeSection, DecodedChunk, DecodedSection, MAX_BLOCK_PALETTE,
};
use super::format::{ClassicChunkFormat, PaletteLayout};
use super::reader::{
    AnvilError, JavaWorld, JavaWorldDimension, RegionFile, ANVIL_REGION_AXIS_CHUNKS,
};

/// Overlay schema 2 adds editable biome cells. Schema 1 documents remain readable.
pub const OVERLAY_SCHEMA_VERSION: u32 = 2;
pub const OVERLAY_MIN_SCHEMA_VERSION: u32 = 1;
pub const MAX_OVERLAY_CHUNKS: usize = 100_000;
pub const MAX_OVERLAY_SECTIONS: usize = 1_000_000;
pub const MAX_OVERLAY_JSON_BYTES: usize = 64 * 1024 * 1024;

/// Identifies one chunk in one dimension.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChunkKey {
    pub dimension: String,
    pub x: i32,
    pub z: i32,
}

impl ChunkKey {
    pub fn new(dimension: impl Into<String>, x: i32, z: i32) -> Self {
        Self {
            dimension: dimension.into(),
            x,
            z,
        }
    }
}

/// One requested block write in an overlay batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockEdit {
    pub dimension: String,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub state: BlockState,
}

impl BlockEdit {
    pub fn new(dimension: impl Into<String>, x: i32, y: i32, z: i32, state: BlockState) -> Self {
        Self {
            dimension: dimension.into(),
            x,
            y,
            z,
            state,
        }
    }
}

/// One requested biome-cell write in an overlay batch. The position must be a block
/// coordinate; the 4x4x4 cell covering it is updated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BiomeEdit {
    pub dimension: String,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub biome: BiomeState,
}

impl BiomeEdit {
    pub fn new(dimension: impl Into<String>, x: i32, y: i32, z: i32, biome: BiomeState) -> Self {
        Self {
            dimension: dimension.into(),
            x,
            y,
            z,
            biome,
        }
    }
}

/// Result of a single `set_biome` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BiomeChange {
    pub previous: BiomeState,
    pub current: BiomeState,
    pub changed: bool,
}

/// Result of a single `set_block` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockChange {
    pub previous: BlockState,
    pub current: BlockState,
    pub changed: bool,
}

/// Result of an atomic overlay batch.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ApplyReport {
    pub requested: usize,
    /// Number of writes that actually changed a block.
    pub changed: usize,
    /// Number of dirty chunks after the batch.
    pub chunks: usize,
    /// Number of dirty sections after the batch.
    pub sections: usize,
}

/// A merged view of a source chunk and its overlay sections.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayChunk {
    pub chunk: DecodedChunk,
    pub dirty_sections: BTreeSet<i32>,
    pub changes: usize,
    pub overridden: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct ChunkEdits {
    sections: BTreeMap<i32, Section>,
    biomes: BTreeMap<i32, DomainBiomeSection>,
    changes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum OverlayError {
    #[error(transparent)]
    Anvil(#[from] AnvilError),
    #[error("unknown dimension: {0}")]
    UnknownDimension(String),
    #[error("block Y {y} is outside dimension `{dimension}` range {min_y}..={max_y}")]
    HeightOutOfRange {
        dimension: String,
        y: i32,
        min_y: i32,
        max_y: i32,
    },
    #[error("chunk ({x}, {z}) is not present in the source world")]
    ChunkNotPresent { x: i32, z: i32 },
    #[error(
        "section Y {y} is not present in source chunk ({x}, {z}); creating sections requires metadata generation"
    )]
    SectionNotPresent { x: i32, z: i32, y: i32 },
    #[error("duplicate edit for block ({x}, {y}, {z}) in dimension `{dimension}`")]
    DuplicateEdit {
        dimension: String,
        x: i32,
        y: i32,
        z: i32,
    },
    #[error("duplicate biome edit for cell ({x}, {y}, {z}) in dimension `{dimension}`")]
    DuplicateBiomeEdit {
        dimension: String,
        x: i32,
        y: i32,
        z: i32,
    },
    #[error("section Y {y} in chunk ({x}, {z}) has no biome data to edit")]
    BiomeNotPresent { x: i32, z: i32, y: i32 },
    #[error("biome palette is full in section Y {y} of chunk ({x}, {z})")]
    BiomePaletteFull { x: i32, z: i32, y: i32 },
    #[error("section palette is full at block ({x}, {y}, {z}) in dimension `{dimension}`")]
    PaletteFull {
        dimension: String,
        x: i32,
        y: i32,
        z: i32,
    },
    #[error("overlay DataVersion {found} does not match the world; expected {supported:?}")]
    UnsupportedDataVersion { found: i32, supported: Vec<i32> },
    #[error("overlay schema version {found} is not supported; supported is {supported}")]
    UnsupportedSchema { found: u32, supported: u32 },
    #[error("overlay document is invalid: {0}")]
    InvalidOverlay(String),
    #[error("overlay limit exceeded: {0}")]
    LimitExceeded(String),
    #[error("overlay verification failed: {0}")]
    VerificationFailed(String),
    #[error("overlay I/O failed: {0}")]
    Io(String),
}

impl OverlayError {
    pub fn code(&self) -> &'static str {
        match self {
            OverlayError::Anvil(error) => error.code(),
            OverlayError::UnknownDimension(_)
            | OverlayError::HeightOutOfRange { .. }
            | OverlayError::DuplicateEdit { .. }
            | OverlayError::DuplicateBiomeEdit { .. } => "invalid_argument",
            OverlayError::ChunkNotPresent { .. } | OverlayError::BiomeNotPresent { .. } => {
                "not_found"
            }
            OverlayError::SectionNotPresent { .. }
            | OverlayError::InvalidOverlay(_)
            | OverlayError::VerificationFailed(_) => "unsupported_format",
            OverlayError::PaletteFull { .. }
            | OverlayError::BiomePaletteFull { .. }
            | OverlayError::LimitExceeded(_) => "limit_exceeded",
            OverlayError::UnsupportedDataVersion { .. }
            | OverlayError::UnsupportedSchema { .. } => "unsupported_version",
            OverlayError::Io(_) => "io_error",
        }
    }
}

/// Edits over an immutable imported world.
#[derive(Debug, Clone)]
pub struct WorldOverlay {
    world: JavaWorld,
    chunks: BTreeMap<ChunkKey, ChunkEdits>,
    change_count: usize,
}

impl WorldOverlay {
    /// Opens a supported world read-only and starts with no edits.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, OverlayError> {
        world_codec().load_overlay(root.as_ref())
    }

    pub(super) fn open_classic(root: &Path) -> Result<Self, OverlayError> {
        Ok(Self {
            world: JavaWorld::open(root)?,
            chunks: BTreeMap::new(),
            change_count: 0,
        })
    }

    pub fn world(&self) -> &JavaWorld {
        &self.world
    }

    pub fn change_count(&self) -> usize {
        self.change_count
    }

    pub fn dirty_chunks(&self) -> impl Iterator<Item = &ChunkKey> {
        self.chunks.keys()
    }

    pub fn dirty_section_count(&self) -> usize {
        self.chunks.values().map(dirty_section_count).sum()
    }

    pub fn is_dirty(&self) -> bool {
        self.change_count > 0
    }

    /// Returns the effective state of one block, reading source data when it is not dirty.
    pub fn get_block(
        &self,
        dimension_id: &str,
        x: i32,
        y: i32,
        z: i32,
    ) -> Result<BlockState, OverlayError> {
        self.validate_position(dimension_id, x, y, z)?;
        let key = chunk_key(dimension_id, x, z);
        let section_y = y.div_euclid(SECTION_SIZE);
        let local_y = y.rem_euclid(SECTION_SIZE) as u8;
        let local_x = x.rem_euclid(CHUNK_SIZE) as u8;
        let local_z = z.rem_euclid(CHUNK_SIZE) as u8;
        if let Some(edits) = self.chunks.get(&key) {
            if let Some(section) = edits.sections.get(&section_y) {
                return Ok(section
                    .get(local_x, local_y, local_z)
                    .cloned()
                    .unwrap_or_default());
            }
        }
        let chunk = self.source_decoded(dimension_id, key.x, key.z)?;
        Ok(chunk
            .block_at(local_x, y, local_z)
            .cloned()
            .unwrap_or_default())
    }

    /// Sets one block and reports the previous and effective state.
    pub fn set_block(
        &mut self,
        dimension_id: &str,
        x: i32,
        y: i32,
        z: i32,
        state: &BlockState,
    ) -> Result<BlockChange, OverlayError> {
        let previous = self.get_block(dimension_id, x, y, z)?;
        let report = self.apply(&[BlockEdit::new(
            dimension_id.to_owned(),
            x,
            y,
            z,
            state.clone(),
        )])?;
        Ok(BlockChange {
            previous,
            current: state.clone(),
            changed: report.changed == 1,
        })
    }

    /// Returns the effective biome covering one block position.
    pub fn get_biome(
        &self,
        dimension_id: &str,
        x: i32,
        y: i32,
        z: i32,
    ) -> Result<BiomeState, OverlayError> {
        self.validate_position(dimension_id, x, y, z)?;
        let key = chunk_key(dimension_id, x, z);
        let section_y = y.div_euclid(SECTION_SIZE);
        let local_y = y.rem_euclid(SECTION_SIZE) as u8;
        let local_x = x.rem_euclid(CHUNK_SIZE) as u8;
        let local_z = z.rem_euclid(CHUNK_SIZE) as u8;
        if let Some(edits) = self.chunks.get(&key) {
            if let Some(section) = edits.biomes.get(&section_y) {
                return section
                    .get(local_x / 4, local_y / 4, local_z / 4)
                    .cloned()
                    .ok_or_else(|| {
                        OverlayError::InvalidOverlay(format!(
                            "edited biome section {section_y} in chunk ({}, {}) is missing its cell ({}, {}, {})",
                            key.x,
                            key.z,
                            local_x / 4,
                            local_y / 4,
                            local_z / 4
                        ))
                    });
            }
        }
        let chunk = self.source_decoded(dimension_id, key.x, key.z)?;
        let name = chunk
            .biome_at(local_x, y, local_z)
            .ok_or(OverlayError::BiomeNotPresent {
                x: key.x,
                z: key.z,
                y: section_y,
            })?;
        name.parse::<BiomeState>()
            .map_err(|error| OverlayError::VerificationFailed(error.to_string()))
    }

    /// Sets the biome cell covering one block and reports the previous and effective biome.
    pub fn set_biome(
        &mut self,
        dimension_id: &str,
        x: i32,
        y: i32,
        z: i32,
        biome: &BiomeState,
    ) -> Result<BiomeChange, OverlayError> {
        let previous = self.get_biome(dimension_id, x, y, z)?;
        let report = self.apply_biomes(&[BiomeEdit::new(
            dimension_id.to_owned(),
            x,
            y,
            z,
            biome.clone(),
        )])?;
        Ok(BiomeChange {
            previous,
            current: biome.clone(),
            changed: report.changed > 0,
        })
    }

    /// Applies a biome-cell batch atomically. Duplicate cells and missing source biome data
    /// are refused before any overlay state is committed.
    pub fn apply_biomes(&mut self, edits: &[BiomeEdit]) -> Result<ApplyReport, OverlayError> {
        if edits.is_empty() {
            return Ok(ApplyReport {
                requested: 0,
                changed: 0,
                chunks: self.chunks.len(),
                sections: self.dirty_section_count(),
            });
        }

        let mut cells = BTreeSet::new();
        for edit in edits {
            self.validate_position(&edit.dimension, edit.x, edit.y, edit.z)?;
            let cell = (
                edit.dimension.clone(),
                edit.x.div_euclid(4),
                edit.y.div_euclid(4),
                edit.z.div_euclid(4),
            );
            if !cells.insert(cell) {
                return Err(OverlayError::DuplicateBiomeEdit {
                    dimension: edit.dimension.clone(),
                    x: edit.x.div_euclid(4),
                    y: edit.y.div_euclid(4),
                    z: edit.z.div_euclid(4),
                });
            }
        }

        let mut grouped: BTreeMap<ChunkKey, Vec<&BiomeEdit>> = BTreeMap::new();
        for edit in edits {
            grouped
                .entry(chunk_key(&edit.dimension, edit.x, edit.z))
                .or_default()
                .push(edit);
        }

        let mut staged: BTreeMap<ChunkKey, Option<ChunkEdits>> = BTreeMap::new();
        let mut next_change_count = self.change_count;
        let mut next_chunk_count = self.chunks.len();
        let mut next_section_count = self.dirty_section_count();
        let mut report = ApplyReport {
            requested: edits.len(),
            ..ApplyReport::default()
        };

        for (key, group) in grouped {
            let source = self.source_decoded(&key.dimension, key.x, key.z)?;
            let previous = self.chunks.get(&key).cloned().unwrap_or_default();
            let previous_section_count = dirty_section_count(&previous);
            let sections = previous.sections;
            let mut biomes = previous.biomes;

            for edit in group {
                let section_y = edit.y.div_euclid(SECTION_SIZE);
                let source_section =
                    source
                        .sections
                        .get(&section_y)
                        .ok_or(OverlayError::SectionNotPresent {
                            x: key.x,
                            z: key.z,
                            y: section_y,
                        })?;
                let source_biomes =
                    source_section
                        .biomes
                        .to_domain()
                        .ok_or(OverlayError::BiomeNotPresent {
                            x: key.x,
                            z: key.z,
                            y: section_y,
                        })?;
                let section = biomes.entry(section_y).or_insert(source_biomes);
                let cell_x = (edit.x.rem_euclid(CHUNK_SIZE) as u8) / 4;
                let cell_y = (edit.y.rem_euclid(SECTION_SIZE) as u8) / 4;
                let cell_z = (edit.z.rem_euclid(CHUNK_SIZE) as u8) / 4;
                let changed = section.set(cell_x, cell_y, cell_z, &edit.biome).ok_or(
                    OverlayError::BiomePaletteFull {
                        x: key.x,
                        z: key.z,
                        y: section_y,
                    },
                )?;
                if changed {
                    report.changed += 1;
                }
            }

            let mut final_sections = BTreeMap::new();
            let mut block_changes = 0usize;
            for (section_y, section) in &sections {
                let source_section = source.sections.get(section_y).ok_or_else(|| {
                    OverlayError::InvalidOverlay(format!(
                        "overlay section {section_y} is absent from source chunk ({}, {})",
                        key.x, key.z
                    ))
                })?;
                let difference = section_difference_count(section, &source_section.blocks);
                if difference > 0 {
                    final_sections.insert(*section_y, section.clone());
                    block_changes += difference;
                }
            }
            let mut final_biomes = BTreeMap::new();
            let mut biome_changes = 0usize;
            for (section_y, section) in &biomes {
                let source_section = source.sections.get(section_y).ok_or_else(|| {
                    OverlayError::InvalidOverlay(format!(
                        "overlay biome section {section_y} is absent from source chunk ({}, {})",
                        key.x, key.z
                    ))
                })?;
                let difference = biome_difference_count(section, &source_section.biomes);
                if difference > 0 {
                    final_biomes.insert(*section_y, section.clone());
                    biome_changes += difference;
                }
            }
            let chunk_changes = block_changes + biome_changes;

            let old_changes = self
                .chunks
                .get(&key)
                .map(|edits| edits.changes)
                .unwrap_or(0);
            next_change_count = next_change_count
                .checked_sub(old_changes)
                .and_then(|value| value.checked_add(chunk_changes))
                .ok_or_else(|| {
                    OverlayError::LimitExceeded("overlay change counter overflow".to_owned())
                })?;
            next_section_count = next_section_count
                .checked_sub(previous_section_count)
                .and_then(|value| {
                    value.checked_add(dirty_section_union(&final_sections, &final_biomes))
                })
                .ok_or_else(|| {
                    OverlayError::LimitExceeded("overlay section counter overflow".to_owned())
                })?;
            let was_present = previous_section_count > 0;
            let is_present = !final_sections.is_empty() || !final_biomes.is_empty();
            next_chunk_count = next_chunk_count
                .checked_sub(usize::from(was_present))
                .and_then(|value| value.checked_add(usize::from(is_present)))
                .ok_or_else(|| {
                    OverlayError::LimitExceeded("overlay chunk counter overflow".to_owned())
                })?;
            if next_chunk_count > MAX_OVERLAY_CHUNKS {
                return Err(OverlayError::LimitExceeded(format!(
                    "overlay would contain {next_chunk_count} chunks, limit is {MAX_OVERLAY_CHUNKS}"
                )));
            }
            if next_section_count > MAX_OVERLAY_SECTIONS {
                return Err(OverlayError::LimitExceeded(format!(
                    "overlay would contain {next_section_count} sections, limit is {MAX_OVERLAY_SECTIONS}"
                )));
            }

            if is_present {
                staged.insert(
                    key,
                    Some(ChunkEdits {
                        sections: final_sections,
                        biomes: final_biomes,
                        changes: chunk_changes,
                    }),
                );
            } else {
                staged.insert(key, None);
            }
        }

        for (key, edits) in staged {
            match edits {
                Some(edits) => {
                    self.chunks.insert(key, edits);
                }
                None => {
                    self.chunks.remove(&key);
                }
            }
        }
        self.change_count = next_change_count;
        report.chunks = self.chunks.len();
        report.sections = self.dirty_section_count();
        Ok(report)
    }

    /// Applies a batch atomically against the in-memory overlay.
    ///
    /// Every edit is validated before the first section is staged, duplicate positions are
    /// refused, and no overlay state is committed when any read or validation fails.
    pub fn apply(&mut self, edits: &[BlockEdit]) -> Result<ApplyReport, OverlayError> {
        if edits.is_empty() {
            return Ok(ApplyReport {
                requested: 0,
                changed: 0,
                chunks: self.chunks.len(),
                sections: self.dirty_section_count(),
            });
        }

        let mut positions = BTreeSet::new();
        for edit in edits {
            self.validate_position(&edit.dimension, edit.x, edit.y, edit.z)?;
            if !positions.insert((edit.dimension.clone(), edit.x, edit.y, edit.z)) {
                return Err(OverlayError::DuplicateEdit {
                    dimension: edit.dimension.clone(),
                    x: edit.x,
                    y: edit.y,
                    z: edit.z,
                });
            }
        }

        let mut grouped: BTreeMap<ChunkKey, Vec<&BlockEdit>> = BTreeMap::new();
        for edit in edits {
            grouped
                .entry(chunk_key(&edit.dimension, edit.x, edit.z))
                .or_default()
                .push(edit);
        }

        let mut staged: BTreeMap<ChunkKey, Option<ChunkEdits>> = BTreeMap::new();
        let mut next_change_count = self.change_count;
        let mut next_chunk_count = self.chunks.len();
        let mut next_section_count = self.dirty_section_count();
        let mut report = ApplyReport {
            requested: edits.len(),
            ..ApplyReport::default()
        };

        for (key, group) in grouped {
            let source = self.source_decoded(&key.dimension, key.x, key.z)?;
            let previous = self.chunks.get(&key).cloned().unwrap_or_default();
            let previous_section_count = dirty_section_count(&previous);
            let mut sections = previous.sections;
            let biomes = previous.biomes;

            for edit in group {
                let local_x = edit.x.rem_euclid(CHUNK_SIZE) as u8;
                let local_z = edit.z.rem_euclid(CHUNK_SIZE) as u8;
                let section_y = edit.y.div_euclid(SECTION_SIZE);
                let local_y = edit.y.rem_euclid(SECTION_SIZE) as u8;
                let source_section =
                    source
                        .sections
                        .get(&section_y)
                        .ok_or(OverlayError::SectionNotPresent {
                            x: key.x,
                            z: key.z,
                            y: section_y,
                        })?;
                let section = sections
                    .entry(section_y)
                    .or_insert_with(|| source_section.blocks.clone());
                let changed = section.set(local_x, local_y, local_z, &edit.state).ok_or(
                    OverlayError::PaletteFull {
                        dimension: edit.dimension.clone(),
                        x: edit.x,
                        y: edit.y,
                        z: edit.z,
                    },
                )?;
                if changed {
                    report.changed += 1;
                }
            }

            let mut final_sections = BTreeMap::new();
            let mut chunk_changes = 0usize;
            for (section_y, section) in &sections {
                let source_section = source.sections.get(section_y).ok_or_else(|| {
                    OverlayError::InvalidOverlay(format!(
                        "overlay section {section_y} is absent from source chunk ({}, {})",
                        key.x, key.z
                    ))
                })?;
                let difference = section_difference_count(section, &source_section.blocks);
                if difference > 0 {
                    final_sections.insert(*section_y, section.clone());
                    chunk_changes += difference;
                }
            }
            let mut final_biomes = BTreeMap::new();
            for (section_y, section) in &biomes {
                let source_section = source.sections.get(section_y).ok_or_else(|| {
                    OverlayError::InvalidOverlay(format!(
                        "overlay biome section {section_y} is absent from source chunk ({}, {})",
                        key.x, key.z
                    ))
                })?;
                let difference = biome_difference_count(section, &source_section.biomes);
                if difference > 0 {
                    final_biomes.insert(*section_y, section.clone());
                    chunk_changes += difference;
                }
            }

            let old_changes = self
                .chunks
                .get(&key)
                .map(|edits| edits.changes)
                .unwrap_or(0);
            next_change_count = next_change_count
                .checked_sub(old_changes)
                .and_then(|value| value.checked_add(chunk_changes))
                .ok_or_else(|| {
                    OverlayError::LimitExceeded("overlay change counter overflow".to_owned())
                })?;

            next_section_count = next_section_count
                .checked_sub(previous_section_count)
                .and_then(|value| {
                    value.checked_add(dirty_section_union(&final_sections, &final_biomes))
                })
                .ok_or_else(|| {
                    OverlayError::LimitExceeded("overlay section counter overflow".to_owned())
                })?;
            let was_present = previous_section_count > 0;
            let is_present = !final_sections.is_empty() || !final_biomes.is_empty();
            next_chunk_count = next_chunk_count
                .checked_sub(usize::from(was_present))
                .and_then(|value| value.checked_add(usize::from(is_present)))
                .ok_or_else(|| {
                    OverlayError::LimitExceeded("overlay chunk counter overflow".to_owned())
                })?;
            if next_chunk_count > MAX_OVERLAY_CHUNKS {
                return Err(OverlayError::LimitExceeded(format!(
                    "overlay would contain {next_chunk_count} chunks, limit is {MAX_OVERLAY_CHUNKS}"
                )));
            }
            if next_section_count > MAX_OVERLAY_SECTIONS {
                return Err(OverlayError::LimitExceeded(format!(
                    "overlay would contain {next_section_count} sections, limit is {MAX_OVERLAY_SECTIONS}"
                )));
            }

            if is_present {
                staged.insert(
                    key,
                    Some(ChunkEdits {
                        sections: final_sections,
                        biomes: final_biomes,
                        changes: chunk_changes,
                    }),
                );
            } else {
                staged.insert(key, None);
            }
        }

        for (key, edits) in staged {
            match edits {
                Some(edits) => {
                    self.chunks.insert(key, edits);
                }
                None => {
                    self.chunks.remove(&key);
                }
            }
        }
        self.change_count = next_change_count;
        report.chunks = self.chunks.len();
        report.sections = self.dirty_section_count();
        Ok(report)
    }

    /// Reads a merged chunk. Source biomes, block-entity counts and entity counts are kept.
    pub fn read_chunk(
        &self,
        dimension_id: &str,
        x: i32,
        z: i32,
    ) -> Result<OverlayChunk, OverlayError> {
        self.validate_chunk_position(dimension_id, x, z)?;
        let key = ChunkKey::new(dimension_id.to_owned(), x, z);
        let mut chunk = self.source_decoded(dimension_id, x, z)?;
        let mut dirty_sections = BTreeSet::new();
        let mut changes = 0usize;
        if let Some(edits) = self.chunks.get(&key) {
            for (section_y, section) in &edits.sections {
                let source_section = chunk.sections.get(section_y).ok_or_else(|| {
                    OverlayError::InvalidOverlay(format!(
                        "overlay section {section_y} is absent from source chunk ({x}, {z})"
                    ))
                })?;
                chunk.sections.insert(
                    *section_y,
                    DecodedSection {
                        y: *section_y,
                        blocks: section.clone(),
                        biomes: source_section.biomes.clone(),
                    },
                );
                dirty_sections.insert(*section_y);
            }
            for (section_y, section) in &edits.biomes {
                let java_biomes = JavaBiomeSection::from_domain(section).ok_or_else(|| {
                    OverlayError::InvalidOverlay(format!(
                        "overlay biome section {section_y} cannot be converted for chunk ({x}, {z})"
                    ))
                })?;
                let entry = chunk.sections.get_mut(section_y).ok_or_else(|| {
                    OverlayError::InvalidOverlay(format!(
                        "overlay biome section {section_y} is absent from source chunk ({x}, {z})"
                    ))
                })?;
                entry.biomes = java_biomes;
                dirty_sections.insert(*section_y);
            }
            changes = edits.changes;
        }
        Ok(OverlayChunk {
            chunk,
            dirty_sections,
            changes,
            overridden: changes > 0,
        })
    }

    /// Drops all edits for one chunk and returns whether anything was removed.
    pub fn clear_chunk(&mut self, dimension_id: &str, x: i32, z: i32) -> bool {
        let key = ChunkKey::new(dimension_id.to_owned(), x, z);
        match self.chunks.remove(&key) {
            Some(edits) => {
                self.change_count -= edits.changes;
                true
            }
            None => false,
        }
    }

    /// Drops every edit while leaving the source world untouched.
    pub fn clear(&mut self) {
        self.chunks.clear();
        self.change_count = 0;
    }

    /// Re-encodes one dirty chunk to NBT without writing it to disk.
    ///
    /// `None` means the chunk has no overlay edits and should be copied through unchanged.
    /// The returned NBT is verified by re-decoding it and by comparing every non-block-state
    /// field with the source chunk.
    pub fn materialize_chunk_nbt(
        &self,
        dimension_id: &str,
        x: i32,
        z: i32,
    ) -> Result<Option<NbtFile>, OverlayError> {
        world_codec().encode_chunk(self, dimension_id, ChunkPos::new(x, z))
    }

    pub(super) fn encode_chunk(
        &self,
        dimension_id: &str,
        x: i32,
        z: i32,
    ) -> Result<Option<NbtFile>, OverlayError> {
        let key = ChunkKey::new(dimension_id.to_owned(), x, z);
        let Some(edits) = self.chunks.get(&key) else {
            return Ok(None);
        };
        let dimension = self
            .world
            .dimension(dimension_id)
            .ok_or_else(|| OverlayError::UnknownDimension(dimension_id.to_owned()))?;
        let region = self.region(dimension_id, x, z)?;
        let source = region.read_chunk(x, z).map_err(map_missing_chunk)?;
        let source_decoded = source.decode(dimension)?;
        let format = ClassicChunkFormat::for_read(source_decoded.data_version)?;
        let mut patched = source.nbt.clone();

        {
            let root = match &mut patched.root {
                NbtValue::Compound(map) => map,
                _ => {
                    return Err(OverlayError::VerificationFailed(
                        "patched chunk root is not a compound".to_owned(),
                    ))
                }
            };
            let sections_value = root.get_mut(format.sections).ok_or_else(|| {
                OverlayError::VerificationFailed("patched chunk has no sections list".to_owned())
            })?;
            let items = match sections_value {
                NbtValue::List { kind, items } => {
                    if *kind != TAG_COMPOUND {
                        return Err(OverlayError::VerificationFailed(
                            "patched chunk sections list is not a compound list".to_owned(),
                        ));
                    }
                    items
                }
                _ => {
                    return Err(OverlayError::VerificationFailed(
                        "patched chunk sections is not a list".to_owned(),
                    ))
                }
            };
            let mut edited_sections = BTreeSet::new();
            edited_sections.extend(edits.sections.keys().copied());
            edited_sections.extend(edits.biomes.keys().copied());
            for section_y in edited_sections {
                let entry = items
                    .iter_mut()
                    .find(|item| section_y_of(item, format) == Some(section_y))
                    .ok_or_else(|| {
                        OverlayError::VerificationFailed(format!(
                            "source chunk ({x}, {z}) has no section {section_y}"
                        ))
                    })?;
                let compound = match entry {
                    NbtValue::Compound(map) => map,
                    _ => {
                        return Err(OverlayError::VerificationFailed(
                            "section is not a compound".to_owned(),
                        ))
                    }
                };
                if let Some(section) = edits.sections.get(&section_y) {
                    compound.insert(
                        format.blocks.container.to_owned(),
                        encode_block_states(section, &format.blocks),
                    );
                }
                if let Some(section) = edits.biomes.get(&section_y) {
                    compound.insert(
                        format.biomes.container.to_owned(),
                        encode_biomes(section, &format.biomes),
                    );
                }
            }
        }

        verify_preserved_fields(&source.nbt, &patched, format)?;
        verify_edit_scope(
            &source.nbt,
            &patched,
            edits.sections.keys().copied().collect(),
            edits.biomes.keys().copied().collect(),
            format,
        )?;

        let decoded = self.world.decode_chunk_nbt(dimension_id, x, z, &patched)?;
        for (section_y, section) in &edits.sections {
            let decoded_section = decoded.sections.get(section_y).ok_or_else(|| {
                OverlayError::VerificationFailed(format!(
                    "re-decoded chunk lost section {section_y}"
                ))
            })?;
            if &decoded_section.blocks != section {
                return Err(OverlayError::VerificationFailed(format!(
                    "section {section_y} did not round-trip through NBT (palette {} -> {}, bits {} -> {})",
                    section.palette_len(),
                    decoded_section.blocks.palette_len(),
                    section.bits_per_index(),
                    decoded_section.blocks.bits_per_index()
                )));
            }
            let source_section = source_decoded.sections.get(section_y).ok_or_else(|| {
                OverlayError::VerificationFailed(format!(
                    "source lost section {section_y} during verification"
                ))
            })?;
            let expected_biomes = match edits.biomes.get(section_y) {
                Some(edited) => JavaBiomeSection::from_domain(edited).ok_or_else(|| {
                    OverlayError::VerificationFailed(format!(
                        "edited biome section {section_y} cannot be represented"
                    ))
                })?,
                None => source_section.biomes.clone(),
            };
            if decoded_section.biomes != expected_biomes {
                return Err(OverlayError::VerificationFailed(format!(
                    "section {section_y} biomes changed without the expected biome edit"
                )));
            }
        }
        for (section_y, section) in &edits.biomes {
            let decoded_section = decoded.sections.get(section_y).ok_or_else(|| {
                OverlayError::VerificationFailed(format!(
                    "re-decoded chunk lost biome section {section_y}"
                ))
            })?;
            let decoded_biomes = decoded_section.biomes.to_domain().ok_or_else(|| {
                OverlayError::VerificationFailed(format!(
                    "re-decoded biome section {section_y} cannot be represented"
                ))
            })?;
            if &decoded_biomes != section {
                return Err(OverlayError::VerificationFailed(format!(
                    "biome section {section_y} did not round-trip through NBT"
                )));
            }
            let source_section = source_decoded.sections.get(section_y).ok_or_else(|| {
                OverlayError::VerificationFailed(format!(
                    "source lost biome section {section_y} during verification"
                ))
            })?;
            let expected_blocks = match edits.sections.get(section_y) {
                Some(edited) => edited,
                None => &source_section.blocks,
            };
            if &decoded_section.blocks != expected_blocks {
                return Err(OverlayError::VerificationFailed(format!(
                    "section {section_y} blocks changed without the expected block edit"
                )));
            }
        }
        Ok(Some(patched))
    }

    /// Serialises the current edits into a schematized document.
    pub fn document(&self) -> OverlayDocument {
        OverlayDocument {
            schema_version: OVERLAY_SCHEMA_VERSION,
            data_version: self.world.level.data_version,
            chunks: self
                .chunks
                .iter()
                .map(|(key, edits)| OverlayChunkDocument {
                    dimension: key.dimension.clone(),
                    x: key.x,
                    z: key.z,
                    sections: edits
                        .sections
                        .iter()
                        .map(|(y, section)| OverlaySectionDocument {
                            y: *y,
                            palette: section.palette().iter().map(ToString::to_string).collect(),
                            bits_per_index: section.bits_per_index(),
                            data: section
                                .packed_data()
                                .iter()
                                .map(|word| format!("{word:016x}"))
                                .collect(),
                        })
                        .collect(),
                    biomes: edits
                        .biomes
                        .iter()
                        .map(|(y, section)| OverlayBiomeSectionDocument {
                            y: *y,
                            palette: section
                                .palette()
                                .iter()
                                .map(|biome| biome.as_str().to_owned())
                                .collect(),
                            cells: section.cells().to_vec(),
                        })
                        .collect(),
                })
                .collect(),
        }
    }

    pub fn to_json_pretty(&self) -> Result<String, OverlayError> {
        let json = serde_json::to_string_pretty(&self.document()).map_err(|error| {
            OverlayError::InvalidOverlay(format!("JSON serialization: {error}"))
        })?;
        if json.len() > MAX_OVERLAY_JSON_BYTES {
            return Err(OverlayError::LimitExceeded(format!(
                "overlay JSON is {} bytes, limit is {MAX_OVERLAY_JSON_BYTES}",
                json.len()
            )));
        }
        Ok(json)
    }

    /// Rebuilds an overlay from a document, revalidating every chunk against the source.
    pub fn from_document(
        world: JavaWorld,
        document: OverlayDocument,
    ) -> Result<Self, OverlayError> {
        if !(OVERLAY_MIN_SCHEMA_VERSION..=OVERLAY_SCHEMA_VERSION).contains(&document.schema_version)
        {
            return Err(OverlayError::UnsupportedSchema {
                found: document.schema_version,
                supported: OVERLAY_SCHEMA_VERSION,
            });
        }
        if document.data_version != world.profile.data_version
            || world.level.data_version != document.data_version
        {
            return Err(OverlayError::UnsupportedDataVersion {
                found: document.data_version,
                supported: vec![world.profile.data_version],
            });
        }
        if document.chunks.len() > MAX_OVERLAY_CHUNKS {
            return Err(OverlayError::LimitExceeded(format!(
                "{} overlay chunks, limit is {MAX_OVERLAY_CHUNKS}",
                document.chunks.len()
            )));
        }

        let mut overlay = Self {
            world,
            chunks: BTreeMap::new(),
            change_count: 0,
        };
        let mut seen_chunks = BTreeSet::new();
        let mut section_count = 0usize;

        for chunk_document in document.chunks {
            let key = ChunkKey::new(chunk_document.dimension, chunk_document.x, chunk_document.z);
            if !seen_chunks.insert(key.clone()) {
                return Err(OverlayError::InvalidOverlay(format!(
                    "duplicate chunk ({}, {}) in dimension `{}`",
                    key.x, key.z, key.dimension
                )));
            }
            if chunk_document.sections.is_empty() && chunk_document.biomes.is_empty() {
                return Err(OverlayError::InvalidOverlay(format!(
                    "chunk ({}, {}) has no sections",
                    key.x, key.z
                )));
            }
            if document.schema_version < 2 && !chunk_document.biomes.is_empty() {
                return Err(OverlayError::InvalidOverlay(format!(
                    "chunk ({}, {}) has biomes in a schema {} document",
                    key.x, key.z, document.schema_version
                )));
            }
            overlay.validate_chunk_position(&key.dimension, key.x, key.z)?;
            let source = overlay.source_decoded(&key.dimension, key.x, key.z)?;
            let mut sections = BTreeMap::new();
            let mut seen_sections = BTreeSet::new();
            let mut changes = 0usize;

            for section_document in chunk_document.sections {
                section_count += 1;
                if section_count > MAX_OVERLAY_SECTIONS {
                    return Err(OverlayError::LimitExceeded(format!(
                        "overlay has more than {MAX_OVERLAY_SECTIONS} sections"
                    )));
                }
                if !seen_sections.insert(section_document.y) {
                    return Err(OverlayError::InvalidOverlay(format!(
                        "duplicate section Y {} in chunk ({}, {})",
                        section_document.y, key.x, key.z
                    )));
                }
                let source_section = source.sections.get(&section_document.y).ok_or(
                    OverlayError::SectionNotPresent {
                        x: key.x,
                        z: key.z,
                        y: section_document.y,
                    },
                )?;
                if section_document.palette.is_empty() {
                    return Err(OverlayError::InvalidOverlay(format!(
                        "section {} has an empty palette",
                        section_document.y
                    )));
                }
                if section_document.palette.len() > MAX_BLOCK_PALETTE {
                    return Err(OverlayError::LimitExceeded(format!(
                        "section {} palette has {} entries, limit is {MAX_BLOCK_PALETTE}",
                        section_document.y,
                        section_document.palette.len()
                    )));
                }
                let palette = section_document
                    .palette
                    .iter()
                    .map(|value| {
                        value.parse::<BlockState>().map_err(|error| {
                            OverlayError::InvalidOverlay(format!(
                                "invalid block state `{value}`: {error}"
                            ))
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let mut data = Vec::with_capacity(section_document.data.len());
                for word in &section_document.data {
                    if word.len() > 16 || !word.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                        return Err(OverlayError::InvalidOverlay(format!(
                            "section {} has an invalid packed word `{word}`",
                            section_document.y
                        )));
                    }
                    data.push(u64::from_str_radix(word, 16).map_err(|error| {
                        OverlayError::InvalidOverlay(format!(
                            "section {} packed word `{word}` is invalid: {error}",
                            section_document.y
                        ))
                    })?);
                }
                let section = Section::from_raw(palette, data, section_document.bits_per_index)
                    .ok_or_else(|| {
                        OverlayError::InvalidOverlay(format!(
                            "section {} palette or packed data is inconsistent",
                            section_document.y
                        ))
                    })?;
                let difference = section_difference_count(&section, &source_section.blocks);
                if difference > 0 {
                    changes += difference;
                    sections.insert(section_document.y, section);
                }
            }

            let mut biomes = BTreeMap::new();
            let mut seen_biomes = BTreeSet::new();
            for biome_document in chunk_document.biomes {
                section_count += 1;
                if section_count > MAX_OVERLAY_SECTIONS {
                    return Err(OverlayError::LimitExceeded(format!(
                        "overlay has more than {MAX_OVERLAY_SECTIONS} sections"
                    )));
                }
                if !seen_biomes.insert(biome_document.y) {
                    return Err(OverlayError::InvalidOverlay(format!(
                        "duplicate biome section Y {} in chunk ({}, {})",
                        biome_document.y, key.x, key.z
                    )));
                }
                let source_section = source.sections.get(&biome_document.y).ok_or(
                    OverlayError::SectionNotPresent {
                        x: key.x,
                        z: key.z,
                        y: biome_document.y,
                    },
                )?;
                if biome_document.palette.is_empty() {
                    return Err(OverlayError::InvalidOverlay(format!(
                        "biome section {} has an empty palette",
                        biome_document.y
                    )));
                }
                if biome_document.palette.len() > MAX_BIOME_PALETTE {
                    return Err(OverlayError::LimitExceeded(format!(
                        "biome section {} palette has {} entries, limit is {MAX_BIOME_PALETTE}",
                        biome_document.y,
                        biome_document.palette.len()
                    )));
                }
                if biome_document.cells.len() != BIOME_CELLS {
                    return Err(OverlayError::InvalidOverlay(format!(
                        "biome section {} has {} cells, expected {BIOME_CELLS}",
                        biome_document.y,
                        biome_document.cells.len()
                    )));
                }
                let palette = biome_document
                    .palette
                    .iter()
                    .map(|value| {
                        value.parse::<BiomeState>().map_err(|error| {
                            OverlayError::InvalidOverlay(format!(
                                "invalid biome state `{value}`: {error}"
                            ))
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let section = DomainBiomeSection::from_palette(palette, biome_document.cells)
                    .ok_or_else(|| {
                        OverlayError::InvalidOverlay(format!(
                            "biome section {} palette or cells are inconsistent",
                            biome_document.y
                        ))
                    })?;
                let difference = biome_difference_count(&section, &source_section.biomes);
                if difference > 0 {
                    changes += difference;
                    biomes.insert(biome_document.y, section);
                }
            }

            if sections.is_empty() && biomes.is_empty() {
                continue;
            }
            overlay.change_count += changes;
            overlay.chunks.insert(
                key,
                ChunkEdits {
                    sections,
                    biomes,
                    changes,
                },
            );
        }
        Ok(overlay)
    }

    pub fn from_json(root: impl AsRef<Path>, json: &str) -> Result<Self, OverlayError> {
        if json.len() > MAX_OVERLAY_JSON_BYTES {
            return Err(OverlayError::LimitExceeded(format!(
                "overlay JSON is {} bytes, limit is {MAX_OVERLAY_JSON_BYTES}",
                json.len()
            )));
        }
        let document: OverlayDocument = serde_json::from_str(json).map_err(|error| {
            OverlayError::InvalidOverlay(format!("overlay JSON is invalid: {error}"))
        })?;
        Self::from_document(JavaWorld::open(root)?, document)
    }

    fn validate_position(
        &self,
        dimension_id: &str,
        _x: i32,
        y: i32,
        _z: i32,
    ) -> Result<(), OverlayError> {
        let dimension = self
            .world
            .dimension(dimension_id)
            .ok_or_else(|| OverlayError::UnknownDimension(dimension_id.to_owned()))?;
        validate_height(dimension, y)
    }

    fn validate_chunk_position(
        &self,
        dimension_id: &str,
        _x: i32,
        _z: i32,
    ) -> Result<(), OverlayError> {
        self.world
            .dimension(dimension_id)
            .ok_or_else(|| OverlayError::UnknownDimension(dimension_id.to_owned()))?;
        Ok(())
    }

    fn region(&self, dimension_id: &str, x: i32, z: i32) -> Result<RegionFile, OverlayError> {
        let region_x = x.div_euclid(ANVIL_REGION_AXIS_CHUNKS);
        let region_z = z.div_euclid(ANVIL_REGION_AXIS_CHUNKS);
        self.world
            .read_region(dimension_id, region_x, region_z)
            .map_err(|error| match error {
                AnvilError::Missing(_) => OverlayError::ChunkNotPresent { x, z },
                other => OverlayError::Anvil(other),
            })
    }

    fn source_decoded(
        &self,
        dimension_id: &str,
        x: i32,
        z: i32,
    ) -> Result<DecodedChunk, OverlayError> {
        let dimension = self
            .world
            .dimension(dimension_id)
            .ok_or_else(|| OverlayError::UnknownDimension(dimension_id.to_owned()))?;
        let region = self.region(dimension_id, x, z)?;
        let chunk = region.read_chunk(x, z).map_err(map_missing_chunk)?;
        Ok(chunk.decode(dimension)?)
    }
}

fn chunk_key(dimension_id: &str, x: i32, z: i32) -> ChunkKey {
    ChunkKey::new(
        dimension_id.to_owned(),
        x.div_euclid(CHUNK_SIZE),
        z.div_euclid(CHUNK_SIZE),
    )
}

fn validate_height(dimension: &JavaWorldDimension, y: i32) -> Result<(), OverlayError> {
    let max_y = dimension
        .min_y
        .checked_add(dimension.height)
        .and_then(|value| value.checked_sub(1))
        .ok_or_else(|| {
            OverlayError::InvalidOverlay(format!("dimension `{}` height overflows", dimension.id))
        })?;
    if y < dimension.min_y || y > max_y {
        return Err(OverlayError::HeightOutOfRange {
            dimension: dimension.id.clone(),
            y,
            min_y: dimension.min_y,
            max_y,
        });
    }
    Ok(())
}

fn map_missing_chunk(error: AnvilError) -> OverlayError {
    match error {
        AnvilError::ChunkNotPresent { x, z } => OverlayError::ChunkNotPresent { x, z },
        other => OverlayError::Anvil(other),
    }
}

fn section_y_of(value: &NbtValue, format: &ClassicChunkFormat) -> Option<i32> {
    let compound = value.as_compound()?;
    match compound.get(format.section_y) {
        Some(NbtValue::Byte(y)) => Some(*y as i32),
        _ => None,
    }
}

fn dirty_section_count(edits: &ChunkEdits) -> usize {
    dirty_section_union(&edits.sections, &edits.biomes)
}

fn dirty_section_union(
    sections: &BTreeMap<i32, Section>,
    biomes: &BTreeMap<i32, DomainBiomeSection>,
) -> usize {
    sections
        .keys()
        .chain(biomes.keys())
        .collect::<BTreeSet<_>>()
        .len()
}

fn section_difference_count(a: &Section, b: &Section) -> usize {
    if a == b {
        return 0;
    }
    let mut count = 0usize;
    for y in 0..16 {
        for z in 0..16 {
            for x in 0..16 {
                if a.get(x, y, z) != b.get(x, y, z) {
                    count += 1;
                }
            }
        }
    }
    count
}

fn biome_difference_count(a: &DomainBiomeSection, b: &JavaBiomeSection) -> usize {
    let Some(b) = b.to_domain() else {
        return BIOME_CELLS;
    };
    let mut count = 0usize;
    for y in 0..4 {
        for z in 0..4 {
            for x in 0..4 {
                if a.get(x, y, z) != b.get(x, y, z) {
                    count += 1;
                }
            }
        }
    }
    count
}

fn encode_block_states(section: &Section, layout: &PaletteLayout) -> NbtValue {
    let palette = section
        .palette()
        .iter()
        .map(|state| {
            let mut compound = BTreeMap::new();
            compound.insert("Name".to_owned(), NbtValue::String(state.name().to_owned()));
            if !state.properties().is_empty() {
                compound.insert(
                    "Properties".to_owned(),
                    NbtValue::Compound(
                        state
                            .properties()
                            .iter()
                            .map(|(key, value)| (key.clone(), NbtValue::String(value.clone())))
                            .collect(),
                    ),
                );
            }
            NbtValue::Compound(compound)
        })
        .collect::<Vec<_>>();

    let mut block_states = BTreeMap::new();
    block_states.insert(
        layout.palette.to_owned(),
        NbtValue::List {
            kind: TAG_COMPOUND,
            items: palette,
        },
    );
    if section.palette_len() > 1 {
        let indices = (0..layout.count).map(|index| {
            let x = (index % 16) as u8;
            let z = ((index / 16) % 16) as u8;
            let y = ((index / 256) % 16) as u8;
            section
                .palette_index(x, y, z)
                .expect("every local section position has a palette index")
        });
        block_states.insert(
            layout.data.to_owned(),
            NbtValue::LongArray(layout.packing(section.palette_len()).pack(indices)),
        );
    }
    NbtValue::Compound(block_states)
}

fn encode_biomes(section: &DomainBiomeSection, layout: &PaletteLayout) -> NbtValue {
    let mut biomes = BTreeMap::new();
    biomes.insert(
        layout.palette.to_owned(),
        NbtValue::List {
            kind: TAG_STRING,
            items: section
                .palette()
                .iter()
                .map(|biome| NbtValue::String(biome.as_str().to_owned()))
                .collect(),
        },
    );
    if section.palette().len() > 1 {
        biomes.insert(
            layout.data.to_owned(),
            NbtValue::LongArray(
                layout
                    .packing(section.palette().len())
                    .pack(section.cells().iter().copied()),
            ),
        );
    }
    NbtValue::Compound(biomes)
}

pub(super) fn verify_preserved_fields(
    source: &NbtFile,
    patched: &NbtFile,
    format: &ClassicChunkFormat,
) -> Result<(), OverlayError> {
    if source.root_name != patched.root_name {
        return Err(OverlayError::VerificationFailed(
            "root name changed while editing blocks".to_owned(),
        ));
    }
    let source_root = source.root.as_compound().ok_or_else(|| {
        OverlayError::VerificationFailed("source root is not a compound".to_owned())
    })?;
    let patched_root = patched.root.as_compound().ok_or_else(|| {
        OverlayError::VerificationFailed("patched root is not a compound".to_owned())
    })?;
    if source_root.keys().collect::<Vec<_>>() != patched_root.keys().collect::<Vec<_>>() {
        return Err(OverlayError::VerificationFailed(
            "chunk root tags changed while editing blocks".to_owned(),
        ));
    }
    for (key, value) in source_root {
        if key == format.sections {
            continue;
        }
        if patched_root.get(key) != Some(value) {
            return Err(OverlayError::VerificationFailed(format!(
                "chunk root tag `{key}` changed while editing blocks"
            )));
        }
    }

    let source_sections = section_compounds(source_root, format)?;
    let patched_sections = section_compounds(patched_root, format)?;
    if source_sections.keys().collect::<Vec<_>>() != patched_sections.keys().collect::<Vec<_>>() {
        return Err(OverlayError::VerificationFailed(
            "section list changed while editing blocks".to_owned(),
        ));
    }
    if section_order(source_root, format)? != section_order(patched_root, format)? {
        return Err(OverlayError::VerificationFailed(
            "section order changed while editing blocks".to_owned(),
        ));
    }
    for (y, source_section) in source_sections {
        let patched_section = patched_sections
            .get(&y)
            .ok_or_else(|| OverlayError::VerificationFailed(format!("section {y} disappeared")))?;
        let source_rest = source_section
            .iter()
            .filter(|(key, _)| {
                key.as_str() != format.blocks.container && key.as_str() != format.biomes.container
            })
            .collect::<Vec<_>>();
        let patched_rest = patched_section
            .iter()
            .filter(|(key, _)| {
                key.as_str() != format.blocks.container && key.as_str() != format.biomes.container
            })
            .collect::<Vec<_>>();
        if source_rest != patched_rest {
            return Err(OverlayError::VerificationFailed(format!(
                "section {y} non-editable tags changed"
            )));
        }
    }
    Ok(())
}

/// Verifies that only the explicitly edited fields changed: sections without a block
/// edit keep their source `block_states`, sections without a biome edit keep their source
/// `biomes`.
fn verify_edit_scope(
    source: &NbtFile,
    patched: &NbtFile,
    block_sections: BTreeSet<i32>,
    biome_sections: BTreeSet<i32>,
    format: &ClassicChunkFormat,
) -> Result<(), OverlayError> {
    let source_root = source.root.as_compound().ok_or_else(|| {
        OverlayError::VerificationFailed("source root is not a compound".to_owned())
    })?;
    let patched_root = patched.root.as_compound().ok_or_else(|| {
        OverlayError::VerificationFailed("patched root is not a compound".to_owned())
    })?;
    let source_sections = section_compounds(source_root, format)?;
    let patched_sections = section_compounds(patched_root, format)?;
    for (y, source_section) in source_sections {
        let patched_section = patched_sections
            .get(&y)
            .ok_or_else(|| OverlayError::VerificationFailed(format!("section {y} disappeared")))?;
        if !block_sections.contains(&y)
            && source_section.get(format.blocks.container)
                != patched_section.get(format.blocks.container)
        {
            return Err(OverlayError::VerificationFailed(format!(
                "section {y} block_states changed without a block edit"
            )));
        }
        if !biome_sections.contains(&y)
            && source_section.get(format.biomes.container)
                != patched_section.get(format.biomes.container)
        {
            return Err(OverlayError::VerificationFailed(format!(
                "section {y} biomes changed without a biome edit"
            )));
        }
    }
    Ok(())
}

fn section_compounds<'a>(
    root: &'a BTreeMap<String, NbtValue>,
    format: &ClassicChunkFormat,
) -> Result<BTreeMap<i32, &'a BTreeMap<String, NbtValue>>, OverlayError> {
    let value = root
        .get(format.sections)
        .ok_or_else(|| OverlayError::VerificationFailed("missing sections list".to_owned()))?;
    let (_, items) = value
        .as_list()
        .ok_or_else(|| OverlayError::VerificationFailed("sections is not a list".to_owned()))?;
    let mut sections = BTreeMap::new();
    for item in items {
        let compound = item.as_compound().ok_or_else(|| {
            OverlayError::VerificationFailed("section is not a compound".to_owned())
        })?;
        let y = match compound.get(format.section_y) {
            Some(NbtValue::Byte(value)) => *value as i32,
            _ => {
                return Err(OverlayError::VerificationFailed(
                    "section has no byte Y".to_owned(),
                ))
            }
        };
        if sections.insert(y, compound).is_some() {
            return Err(OverlayError::VerificationFailed(format!(
                "duplicate section Y {y}"
            )));
        }
    }
    Ok(sections)
}

fn section_order(
    root: &BTreeMap<String, NbtValue>,
    format: &ClassicChunkFormat,
) -> Result<Vec<i32>, OverlayError> {
    let value = root
        .get(format.sections)
        .ok_or_else(|| OverlayError::VerificationFailed("missing sections list".to_owned()))?;
    let (_, items) = value
        .as_list()
        .ok_or_else(|| OverlayError::VerificationFailed("sections is not a list".to_owned()))?;
    let mut order = Vec::with_capacity(items.len());
    for item in items {
        let compound = item.as_compound().ok_or_else(|| {
            OverlayError::VerificationFailed("section is not a compound".to_owned())
        })?;
        match compound.get(format.section_y) {
            Some(NbtValue::Byte(value)) => order.push(*value as i32),
            _ => {
                return Err(OverlayError::VerificationFailed(
                    "section has no byte Y".to_owned(),
                ))
            }
        }
    }
    Ok(order)
}

/// Serialized overlay document. Packed words are lowercase hexadecimal strings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OverlayDocument {
    pub schema_version: u32,
    pub data_version: i32,
    pub chunks: Vec<OverlayChunkDocument>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OverlayChunkDocument {
    pub dimension: String,
    pub x: i32,
    pub z: i32,
    pub sections: Vec<OverlaySectionDocument>,
    /// Biome edits were added in schema 2. Older documents default to none.
    #[serde(default)]
    pub biomes: Vec<OverlayBiomeSectionDocument>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OverlaySectionDocument {
    pub y: i32,
    pub palette: Vec<String>,
    pub bits_per_index: u8,
    pub data: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OverlayBiomeSectionDocument {
    pub y: i32,
    pub palette: Vec<String>,
    pub cells: Vec<u16>,
}
