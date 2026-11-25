use std::collections::BTreeMap;

use minecraft_version::{default_profile, VersionProfile};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::biome::{BiomeSection, BiomeState};
use crate::block::BlockState;
use crate::coords::{BlockPos, ChunkPos};
use crate::dimension::{BiomeWrite, BlockWrite, Dimension, DimensionError, DimensionInfo};
use crate::section::Section;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum WorldError {
    #[error("unknown dimension `{0}`")]
    UnknownDimension(String),
    #[error(transparent)]
    Dimension(#[from] DimensionError),
}

/// The Minecraft Java version that an editable world belongs to.
///
/// A world version is established when a world is created or imported. It is never
/// inferred from a global constant at read time, so multiple projects with different
/// versions can coexist in one process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldVersion {
    minecraft_version: String,
    data_version: i32,
}

impl WorldVersion {
    pub fn new(
        minecraft_version: impl Into<String>,
        data_version: i32,
    ) -> Result<Self, WorldVersionError> {
        let version = Self {
            minecraft_version: minecraft_version.into(),
            data_version,
        };
        version.validate()?;
        Ok(version)
    }

    /// Default for a brand-new empty project before a user selects a version.
    ///
    /// Imported and generated worlds always replace this with their own verified version.
    pub fn empty_project_default() -> Self {
        let profile = default_profile();
        Self::new(profile.minecraft_version.as_str(), profile.data_version)
            .expect("the default version profile carries a valid identity")
    }

    pub fn minecraft_version(&self) -> &str {
        &self.minecraft_version
    }

    pub fn data_version(&self) -> i32 {
        self.data_version
    }

    pub fn validate(&self) -> Result<(), WorldVersionError> {
        if self.minecraft_version.trim().is_empty()
            || self.minecraft_version.len() > 64
            || self.minecraft_version.chars().any(char::is_whitespace)
        {
            return Err(WorldVersionError::InvalidMinecraftVersion(
                self.minecraft_version.clone(),
            ));
        }
        if self.data_version <= 0 {
            return Err(WorldVersionError::InvalidDataVersion(self.data_version));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum WorldVersionError {
    #[error("invalid Minecraft version id: `{0}`")]
    InvalidMinecraftVersion(String),
    #[error("invalid Minecraft DataVersion: {0}")]
    InvalidDataVersion(i32),
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
    version: WorldVersion,
    dimensions: BTreeMap<String, Dimension>,
    revision: u64,
}

impl Default for World {
    fn default() -> Self {
        Self::new_empty()
    }
}

impl World {
    /// Empty world with the default profile's dimensions.
    ///
    /// Versioned creation, import and snapshot decoding use [`World::for_profile`] so the
    /// dimension set and vertical bounds always come from the project's exact profile.
    pub fn new_empty() -> Self {
        Self::for_profile(&default_profile())
    }

    /// Builds an empty world with exactly the dimensions declared by a verified profile.
    pub fn for_profile(profile: &VersionProfile) -> Self {
        let dimensions = profile
            .dimension_profiles
            .iter()
            .map(|spec| {
                let dimension = Dimension::from_profile(spec);
                (dimension.id().to_owned(), dimension)
            })
            .collect();
        Self {
            version: WorldVersion::new(profile.minecraft_version.as_str(), profile.data_version)
                .expect("verified version profiles carry a valid identity"),
            dimensions,
            revision: 0,
        }
    }

    pub fn version(&self) -> &WorldVersion {
        &self.version
    }

    pub fn set_version(&mut self, version: WorldVersion) {
        self.version = version;
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
        let mut dimensions: Vec<DimensionInfo> =
            self.dimensions.values().map(Dimension::info).collect();
        dimensions.sort_by(|left, right| {
            left.kind
                .cmp(&right.kind)
                .then_with(|| left.id.cmp(&right.id))
        });
        WorldInfo {
            target_minecraft_version: self.version.minecraft_version.clone(),
            target_data_version: self.version.data_version,
            revision: self.revision,
            dimensions,
            chunk_count: self.chunk_count(),
            section_count: self.section_count(),
            block_count: self.block_count(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clearing_solid_blocks_keeps_existing_explicit_air_states() {
        let mut world = World::new_empty();
        let dimension = "minecraft:overworld";
        let special_pos = BlockPos::new(0, 64, 0);
        let solid_pos = BlockPos::new(1, 64, 0);
        let special: BlockState = "minecraft:cave_air[custom=true]".parse().unwrap();
        let implicit = BlockState::default();
        world.set_block(dimension, special_pos, &special).unwrap();
        world.set_block(dimension, solid_pos, &stone()).unwrap();
        world.set_block(dimension, solid_pos, &implicit).unwrap();
        assert_eq!(world.block_count(), 0);
        assert_eq!(world.section_count(), 1);
        assert_eq!(world.get_block(dimension, special_pos).unwrap(), special);
        world.set_block(dimension, special_pos, &implicit).unwrap();
        assert_eq!(world.section_count(), 0);
    }

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
        let ids: Vec<_> = world
            .info()
            .dimensions
            .into_iter()
            .map(|dimension| dimension.id)
            .collect();
        assert_eq!(
            ids,
            [
                "minecraft:overworld",
                "minecraft:the_nether",
                "minecraft:the_end"
            ],
            "WorldInfo lists dimensions in profile order, not BTreeMap order"
        );
    }

    #[test]
    fn dimensions_and_bounds_come_from_the_exact_profile() {
        let mut profile = minecraft_version::default_profile();
        profile.minecraft_version = "1.16.5".parse().expect("version id");
        profile.data_version = 2586;
        profile.dimension_profiles[0].min_y = 0;
        profile.dimension_profiles[0].height = 256;
        let mut world = World::for_profile(&profile);
        let info = world.info();

        assert_eq!(info.target_minecraft_version, "1.16.5");
        assert_eq!(info.dimensions.len(), 3);
        assert_eq!(info.dimensions[0].id, "minecraft:overworld");
        assert_eq!(info.dimensions[0].min_y, 0);
        assert_eq!(info.dimensions[0].max_y, 255);
        assert_eq!(info.dimensions[1].id, "minecraft:the_nether");
        assert_eq!(info.dimensions[1].height, 256);
        assert!(world
            .set_block("minecraft:overworld", BlockPos::new(0, -1, 0), &stone())
            .is_err());
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
        assert_eq!(info.target_minecraft_version, "1.21.4");
    }

    #[test]
    fn world_can_belong_to_another_minecraft_version() {
        let mut world = World::new_empty();
        world.set_version(WorldVersion::new("1.20.6", 3839).expect("valid version"));

        let info = world.info();
        assert_eq!(world.version().minecraft_version(), "1.20.6");
        assert_eq!(world.version().data_version(), 3839);
        assert_eq!(info.target_minecraft_version, "1.20.6");
        assert_eq!(info.target_data_version, 3839);
    }

    #[test]
    fn world_version_rejects_invalid_identity() {
        assert!(matches!(
            WorldVersion::new("", 4189),
            Err(WorldVersionError::InvalidMinecraftVersion(_))
        ));
        assert!(matches!(
            WorldVersion::new("1.21.4", 0),
            Err(WorldVersionError::InvalidDataVersion(0))
        ));
    }
}
