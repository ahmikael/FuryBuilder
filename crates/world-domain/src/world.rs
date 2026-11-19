use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::biome::{BiomeSection, BiomeState};
use crate::block::BlockState;
use crate::coords::{BlockPos, ChunkPos};
use crate::dimension::{BiomeWrite, BlockWrite, Dimension, DimensionError, DimensionInfo};
use crate::section::Section;
use crate::{TARGET_DATA_VERSION, TARGET_MINECRAFT_VERSION};

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum WorldError {
    #[error("unknown dimension `{0}`")]
    UnknownDimension(String),
    #[error(transparent)]
    Dimension(#[from] DimensionError),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldInfo {
    pub target_minecraft_version: String,
    pub target_data_version: i32,
    pub revision: u64,
    pub dimensions: Vec<DimensionInfo>,
    pub chunk_count: usize,
    pub section_count: usize,
    pub block_count: usize,
}

/// Sparse, multi-dimension world. This is the single source of truth for block data.
#[derive(Debug, Clone)]
pub struct World {
    dimensions: BTreeMap<String, Dimension>,
    revision: u64,
}

impl Default for World {
    fn default() -> Self {
        Self::new_empty()
    }
}

impl World {
    pub fn new_empty() -> Self {
        let mut dimensions = BTreeMap::new();
        for dimension in [
            Dimension::overworld(),
            Dimension::nether(),
            Dimension::end(),
        ] {
            dimensions.insert(dimension.id().to_owned(), dimension);
        }
        Self {
            dimensions,
            revision: 0,
        }
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Increments and returns the new revision. Only committed transactions call this.
    pub fn bump_revision(&mut self) -> u64 {
        self.revision += 1;
        self.revision
    }

    /// Restores the persisted revision when loading a project snapshot. Live edits must
    /// still use `bump_revision`.
    pub fn restore_revision(&mut self, revision: u64) {
        self.revision = revision;
    }

    pub fn dimensions(&self) -> &BTreeMap<String, Dimension> {
        &self.dimensions
    }

    pub fn dimension(&self, id: &str) -> Result<&Dimension, WorldError> {
        self.dimensions
            .get(id)
            .ok_or_else(|| WorldError::UnknownDimension(id.to_owned()))
    }

    pub fn dimension_mut(&mut self, id: &str) -> Result<&mut Dimension, WorldError> {
        self.dimensions
            .get_mut(id)
            .ok_or_else(|| WorldError::UnknownDimension(id.to_owned()))
    }

    pub fn get_block(&self, dimension: &str, pos: BlockPos) -> Result<BlockState, WorldError> {
        Ok(self.dimension(dimension)?.get_block(pos)?)
    }

    pub fn set_block(
        &mut self,
        dimension: &str,
        pos: BlockPos,
        state: &BlockState,
    ) -> Result<BlockWrite, WorldError> {
        Ok(self.dimension_mut(dimension)?.set_block(pos, state)?)
    }

    pub fn get_biome(
        &self,
        dimension: &str,
        pos: BlockPos,
    ) -> Result<Option<BiomeState>, WorldError> {
        Ok(self.dimension(dimension)?.get_biome(pos)?)
    }

    pub fn set_biome(
        &mut self,
        dimension: &str,
        pos: BlockPos,
        biome: &BiomeState,
    ) -> Result<Option<BiomeWrite>, WorldError> {
        Ok(self.dimension_mut(dimension)?.set_biome(pos, biome)?)
    }

    pub fn install_biomes(
        &mut self,
        dimension: &str,
        chunk: ChunkPos,
        section_y: i32,
        biomes: Option<BiomeSection>,
    ) -> Result<(), WorldError> {
        self.dimension_mut(dimension)?
            .install_biomes(chunk, section_y, biomes);
        Ok(())
    }

    pub fn section(
        &self,
        dimension: &str,
        chunk: ChunkPos,
        section_y: i32,
    ) -> Result<Option<&Section>, WorldError> {
        Ok(self.dimension(dimension)?.section(chunk, section_y))
    }

    pub fn install_section(
        &mut self,
        dimension: &str,
        chunk: ChunkPos,
        section_y: i32,
        section: Option<Section>,
    ) -> Result<(), WorldError> {
        self.dimension_mut(dimension)?
            .install_section(chunk, section_y, section);
        Ok(())
    }

    pub fn chunk_count(&self) -> usize {
        self.dimensions.values().map(|d| d.chunks().len()).sum()
    }

    pub fn section_count(&self) -> usize {
        self.dimensions.values().map(Dimension::section_count).sum()
    }

    pub fn block_count(&self) -> usize {
        self.dimensions.values().map(Dimension::block_count).sum()
    }

    pub fn info(&self) -> WorldInfo {
        WorldInfo {
            target_minecraft_version: TARGET_MINECRAFT_VERSION.to_owned(),
            target_data_version: TARGET_DATA_VERSION,
            revision: self.revision,
            dimensions: self.dimensions.values().map(Dimension::info).collect(),
            chunk_count: self.chunk_count(),
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
    fn empty_world_has_three_dimensions_and_zero_revision() {
        let world = World::new_empty();
        assert_eq!(world.dimensions().len(), 3);
        assert_eq!(world.revision(), 0);
        assert_eq!(world.chunk_count(), 0);
        assert_eq!(world.block_count(), 0);
        assert!(world.dimension("minecraft:overworld").is_ok());
        assert!(world.dimension("minecraft:the_nether").is_ok());
        assert!(world.dimension("minecraft:the_end").is_ok());
        assert!(matches!(
            world.dimension("custom:missing"),
            Err(WorldError::UnknownDimension(_))
        ));
    }

    #[test]
    fn dimensions_do_not_share_coordinates() {
        let mut world = World::new_empty();
        world
            .set_block("minecraft:overworld", BlockPos::new(0, 64, 0), &stone())
            .unwrap();
        assert_eq!(
            world
                .get_block("minecraft:overworld", BlockPos::new(0, 64, 0))
                .unwrap(),
            stone()
        );
        assert!(world
            .get_block("minecraft:the_nether", BlockPos::new(0, 64, 0))
            .unwrap()
            .is_air());
        assert_eq!(world.chunk_count(), 1);
    }

    #[test]
    fn revision_increments_explicitly() {
        let mut world = World::new_empty();
        assert_eq!(world.bump_revision(), 1);
        assert_eq!(world.bump_revision(), 2);
        assert_eq!(world.info().revision, 2);
    }

    #[test]
    fn info_is_consistent_with_contents() {
        let mut world = World::new_empty();
        world
            .set_block("minecraft:overworld", BlockPos::new(-1, -64, -1), &stone())
            .unwrap();
        world
            .set_block("minecraft:overworld", BlockPos::new(0, 100, 0), &stone())
            .unwrap();
        let info = world.info();
        assert_eq!(info.chunk_count, 2);
        assert_eq!(info.block_count, 2);
        assert_eq!(info.section_count, 2);
        assert_eq!(info.target_data_version, 4189);
    }
}
