use serde::Serialize;

/// The one Minecraft Java version whose world and asset formats this build verifies.
pub const TARGET_MINECRAFT_VERSION: &str = "1.21.4";
/// `DataVersion` stored by 1.21.4 worlds and by its internal `version.json`.
pub const TARGET_DATA_VERSION: i32 = 4189;
/// Resource pack format declared by the 1.21.4 client `version.json`.
pub const TARGET_RESOURCE_PACK_FORMAT: i32 = 46;

/// What a filesystem selection turned out to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetSelectionKind {
    /// A folder with `versions/` or `assets/`: a launcher/profile root.
    MinecraftRoot,
    /// A single `versions/<id>/` folder.
    VersionDirectory,
    /// A `client.jar` or another version JAR.
    ClientJar,
    /// A resource pack `.zip` or folder.
    ResourcePack,
    NotFound,
    UnsafeLink,
    Unknown,
}

impl AssetSelectionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            AssetSelectionKind::MinecraftRoot => "minecraft_root",
            AssetSelectionKind::VersionDirectory => "version_directory",
            AssetSelectionKind::ClientJar => "client_jar",
            AssetSelectionKind::ResourcePack => "resource_pack",
            AssetSelectionKind::NotFound => "not_found",
            AssetSelectionKind::UnsafeLink => "unsafe_link",
            AssetSelectionKind::Unknown => "unknown",
        }
    }
}

/// How an installation root was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallationSource {
    DefaultRoot,
    LauncherProfile,
    Manual,
}

impl InstallationSource {
    pub fn as_str(self) -> &'static str {
        match self {
            InstallationSource::DefaultRoot => "default_root",
            InstallationSource::LauncherProfile => "launcher_profile",
            InstallationSource::Manual => "manual",
        }
    }
}

/// Compatibility of one version folder with the fixed 1.21.4 target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VersionStatus {
    /// The target version, its client JAR checksum verified and its internal
    /// `version.json` agrees with `DataVersion` 4189.
    Compatible,
    /// The target version, internally consistent, but no checksum was available to
    /// verify the JAR against.
    CompatibleUnverified,
    /// A different Minecraft version.
    WrongVersion,
    /// A modded profile (`inheritsFrom`) that is reported but not used as vanilla assets.
    Modded,
    MissingMetadata,
    MissingClientJar,
    ChecksumMismatch,
    WorldVersionMismatch,
    InvalidMetadata,
    InvalidClientJar,
}

impl VersionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            VersionStatus::Compatible => "compatible",
            VersionStatus::CompatibleUnverified => "compatible_unverified",
            VersionStatus::WrongVersion => "wrong_version",
            VersionStatus::Modded => "modded",
            VersionStatus::MissingMetadata => "missing_metadata",
            VersionStatus::MissingClientJar => "missing_client_jar",
            VersionStatus::ChecksumMismatch => "checksum_mismatch",
            VersionStatus::WorldVersionMismatch => "world_version_mismatch",
            VersionStatus::InvalidMetadata => "invalid_metadata",
            VersionStatus::InvalidClientJar => "invalid_client_jar",
        }
    }
}

/// One installed Minecraft version, described without launching it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionReport {
    pub id: String,
    pub version_type: Option<String>,
    pub release_time: Option<String>,
    pub java_major: Option<u32>,
    pub inherits_from: Option<String>,
    pub metadata_path: String,
    pub metadata_present: bool,
    pub jar_path: String,
    pub jar_present: bool,
    pub jar_bytes: Option<u64>,
    pub expected_sha1: Option<String>,
    pub actual_sha1: Option<String>,
    pub checksum_verified: bool,
    pub internal_version_id: Option<String>,
    pub world_version: Option<i32>,
    pub resource_pack_format: Option<i32>,
    pub java_version_from_jar: Option<u32>,
    pub asset_index: Option<String>,
    pub asset_index_present: bool,
    pub asset_index_checksum_verified: bool,
    pub status: VersionStatus,
    pub notes: Vec<String>,
}

/// One resource pack found in `resourcepacks/` or selected manually.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourcePackReport {
    pub path: String,
    pub name: String,
    pub kind: ResourcePackKind,
    pub bytes: u64,
    pub pack_format: Option<i32>,
    pub supported_formats: Option<String>,
    pub description: Option<String>,
    pub compatible: bool,
    pub has_blockstates: bool,
    pub has_textures: bool,
    pub entry_count: usize,
    pub status: ResourcePackStatus,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourcePackKind {
    Zip,
    Folder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourcePackStatus {
    Compatible,
    IncompatibleFormat,
    MissingMetadata,
    InvalidMetadata,
    UnsafeContents,
    Unreadable,
}

/// One launcher or instance root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallationReport {
    pub root: String,
    pub source: InstallationSource,
    pub versions_dir_present: bool,
    pub assets_dir_present: bool,
    pub asset_indexes: usize,
    pub versions: Vec<VersionReport>,
    pub compatible_versions: Vec<String>,
    pub resource_packs: Vec<ResourcePackReport>,
    pub warnings: Vec<String>,
}

/// Result of scanning every known root and launcher profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetDiscoveryReport {
    pub target_minecraft_version: String,
    pub target_data_version: i32,
    pub target_resource_pack_format: i32,
    pub default_root_present: bool,
    pub searched_roots: Vec<String>,
    pub installations: Vec<InstallationReport>,
    pub problems: Vec<String>,
    pub manual_hint: String,
}

/// Result of inspecting one explicitly selected path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetPathReport {
    pub path: String,
    pub kind: AssetSelectionKind,
    /// True when the selection can be used for the fixed target.
    pub valid: bool,
    /// True only for a compatible 1.21.4 root or client JAR.
    pub target_match: bool,
    pub installation: Option<InstallationReport>,
    pub version: Option<VersionReport>,
    pub resource_pack: Option<ResourcePackReport>,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}
