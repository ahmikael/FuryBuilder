use serde::{Deserialize, Serialize};

/// Version of the small `runtime.json` settings file.
pub const RUNTIME_SETTINGS_SCHEMA_VERSION: u32 = 1;

/// Where a Java executable was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeSource {
    Managed,
    JavaHome,
    Path,
    Common,
    Manual,
}

impl RuntimeSource {
    pub fn as_str(self) -> &'static str {
        match self {
            RuntimeSource::Managed => "managed",
            RuntimeSource::JavaHome => "java_home",
            RuntimeSource::Path => "path",
            RuntimeSource::Common => "common",
            RuntimeSource::Manual => "manual",
        }
    }
}

/// Result of actually executing a candidate with `-XshowSettings:properties -version`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeStatus {
    Compatible,
    Outdated,
    Newer,
    UnsupportedArch,
    NotJava,
    Unreadable,
}

impl RuntimeStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            RuntimeStatus::Compatible => "compatible",
            RuntimeStatus::Outdated => "outdated",
            RuntimeStatus::Newer => "newer",
            RuntimeStatus::UnsupportedArch => "unsupported_arch",
            RuntimeStatus::NotJava => "not_java",
            RuntimeStatus::Unreadable => "unreadable",
        }
    }
}

/// A probed Java executable. Reports are facts from execution, not a filename guess.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaRuntimeReport {
    pub path: String,
    pub source: RuntimeSource,
    pub status: RuntimeStatus,
    pub version: Option<String>,
    pub major: Option<u32>,
    pub vendor: Option<String>,
    pub arch: Option<String>,
    pub vm: Option<String>,
    pub managed: bool,
    pub warnings: Vec<String>,
    pub error: Option<String>,
}

impl JavaRuntimeReport {
    pub fn is_compatible(&self) -> bool {
        self.status == RuntimeStatus::Compatible
    }
}

/// A verified runtime owned by FuryBuilder under its application-data directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedRuntime {
    pub id: String,
    pub path: String,
    pub java_path: String,
    pub version: String,
    pub release_name: String,
    pub sha256: String,
    pub source_url: String,
    pub vendor: String,
    pub os: String,
    pub arch: String,
    pub installed_at: u64,
    pub bytes: u64,
}

/// Download metadata shown to the user before any bytes are requested.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeReleaseInfo {
    pub version: String,
    pub release_name: String,
    pub download_url: String,
    pub sha256: String,
    pub size: u64,
    pub vendor: String,
    pub os: String,
    pub arch: String,
    pub image_type: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeDiscoveryReport {
    pub target_java_major: u32,
    pub target_minecraft_version: String,
    pub candidates: Vec<JavaRuntimeReport>,
    pub saved: Option<JavaRuntimeReport>,
    pub managed_root: String,
    pub managed: Vec<ManagedRuntime>,
    pub managed_bytes: u64,
    pub problems: Vec<String>,
    pub manual_hint: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledRuntime {
    pub runtime: ManagedRuntime,
    pub report: JavaRuntimeReport,
    pub selected: bool,
}

/// Progress is emitted for explicit, user-requested downloads only.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeProgress {
    pub phase: String,
    pub downloaded: u64,
    pub total: u64,
}

/// Managed cache inventory. `problems` explains skipped directories without deleting them.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedCacheReport {
    pub root: String,
    pub runtimes: Vec<ManagedRuntime>,
    pub total_bytes: u64,
    pub problems: Vec<String>,
}
