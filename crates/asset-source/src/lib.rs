//! Read-only discovery and validation of local Minecraft Java assets (M9.1).
//!
//! FuryBuilder never downloads, bundles or redistributes Minecraft client JARs, asset
//! indexes, textures or resource packs. This crate only inspects files the user already
//! has, either in conventional launcher roots or at a path the user explicitly selects.
//!
//! The target is the same fixed version as the world adapters: Minecraft Java 1.21.4,
//! `DataVersion` 4189 and resource pack format 46. Other versions and modded profiles are
//! reported, not silently treated as compatible.
//!
//! Security boundaries:
//!
//! - no file is written outside [`AssetSettingsStore`]'s own settings file;
//! - zip/JAR archives are parsed in memory, never extracted to disk;
//! - archive entry names are validated against traversal and absolute paths;
//! - decompressed bytes, entry counts and filesystem walks are capped;
//! - symlinks at the selected root are reported instead of followed.

mod archive;
mod discovery;
mod jar;
mod resource_pack;
mod settings;
mod types;

pub use discovery::{
    default_minecraft_roots, discover_assets, discover_root, discover_roots, inspect_asset_path,
    DEFAULT_RESOURCE_PACK_FORMAT,
};
pub use settings::{
    AssetSettingsError, AssetSettingsStore, AssetSourceSetting, SavedAssetSource,
    ASSET_SETTINGS_SCHEMA_VERSION,
};
pub use types::{
    AssetDiscoveryReport, AssetPathReport, AssetSelectionKind, InstallationReport,
    InstallationSource, ResourcePackKind, ResourcePackReport, ResourcePackStatus, VersionReport,
    VersionStatus, TARGET_DATA_VERSION, TARGET_MINECRAFT_VERSION, TARGET_RESOURCE_PACK_FORMAT,
};
