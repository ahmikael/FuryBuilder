//! FuryBuilder world domain.
//!
//! This crate is independent of Tauri, React, Three.js and the operating system.
//! It owns block-state identity, coordinates, chunks, sections, sparse storage and revisions.

pub mod block;
pub mod bounds;
pub mod chunk;
pub mod coords;
pub mod dimension;
pub mod registry;
pub mod section;
pub mod world;

pub use block::{BlockState, BlockStateError};
pub use bounds::{BlockBox, BoundsError};
pub use chunk::Chunk;
pub use coords::{
    div_floor, mod_floor, BlockPos, ChunkPos, RegionPos, SectionPos, CHUNK_SIZE, REGION_CHUNKS,
    SECTION_SIZE,
};
pub use dimension::{BlockWrite, Dimension, DimensionError, DimensionInfo, DimensionKind};
pub use registry::{BlockDefinition, BlockRegistry, RegistryError, CATALOG_VERSION};
pub use section::Section;
pub use world::{World, WorldError, WorldInfo};

/// First write-verified Minecraft Java version (ADR-001).
pub const TARGET_MINECRAFT_VERSION: &str = "1.21.4";
/// `DataVersion` for Minecraft Java 1.21.4.
pub const TARGET_DATA_VERSION: i32 = 4189;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_version_is_pinned() {
        assert_eq!(TARGET_MINECRAFT_VERSION, "1.21.4");
        assert_eq!(TARGET_DATA_VERSION, 4189);
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
