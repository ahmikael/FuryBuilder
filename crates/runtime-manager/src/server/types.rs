use minecraft_version::VersionId;
use serde::{Deserialize, Serialize};

use crate::error::ServerError;
use crate::target::{target_data_version, target_java_major, target_minecraft_version};

/// Version of `<app-data>/server.json`. Schema 2 keeps one selection per exact Minecraft
/// version; older schemas are not read.
pub const SERVER_SETTINGS_SCHEMA_VERSION: u32 = 2;
/// Version of the manifest stored next to a managed server JAR.
pub const SERVER_MANIFEST_SCHEMA_VERSION: u32 = 2;

/// Exact facts a server JAR must declare to belong to one project target.
///
/// The server's own `version.json` carries the version id, `world_version` (DataVersion)
/// and the required Java major. A JAR whose facts differ is never silently substituted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerTarget {
    pub minecraft_version: VersionId,
    pub data_version: i32,
    pub java_major: u32,
}

impl ServerTarget {
    pub fn new(
        minecraft_version: VersionId,
        data_version: i32,
        java_major: u32,
    ) -> Result<Self, ServerError> {
        if data_version <= 0 {
            return Err(ServerError::Invalid(format!(
                "server target DataVersion must be positive, got {data_version}"
            )));
        }
        if java_major == 0 {
            return Err(ServerError::Invalid(
                "server target Java major must be positive".to_owned(),
            ));
        }
        Ok(Self {
            minecraft_version,
            data_version,
            java_major,
        })
    }

    /// The profile the isolated generation path still supports. Component provisioning is
    /// no longer limited to it, but generation remains an explicit baseline capability.
    pub fn baseline() -> Self {
        let profile = crate::target::target_profile();
        Self {
            minecraft_version: profile.minecraft_version.clone(),
            data_version: profile.data_version,
            java_major: profile.java.recommended_major,
        }
    }
}
/// Hard cap for a downloaded/manual server JAR (the official baseline jar is ~54 MiB).
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
    pub version_json_sha1: String,
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
    pub eula: Option<SavedEula>,
    pub eula_matches_selected: bool,
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
    pub minecraft_version: String,
    pub path: String,
    pub sha1: String,
    pub size: u64,
    pub data_version: i32,
}

impl ServerDiscoveryReport {
    pub fn target() -> (&'static str, i32, u32) {
        (
            target_minecraft_version(),
            target_data_version(),
            target_java_major(),
        )
    }
}

/// Official Minecraft EULA text. FuryBuilder never accepts it on the user's behalf.
pub const EULA_URL: &str = "https://aka.ms/MinecraftEULA";
/// Version of the `.furybuilder-run.json` manifest in a generation run directory.
pub const RUN_MANIFEST_SCHEMA_VERSION: u32 = 1;

/// An explicit EULA acceptance tied to the exact server JAR that will be executed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedEula {
    pub accepted_at: u64,
    pub minecraft_version: String,
    pub jar_sha1: String,
    pub eula_url: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorldPreset {
    #[default]
    Normal,
    Flat,
    LargeBiomes,
    Amplified,
}

impl WorldPreset {
    pub fn level_type(self) -> &'static str {
        match self {
            WorldPreset::Normal => "minecraft:normal",
            WorldPreset::Flat => "minecraft:flat",
            WorldPreset::LargeBiomes => "minecraft:large_biomes",
            WorldPreset::Amplified => "minecraft:amplified",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            WorldPreset::Normal => "normal",
            WorldPreset::Flat => "flat",
            WorldPreset::LargeBiomes => "large_biomes",
            WorldPreset::Amplified => "amplified",
        }
    }
}

/// User-requested isolated generation. Values are validated again on the backend; this is
/// not a promise that every Minecraft world setting is exposed.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationRequest {
    pub seed: String,
    #[serde(default)]
    pub preset: WorldPreset,
    pub center_x: i32,
    pub center_z: i32,
    pub radius_chunks: u32,
    pub time_limit_seconds: u32,
    pub memory_mb: u32,
    pub view_distance: u8,
    pub cpu_threads: u8,
    /// Worldgen structures. This is a supported server setting, not a FuryBuilder generator.
    #[serde(default = "default_true")]
    pub generate_structures: bool,
    /// Separate from the EULA: the user explicitly starts the local Java process for this run.
    #[serde(default)]
    pub consent: bool,
    /// Locally selected `.zip` data packs, validated and copied into the isolated world.
    #[serde(default)]
    pub datapacks: Vec<DatapackSelection>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DatapackSelection {
    pub path: String,
}

