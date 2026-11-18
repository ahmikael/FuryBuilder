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
use world_domain::{BlockState, Section, CHUNK_SIZE, SECTION_SIZE, TARGET_DATA_VERSION};

use crate::anvil::{AnvilError, JavaWorld, JavaWorldDimension, ANVIL_REGION_AXIS_CHUNKS};
use crate::java_chunk::{DecodedChunk, DecodedSection, BLOCK_SECTION_VOLUME, MAX_BLOCK_PALETTE};
use crate::nbt::{NbtFile, NbtValue, TAG_COMPOUND};

pub const OVERLAY_SCHEMA_VERSION: u32 = 1;
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
    #[error("section palette is full at block ({x}, {y}, {z}) in dimension `{dimension}`")]
    PaletteFull {
        dimension: String,
        x: i32,
        y: i32,
        z: i32,
    },
    #[error("overlay DataVersion {found} is not supported; supported version is {supported}")]
    UnsupportedDataVersion { found: i32, supported: i32 },
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
            | OverlayError::DuplicateEdit { .. } => "invalid_argument",
            OverlayError::ChunkNotPresent { .. } => "not_found",
            OverlayError::SectionNotPresent { .. }
            | OverlayError::InvalidOverlay(_)
            | OverlayError::VerificationFailed(_) => "unsupported_format",
            OverlayError::PaletteFull { .. } | OverlayError::LimitExceeded(_) => "limit_exceeded",
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
        self.chunks.values().map(|edits| edits.sections.len()).sum()
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
            let previous_section_count = previous.sections.len();
            let mut sections = previous.sections;

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
                .and_then(|value| value.checked_add(final_sections.len()))
                .ok_or_else(|| {
                    OverlayError::LimitExceeded("overlay section counter overflow".to_owned())
                })?;
            let was_present = previous_section_count > 0;
            let is_present = !final_sections.is_empty();
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

            if final_sections.is_empty() {
                staged.insert(key, None);
            } else {
                staged.insert(
                    key,
                    Some(ChunkEdits {
                        sections: final_sections,
                        changes: chunk_changes,
                    }),
                );
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
            let sections_value = root.get_mut("sections").ok_or_else(|| {
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
            for (section_y, section) in &edits.sections {
                let entry = items
                    .iter_mut()
                    .find(|item| section_y_of(item) == Some(*section_y))
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
                compound.insert("block_states".to_owned(), encode_block_states(section));
            }
        }

        verify_preserved_fields(&source.nbt, &patched)?;

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
            if decoded_section.biomes != source_section.biomes {
                return Err(OverlayError::VerificationFailed(format!(
                    "section {section_y} biomes changed while editing blocks"
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
        if document.schema_version != OVERLAY_SCHEMA_VERSION {
            return Err(OverlayError::UnsupportedSchema {
                found: document.schema_version,
                supported: OVERLAY_SCHEMA_VERSION,
            });
        }
        if document.data_version != TARGET_DATA_VERSION
            || world.level.data_version != document.data_version
        {
            return Err(OverlayError::UnsupportedDataVersion {
                found: document.data_version,
                supported: TARGET_DATA_VERSION,
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
            if chunk_document.sections.is_empty() {
                return Err(OverlayError::InvalidOverlay(format!(
                    "chunk ({}, {}) has no sections",
                    key.x, key.z
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

            if sections.is_empty() {
                continue;
            }
            overlay.change_count += changes;
            overlay.chunks.insert(key, ChunkEdits { sections, changes });
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

    fn region(
        &self,
        dimension_id: &str,
        x: i32,
        z: i32,
    ) -> Result<crate::anvil::RegionFile, OverlayError> {
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

fn section_y_of(value: &NbtValue) -> Option<i32> {
    let compound = value.as_compound()?;
    match compound.get("Y") {
        Some(NbtValue::Byte(y)) => Some(*y as i32),
        _ => None,
    }
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

fn encode_block_states(section: &Section) -> NbtValue {
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
        "palette".to_owned(),
        NbtValue::List {
            kind: TAG_COMPOUND,
            items: palette,
        },
    );
    if section.palette_len() > 1 {
        block_states.insert(
            "data".to_owned(),
            NbtValue::LongArray(pack_block_indices(section)),
        );
    }
    NbtValue::Compound(block_states)
}

fn pack_block_indices(section: &Section) -> Vec<i64> {
    let bits = block_bits(section.palette_len());
    let values_per_long = 64 / bits as usize;
    let word_count = BLOCK_SECTION_VOLUME.div_ceil(values_per_long);
    let mut words = vec![0u64; word_count];
    for index in 0..BLOCK_SECTION_VOLUME {
        let x = (index % 16) as u8;
        let z = ((index / 16) % 16) as u8;
        let y = ((index / 256) % 16) as u8;
        let palette_index = section
            .palette_index(x, y, z)
            .expect("every local section position has a palette index");
        let word = index / values_per_long;
        let shift = ((index % values_per_long) * bits as usize) as u32;
        words[word] |= (palette_index as u64) << shift;
    }
    words.into_iter().map(|word| word as i64).collect()
}

fn block_bits(palette_len: usize) -> u8 {
    debug_assert!(palette_len >= 2);
    let bits = (usize::BITS - (palette_len - 1).leading_zeros()) as u8;
    bits.max(4)
}

fn verify_preserved_fields(source: &NbtFile, patched: &NbtFile) -> Result<(), OverlayError> {
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
        if key == "sections" {
            continue;
        }
        if patched_root.get(key) != Some(value) {
            return Err(OverlayError::VerificationFailed(format!(
                "chunk root tag `{key}` changed while editing blocks"
            )));
        }
    }

    let source_sections = section_compounds(source_root)?;
    let patched_sections = section_compounds(patched_root)?;
    if source_sections.keys().collect::<Vec<_>>() != patched_sections.keys().collect::<Vec<_>>() {
        return Err(OverlayError::VerificationFailed(
            "section list changed while editing blocks".to_owned(),
        ));
    }
    for (y, source_section) in source_sections {
        let patched_section = patched_sections
            .get(&y)
            .ok_or_else(|| OverlayError::VerificationFailed(format!("section {y} disappeared")))?;
        let source_rest = source_section
            .iter()
            .filter(|(key, _)| key.as_str() != "block_states")
            .collect::<Vec<_>>();
        let patched_rest = patched_section
            .iter()
            .filter(|(key, _)| key.as_str() != "block_states")
            .collect::<Vec<_>>();
        if source_rest != patched_rest {
            return Err(OverlayError::VerificationFailed(format!(
                "section {y} non-block-state tags changed"
            )));
        }
    }
    Ok(())
}

fn section_compounds(
    root: &BTreeMap<String, NbtValue>,
) -> Result<BTreeMap<i32, &BTreeMap<String, NbtValue>>, OverlayError> {
    let value = root
        .get("sections")
        .ok_or_else(|| OverlayError::VerificationFailed("missing sections list".to_owned()))?;
    let (_, items) = value
        .as_list()
        .ok_or_else(|| OverlayError::VerificationFailed("sections is not a list".to_owned()))?;
    let mut sections = BTreeMap::new();
    for item in items {
        let compound = item.as_compound().ok_or_else(|| {
            OverlayError::VerificationFailed("section is not a compound".to_owned())
        })?;
        let y = match compound.get("Y") {
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OverlaySectionDocument {
    pub y: i32,
    pub palette: Vec<String>,
    pub bits_per_index: u8,
    pub data: Vec<String>,
}
