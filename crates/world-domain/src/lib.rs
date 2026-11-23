//! FuryBuilder world domain.
//!
//! This crate is independent of Tauri, React, Three.js and the operating system.
//! It owns block-state identity, coordinates, chunks, sections, sparse storage and revisions.

pub mod biome;
pub mod block;
pub mod bounds;
pub mod chunk;
pub mod coords;
pub mod dimension;
pub mod registry;
pub mod section;
pub mod world;

pub use biome::{BiomeSection, BiomeState, BiomeStateError, BIOME_CELLS, MAX_BIOME_PALETTE};
pub use block::{BlockState, BlockStateError};
pub use bounds::{BlockBox, BoundsError};
pub use chunk::Chunk;
pub use coords::{
    div_floor, mod_floor, BlockPos, ChunkPos, RegionPos, SectionPos, CHUNK_SIZE, REGION_CHUNKS,
    SECTION_SIZE,
};
pub use dimension::{
    BiomeWrite, BlockWrite, Dimension, DimensionError, DimensionInfo, DimensionKind,
};
pub use registry::{BlockDefinition, BlockRegistry, RegistryError};
pub use section::Section;
pub use world::{World, WorldError, WorldInfo, WorldVersion, WorldVersionError};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_project_uses_the_verified_baseline_version() {
        let version = WorldVersion::empty_project_default();
        assert_eq!(version.minecraft_version(), "1.21.4");
        assert_eq!(version.data_version(), 4189);
    }

    #[test]
    fn public_types_compose() {
        let mut world = World::new_empty();
        let state = BlockState::simple("minecraft:stone_bricks").unwrap();
        world
            .set_block("minecraft:overworld", BlockPos::new(0, 64, 0), &state)
            .unwrap();
        assert_eq!(world.block_count(), 1);
        assert!(BlockRegistry::target_1_21_4().len() >= 60);
    }
}
