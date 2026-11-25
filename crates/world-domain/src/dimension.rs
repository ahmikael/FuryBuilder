use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::biome::{BiomeSection, BiomeState};
use crate::block::BlockState;
use crate::chunk::Chunk;
use crate::coords::{BlockPos, ChunkPos};
use crate::section::Section;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DimensionKind {
    Overworld,
    Nether,
    End,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DimensionError {
    #[error("Y={y} is outside dimension `{dimension}` ({min}..={max})")]
    OutOfBounds {
        dimension: String,
        y: i32,
        min: i32,
        max: i32,
    },
}

#[derive(Debug, Clone)]
pub struct Dimension {
    id: String,
    kind: DimensionKind,
    min_y: i32,
    height: i32,
    chunks: BTreeMap<ChunkPos, Chunk>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DimensionInfo {
    pub id: String,
    pub kind: DimensionKind,
    pub min_y: i32,
    pub max_y: i32,
    pub height: i32,
    pub chunk_count: usize,
    pub section_count: usize,
    pub block_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockWrite {
    pub previous: BlockState,
    pub changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BiomeWrite {
    pub previous: BiomeState,
    pub changed: bool,
}

impl Dimension {
    pub fn new(
        id: impl Into<String>,
        kind: DimensionKind,
        min_y: i32,
        height: i32,
    ) -> Result<Self, DimensionError> {
        if height <= 0 || height % 16 != 0 {
            // The caller must supply a section-aligned height; this is a programming error,
            // not user input, but we still fail without panicking.
            return Err(DimensionError::OutOfBounds {
                dimension: id.into(),
                y: min_y,
                min: min_y,
                max: min_y + height - 1,
            });
        }
        Ok(Self {
            id: id.into(),
            kind,
            min_y,
            height,
            chunks: BTreeMap::new(),
        })
    }

    pub fn overworld() -> Self {
        Self::new("minecraft:overworld", DimensionKind::Overworld, -64, 384)
            .expect("valid overworld bounds")
    }

    pub fn nether() -> Self {
        Self::new("minecraft:the_nether", DimensionKind::Nether, 0, 256)
            .expect("valid nether bounds")
    }

    pub fn end() -> Self {
        Self::new("minecraft:the_end", DimensionKind::End, 0, 256).expect("valid end bounds")
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn kind(&self) -> DimensionKind {
        self.kind
    }

    pub fn min_y(&self) -> i32 {
        self.min_y
    }

    pub fn max_y(&self) -> i32 {
        self.min_y + self.height - 1
    }

    pub fn height(&self) -> i32 {
        self.height
    }

    pub fn contains_y(&self, y: i32) -> bool {
        y >= self.min_y && y <= self.max_y()
    }

    pub fn chunks(&self) -> &BTreeMap<ChunkPos, Chunk> {
        &self.chunks
    }

    pub fn chunk(&self, pos: ChunkPos) -> Option<&Chunk> {
        self.chunks.get(&pos)
    }

    pub fn chunk_mut(&mut self, pos: ChunkPos) -> Option<&mut Chunk> {
        self.chunks.get_mut(&pos)
    }

    pub fn ensure_chunk(&mut self, pos: ChunkPos) -> &mut Chunk {
        self.chunks.entry(pos).or_insert_with(|| Chunk::new(pos))
    }

    pub fn section(&self, chunk: ChunkPos, section_y: i32) -> Option<&Section> {
        self.chunks.get(&chunk).and_then(|c| c.section(section_y))
    }

    fn check_y(&self, y: i32) -> Result<(), DimensionError> {
        if self.contains_y(y) {
            Ok(())
        } else {
            Err(DimensionError::OutOfBounds {
                dimension: self.id.clone(),
                y,
                min: self.min_y,
                max: self.max_y(),
            })
        }
    }

    pub fn get_block(&self, pos: BlockPos) -> Result<BlockState, DimensionError> {
        self.check_y(pos.y)?;
        Ok(self
            .chunks
            .get(&pos.chunk_pos())
            .map(|chunk| chunk.get_local(pos.local_x() as u8, pos.local_z() as u8, pos.y))
            .unwrap_or_default())
    }

    pub fn set_block(
        &mut self,
        pos: BlockPos,
        state: &BlockState,
    ) -> Result<BlockWrite, DimensionError> {
        self.check_y(pos.y)?;
        let chunk_pos = pos.chunk_pos();
        if state.is_implicit_air() {
            let Some(chunk) = self.chunks.get_mut(&chunk_pos) else {
                return Ok(BlockWrite {
                    previous: BlockState::default(),
                    changed: false,
                });
            };
            let previous = chunk.get_local(pos.local_x() as u8, pos.local_z() as u8, pos.y);
            let changed = chunk.set_local(pos.local_x() as u8, pos.local_z() as u8, pos.y, state);
            if chunk.is_empty() {
                self.chunks.remove(&chunk_pos);
            }
            return Ok(BlockWrite { previous, changed });
        }

        let chunk = self.ensure_chunk(chunk_pos);
        let previous = chunk.get_local(pos.local_x() as u8, pos.local_z() as u8, pos.y);
        let changed = chunk.set_local(pos.local_x() as u8, pos.local_z() as u8, pos.y, state);
        Ok(BlockWrite { previous, changed })
    }

    /// Installs or removes a section snapshot (used by undo/redo).
    pub fn install_section(
        &mut self,
        chunk_pos: ChunkPos,
        section_y: i32,
        section: Option<Section>,
    ) {
        match section {
            Some(section) if !section.is_void() => {
                self.ensure_chunk(chunk_pos)
                    .install_section(section_y, Some(section));
            }
            _ => {
                if let Some(chunk) = self.chunks.get_mut(&chunk_pos) {
                    chunk.install_section(section_y, None);
                    if chunk.is_empty() {
                        self.chunks.remove(&chunk_pos);
                    }
                }
            }
        }
    }

    pub fn get_biome(&self, pos: BlockPos) -> Result<Option<BiomeState>, DimensionError> {
        self.check_y(pos.y)?;
        Ok(self
            .chunks
            .get(&pos.chunk_pos())
            .and_then(|chunk| chunk.get_biome(pos.local_x() as u8, pos.local_z() as u8, pos.y))
            .cloned())
    }

    /// Sets the 4x4x4 biome cell covering `pos`. Missing biome data is reported as `None`
    /// instead of being replaced with an invented default.
    pub fn set_biome(
        &mut self,
        pos: BlockPos,
        biome: &BiomeState,
    ) -> Result<Option<BiomeWrite>, DimensionError> {
        self.check_y(pos.y)?;
        let chunk_pos = pos.chunk_pos();
        let Some(chunk) = self.chunks.get_mut(&chunk_pos) else {
            return Ok(None);
        };
        let Some(previous) = chunk
            .get_biome(pos.local_x() as u8, pos.local_z() as u8, pos.y)
            .cloned()
        else {
            return Ok(None);
        };
        let changed = chunk
            .set_biome(pos.local_x() as u8, pos.local_z() as u8, pos.y, biome)
            .unwrap_or(false);
        Ok(Some(BiomeWrite { previous, changed }))
    }

    /// Installs biome data for one section without touching block data.
    pub fn install_biomes(
        &mut self,
        chunk_pos: ChunkPos,
        section_y: i32,
        biomes: Option<BiomeSection>,
    ) {
        match biomes {
            Some(biomes) => {
                self.ensure_chunk(chunk_pos)
                    .install_biomes(section_y, Some(biomes));
            }
            None => {
                if let Some(chunk) = self.chunks.get_mut(&chunk_pos) {
                    chunk.install_biomes(section_y, None);
                    if chunk.is_empty() {
                        self.chunks.remove(&chunk_pos);
                    }
                }
            }
        }
    }

    pub fn section_count(&self) -> usize {
        self.chunks
            .values()
            .map(|chunk| chunk.sections().len())
            .sum()
    }

    pub fn block_count(&self) -> usize {
        self.chunks.values().map(Chunk::block_count).sum()
    }

    pub fn info(&self) -> DimensionInfo {
        DimensionInfo {
            id: self.id.clone(),
            kind: self.kind,
            min_y: self.min_y,
            max_y: self.max_y(),
            height: self.height,
            chunk_count: self.chunks.len(),
            section_count: self.section_count(),
            block_count: self.block_count(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stone() -> BlockState {
        "minecraft:stone".parse().unwrap()
    }

    #[test]
    fn overworld_bounds_and_negative_y() {
        let mut dimension = Dimension::overworld();
        assert_eq!(dimension.min_y(), -64);
        assert_eq!(dimension.max_y(), 319);
        assert!(dimension
            .set_block(BlockPos::new(0, -64, 0), &stone())
            .is_ok());
        assert!(dimension
            .set_block(BlockPos::new(0, -65, 0), &stone())
            .is_err());
        assert!(dimension
            .set_block(BlockPos::new(0, 320, 0), &stone())
            .is_err());
        assert_eq!(
            dimension.get_block(BlockPos::new(0, -64, 0)).unwrap(),
            stone()
        );
    }

    #[test]
    fn neighbours_at_chunk_boundary_are_preserved() {
        let mut dimension = Dimension::overworld();
        dimension
            .set_block(BlockPos::new(15, 64, 15), &stone())
            .unwrap();
        dimension
            .set_block(BlockPos::new(16, 64, 0), &stone())
            .unwrap();
        dimension
            .set_block(BlockPos::new(-1, 64, -1), &stone())
            .unwrap();
        assert_eq!(dimension.chunks().len(), 3);
        assert_eq!(
            dimension.get_block(BlockPos::new(15, 64, 15)).unwrap(),
            stone()
        );
        assert_eq!(
            dimension.get_block(BlockPos::new(16, 64, 0)).unwrap(),
            stone()
        );
        assert!(dimension
            .get_block(BlockPos::new(14, 64, 15))
            .unwrap()
            .is_air());
    }

    #[test]
    fn setting_air_never_creates_chunks() {
        let mut dimension = Dimension::overworld();
        let write = dimension
            .set_block(BlockPos::new(100, 64, 100), &BlockState::default())
            .unwrap();
        assert!(!write.changed);
        assert!(dimension.chunks().is_empty());
    }

    #[test]
    fn install_section_supports_undo_snapshots() {
        let mut dimension = Dimension::overworld();
        dimension
            .set_block(BlockPos::new(0, 0, 0), &stone())
            .unwrap();
        let snapshot = dimension.section(ChunkPos::new(0, 0), 0).cloned().unwrap();
        dimension.install_section(ChunkPos::new(0, 0), 0, None);
        assert!(dimension.chunks().is_empty());
        dimension.install_section(ChunkPos::new(0, 0), 0, Some(snapshot));
        assert_eq!(
            dimension.get_block(BlockPos::new(0, 0, 0)).unwrap(),
            stone()
        );
    }
}
