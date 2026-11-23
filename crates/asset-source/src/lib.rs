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
//! - no file is written outside [`AssetSettingsStore`]'s own settings file;
//! - zip/JAR archives are parsed in memory, never extracted to disk;
//! - archive entry names are validated against traversal and absolute paths;
//! - decompressed bytes, entry counts and filesystem walks are capped;
//! - symlinks at the selected root are reported instead of followed.

mod archive;
mod bundle;
mod discovery;
mod jar;
mod resource_pack;
mod settings;
mod target;
mod types;

pub use bundle::{
    AssetBundle, AssetBundleCache, AssetBundleService, BlockStateRequest, BundleBlock,
    BundleElement, BundleElementRotation, BundleError, BundleFace, BundlePack, BundlePart,
    BundleTexture, ASSET_BUNDLE_SCHEMA_VERSION, MAX_BLOCK_STATES,
};
pub use discovery::{
    default_minecraft_roots, default_resource_pack_format, discover_assets, discover_root,
    discover_roots, inspect_asset_path,
};
pub use settings::{
    AssetSettingsError, AssetSettingsStore, AssetSourceSetting, SavedAssetSource,
    ASSET_SETTINGS_SCHEMA_VERSION, MAX_RESOURCE_PACKS,
};
pub use target::{
    target_data_version, target_minecraft_version, target_profile, target_resource_pack_format,
};
pub use types::{
    AssetDiscoveryReport, AssetPathReport, AssetSelectionKind, InstallationReport,
    InstallationSource, ResourcePackKind, ResourcePackReport, ResourcePackStatus, VersionReport,
    VersionStatus,
};
