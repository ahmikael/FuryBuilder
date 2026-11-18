//! Versioned import/export adapters for Minecraft Java formats.
//!
//! This crate converts between the authoritative `world_domain::World` and interchange
//! formats. It owns a bounded NBT codec and a format-neutral structure model so adapters
//! cannot bypass validation. The native FuryBuilder project format stays in
//! `project_store`; interchange formats are never a storage backend for editor metadata.
//!
//! First adapter (ADR-007): Java structure `.nbt` for Minecraft Java 1.21.4
//! (`DataVersion` 4189). Sponge `.schem` v3 is the planned second adapter.

pub mod anvil;
pub mod exchange;
pub mod java_chunk;
pub mod java_structure;
pub mod nbt;
pub mod structure;

pub use anvil::{
    region_file_name, AnvilError, ChunkCompression, JavaWorld, JavaWorldDimension, LevelDat,
    RegionChunk, RegionChunkSummary, RegionEntry, RegionFile, ANVIL_HEADER_BYTES,
    ANVIL_LOCATION_ENTRIES, ANVIL_REGION_AXIS_CHUNKS, ANVIL_REGION_EXTENSION, ANVIL_SECTOR_BYTES,
    MAX_LEVEL_DAT_BYTES, MAX_REGION_FILE_BYTES,
};
pub use exchange::{
    content_bounds, import_preview, palette_counts, palette_usage, prepare_export,
    preview_from_structure, read_verified_import, ExportBundle, PaletteCount,
    StructureImportPreview, MAX_PALETTE_PREVIEW, MAX_SCAN_SECTIONS,
};
pub use java_chunk::{
    BiomeSection, DecodedChunk, DecodedSection, BIOME_SECTION_VOLUME, BLOCK_SECTION_VOLUME,
    MAX_BIOME_PALETTE, MAX_BLOCK_PALETTE,
};
pub use java_structure::{
    inspect_java_structure, read_java_structure, read_java_structure_unchecked,
    write_java_structure, JavaStructure, JavaStructureError, StructureInspection,
    JAVA_STRUCTURE_EXTENSION, JAVA_STRUCTURE_FORMAT,
};
pub use nbt::{NbtError, NbtFile, NbtValue};
pub use structure::{
    Structure, StructureBlock, StructureError, MAX_STRUCTURE_AXIS, MAX_STRUCTURE_PALETTE,
    MAX_STRUCTURE_VOLUME,
};
