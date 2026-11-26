//! Managed Java runtimes and official server JARs for Minecraft Vanilla World generation
//! (M9.3-M9.5).
//!
//! The editor itself never needs Java.
//!
//! - Java candidates are discovered from FuryBuilder's cache, `JAVA_HOME`, `PATH`, common
//!   vendor directories and the official launcher's `runtimes/` folder;
//! - every Java candidate is executed with `-XshowSettings:properties -version`; its name is
//!   never trusted and only the Java major required by the target profile is accepted;
//! - a manual executable can be selected, but it must pass the same probe;
//! - managed Eclipse Temurin JREs are selected by the Java major the project target needs,
//!   downloaded only after an explicit request, from the official Adoptium API/allowlisted
//!   GitHub hosts, verified by SHA-256 and size, extracted with zip-slip/symlink/size
//!   protections into `<cache>/<major>/<release-id>`, probed and then published atomically;
//! - the official `server.jar` is resolved from Mojang's `version_manifest_v2` → version
//!   JSON → `downloads.server`, verified by the announced SHA-1/size, inspected for the
//!   target profile's exact `world_version` and published into a clearable offline cache;
//! - a manual local `server.jar` is accepted only after the same internal checks;
//! - isolated generation requires an explicit EULA acceptance for that exact JAR, binds the
//!   server to loopback only and places it in a kill-on-close Job Object with memory and time
//!   limits, periodic saves, progress and a guaranteed process-tree termination on cancel.
//!
//! Nothing is installed globally and no `PATH`/registry entry is changed. No EULA is ever
//! accepted automatically and no server starts without an explicit user action.

mod discovery;
mod error;
mod managed;
mod manager;
mod probe;
mod server;
mod settings;
mod target;
mod temurin;
mod types;

/// Eclipse Temurin majors that Minecraft Java versions require. Versions outside this list
/// are refused before any network request instead of guessing a compatible JRE.
pub const JAVA_MAJORS: &[u32] = &[8, 16, 17, 21, 25];

/// True when FuryBuilder can provision and verify a managed JRE for this Java major.
pub fn is_supported_java_major(major: u32) -> bool {
    JAVA_MAJORS.contains(&major)
}

/// Rejects a Java major that no Minecraft version profile in the supported range asks for.
pub fn validate_java_major(major: u32) -> Result<(), RuntimeError> {
    if is_supported_java_major(major) {
        Ok(())
    } else {
        Err(RuntimeError::Unsupported {
            target: target::target_minecraft_version(),
            detail: format!(
                "Java {major} is not a supported Minecraft runtime; expected one of {JAVA_MAJORS:?}"
            ),
        })
    }
}

pub use error::{RuntimeError, ServerError};
pub use managed::{
    sha256_file, ManagedRuntimeCache, RuntimeManifest, StagedRuntime,
    RUNTIME_MANIFEST_SCHEMA_VERSION,
};
pub use manager::RuntimeManager;
pub use probe::JavaProbe;
pub use server::{
    inspect_server_jar, sha1_file, DatapackInspection, DatapackSelection, GeneratedWorld,
    GenerationDatapack, GenerationEstimate, GenerationLimits, GenerationPhase, GenerationReport,
    GenerationRequest, GenerationRunInfo, GenerationRunsReport, GenerationSnapshot,
    GenerationStatusReport, GenerationStopReason, GenerationTarget, InstalledServerJar,
    ManagedServerCacheReport, ManagedServerJar, RunManifest, SavedEula, SavedServerJar,
    ServerDiscoveryReport, ServerJarInfo, ServerJarReport, ServerManager, ServerProgress,
    ServerReleaseInfo, ServerSource, ServerTarget, WorldPreset, EULA_URL, MAX_SERVER_JAR_BYTES,
    MIN_SERVER_JAR_BYTES, RUN_MANIFEST_SCHEMA_VERSION, SERVER_MANIFEST_SCHEMA_VERSION,
    SERVER_SETTINGS_SCHEMA_VERSION, VERSION_MANIFEST_URL,
};
pub use settings::RuntimeSettingsStore;
pub use target::{
    target_data_version, target_datapack_format, target_java_major, target_minecraft_version,
    target_profile,
};
pub use types::{
    InstalledRuntime, JavaRuntimeReport, ManagedCacheReport, ManagedRuntime,
    RuntimeDiscoveryReport, RuntimeProgress, RuntimeReleaseInfo, RuntimeSource, RuntimeStatus,
    RUNTIME_SETTINGS_SCHEMA_VERSION,
};
