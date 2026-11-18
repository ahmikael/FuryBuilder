//! Managed Java runtimes for Minecraft Vanilla World generation (M9.3).
//!
//! The editor itself never needs Java. This crate is used only when the user opens the
//! Java/world-generation settings or starts a Vanilla World flow:
//!
//! - candidates are discovered from FuryBuilder's cache, `JAVA_HOME`, `PATH`, common vendor
//!   directories and the official launcher's `runtimes/` folder;
//! - every candidate is executed with `-XshowSettings:properties -version`; its name is
//!   never trusted and only Java 21 is accepted for Minecraft 1.21.4;
//! - a manual executable can be selected, but it must pass the same probe;
//! - managed Eclipse Temurin JREs are downloaded only after an explicit request, from the
//!   official Adoptium API/allowlisted GitHub hosts, verified by SHA-256 and size, extracted
//!   with zip-slip/symlink/size protections into the application data directory, probed and
//!   then published atomically.
//!
//! Nothing is installed globally, no `PATH`/registry entry is changed, no EULA is accepted
//! and no Minecraft server is started by this crate.

mod discovery;
mod error;
mod managed;
mod manager;
mod probe;
mod settings;
mod temurin;
mod types;

pub use error::RuntimeError;
pub use managed::{sha256_file, ManagedRuntimeCache, RuntimeManifest, StagedRuntime};
pub use manager::RuntimeManager;
pub use probe::JavaProbe;
pub use settings::RuntimeSettingsStore;
pub use types::{
    InstalledRuntime, JavaRuntimeReport, ManagedCacheReport, ManagedRuntime,
    RuntimeDiscoveryReport, RuntimeProgress, RuntimeReleaseInfo, RuntimeSource, RuntimeStatus,
    RUNTIME_SETTINGS_SCHEMA_VERSION, TARGET_JAVA_MAJOR, TARGET_MINECRAFT_VERSION,
};
