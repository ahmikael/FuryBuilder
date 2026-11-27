//! Classic Anvil world codec. The reader and writer keep the currently
//! verified version and layout checks; registering this codec does not
//! enable additional versions.

pub mod chunk;
pub(super) mod flattened_biomes;
pub(super) mod flattened_chunk;
pub(crate) mod format;
pub(super) mod numeric_chunk;
pub mod overlay;
pub mod reader;
pub mod region_write;
pub mod world_export;
pub mod world_import;

use std::path::Path;

use minecraft_version::{VersionFamily, VersionProfile};
use world_domain::{ChunkPos, World};

use crate::nbt::NbtFile;
use crate::world_codec::WorldCodec;

use chunk::DecodedChunk;
use overlay::{OverlayError, WorldOverlay};
use reader::{AnvilError, JavaWorld, JavaWorldDimension};
use region_write::{
    DerivedDataPolicy, RegionWriteError, RegionWriteReport, VerifiedChunkReplacement,
};
use world_export::{WorldExportError, WorldExportOptions, WorldExportReport};
use world_import::{JavaWorldAnalysis, JavaWorldLoadReport, WorldImportError, WorldImportOptions};

pub(super) struct ClassicAnvilCodec;

impl WorldCodec for ClassicAnvilCodec {
    fn inspect(&self, root: &Path) -> JavaWorldAnalysis {
        world_import::inspect_root(root)
    }

    fn load_overlay(&self, root: &Path) -> Result<WorldOverlay, OverlayError> {
        WorldOverlay::open_classic(root)
    }

    fn load_world(
        &self,
        root: &Path,
        options: WorldImportOptions,
    ) -> Result<(World, JavaWorldLoadReport), WorldImportError> {
        world_import::load_world(root, options)
    }

    fn decode_chunk(
        &self,
        nbt: &NbtFile,
        position: ChunkPos,
        dimension: &JavaWorldDimension,
        profile: &VersionProfile,
    ) -> Result<DecodedChunk, AnvilError> {
        match profile.family {
            VersionFamily::Flattened => {
                flattened_chunk::decode_chunk(nbt, position.x, position.z, dimension, profile)
            }
            VersionFamily::Numeric => {
                numeric_chunk::decode_chunk(nbt, position.x, position.z, dimension, profile)
            }
            _ => chunk::decode_chunk(nbt, position.x, position.z, dimension, profile),
        }
    }

    fn encode_chunk(
        &self,
        overlay: &WorldOverlay,
        dimension: &str,
        position: ChunkPos,
    ) -> Result<Option<NbtFile>, OverlayError> {
        overlay.encode_chunk(dimension, position.x, position.z)
    }

    fn write_region(
        &self,
        world: &JavaWorld,
        dimension: &str,
        (region_x, region_z): (i32, i32),
        replacements: &[VerifiedChunkReplacement],
        destination: &Path,
        policy: DerivedDataPolicy,
    ) -> Result<RegionWriteReport, RegionWriteError> {
        region_write::write_region(
            world,
            dimension,
            region_x,
            region_z,
            replacements,
            destination,
            policy,
        )
    }

    fn export_world(
        &self,
        overlay: &WorldOverlay,
        destination: &Path,
        options: WorldExportOptions,
    ) -> Result<WorldExportReport, WorldExportError> {
        world_export::export_world(overlay, destination, options)
    }
}
