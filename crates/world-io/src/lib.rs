//! Versioned import/export adapters for Minecraft Java formats.
//!
//! This crate converts between the authoritative `world_domain::World` and interchange
//! formats. It owns a bounded NBT codec and a format-neutral structure model so adapters
//! cannot bypass validation. The native FuryBuilder project format stays in
//! `project_store`; interchange formats are never a storage backend for editor metadata.
//!
//! First adapter (ADR-007): Java structure `.nbt` for the verified classic profile.
//! Sponge `.schem` v3 is the planned second adapter.

mod classic_anvil;
pub mod derived;
pub mod exchange;
pub mod java_structure;
pub mod nbt;
pub mod parity;
pub mod passthrough;
mod split_anvil;
pub mod structure;
pub mod support_matrix;
pub mod version_support;
mod world_codec;
pub mod world_selection;
pub mod world_source;

pub use classic_anvil::flattened_biomes::{
    decode_flattened_chunk_biomes, patch_flattened_chunk_biomes, ColumnBiomePolicy,
    FlattenedBiomeError, FlattenedBiomeProjection,
};
pub use classic_anvil::{
    chunk as java_chunk, overlay, reader as anvil, region_write, world_export, world_import,
};

pub use anvil::{
    region_file_name, AnvilError, ChunkCompression, JavaWorld, JavaWorldDimension, LevelDat,
    RegionChunk, RegionChunkSummary, RegionEntry, RegionFile, ANVIL_HEADER_BYTES,
    ANVIL_LOCATION_ENTRIES, ANVIL_REGION_AXIS_CHUNKS, ANVIL_REGION_EXTENSION, ANVIL_SECTOR_BYTES,
    MAX_LEVEL_DAT_BYTES, MAX_REGION_FILE_BYTES,
};
pub use derived::{
    analyze_java_world_derived, inspect_chunk_derived, ChunkDerivedData, ChunkFingerprints,
    DerivedCounters, DerivedDataError, DerivedDataReport, DimensionDerivedData, HeightmapInfo,
    SidecarScan, MAX_DERIVED_CHUNKS, MAX_DERIVED_REGION_FILES, MAX_DERIVED_SAMPLES,
    MAX_DERIVED_SIDECAR_RECORDS,
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
    inspect_java_structure, read_java_structure, write_java_structure, JavaStructure,
    JavaStructureError, StructureInspection, JAVA_STRUCTURE_EXTENSION, JAVA_STRUCTURE_FORMAT,
};
pub use nbt::{NbtError, NbtFile, NbtValue};
pub use overlay::{
    ApplyReport, BiomeChange, BiomeEdit, BlockChange, BlockEdit, ChunkKey,
    OverlayBiomeSectionDocument, OverlayChunk, OverlayChunkDocument, OverlayDocument, OverlayError,
    OverlaySectionDocument, WorldOverlay, MAX_OVERLAY_CHUNKS, MAX_OVERLAY_JSON_BYTES,
    MAX_OVERLAY_SECTIONS, OVERLAY_MIN_SCHEMA_VERSION, OVERLAY_SCHEMA_VERSION,
};
pub use parity::{
    compare_java_world_to_world, reference_chunks, ParityChunkRef, ParityError, ParityMismatch,
    ParityOptions, ParityReport, ParityStatus, MAX_PARITY_BLOCKS, MAX_PARITY_CHUNKS,
    MAX_PARITY_MISMATCHES, PARITY_DIMENSION,
};
pub use passthrough::{
    passthrough_java_world, passthrough_java_world_observed, passthrough_java_world_with_options,
    CopiedFile, CopyObserver, NoopCopyObserver, PassthroughError, PassthroughOptions,
    PassthroughReport, SkippedEntry, MAX_PASSTHROUGH_FILES, MAX_PASSTHROUGH_FILE_BYTES,
    MAX_PASSTHROUGH_TOTAL_BYTES,
};
pub use region_write::{
    verified_write_support, write_region_from_overlay, write_region_from_overlay_with_policy,
    write_verified_region, write_verified_region_with_policy, DerivedDataPolicy, RegionWriteError,
    RegionWriteReport, VerifiedChunkReplacement, VerifiedWriteSupport, VERIFIED_WRITE_FIELDS,
    VERIFIED_WRITE_LAYOUT,
};
pub use structure::{
    Structure, StructureBlock, StructureError, MAX_STRUCTURE_AXIS, MAX_STRUCTURE_PALETTE,
    MAX_STRUCTURE_VOLUME,
};
pub use support_matrix::{
    support_matrix, CategorySupport, SupportMatrix, VersionSupport, SUPPORT_MATRIX_SCHEMA_VERSION,
};
pub use version_support::{
    default_version_profile, profile_for, supported_data_versions, supports_data_version,
    world_version,
};
pub use world_export::{
    export_java_world, export_java_world_with_options, ExportedRegion, WorldExportError,
    WorldExportOptions, WorldExportReport, MAX_EXPORT_REGIONS,
};
pub use world_import::{
    analyze_java_world, load_java_world, load_java_world_with_options, ImportDimensionAnalysis,
    JavaWorldAnalysis, JavaWorldLoadReport, WorldImportError, WorldImportOptions,
    MAX_IMPORT_CHUNKS, MAX_IMPORT_NON_AIR_BLOCKS, MAX_IMPORT_SECTIONS, SUPPORTED_LAYOUT,
};
pub use world_selection::{
    classify_world_selection, SelectionDimension, SelectionRegionFile, WorldSelectionKind,
    WorldSelectionReport, CLASSIC_ANVIL_LAYOUT,
};
pub use world_source::{
    analyze_world_source, prepare_world_source, ArchiveReport, ImportStrategy, PreparedWorldSource,
    WorldSourceError, WorldSourceKind, MAX_ARCHIVE_BYTES, MAX_ARCHIVE_ENTRIES,
    MAX_ARCHIVE_ENTRY_BYTES, MAX_WORLD_ROOT_DEPTH,
};
