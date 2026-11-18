//! Managed Java runtimes and official server JARs for Minecraft Vanilla World generation
//! (M9.3-M9.4).
//!
//! The editor itself never needs Java.
//!
//! - Java candidates are discovered from FuryBuilder's cache, `JAVA_HOME`, `PATH`, common
//!   vendor directories and the official launcher's `runtimes/` folder;
//! - every Java candidate is executed with `-XshowSettings:properties -version`; its name is
//!   never trusted and only Java 21 is accepted for Minecraft 1.21.4;
//! - a manual executable can be selected, but it must pass the same probe;
//! - managed Eclipse Temurin JREs are downloaded only after an explicit request, from the
//!   official Adoptium API/allowlisted GitHub hosts, verified by SHA-256 and size, extracted
//!   with zip-slip/symlink/size protections into the application data directory, probed and
//!   then published atomically;
//! - the official `server.jar` is resolved from Mojang's `version_manifest_v2` → version
//!   JSON → `downloads.server`, verified by the announced SHA-1/size, inspected for the
//!   exact 1.21.4 `world_version` 4189 and published into a clearable offline cache;
//! - a manual local `server.jar` is accepted only after the same internal checks.
//!
//! Nothing is installed globally, no `PATH`/registry entry is changed, no EULA is accepted
//! and no Minecraft server is started by this crate.

mod discovery;
mod error;
mod managed;
mod manager;
mod net;
mod probe;
mod server;
mod settings;
mod temurin;
mod types;

pub use error::{RuntimeError, ServerError};
pub use managed::{sha256_file, ManagedRuntimeCache, RuntimeManifest, StagedRuntime};
pub use manager::RuntimeManager;
pub use probe::JavaProbe;
pub use server::{
    inspect_server_jar, sha1_file, InstalledServerJar, ManagedServerCacheReport, ManagedServerJar,
    SavedServerJar, ServerDiscoveryReport, ServerJarInfo, ServerJarReport, ServerManager,
    ServerProgress, ServerReleaseInfo, ServerSource, MAX_SERVER_JAR_BYTES, MIN_SERVER_JAR_BYTES,
    SERVER_MANIFEST_SCHEMA_VERSION, SERVER_SETTINGS_SCHEMA_VERSION, TARGET_DATA_VERSION,
    VERSION_MANIFEST_URL,
};
pub use settings::RuntimeSettingsStore;
pub use types::{
    InstalledRuntime, JavaRuntimeReport, ManagedCacheReport, ManagedRuntime,
    RuntimeDiscoveryReport, RuntimeProgress, RuntimeReleaseInfo, RuntimeSource, RuntimeStatus,
    RUNTIME_SETTINGS_SCHEMA_VERSION, TARGET_JAVA_MAJOR, TARGET_MINECRAFT_VERSION,
};