/// A validated data pack recorded in the run manifest and terminal report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationDatapack {
    pub name: String,
    pub path: String,
    pub sha256: String,
    pub size: u64,
    pub pack_format: u32,
    #[serde(default)]
    pub warnings: Vec<String>,
}

/// Per-path result for the wizard. Invalid entries are reported individually and never start
/// a run; the backend re-validates the accepted selection when generation starts.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatapackInspection {
    pub path: String,
    pub name: String,
    pub sha256: Option<String>,
    pub size: Option<u64>,
    pub pack_format: Option<u32>,
    pub valid: bool,
    pub error: Option<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GenerationPhase {
    Preparing,
    Starting,
    Generating,
    Saving,
    Stopping,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GenerationStopReason {
    Completed,
    Incomplete,
    TimeLimit,
    Cancelled,
    Crashed,
    StartupFailed,
    IsolationFailed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationSnapshot {
    pub run_id: String,
    pub running: bool,
    pub phase: GenerationPhase,
    pub elapsed_ms: u64,
    pub expected_chunks: u64,
    pub generated_chunks: u64,
    pub region_files: u64,
    pub world_bytes: u64,
    pub message: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationReport {
    pub snapshot: GenerationSnapshot,
    pub stop_reason: GenerationStopReason,
    pub world_complete: bool,
    pub exit_code: Option<i32>,
    pub server_pid: Option<u32>,
    pub seed: String,
    pub preset: WorldPreset,
    pub center_x: i32,
    pub center_z: i32,
    pub radius_chunks: u32,
    pub time_limit_seconds: u32,
    pub memory_mb: u32,
    pub view_distance: u8,
    pub cpu_threads: u8,
    #[serde(default)]
    pub generate_structures: bool,
    pub java_path: String,
    pub java_version: String,
    pub server_jar: String,
    pub server_sha1: String,
    pub eula_accepted_at: u64,
    #[serde(default)]
    pub consent_at: u64,
    #[serde(default)]
    pub datapacks: Vec<GenerationDatapack>,
    pub bind_address: Option<String>,
    pub process_tree_guarded: bool,
    pub run_dir: String,
    pub world_dir: String,
    pub log_path: String,
    pub started_at: u64,
    pub finished_at: u64,
    pub log_tail: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationStatusReport {
    pub active: Option<GenerationSnapshot>,
    pub last: Option<GenerationReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationRunInfo {
    pub run_id: String,
    pub path: String,
    pub status: String,
    pub created_at: u64,
    pub finished_at: Option<u64>,
    pub chunks: u64,
    pub expected_chunks: u64,
    pub region_files: u64,
    pub world_bytes: u64,
    pub world_complete: Option<bool>,
    pub stop_reason: Option<GenerationStopReason>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationRunsReport {
    pub root: String,
    pub runs: Vec<GenerationRunInfo>,
    pub total_bytes: u64,
    pub problems: Vec<String>,
}

/// A completed, manifest-owned generation run whose world can be imported into a project.
/// Only terminal runs reported as completed are exposed; the world directory and report are
/// re-verified when this value is produced, and the source folder is never modified.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedWorld {
    pub run_id: String,
    pub world_dir: String,
    pub seed: String,
    pub preset: WorldPreset,
    pub center_x: i32,
    pub center_z: i32,
    pub radius_chunks: u32,
    pub generate_structures: bool,
    pub datapacks: Vec<GenerationDatapack>,
    pub java_version: String,
    pub server_sha1: String,
    pub chunks: u64,
    pub region_files: u64,
    pub world_bytes: u64,
    pub finished_at: u64,
}

/// Marker written before any server process starts. Only directories carrying this manifest
/// may be deleted by `Clear generation runs`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunManifest {
    pub schema_version: u32,
    pub run_id: String,
    pub created_at: u64,
    pub seed: String,
    pub preset: WorldPreset,
    pub center_x: i32,
    pub center_z: i32,
    pub radius_chunks: u32,
    #[serde(default = "default_true")]
    pub generate_structures: bool,
    #[serde(default)]
    pub datapacks: Vec<GenerationDatapack>,
}
