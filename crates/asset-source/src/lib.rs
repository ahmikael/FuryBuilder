//! Read-only discovery and validation of local Minecraft Java assets (M9.1).
//!
//! FuryBuilder never downloads, bundles or redistributes Minecraft client JARs, asset
//! indexes, textures or resource packs. This crate only inspects files the user already
//! has, either in conventional launcher roots or at a path the user explicitly selects.
//!
//! The target version, its `DataVersion` and its resource pack format come from the
//! version catalog. Other versions and modded profiles are reported, not silently treated
//! as compatible.
//!
//! Security boundaries:
//!
//! - explicit pack imports write private snapshots only to the application asset cache;
//! - zip/JAR archives are parsed in memory, never extracted to disk;
//! - archive entry names are validated against traversal and absolute paths;
//! - decompressed bytes, entry counts and filesystem walks are capped;
//! - symlinks at the selected root are reported instead of followed.

mod archive;
mod bundle;
mod catalog;
mod catalog_atlas;
mod catalog_diff;
mod catalog_flags;
mod discovery;
mod jar;
mod legacy;
mod pack_store;
mod resource_pack;
mod settings;
mod target;
mod types;

pub use bundle::{
    AssetBundle, AssetBundleCache, AssetBundleService, BlockStateRequest, BundleBlock,
    BundleElement, BundleElementRotation, BundleError, BundleFace, BundlePack, BundlePart,
    BundleTexture, ASSET_BUNDLE_SCHEMA_VERSION, MAX_BLOCK_STATES,
};
pub use catalog::{
    AssetCatalogService, CatalogAtlas, CatalogAtlasSource, CatalogBuilder, CatalogCounts,
    CatalogEntry, CatalogError, CatalogFlags, CatalogItemForm, CatalogLegacyId, CatalogSnapshot,
    CatalogStates, CatalogVariant, CatalogVariantModel, CATALOG_SCHEMA_VERSION,
    MAX_CATALOG_ATLAS_SOURCES, MAX_CATALOG_BLOCKS, MAX_CATALOG_ITEMS, MAX_CATALOG_JSON_BYTES,
    MAX_CATALOG_LANG_ENTRIES, MAX_CATALOG_TAGS, MAX_CATALOG_TEXTURES, MAX_CATALOG_WARNINGS,
};
pub use catalog_atlas::{
    CatalogAtlasTexture, CatalogIconAtlas, CatalogIconAtlasService, CATALOG_ATLAS_SCHEMA_VERSION,
    MAX_CATALOG_ATLAS_REQUEST_TEXTURES,
};
pub use catalog_diff::{
    CatalogDiff, CatalogDiffEntry, CatalogDiffError, CatalogEntryChange,
    CatalogPropertyValuesChange, CatalogRename, CatalogRenameEvidence, CatalogStatesChange,
    CatalogStringChange,
};
pub use component_model::MAX_RESOURCE_PACKS;
pub use discovery::{
    default_minecraft_roots, default_resource_pack_format, discover_assets, discover_root,
    discover_roots, inspect_asset_path, inspect_asset_path_for,
};
pub use pack_store::{
    PackIssue, PackState, ProjectPackStatus, ResourcePackInput, ResourcePackStore,
};
pub use settings::{
    AssetSettingsError, AssetSettingsStore, AssetSourceSetting, SavedAssetSource,
    ASSET_SETTINGS_SCHEMA_VERSION,
};
pub use target::{
    target_data_version, target_minecraft_version, target_profile, target_resource_pack_format,
};
pub use types::{
    AssetDiscoveryReport, AssetPathReport, AssetSelectionKind, InstallationReport,
    InstallationSource, ResourcePackKind, ResourcePackReport, ResourcePackStatus, VersionReport,
    VersionStatus,
};
