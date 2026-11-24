//! The world-format boundary. Public world I/O helpers supply defaults and
//! funnel actual format work through this contract.

use std::path::Path;

use world_domain::{ChunkPos, World};

use crate::anvil::{AnvilError, JavaWorld, JavaWorldDimension};
use crate::java_chunk::{self, DecodedChunk};
use crate::nbt::NbtFile;
use crate::overlay::{OverlayError, WorldOverlay};
use crate::region_write::{
    self, DerivedDataPolicy, RegionWriteError, RegionWriteReport, VerifiedChunkReplacement,
};
use crate::world_export::{self, WorldExportError, WorldExportOptions, WorldExportReport};
use crate::world_import::{
    self, JavaWorldAnalysis, JavaWorldLoadReport, WorldImportError, WorldImportOptions,
};

pub(crate) trait WorldCodec {
    /// Inspect without modifying the source; unsupported saves report blockers.
    fn inspect(&self, root: &Path) -> JavaWorldAnalysis;

    /// Open the immutable source and bind its verified version profile to the overlay.
    fn load_overlay(&self, root: &Path) -> Result<WorldOverlay, OverlayError>;

    /// Decode the editable subset into the normalized world under import limits.
    fn load_world(
        &self,
        root: &Path,
        options: WorldImportOptions,
    ) -> Result<(World, JavaWorldLoadReport), WorldImportError>;

    /// Decode a source or staged chunk using the source dimension's bounds.
    fn decode_chunk(
        &self,
        nbt: &NbtFile,
        position: ChunkPos,
        dimension: &JavaWorldDimension,
    ) -> Result<DecodedChunk, AnvilError>;

    /// Merge edits into the original NBT. Return `None` for an untouched chunk so
    /// the writer can preserve its original record.
    fn encode_chunk(
        &self,
        overlay: &WorldOverlay,
        dimension: &str,
        position: ChunkPos,
    ) -> Result<Option<NbtFile>, OverlayError>;

    /// Write and verify a new region without modifying the source region.
    fn write_region(
        &self,
        world: &JavaWorld,
        dimension: &str,
        region: (i32, i32),
        replacements: &[VerifiedChunkReplacement],
        destination: &Path,
        policy: DerivedDataPolicy,
    ) -> Result<RegionWriteReport, RegionWriteError>;

    /// Stage, verify and publish a complete save without modifying the source world.
    fn export_world(
        &self,
        overlay: &WorldOverlay,
        destination: &Path,
        options: WorldExportOptions,
    ) -> Result<WorldExportReport, WorldExportError>;
}

/// Only the current Anvil path is wired up. Its existing inspection and
/// profile checks still decide which saves it can open.
pub(crate) fn world_codec() -> &'static dyn WorldCodec {
    &AnvilWorldIo
}

struct AnvilWorldIo;

impl WorldCodec for AnvilWorldIo {
    fn inspect(&self, root: &Path) -> JavaWorldAnalysis {
        world_import::inspect_anvil_world(root)
    }

    fn load_overlay(&self, root: &Path) -> Result<WorldOverlay, OverlayError> {
        WorldOverlay::open_anvil(root)
    }

    fn load_world(
        &self,
        root: &Path,
        options: WorldImportOptions,
    ) -> Result<(World, JavaWorldLoadReport), WorldImportError> {
        world_import::load_anvil_world(root, options)
    }

    fn decode_chunk(
        &self,
        nbt: &NbtFile,
        position: ChunkPos,
        dimension: &JavaWorldDimension,
    ) -> Result<DecodedChunk, AnvilError> {
        java_chunk::decode_chunk(nbt, position.x, position.z, dimension)
    }

    fn encode_chunk(
        &self,
        overlay: &WorldOverlay,
        dimension: &str,
        position: ChunkPos,
    ) -> Result<Option<NbtFile>, OverlayError> {
        overlay.encode_anvil_chunk(dimension, position.x, position.z)
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
        region_write::write_anvil_region(
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
        world_export::export_anvil_world(overlay, destination, options)
    }
}
