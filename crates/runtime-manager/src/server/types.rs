use serde::{Deserialize, Serialize};

use crate::types::{TARGET_JAVA_MAJOR, TARGET_MINECRAFT_VERSION};

/// `world_version` in the official 1.21.4 server's root `version.json`.
pub const TARGET_DATA_VERSION: i32 = 4189;
/// Version of `<app-data>/server.json`.
pub const SERVER_SETTINGS_SCHEMA_VERSION: u32 = 1;
/// Version of the manifest stored next to a managed server JAR.
pub const SERVER_MANIFEST_SCHEMA_VERSION: u32 = 1;
/// Hard cap for a downloaded/manual server JAR (the official 1.21.4 jar is ~54 MiB).
pub const MAX_SERVER_JAR_BYTES: u64 = 200 * 1024 * 1024;
/// A real server jar is larger than this; smaller files are refused early.
pub const MIN_SERVER_JAR_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerSource {
    Managed,
    Manual,
}

impl ServerSource {
    pub fn as_str(self) -> &'static str {
        match self {
            ServerSource::Managed => "managed",
            ServerSource::Manual => "manual",
        }
    }
}

/// A selected or cached server JAR, with facts from the actual file.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerJarReport {
    pub path: String,
    pub source: ServerSource,
    pub managed: bool,
    pub ready: bool,
    pub version: Option<String>,
    pub data_version: Option<i32>,
    pub java_major: Option<u32>,
    pub protocol_version: Option<i32>,
    pub resource_pack_format: Option<i32>,
    pub sha1: Option<String>,
    pub size: Option<u64>,
    pub matches_official: Option<bool>,
    pub warnings: Vec<String>,
    pub error: Option<String>,
}

/// A JAR published by FuryBuilder under its own managed cache.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedServerJar {
    pub version: String,
    pub path: String,
    pub jar_path: String,
    pub sha1: String,
    pub size: u64,
    pub data_version: i32,
    pub java_major: u32,
    pub release_time: String,
    pub source_url: String,
    pub installed_at: u64,
}

/// Download metadata shown before any bytes are requested.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerReleaseInfo {
    pub minecraft_version: String,
    pub version_type: String,
    pub release_time: String,
    pub manifest_url: String,
    pub version_json_sha1: String,
    pub download_url: String,
    pub sha1: String,
    pub size: u64,
    pub java_major: u32,
    pub expected_data_version: i32,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerDiscoveryReport {
    pub target_minecraft_version: String,
    pub target_data_version: i32,
    pub target_java_major: u32,
    pub selected: Option<ServerJarReport>,
    pub managed_root: String,
    pub managed: Vec<ManagedServerJar>,
    pub managed_bytes: u64,
    pub problems: Vec<String>,
    pub manual_hint: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedServerCacheReport {
    pub root: String,
    pub servers: Vec<ManagedServerJar>,
    pub total_bytes: u64,
    pub problems: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerProgress {
    pub phase: String,
    pub downloaded: u64,
    pub total: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledServerJar {
    pub server: ManagedServerJar,
    pub report: ServerJarReport,
    pub selected: bool,
}

/// What `<app-data>/server.json` remembers about a manual selection. The file is re-probed
/// whenever it is shown; this record never replaces validation of the actual JAR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedServerJar {
    pub path: String,
    pub sha1: String,
    pub size: u64,
    pub data_version: i32,
}

impl ServerDiscoveryReport {
    pub fn target() -> (&'static str, i32, u32) {
        (
            TARGET_MINECRAFT_VERSION,
            TARGET_DATA_VERSION,
            TARGET_JAVA_MAJOR,
        )
    }
}
