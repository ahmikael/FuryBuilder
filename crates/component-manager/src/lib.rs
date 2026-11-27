//! Unified provisioning facade for the three project components.
//!
//! A project may need a Java runtime, the Minecraft client assets and an official
//! server JAR. This crate turns the three existing managers into one API with one
//! status vocabulary:
//!
//! - [`ComponentManager::status`] reports what the current project target needs and
//!   what is actually installed;
//! - [`ComponentManager::resolve`] returns the verified path and identity;
//! - [`ComponentManager::relink`] validates and records a manual source;
//! - [`ComponentManager::install`] downloads only when explicitly asked, with
//!   progress and cooperative cancellation;
//! - [`ComponentManager::verify`] re-inspects the selected artifact;
//! - [`ComponentManager::remove`] removes FuryBuilder-owned data or clears a manual
//!   pointer, never the user's file.
//!
//! The facade never downloads in the background, never edits a Minecraft
//! installation and never treats a placeholder as a verified component.

mod client_jar;

pub use client_jar::{
    ClientJarError, ClientJarManager, ClientJarProgress, ClientJarReleaseInfo, ManagedClientJar,
    ManagedClientJarReport, ASSET_MANIFEST_SCHEMA_VERSION, MAX_CLIENT_JAR_BYTES,
    MIN_CLIENT_JAR_BYTES,
};
pub use download_pipeline::{DownloadClient, NetworkConfig};

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use asset_source::{
    AssetPathReport, AssetSelectionKind, AssetSettingsError, AssetSettingsStore,
    AssetSourceSetting, VersionStatus,
};
use component_model::ComponentError as StatusError;
pub use component_model::{
    ComponentHash, ComponentKind, ComponentMode, ComponentState, ComponentStatus, HashAlgorithm,
};
use minecraft_version::{VersionId, VersionProfile};
use runtime_manager::{
    inspect_server_jar, JavaRuntimeReport, ManagedCacheReport, ManagedServerCacheReport,
    RuntimeError, RuntimeManager, RuntimeReleaseInfo, ServerError, ServerManager, ServerProgress,
    ServerReleaseInfo, ServerTarget,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Facts a component status is checked against. They come from the project target and
/// its [`VersionProfile`], never from a global UI setting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentTarget {
    pub profile_id: String,
    pub minecraft_version: VersionId,
    pub data_version: i32,
    pub java_major: u32,
}

impl ComponentTarget {
    pub fn from_profile(profile: &VersionProfile) -> Self {
        Self {
            profile_id: profile.id.clone(),
            minecraft_version: profile.minecraft_version.clone(),
            data_version: profile.data_version,
            java_major: profile.java.recommended_major,
        }
    }

    pub fn validate(&self) -> Result<(), ComponentError> {
        if self.profile_id.trim().is_empty() {
            return Err(ComponentError::InvalidRequest(
                "component target profile id is empty".to_owned(),
            ));
        }
        if self.data_version <= 0 {
            return Err(ComponentError::InvalidRequest(format!(
                "component target DataVersion must be positive, got {}",
                self.data_version
            )));
        }
        if self.java_major == 0 {
            return Err(ComponentError::InvalidRequest(
                "component target Java major must be positive".to_owned(),
            ));
        }
        Ok(())
    }
}

/// Which components a project actually needs. Java is required for server generation,
/// assets for the full block/item catalog and textures, and the server only for Vanilla
/// World creation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentRequirements {
    pub java: bool,
    pub assets: bool,
    pub server: bool,
    /// The user explicitly chose placeholder textures. This never counts as a verified
    /// client JAR and never claims a catalog.
    pub assets_placeholder: bool,
}

impl ComponentRequirements {
    /// Assets only: empty and imported projects can be edited without Java or a server.
    pub const EDITOR: Self = Self {
        java: false,
        assets: true,
        server: false,
        assets_placeholder: false,
    };

    /// Full Vanilla World creation.
    pub const VANILLA_WORLD: Self = Self {
        java: true,
        assets: true,
        server: true,
        assets_placeholder: false,
    };

    pub fn validate(&self) -> Result<(), ComponentError> {
        if self.server && !self.java {
            return Err(ComponentError::InvalidRequest(
                "a server JAR cannot be required without a Java runtime".to_owned(),
            ));
        }
        if self.assets_placeholder && !self.assets {
            return Err(ComponentError::InvalidRequest(
                "placeholder assets require the assets component".to_owned(),
            ));
        }
        Ok(())
    }
}

/// One status snapshot for the whole project target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentReport {
    pub profile_id: String,
    pub minecraft_version: VersionId,
    pub data_version: i32,
    pub java_major: u32,
    pub components: Vec<ComponentStatus>,
    /// True only when every required component is verified. A placeholder is never ready.
    pub ready: bool,
    pub warnings: Vec<String>,
}

impl ComponentReport {
    pub fn status(&self, kind: ComponentKind) -> Option<&ComponentStatus> {
        self.components.iter().find(|status| status.kind == kind)
    }
}

/// Every FuryBuilder-owned artifact on disk, grouped by kind. This is the Settings
/// inventory: it reports what can be verified or removed, not what a project selected.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentInventory {
    pub java: ManagedCacheReport,
    pub assets: client_jar::ManagedClientJarReport,
    pub servers: ManagedServerCacheReport,
}

/// Verified component identity. The path is resolved for use; the project should persist
/// the logical id and hash instead of an absolute path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedComponent {
    pub kind: ComponentKind,
    pub mode: ComponentMode,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub path: String,
    pub hash: ComponentHash,
    pub bytes: u64,
}

/// Result of an explicit re-check. `verified == false` is a normal, reportable outcome;
/// only a missing selection or an unknown id is an error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentVerification {
    pub kind: ComponentKind,
    pub id: String,
    pub verified: bool,
    pub state: ComponentState,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Result of removing a component reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentRemoval {
    pub kind: ComponentKind,
    pub id: String,
    pub removed_managed: bool,
    pub cleared_selection: bool,
}

/// Download progress from any component kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentProgress {
    pub kind: ComponentKind,
    pub phase: String,
    pub downloaded: u64,
    pub total: u64,
}

/// Request for one explicit install. The expected version and hash are the values the
/// user reviewed; the facade refuses a silent replacement by a newer release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentInstallRequest {
    pub kind: ComponentKind,
    pub target: ComponentTarget,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_hash: Option<ComponentHash>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CancelOutcome {
    NotRunning,
    Requested,
    AlreadyRequested,
}

#[derive(Debug, Error)]
pub enum ComponentError {
    #[error("component target is not valid: {0}")]
    InvalidRequest(String),
    #[error(
        "Minecraft {minecraft_version} (DataVersion {data_version}) is not managed by this build: {detail}"
    )]
    UnsupportedTarget {
        minecraft_version: String,
        data_version: i32,
        detail: String,
    },
    #[error("component `{kind}` is not required for this project")]
    NotRequired { kind: &'static str },
    #[error("component `{kind}` is not ready ({state}): {detail}")]
    NotReady {
        kind: &'static str,
        state: &'static str,
        detail: String,
    },
    #[error("component `{kind}` was not found: {id}")]
    NotFound { kind: &'static str, id: String },
    #[error("component `{kind}` operation is already running")]
    Busy { kind: &'static str },
    #[error("component `{kind}` was cancelled")]
    Cancelled { kind: &'static str },
    #[error("component `{kind}` checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch {
        kind: &'static str,
        expected: String,
        actual: String,
    },
    #[error("component hash could not be computed for `{path}`: {error}")]
    Hash { path: String, error: String },
    #[error("component status is invalid: {0}")]
    Status(#[from] StatusError),
    #[error(transparent)]
    Runtime(#[from] RuntimeError),
    #[error(transparent)]
    Server(#[from] ServerError),
    #[error(transparent)]
    Assets(#[from] AssetSettingsError),
    #[error(transparent)]
    ClientJar(#[from] ClientJarError),
}

impl ComponentError {
    pub fn code(&self) -> &'static str {
        match self {
            ComponentError::InvalidRequest(_) => "invalid_argument",
            ComponentError::UnsupportedTarget { .. } => "unsupported_version",
            ComponentError::NotRequired { .. } => "invalid_argument",
            ComponentError::NotReady { .. } => "not_ready",
            ComponentError::NotFound { .. } => "not_found",
            ComponentError::Busy { .. } => "busy",
            ComponentError::Cancelled { .. } => "cancelled",
            ComponentError::ChecksumMismatch { .. } => "checksum_mismatch",
            ComponentError::Hash { .. } => "io_error",
            ComponentError::Status(_) => "invalid_argument",
            ComponentError::Runtime(error) => error.code(),
            ComponentError::Server(error) => error.code(),
            ComponentError::Assets(error) => error.code(),
            ComponentError::ClientJar(error) => error.code(),
        }
    }
}

struct ComponentManagerInner {
    java: RuntimeManager,
    assets: AssetSettingsStore,
    client_jars: ClientJarManager,
    server: ServerManager,
    installs: Mutex<BTreeMap<ComponentKind, Arc<AtomicBool>>>,
}

/// Unified facade. Cloning it shares the managers and the active-install registry.
#[derive(Clone)]
pub struct ComponentManager {
    inner: Arc<ComponentManagerInner>,
}

impl ComponentManager {
    pub fn new(
        java: RuntimeManager,
        assets: AssetSettingsStore,
        client_jars: ClientJarManager,
        server: ServerManager,
    ) -> Self {
        Self {
            inner: Arc::new(ComponentManagerInner {
                java,
                assets,
                client_jars,
                server,
                installs: Mutex::new(BTreeMap::new()),
            }),
        }
    }

    /// Convenience constructor for the desktop composition root.
    pub fn from_paths(
        java_settings: impl Into<PathBuf>,
        java_root: impl Into<PathBuf>,
        assets_settings: impl Into<PathBuf>,
        assets_root: impl Into<PathBuf>,
        manifest_cache: impl Into<PathBuf>,
        server_settings: impl Into<PathBuf>,
        server_root: impl Into<PathBuf>,
    ) -> Self {
        Self::from_paths_with_client(
            java_settings,
            java_root,
            assets_settings,
            assets_root,
            manifest_cache,
            server_settings,
            server_root,
            DownloadClient::new(NetworkConfig::default()),
        )
    }

    /// Composition root with one shared network policy for Java, assets and servers.
    #[allow(clippy::too_many_arguments)]
    pub fn from_paths_with_client(
        java_settings: impl Into<PathBuf>,
        java_root: impl Into<PathBuf>,
        assets_settings: impl Into<PathBuf>,
        assets_root: impl Into<PathBuf>,
        manifest_cache: impl Into<PathBuf>,
        server_settings: impl Into<PathBuf>,
        server_root: impl Into<PathBuf>,
        client: DownloadClient,
    ) -> Self {
        let manifest_cache = manifest_cache.into();
        Self::new(
            RuntimeManager::with_client(java_settings, java_root, client.clone()),
            AssetSettingsStore::new(assets_settings),
            ClientJarManager::with_client(assets_root, manifest_cache.clone(), client.clone()),
            ServerManager::with_client(server_settings, server_root, manifest_cache, client),
        )
    }

    /// Underlying managers, exposed so the desktop shell keeps exactly one instance of each
    /// component source. Mutations still go through this facade.
    pub fn java(&self) -> &RuntimeManager {
        &self.inner.java
    }

    pub fn assets(&self) -> &AssetSettingsStore {
        &self.inner.assets
    }

    /// Loads the official version manifest using the shared component network policy.
    /// Nothing is downloaded beyond the manifest metadata and its cache.
    pub fn version_catalog(&self) -> Result<minecraft_version::VersionCatalog, ComponentError> {
        Ok(self.inner.client_jars.version_catalog()?)
    }

    pub fn server(&self) -> &ServerManager {
        &self.inner.server
    }

    /// Lists every FuryBuilder-owned artifact. Nothing is downloaded, inspected or changed.
    pub fn inventory(&self) -> Result<ComponentInventory, ComponentError> {
        Ok(ComponentInventory {
            java: self.inner.java.managed_list()?,
            assets: self.inner.client_jars.managed_list()?,
            servers: self.inner.server.managed_list()?,
        })
    }

    /// Re-inspects one managed artifact from the Settings inventory. Unlike [`Self::verify`]
    /// this needs no project target: it compares the cache manifest with the bytes on disk
    /// and, for Java, re-probes the executable. An unknown inventory id is refused.
    pub fn verify_inventory(
        &self,
        kind: ComponentKind,
        id: &str,
    ) -> Result<ComponentVerification, ComponentError> {
        match kind {
            ComponentKind::Java => {
                let Some(runtime) = self
                    .inner
                    .java
                    .managed_list()?
                    .runtimes
                    .into_iter()
                    .find(|runtime| runtime.id == id)
                else {
                    return Err(ComponentError::NotFound {
                        kind: kind.as_str(),
                        id: id.to_owned(),
                    });
                };
                let (verified, notes, error) = match self.inner.java.verify_managed(&runtime.id) {
                    Ok(report) => (true, report.warnings, None),
                    Err(error) => (false, Vec::new(), Some(error.to_string())),
                };
                Ok(ComponentVerification {
                    kind,
                    id: runtime.id,
                    verified,
                    state: if verified {
                        ComponentState::Ready
                    } else {
                        ComponentState::Error
                    },
                    notes,
                    error,
                })
            }
            ComponentKind::Assets => self.verify_managed_assets(id),
            ComponentKind::Server => self.verify_managed_server(id),
        }
    }

    fn verify_managed_assets(&self, id: &str) -> Result<ComponentVerification, ComponentError> {
        let kind = ComponentKind::Assets;
        let Some(client) = self
            .inner
            .client_jars
            .managed_list()?
            .clients
            .into_iter()
            .find(|client| client.id == id)
        else {
            return Err(ComponentError::NotFound {
                kind: kind.as_str(),
                id: id.to_owned(),
            });
        };
        // Verification is against the facts recorded for this cache entry, not against the
        // current project target: the same shared client JAR may serve several projects.
        let profile = client
            .version
            .parse()
            .ok()
            .and_then(|version| minecraft_version::profile_for_version(&version))
            .ok_or_else(|| {
                ComponentError::InvalidRequest(format!(
                    "no built-in profile for the cached Minecraft {} client",
                    client.version
                ))
            })?;
        let report = asset_source::inspect_asset_path_for(&client.jar_path, &profile);
        let version = report.version.as_ref();
        let actual = version.and_then(|version| version.actual_sha1.as_deref());
        let reported_id = version.map(|version| version.id.as_str());
        let reported_data_version = version.and_then(|version| version.world_version);
        let id_matches = reported_id == Some(client.version.as_str());
        let data_matches = reported_data_version == Some(client.data_version);
        let size_matches = version.and_then(|version| version.jar_bytes) == Some(client.size);
        let checksum_matches =
            actual.is_some_and(|actual| actual.eq_ignore_ascii_case(&client.sha1));
        let verified = client.matches_inspection(&report);
        let mut notes = report.warnings.clone();
        if let Some(version) = version {
            notes.extend(version.notes.clone());
        }
        let error = if verified {
            None
        } else if !id_matches {
            Some(match reported_id {
                Some(actual) => {
                    format!(
                        "the cached client JAR is Minecraft {actual}, not {}",
                        client.version
                    )
                }
                None => first_reason(&report),
            })
        } else if !data_matches {
            Some(match reported_data_version {
                Some(actual) => format!(
                    "the cached client JAR declares DataVersion {actual}, not {}",
                    client.data_version
                ),
                None => "the cached client JAR has no DataVersion to compare".to_owned(),
            })
        } else if !size_matches {
            Some(format!(
                "the cached client JAR for Minecraft {} changed size",
                client.version
            ))
        } else if actual.is_none() {
            Some("the cached client JAR has no SHA-1 to compare".to_owned())
        } else if !checksum_matches {
            Some(format!(
                "the cached client JAR for Minecraft {} does not match its SHA-1",
                client.version
            ))
        } else {
            Some(first_reason(&report))
        };
        let state = if verified {
            ComponentState::Ready
        } else if actual.is_none() {
            ComponentState::Unverified
        } else {
            ComponentState::Incompatible
        };
        Ok(ComponentVerification {
            kind,
            id: client.id,
            verified,
            state,
            notes,
            error,
        })
    }

    fn verify_managed_server(&self, id: &str) -> Result<ComponentVerification, ComponentError> {
        let kind = ComponentKind::Server;
        let Some(version) = id.strip_prefix("server-managed-") else {
            return Err(ComponentError::NotFound {
                kind: kind.as_str(),
                id: id.to_owned(),
            });
        };
        let Some(server) = self
            .inner
            .server
            .managed_list()?
            .servers
            .into_iter()
            .find(|server| server.version == version)
        else {
            return Err(ComponentError::NotFound {
                kind: kind.as_str(),
                id: id.to_owned(),
            });
        };
        let canonical_id = format!("server-managed-{}", server.version);
        match inspect_server_jar(Path::new(&server.jar_path)) {
            Ok(info) => {
                let version_matches = info.version == server.version;
                let data_matches = info.data_version == server.data_version;
                let size_matches = info.size == server.size;
                let checksum_matches = info.sha1.eq_ignore_ascii_case(&server.sha1);
                let verified = version_matches && data_matches && size_matches && checksum_matches;
                let error = if verified {
                    None
                } else if !version_matches {
                    Some(format!(
                        "the cached server JAR is Minecraft {}, not {}",
                        info.version, server.version
                    ))
                } else if !data_matches {
                    Some(format!(
                        "the cached server JAR declares DataVersion {}, not {}",
                        info.data_version, server.data_version
                    ))
                } else if !size_matches {
                    Some(format!(
                        "the cached server JAR for Minecraft {} changed size",
                        server.version
                    ))
                } else {
                    Some(format!(
                        "the cached server JAR for Minecraft {} does not match its SHA-1",
                        server.version
                    ))
                };
                Ok(ComponentVerification {
                    kind,
                    id: canonical_id,
                    verified,
                    state: if verified {
                        ComponentState::Ready
                    } else {
                        ComponentState::Incompatible
                    },
                    notes: Vec::new(),
                    error,
                })
            }
            Err(error) => Ok(ComponentVerification {
                kind,
                id: canonical_id,
                verified: false,
                state: ComponentState::Error,
                notes: Vec::new(),
                error: Some(error.to_string()),
            }),
        }
    }

    /// Returns the official client JAR metadata for the target version. No bytes are
    /// downloaded until [`Self::install`] is called.
    pub fn assets_release(
        &self,
        target: &ComponentTarget,
    ) -> Result<ClientJarReleaseInfo, ComponentError> {
        target.validate()?;
        self.assets_profile(target)?;
        Ok(self.inner.client_jars.release(&target.minecraft_version)?)
    }

    /// Returns the Adoptium release the target Java major would install. No bytes are
    /// downloaded until [`Self::install`] is called.
    pub fn java_release(
        &self,
        target: &ComponentTarget,
    ) -> Result<RuntimeReleaseInfo, ComponentError> {
        target.validate()?;
        self.ensure_java_major(target)?;
        Ok(self.inner.java.latest_temurin(target.java_major)?)
    }

    /// Returns the official server JAR metadata for the exact target version. No bytes are
    /// downloaded until [`Self::install`] is called.
    pub fn server_release(
        &self,
        target: &ComponentTarget,
    ) -> Result<ServerReleaseInfo, ComponentError> {
        target.validate()?;
        let server_target = server_target(target)?;
        Ok(self.inner.server.release(&server_target)?)
    }

    /// Lists FuryBuilder-owned client JARs for the Settings inventory.
    pub fn assets_inventory(&self) -> Result<client_jar::ManagedClientJarReport, ComponentError> {
        Ok(self.inner.client_jars.managed_list()?)
    }

    /// Resolves a project's managed client independently of the wizard's current selection.
    pub fn resolve_managed_assets(
        &self,
        target: &ComponentTarget,
    ) -> Result<ResolvedComponent, ComponentError> {
        let profile = self.assets_profile(target)?;
        let client = self
            .inner
            .client_jars
            .find(&target.minecraft_version)?
            .ok_or_else(|| ComponentError::NotReady {
                kind: ComponentKind::Assets.as_str(),
                state: "missing",
                detail: format!(
                    "no managed Minecraft {} client JAR is installed",
                    target.minecraft_version
                ),
            })?;
        let report = asset_source::inspect_asset_path_for(&client.jar_path, &profile);
        if !client.matches_inspection(&report) {
            return Err(ComponentError::InvalidRequest(format!(
                "the managed Minecraft {} client JAR no longer matches its cache manifest",
                client.version
            )));
        }
        resolved_from_status(asset_source_status(report, true))
    }

    /// Explicitly selects original placeholder textures. This is a deliberate choice, not a
    /// fallback: the resulting status never carries a client JAR, catalog or checksum and is
    /// never reported as ready.
    pub fn use_placeholder_assets(
        &self,
        target: &ComponentTarget,
    ) -> Result<ComponentStatus, ComponentError> {
        target.validate()?;
        self.inner.assets.use_placeholder()?;
        Ok(self.assets_status(target, true, true))
    }

    pub fn status(
        &self,
        target: &ComponentTarget,
        requirements: &ComponentRequirements,
    ) -> Result<ComponentReport, ComponentError> {
        target.validate()?;
        requirements.validate()?;
        let components = vec![
            self.java_status(target, requirements.java),
            self.assets_status(target, requirements.assets, requirements.assets_placeholder),
            self.server_status(target, requirements.server),
        ];
        let ready = components
            .iter()
            .filter(|status| status.required)
            .all(|status| status.state.is_ready());
        let mut warnings = Vec::new();
        for status in &components {
            for note in &status.notes {
                if !warnings.contains(note) && !note.trim().is_empty() {
                    warnings.push(note.clone());
                }
            }
        }
        Ok(ComponentReport {
            profile_id: target.profile_id.clone(),
            minecraft_version: target.minecraft_version.clone(),
            data_version: target.data_version,
            java_major: target.java_major,
            components,
            ready,
            warnings,
        })
    }

    pub fn resolve(
        &self,
        kind: ComponentKind,
        target: &ComponentTarget,
        requirements: &ComponentRequirements,
    ) -> Result<ResolvedComponent, ComponentError> {
        target.validate()?;
        requirements.validate()?;
        let status = match kind {
            ComponentKind::Java => self.java_status(target, requirements.java),
            ComponentKind::Assets => {
                self.assets_status(target, requirements.assets, requirements.assets_placeholder)
            }
            ComponentKind::Server => self.server_status(target, requirements.server),
        };
        if !status.required {
            return Err(ComponentError::NotRequired {
                kind: kind.as_str(),
            });
        }
        if !status.state.is_ready() {
            return Err(ComponentError::NotReady {
                kind: kind.as_str(),
                state: status.state.as_str(),
                detail: status
                    .error
                    .clone()
                    .or_else(|| status.notes.first().cloned())
                    .unwrap_or_else(|| "no verified component is selected".to_owned()),
            });
        }
        resolved_from_status(status)
    }

    /// Validates and records a manual source. The selected file is never copied or edited.
    pub fn relink(
        &self,
        kind: ComponentKind,
        path: &Path,
        target: &ComponentTarget,
    ) -> Result<ComponentStatus, ComponentError> {
        target.validate()?;
        match kind {
            ComponentKind::Java => {
                self.ensure_java_major(target)?;
                self.inner.java.select_for_major(path, target.java_major)?;
                Ok(self.java_status(target, true))
            }
            ComponentKind::Assets => {
                let profile = self.assets_profile(target)?;
                let report = asset_source::inspect_asset_path_for(path, &profile);
                if !report.valid || report.version.is_none() {
                    return Err(ComponentError::InvalidRequest(format!(
                        "{} is not a usable Minecraft {} client source: {}",
                        report.path,
                        target.minecraft_version,
                        first_reason(&report)
                    )));
                }
                let canonical = fs::canonicalize(path).map_err(|error| {
                    ComponentError::InvalidRequest(format!(
                        "cannot resolve {}: {error}",
                        path.display()
                    ))
                })?;
                self.inner.assets.save(AssetSourceSetting {
                    kind: report.kind.as_str().to_owned(),
                    path: canonical.display().to_string(),
                })?;
                Ok(self.assets_status(target, true, false))
            }
            ComponentKind::Server => {
                let server_target = server_target(target)?;
                self.inner.server.select_manual(path, &server_target)?;
                Ok(self.server_status(target, true))
            }
        }
    }

    /// Re-inspects the selected component and returns the fresh facts. The id must be the
    /// one returned by a current status so a stale UI request cannot verify another file.
    pub fn verify(
        &self,
        kind: ComponentKind,
        id: &str,
        target: &ComponentTarget,
    ) -> Result<ComponentVerification, ComponentError> {
        target.validate()?;
        let status = self.status_for_kind(kind, target, true, false);
        if status.id.as_deref() != Some(id) {
            return Err(ComponentError::NotFound {
                kind: kind.as_str(),
                id: id.to_owned(),
            });
        }
        Ok(ComponentVerification {
            kind,
            id: id.to_owned(),
            verified: status.state.is_ready(),
            state: status.state,
            notes: status.notes,
            error: status.error,
        })
    }

    /// Removes FuryBuilder-owned data or clears a manual pointer. A user file outside the
    /// managed cache is never deleted.
    pub fn remove(
        &self,
        kind: ComponentKind,
        id: &str,
    ) -> Result<ComponentRemoval, ComponentError> {
        let mut removed_managed = false;
        let mut cleared_selection = false;
        match kind {
            ComponentKind::Java => {
                let managed = self.inner.java.managed_list()?;
                if managed.runtimes.iter().any(|runtime| runtime.id == id) {
                    self.inner.java.remove_managed(id)?;
                    removed_managed = true;
                } else if id.starts_with("java-manual-") {
                    if let Some(report) = self.inner.java.saved() {
                        if java_manual_id(&report) == id {
                            self.inner.java.clear_selection()?;
                            cleared_selection = true;
                        }
                    }
                }
            }
            ComponentKind::Assets => {
                if let Some(version) = id.strip_prefix("assets-managed-") {
                    let version =
                        version
                            .parse::<VersionId>()
                            .map_err(|_| ComponentError::NotFound {
                                kind: ComponentKind::Assets.as_str(),
                                id: id.to_owned(),
                            })?;
                    let selected_managed =
                        self.inner
                            .assets
                            .saved_source()
                            .source
                            .is_some_and(|source| {
                                let selected = Path::new(&source.path);
                                selected
                                    .parent()
                                    .and_then(Path::file_name)
                                    .is_some_and(|name| name == version.as_str())
                                    && self.inner.client_jars.contains(selected)
                            });
                    if self.inner.client_jars.remove(&version)? {
                        removed_managed = true;
                    }
                    if removed_managed && selected_managed {
                        self.inner.assets.clear()?;
                        cleared_selection = true;
                    }
                } else if id.starts_with("assets-")
                    && self.inner.assets.saved_source().source.is_some()
                {
                    self.inner.assets.clear()?;
                    cleared_selection = true;
                }
            }
            ComponentKind::Server => {
                if let Some(version) = id.strip_prefix("server-managed-") {
                    let known = self
                        .inner
                        .server
                        .managed_list()?
                        .servers
                        .iter()
                        .any(|server| server.version == version);
                    if known {
                        self.inner.server.remove_managed(version)?;
                        removed_managed = true;
                    }
                } else if let Some(version) = id.strip_prefix("server-manual-") {
                    let version =
                        version
                            .parse::<VersionId>()
                            .map_err(|_| ComponentError::NotFound {
                                kind: ComponentKind::Server.as_str(),
                                id: id.to_owned(),
                            })?;
                    if self.inner.server.has_selection(&version) {
                        self.inner.server.clear_selection(&version)?;
                        cleared_selection = true;
                    }
                }
            }
        }
        if !removed_managed && !cleared_selection {
            return Err(ComponentError::NotFound {
                kind: kind.as_str(),
                id: id.to_owned(),
            });
        }
        Ok(ComponentRemoval {
            kind,
            id: id.to_owned(),
            removed_managed,
            cleared_selection,
        })
    }

    /// Downloads only after an explicit request. The expected version/hash are checked
    /// before any bytes are written. Assets downloading belongs to the asset pipeline and
    /// is reported as unavailable here until the client JAR installer exists.
    pub fn install(
        &self,
        request: &ComponentInstallRequest,
        progress: &dyn Fn(ComponentProgress),
    ) -> Result<ResolvedComponent, ComponentError> {
        request.target.validate()?;
        if let Some(hash) = &request.expected_hash {
            hash.validate()
                .map_err(|error| ComponentError::InvalidRequest(error.to_string()))?;
        }
        match request.kind {
            ComponentKind::Assets => {
                self.assets_profile(&request.target)?;
                let expected_version = request
                    .expected_version
                    .as_deref()
                    .unwrap_or_else(|| request.target.minecraft_version.as_str());
                if expected_version != request.target.minecraft_version.as_str() {
                    return Err(ComponentError::InvalidRequest(format!(
                        "the reviewed client JAR is for Minecraft {expected_version}, not {}; review the download again",
                        request.target.minecraft_version
                    )));
                }
                let guarded = self.begin_install(ComponentKind::Assets)?;
                let cancel = guarded.token();
                let release = self
                    .inner
                    .client_jars
                    .release(&request.target.minecraft_version)?;
                if let Some(expected) = &request.expected_hash {
                    if !expected.matches(HashAlgorithm::Sha1, &release.sha1) {
                        return Err(ComponentError::ChecksumMismatch {
                            kind: ComponentKind::Assets.as_str(),
                            expected: format!("sha1:{}", expected.value),
                            actual: release.sha1.clone(),
                        });
                    }
                }
                let installed = self.inner.client_jars.install(
                    &request.target.minecraft_version,
                    Some(&release.sha1),
                    Some(request.target.data_version),
                    &cancel,
                    &|update: ClientJarProgress| {
                        progress(ComponentProgress {
                            kind: ComponentKind::Assets,
                            phase: update.phase,
                            downloaded: update.downloaded,
                            total: update.total,
                        });
                    },
                )?;
                self.inner.assets.save(AssetSourceSetting {
                    kind: "client_jar".to_owned(),
                    path: installed.jar_path,
                })?;
                let status = self.assets_status(&request.target, true, false);
                resolved_from_status(status)
            }
            ComponentKind::Java => {
                self.ensure_java_major(&request.target)?;
                let guarded = self.begin_install(ComponentKind::Java)?;
                let cancel = guarded.token();
                let release = self.inner.java.latest_temurin(request.target.java_major)?;
                let expected_version = request.expected_version.as_deref().ok_or_else(|| {
                    ComponentError::InvalidRequest(
                        "a Java install requires the reviewed Adoptium release version".to_owned(),
                    )
                })?;
                if release.version != expected_version {
                    return Err(ComponentError::InvalidRequest(format!(
                        "Adoptium now offers {}, not {expected_version}; review the download again",
                        release.version
                    )));
                }
                if let Some(expected) = &request.expected_hash {
                    if !expected.matches(HashAlgorithm::Sha256, &release.sha256) {
                        return Err(ComponentError::ChecksumMismatch {
                            kind: ComponentKind::Java.as_str(),
                            expected: format!("sha256:{}", expected.value),
                            actual: release.sha256.clone(),
                        });
                    }
                }
                self.inner.java.install_temurin(
                    request.target.java_major,
                    expected_version,
                    &cancel,
                    &|update| {
                        progress(ComponentProgress {
                            kind: ComponentKind::Java,
                            phase: update.phase,
                            downloaded: update.downloaded,
                            total: update.total,
                        });
                    },
                )?;
                let status = self.java_status(&request.target, true);
                resolved_from_status(status)
            }
            ComponentKind::Server => {
                let server_target = server_target(&request.target)?;
                let guarded = self.begin_install(ComponentKind::Server)?;
                let cancel = guarded.token();
                let release = self.inner.server.release(&server_target)?;
                let expected_version = request
                    .expected_version
                    .as_deref()
                    .unwrap_or_else(|| request.target.minecraft_version.as_str());
                if release.minecraft_version != expected_version {
                    return Err(ComponentError::InvalidRequest(format!(
                        "Mojang now offers Minecraft {}, not {expected_version}; review the download again",
                        release.minecraft_version
                    )));
                }
                if let Some(expected) = &request.expected_hash {
                    if !expected.matches(HashAlgorithm::Sha1, &release.sha1) {
                        return Err(ComponentError::ChecksumMismatch {
                            kind: ComponentKind::Server.as_str(),
                            expected: format!("sha1:{}", expected.value),
                            actual: release.sha1.clone(),
                        });
                    }
                }
                self.inner
                    .server
                    .install(&server_target, &cancel, &|update: ServerProgress| {
                        progress(ComponentProgress {
                            kind: ComponentKind::Server,
                            phase: update.phase,
                            downloaded: update.downloaded,
                            total: update.total,
                        });
                    })?;
                let status = self.server_status(&request.target, true);
                resolved_from_status(status)
            }
        }
    }

    /// Requests cancellation of an active install. The download loops check the flag
    /// between chunks and clean up their partial files.
    pub fn cancel(&self, kind: ComponentKind) -> CancelOutcome {
        let Ok(installs) = self.inner.installs.lock() else {
            return CancelOutcome::NotRunning;
        };
        let Some(token) = installs.get(&kind) else {
            return CancelOutcome::NotRunning;
        };
        if token.swap(true, Ordering::SeqCst) {
            CancelOutcome::AlreadyRequested
        } else {
            CancelOutcome::Requested
        }
    }

    fn begin_install(&self, kind: ComponentKind) -> Result<ActiveInstall<'_>, ComponentError> {
        let mut installs = self
            .inner
            .installs
            .lock()
            .map_err(|_| ComponentError::Busy {
                kind: kind.as_str(),
            })?;
        if installs.contains_key(&kind) {
            return Err(ComponentError::Busy {
                kind: kind.as_str(),
            });
        }
        let token = Arc::new(AtomicBool::new(false));
        installs.insert(kind, Arc::clone(&token));
        Ok(ActiveInstall {
            installs: &self.inner.installs,
            kind,
            token,
        })
    }

    fn status_for_kind(
        &self,
        kind: ComponentKind,
        target: &ComponentTarget,
        required: bool,
        assets_placeholder: bool,
    ) -> ComponentStatus {
        match kind {
            ComponentKind::Java => self.java_status(target, required),
            ComponentKind::Assets => self.assets_status(target, required, assets_placeholder),
            ComponentKind::Server => self.server_status(target, required),
        }
    }

    fn java_status(&self, target: &ComponentTarget, required: bool) -> ComponentStatus {
        if !required {
            return ComponentStatus::not_required(ComponentKind::Java);
        }
        let Some(report) = self.inner.java.saved_for_major(target.java_major) else {
            let mut status = ComponentStatus::new(ComponentKind::Java, ComponentState::Missing);
            status
                .notes
                .push("no Java runtime is selected for this project".to_owned());
            return finalize(status);
        };
        let mut status = ComponentStatus::new(ComponentKind::Java, ComponentState::Missing);
        status.mode = Some(if report.managed {
            ComponentMode::Managed
        } else {
            ComponentMode::Manual
        });
        status.path = Some(report.path.clone());
        status.version = report.version.clone();
        status.notes.extend(report.warnings.clone());
        if let Ok(metadata) = fs::metadata(&report.path) {
            status.bytes = Some(metadata.len());
        }
        let managed_id = match self.inner.java.managed_list() {
            Ok(list) => list
                .runtimes
                .into_iter()
                .find(|runtime| same_path(&runtime.java_path, &report.path))
                .map(|runtime| runtime.id),
            Err(error) => {
                status
                    .notes
                    .push(format!("managed Java inventory is unavailable: {error}"));
                None
            }
        };
        status.id = Some(managed_id.unwrap_or_else(|| java_manual_id(&report)));

        if let Some(error) = report.error.clone() {
            status.state = ComponentState::Error;
            status.error = Some(error);
        } else {
            let (state, error) = java_compatibility(&report, target);
            status.state = state;
            status.error = error;
        }

        if status.state.is_ready() {
            if !Path::new(&report.path).is_file() {
                status.state = ComponentState::Missing;
                status.error = Some(format!(
                    "the selected Java executable no longer exists: {}",
                    report.path
                ));
                status.hash = None;
            } else {
                match sha256_file(Path::new(&report.path)) {
                    Ok(hash) => status.hash = Some(hash),
                    Err(error) => {
                        status.state = ComponentState::Error;
                        status.error = Some(error.to_string());
                    }
                }
            }
        }
        finalize(status)
    }

    fn assets_status(
        &self,
        target: &ComponentTarget,
        required: bool,
        placeholder: bool,
    ) -> ComponentStatus {
        if !required {
            return ComponentStatus::not_required(ComponentKind::Assets);
        }
        if placeholder {
            return finalize(placeholder_assets_status());
        }
        let profile = match self.assets_profile(target) {
            Ok(profile) => profile,
            Err(error) => {
                let mut status =
                    ComponentStatus::new(ComponentKind::Assets, ComponentState::Incompatible);
                status.error = Some(error.to_string());
                return finalize(status);
            }
        };
        let saved = self.inner.assets.saved_source_for(&profile);
        // An explicit placeholder choice wins over any cached source: the project asked for
        // original FuryBuilder textures, not for Minecraft assets from another project.
        if saved.placeholder {
            return finalize(placeholder_assets_status());
        }
        let selected_for_target = saved.report.as_ref().is_some_and(|report| {
            report
                .version
                .as_ref()
                .is_some_and(|version| version.id == target.minecraft_version.as_str())
        });
        let managed_fallback = if !selected_for_target {
            match self.inner.client_jars.find(&target.minecraft_version) {
                Ok(found) => found,
                Err(error) => {
                    let mut status =
                        ComponentStatus::new(ComponentKind::Assets, ComponentState::Error);
                    status.error =
                        Some(format!("managed assets inventory is unavailable: {error}"));
                    return finalize(status);
                }
            }
        } else {
            None
        };
        let report = match managed_fallback {
            Some(client) => Some(asset_source::inspect_asset_path_for(
                &client.jar_path,
                &profile,
            )),
            None => saved.report,
        };
        let Some(report) = report else {
            let mut status = ComponentStatus::new(ComponentKind::Assets, ComponentState::Missing);
            status.notes.push(
                "no Minecraft client assets are selected; download the client JAR or choose a .minecraft root, version folder or client JAR"
                    .to_owned(),
            );
            return finalize(status);
        };
        let managed = self.inner.client_jars.contains(Path::new(&report.path));
        let exact_cache_match = !managed
            || self
                .inner
                .client_jars
                .find(&target.minecraft_version)
                .is_ok_and(|client| {
                    client.is_some_and(|client| client.matches_inspection(&report))
                });
        let mut status = asset_source_status(report, managed);
        if status.state.is_ready() && !exact_cache_match {
            status.state = ComponentState::Incompatible;
            status.error =
                Some("the client JAR no longer matches its managed cache manifest".to_owned());
        }
        status.notes.extend(saved.load_warning);
        finalize(status)
    }

    fn server_status(&self, target: &ComponentTarget, required: bool) -> ComponentStatus {
        if !required {
            return ComponentStatus::not_required(ComponentKind::Server);
        }
        let server_target = match server_target(target) {
            Ok(target) => target,
            Err(error) => {
                let mut status = ComponentStatus::new(ComponentKind::Server, ComponentState::Error);
                status.error = Some(error.to_string());
                return finalize(status);
            }
        };
        let Some(report) = self.inner.server.selected(&server_target) else {
            let mut status = ComponentStatus::new(ComponentKind::Server, ComponentState::Missing);
            status
                .notes
                .push("no server JAR is selected for Vanilla World generation".to_owned());
            return finalize(status);
        };
        let mut status = ComponentStatus::new(ComponentKind::Server, ComponentState::Missing);
        status.mode = Some(if report.managed {
            ComponentMode::Managed
        } else {
            ComponentMode::Manual
        });
        status.path = Some(report.path.clone());
        status.version = report.version.clone();
        status.bytes = report.size;
        status.notes.extend(report.warnings.clone());
        status.id = Some(server_id(&report, target.minecraft_version.as_str()));
        if let Some(error) = report.error {
            status.state = ComponentState::Error;
            status.error = Some(error);
            return finalize(status);
        }
        let Some(version) = report.version.as_deref() else {
            status.state = ComponentState::Incompatible;
            status.error = Some("the server JAR does not declare a Minecraft version".to_owned());
            return finalize(status);
        };
        if version != target.minecraft_version.as_str() {
            status.state = ComponentState::Incompatible;
            status.error = Some(format!(
                "the selected server JAR is Minecraft {version}, not {}",
                target.minecraft_version
            ));
            return finalize(status);
        }
        if report.data_version != Some(target.data_version) {
            status.state = ComponentState::Incompatible;
            status.error = Some(format!(
                "the server JAR declares DataVersion {:?}, not {}",
                report.data_version, target.data_version
            ));
            return finalize(status);
        }
        let Some(sha1) = report.sha1 else {
            status.state = ComponentState::Unverified;
            status
                .notes
                .push("no SHA-1 is available for the server JAR".to_owned());
            return finalize(status);
        };
        status.hash = Some(ComponentHash::sha1(sha1));
        status.state = if report.ready {
            ComponentState::Ready
        } else {
            ComponentState::Unverified
        };
        finalize(status)
    }

    fn assets_profile(&self, target: &ComponentTarget) -> Result<VersionProfile, ComponentError> {
        minecraft_version::profile_for_id(&target.profile_id)
            .filter(|profile| {
                ComponentTarget::from_profile(profile) == *target && profile.capabilities.assets
            })
            .ok_or_else(|| ComponentError::UnsupportedTarget {
                minecraft_version: target.minecraft_version.to_string(),
                data_version: target.data_version,
                detail: "client assets require an exact supported profile".to_owned(),
            })
    }

    fn ensure_java_major(&self, target: &ComponentTarget) -> Result<(), ComponentError> {
        if runtime_manager::is_supported_java_major(target.java_major) {
            return Ok(());
        }
        Err(ComponentError::UnsupportedTarget {
            minecraft_version: target.minecraft_version.to_string(),
            data_version: target.data_version,
            detail: format!(
                "Temurin provisioning supports Java {:?}; this target asks for Java {}",
                runtime_manager::JAVA_MAJORS,
                target.java_major
            ),
        })
    }
}

struct ActiveInstall<'a> {
    installs: &'a Mutex<BTreeMap<ComponentKind, Arc<AtomicBool>>>,
    kind: ComponentKind,
    token: Arc<AtomicBool>,
}

impl ActiveInstall<'_> {
    fn token(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.token)
    }
}

impl Drop for ActiveInstall<'_> {
    fn drop(&mut self) {
        if let Ok(mut installs) = self.installs.lock() {
            installs.remove(&self.kind);
        }
    }
}

fn resolved_from_status(status: ComponentStatus) -> Result<ResolvedComponent, ComponentError> {
    let kind = status.kind;
    let Some(mode) = status.mode else {
        return Err(ComponentError::NotReady {
            kind: kind.as_str(),
            state: status.state.as_str(),
            detail: "a verified component needs a managed or manual mode".to_owned(),
        });
    };
    let Some(id) = status.id else {
        return Err(ComponentError::NotReady {
            kind: kind.as_str(),
            state: status.state.as_str(),
            detail: "a verified component needs a logical id".to_owned(),
        });
    };
    let Some(path) = status.path else {
        return Err(ComponentError::NotReady {
            kind: kind.as_str(),
            state: status.state.as_str(),
            detail: "a verified component needs a path".to_owned(),
        });
    };
    let Some(hash) = status.hash else {
        return Err(ComponentError::NotReady {
            kind: kind.as_str(),
            state: status.state.as_str(),
            detail: "a verified component needs a checksum".to_owned(),
        });
    };
    let Some(bytes) = status.bytes else {
        return Err(ComponentError::NotReady {
            kind: kind.as_str(),
            state: status.state.as_str(),
            detail: "a verified component needs a size".to_owned(),
        });
    };
    Ok(ResolvedComponent {
        kind,
        mode,
        id,
        version: status.version,
        path,
        hash,
        bytes,
    })
}

/// Adds an explicit error status if a bug ever produces an invariant-breaking DTO instead
/// of panicking in production.
fn finalize(mut status: ComponentStatus) -> ComponentStatus {
    if let Err(error) = status.validate() {
        let notes = std::mem::take(&mut status.notes);
        status.state = ComponentState::Error;
        status.hash = None;
        status.error = Some(format!("component status invariant failed: {error}"));
        status.notes = notes;
    }
    status
}

/// The one status used for an explicit placeholder selection. It never carries an identity,
/// checksum, path or catalog and is never reported as ready.
fn placeholder_assets_status() -> ComponentStatus {
    let mut status = ComponentStatus::new(ComponentKind::Assets, ComponentState::Unverified);
    status.mode = Some(ComponentMode::Placeholder);
    status.notes.push(
        "placeholder textures are active; there is no client JAR catalog for this project and block data is unchanged"
            .to_owned(),
    );
    status
}

fn asset_source_status(report: AssetPathReport, managed: bool) -> ComponentStatus {
    let mut status = ComponentStatus::new(ComponentKind::Assets, ComponentState::Incompatible);
    status.mode = Some(if managed {
        ComponentMode::Managed
    } else {
        ComponentMode::Manual
    });
    status.path = Some(report.path.clone());
    status.notes.extend(report.warnings.iter().cloned());
    let Some(version) = report.version.as_ref() else {
        status.error = Some(match report.kind {
            AssetSelectionKind::ResourcePack => {
                "a resource pack is not a client JAR; select a Minecraft installation or client JAR"
                    .to_owned()
            }
            _ => first_reason(&report),
        });
        return finalize(status);
    };
    status.id = Some(assets_id(managed, &version.id));
    status.version = Some(version.id.clone());
    status.path = Some(version.jar_path.clone());
    status.bytes = version.jar_bytes;
    status.notes.extend(version.notes.iter().cloned());
    if !report.valid {
        status.state = match version.status {
            VersionStatus::InvalidClientJar
            | VersionStatus::InvalidMetadata
            | VersionStatus::MissingMetadata => ComponentState::Error,
            VersionStatus::MissingClientJar => ComponentState::Missing,
            _ => ComponentState::Incompatible,
        };
        status.error = Some(first_reason(&report));
        return finalize(status);
    }
    status.hash = version.actual_sha1.clone().map(ComponentHash::sha1);
    status.state = ComponentState::Ready;
    if !version.checksum_verified {
        status.notes.push(
            "the client JAR SHA-1 was not matched against launcher metadata; the local file is used as-is".to_owned(),
        );
    }
    finalize(status)
}

fn first_reason(report: &AssetPathReport) -> String {
    report
        .errors
        .first()
        .cloned()
        .or_else(|| report.warnings.first().cloned())
        .unwrap_or_else(|| format!("status {}", report.kind.as_str()))
}

/// Classifies a probed runtime against the project's Java requirement. The underlying
/// manager reports against the baseline profile; the project target is authoritative here.
/// Classifies a probed runtime against the project Java major. Version warnings come from
/// the probe itself, so this only decides whether the runtime may be used.
fn java_compatibility(
    report: &JavaRuntimeReport,
    target: &ComponentTarget,
) -> (ComponentState, Option<String>) {
    if let Some(error) = &report.error {
        return (ComponentState::Error, Some(error.clone()));
    }
    if let Some(arch) = report.arch.as_deref() {
        let arch = arch.to_ascii_lowercase();
        if arch == "x86" || arch == "i386" {
            return (
                ComponentState::Incompatible,
                Some("32-bit Java cannot run Minecraft".to_owned()),
            );
        }
    }
    match report.major {
        None => (
            ComponentState::Incompatible,
            Some("Java did not report a major version".to_owned()),
        ),
        Some(major) if major < target.java_major => (
            ComponentState::Incompatible,
            Some(format!(
                "Java {major} is older than the required Java {}",
                target.java_major
            )),
        ),
        Some(_) => (ComponentState::Ready, None),
    }
}

fn java_manual_id(report: &JavaRuntimeReport) -> String {
    format!("java-manual-{}", report.major.unwrap_or(0))
}

fn server_target(target: &ComponentTarget) -> Result<ServerTarget, ComponentError> {
    ServerTarget::new(
        target.minecraft_version.clone(),
        target.data_version,
        target.java_major,
    )
    .map_err(|error| ComponentError::InvalidRequest(error.to_string()))
}

fn server_id(report: &runtime_manager::ServerJarReport, fallback_version: &str) -> String {
    let mode = if report.managed { "managed" } else { "manual" };
    format!(
        "server-{mode}-{}",
        report.version.as_deref().unwrap_or(fallback_version)
    )
}

fn assets_id(managed: bool, version: &str) -> String {
    if managed {
        format!("assets-managed-{version}")
    } else {
        format!("assets-{version}")
    }
}

fn same_path(left: &str, right: &str) -> bool {
    let left = fs::canonicalize(left).unwrap_or_else(|_| PathBuf::from(left));
    let right = fs::canonicalize(right).unwrap_or_else(|_| PathBuf::from(right));
    if cfg!(windows) {
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    } else {
        left == right
    }
}

fn sha256_file(path: &Path) -> Result<ComponentHash, ComponentError> {
    let bytes = fs::read(path).map_err(|error| ComponentError::Hash {
        path: path.display().to_string(),
        error: error.to_string(),
    })?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(ComponentHash::sha256(hex(&hasher.finalize())))
}

fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        let _ = write!(text, "{byte:02x}");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use runtime_manager::RuntimeStatus;
    use std::io::{Cursor, Write};
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "furybuilder-components-{label}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|duration| duration.as_nanos())
                    .unwrap_or(0)
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("create temp directory");
            Self { path }
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn manager(root: &TempDir) -> ComponentManager {
        ComponentManager::from_paths(
            root.path.join("runtime.json"),
            root.path.join("runtimes"),
            root.path.join("assets.json"),
            root.path.join("components").join("assets"),
            root.path.join("cache").join("version_manifest_v2.json"),
            root.path.join("server.json"),
            root.path.join("servers"),
        )
    }

    fn target() -> ComponentTarget {
        ComponentTarget::from_profile(&minecraft_version::default_profile())
    }

    /// Creates a conventional `.minecraft/versions/<version>/` fixture with a valid client
    /// JAR and its launcher metadata, then returns the root and the version folder.
    fn manual_installation(root: &TempDir, version: &str, data_version: i32) -> (PathBuf, PathBuf) {
        let (_, document, jar) = client_jar::test_fixture(version, data_version);
        let minecraft = root.path.join(".minecraft");
        let version_dir = minecraft.join("versions").join(version);
        fs::create_dir_all(&version_dir).expect("create version directory");
        fs::write(version_dir.join(format!("{version}.jar")), jar).expect("write client jar");
        fs::write(version_dir.join(format!("{version}.json")), document)
            .expect("write launcher metadata");
        (minecraft, version_dir)
    }

    /// Creates a directory reparse point: a symlink on Unix, a junction on Windows. Junctions
    /// need no administrator rights and are refused by the same guard as symlinks.
    fn link_directory(link: &Path, target: &Path) {
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, link).expect("create directory symlink");
        }
        #[cfg(windows)]
        {
            let status = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(link)
                .arg(target)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .expect("run mklink");
            assert!(status.success(), "mklink /J must create a junction");
        }
    }

    /// Seeds a FuryBuilder-owned managed server cache entry so inventory verification can be
    /// exercised without the network.
    fn seed_managed_server(
        root: &TempDir,
        version: &str,
        data_version: i32,
        java_major: u32,
    ) -> String {
        let directory = root.path.join("servers").join(version);
        fs::create_dir_all(&directory).expect("create server cache directory");
        let jar_path = directory.join("server.jar");
        write_server_jar(&jar_path, version, data_version, java_major);
        let bytes = fs::read(&jar_path).expect("read synthetic server jar");
        let sha1 = hex(&sha1::Sha1::digest(&bytes));
        let manifest = serde_json::json!({
            "schemaVersion": 2,
            "version": version,
            "dataVersion": data_version,
            "javaMajor": java_major,
            "sha1": sha1,
            "size": bytes.len(),
            "releaseTime": "2024-12-03T10:12:57+00:00",
            "sourceUrl": "https://piston-data.mojang.com/v1/objects/server.jar",
            "versionJsonSha1": "d".repeat(40),
            "installedAt": 1_700_000_000u64,
        });
        fs::write(
            directory.join(".furybuilder-server.json"),
            serde_json::to_vec_pretty(&manifest).expect("serialize server manifest"),
        )
        .expect("write server manifest");
        format!("server-managed-{version}")
    }

    /// Writes a structurally valid synthetic server JAR with the exact declared facts.
    fn write_server_jar(path: &Path, version: &str, data_version: i32, java_major: u32) {
        let file = fs::File::create(path).expect("create server jar");
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file("version.json", options)
            .expect("start version.json");
        let version_json = serde_json::to_vec(&serde_json::json!({
            "id": version,
            "name": version,
            "world_version": data_version,
            "protocol_version": 769,
            "pack_version": { "resource": 46, "data": 61 },
            "java_version": java_major
        }))
        .expect("version json");
        zip.write_all(&version_json).expect("write version.json");
        zip.start_file("net/minecraft/bundler/Main.class", options)
            .expect("start main");
        zip.write_all(b"not-a-real-main-class").expect("write main");
        zip.start_file("padding.bin", options)
            .expect("start padding");
        let chunk = vec![0u8; 1024 * 1024];
        let mut written = 0usize;
        while written < runtime_manager::MIN_SERVER_JAR_BYTES as usize {
            zip.write_all(&chunk).expect("write padding");
            written += chunk.len();
        }
        zip.finish().expect("finish server jar");
    }

    fn assert_manual_assets_ready(
        manager: &ComponentManager,
        status: &ComponentStatus,
        jar: &Path,
        expected_kind: &str,
    ) {
        assert_eq!(status.mode, Some(ComponentMode::Manual));
        assert_eq!(status.state, ComponentState::Ready);
        assert_eq!(status.id.as_deref(), Some("assets-1.21.4"));
        assert_eq!(status.version.as_deref(), Some("1.21.4"));
        assert!(
            status
                .path
                .as_deref()
                .is_some_and(|path| same_path(path, &jar.display().to_string())),
            "status path {:?} must resolve to {}",
            status.path,
            jar.display()
        );
        assert_eq!(
            status.hash.as_ref().map(|hash| hash.algorithm),
            Some(component_model::HashAlgorithm::Sha1)
        );
        assert!(status.bytes.is_some_and(|bytes| bytes > 0));
        status.validate().expect("valid manual assets status");
        assert_eq!(
            manager
                .inner
                .assets
                .saved_source()
                .source
                .as_ref()
                .map(|source| source.kind.as_str()),
            Some(expected_kind)
        );
    }

    #[test]
    fn status_without_components_is_explicit() {
        let root = TempDir::new("empty-status");
        let report = manager(&root)
            .status(&target(), &ComponentRequirements::VANILLA_WORLD)
            .expect("status");
        assert!(!report.ready);
        assert_eq!(report.components.len(), 3);
        for kind in [
            ComponentKind::Java,
            ComponentKind::Assets,
            ComponentKind::Server,
        ] {
            let status = report.status(kind).expect("component status");
            assert_eq!(status.kind, kind);
            assert_eq!(status.state, ComponentState::Missing);
            assert!(status.required);
            status.validate().expect("valid status");
        }
        let json = serde_json::to_value(&report).expect("serialize");
        assert_eq!(json["components"][0]["kind"], "java");
        assert_eq!(json["components"][0]["state"], "missing");
        assert_eq!(json["ready"], false);
    }

    #[test]
    fn editor_requirements_do_not_demand_java_or_a_server() {
        let root = TempDir::new("editor-status");
        let report = manager(&root)
            .status(&target(), &ComponentRequirements::EDITOR)
            .expect("status");
        assert_eq!(
            report.status(ComponentKind::Java).expect("java").state,
            ComponentState::NotRequired
        );
        assert_eq!(
            report.status(ComponentKind::Server).expect("server").state,
            ComponentState::NotRequired
        );
        assert_eq!(
            report.status(ComponentKind::Assets).expect("assets").state,
            ComponentState::Missing
        );
        assert!(!report.ready);
    }

    #[test]
    fn explicit_assets_placeholder_is_unverified_not_ready() {
        let root = TempDir::new("placeholder");
        let requirements = ComponentRequirements {
            assets_placeholder: true,
            ..ComponentRequirements::EDITOR
        };
        let report = manager(&root)
            .status(&target(), &requirements)
            .expect("status");
        let assets = report.status(ComponentKind::Assets).expect("assets");
        assert_eq!(assets.mode, Some(ComponentMode::Placeholder));
        assert_eq!(assets.state, ComponentState::Unverified);
        assert!(assets.id.is_none());
        assert!(assets.path.is_none());
        assert!(assets.hash.is_none());
        assert!(!report.ready);
        assert!(assets
            .notes
            .iter()
            .any(|note| note.contains("placeholder textures")));
    }

    #[test]
    fn explicit_placeholder_selection_wins_over_a_global_source_and_is_never_ready() {
        let root = TempDir::new("placeholder-selection");
        let manager = manager(&root);
        let (minecraft, _) = manual_installation(&root, "1.21.4", 4189);
        manager
            .inner
            .assets
            .save(AssetSourceSetting {
                kind: "minecraft_root".to_owned(),
                path: minecraft.display().to_string(),
            })
            .expect("save source");
        assert!(
            manager
                .status(&target(), &ComponentRequirements::EDITOR)
                .expect("status")
                .ready
        );

        manager
            .inner
            .assets
            .use_placeholder()
            .expect("select placeholder");
        let report = manager
            .status(&target(), &ComponentRequirements::EDITOR)
            .expect("status");
        let assets = report.status(ComponentKind::Assets).expect("assets");
        assert_eq!(assets.mode, Some(ComponentMode::Placeholder));
        assert_eq!(assets.state, ComponentState::Unverified);
        assert!(assets.id.is_none());
        assert!(assets.path.is_none());
        assert!(assets.hash.is_none());
        assert!(!report.ready);
        assert!(report
            .warnings
            .iter()
            .any(|warning| warning.contains("placeholder textures")));

        match manager.resolve(
            ComponentKind::Assets,
            &target(),
            &ComponentRequirements::EDITOR,
        ) {
            Err(ComponentError::NotReady { state, .. }) => assert_eq!(state, "unverified"),
            other => panic!("expected placeholder to be unresolvable, got {other:?}"),
        }
    }

    #[test]
    fn facade_selects_placeholder_assets_explicitly() {
        let root = TempDir::new("facade-placeholder");
        let manager = manager(&root);
        let (minecraft, _) = manual_installation(&root, "1.21.4", 4189);
        manager
            .inner
            .assets
            .save(AssetSourceSetting {
                kind: "minecraft_root".to_owned(),
                path: minecraft.display().to_string(),
            })
            .expect("save source");

        let flattened = minecraft_version::profile_for_id("java-classic-1.16.5").unwrap();
        let flattened = ComponentTarget::from_profile(&flattened);
        let status = manager
            .use_placeholder_assets(&flattened)
            .expect("select placeholder");
        assert_eq!(status.kind, ComponentKind::Assets);
        assert_eq!(status.mode, Some(ComponentMode::Placeholder));
        assert_eq!(status.state, ComponentState::Unverified);
        assert!(status.id.is_none());
        assert!(status.path.is_none());
        assert!(status.hash.is_none());
        assert!(status
            .notes
            .iter()
            .any(|note| note.contains("placeholder textures")));

        let report = manager
            .status(&flattened, &ComponentRequirements::EDITOR)
            .expect("status");
        let assets = report.status(ComponentKind::Assets).expect("assets");
        assert_eq!(assets.mode, Some(ComponentMode::Placeholder));
        assert_eq!(assets.state, ComponentState::Unverified);
        assert!(!report.ready);
        assert!(report
            .warnings
            .iter()
            .any(|warning| warning.contains("placeholder textures")));
    }

    #[test]
    fn invalid_requirements_are_refused() {
        let root = TempDir::new("invalid-requirements");
        let manager = manager(&root);
        let server_without_java = ComponentRequirements {
            java: false,
            assets: true,
            server: true,
            assets_placeholder: false,
        };
        match manager.status(&target(), &server_without_java) {
            Err(ComponentError::InvalidRequest(message)) => {
                assert!(message.contains("without a Java runtime"));
            }
            other => panic!("expected invalid requirements, got {other:?}"),
        }
        let placeholder_without_assets = ComponentRequirements {
            java: false,
            assets: false,
            server: false,
            assets_placeholder: true,
        };
        assert!(manager
            .status(&target(), &placeholder_without_assets)
            .is_err());
    }

    #[test]
    fn resolving_a_missing_component_reports_its_state() {
        let root = TempDir::new("resolve-missing");
        match manager(&root).resolve(
            ComponentKind::Assets,
            &target(),
            &ComponentRequirements::VANILLA_WORLD,
        ) {
            Err(ComponentError::NotReady { kind, state, .. }) => {
                assert_eq!(kind, "assets");
                assert_eq!(state, "missing");
            }
            other => panic!("expected not-ready, got {other:?}"),
        }
    }

    #[test]
    fn resolving_an_optional_component_is_refused() {
        let root = TempDir::new("resolve-optional");
        match manager(&root).resolve(
            ComponentKind::Java,
            &target(),
            &ComponentRequirements::EDITOR,
        ) {
            Err(ComponentError::NotRequired { kind }) => assert_eq!(kind, "java"),
            other => panic!("expected not-required, got {other:?}"),
        }
    }

    #[test]
    fn relink_rejects_a_path_without_a_client_jar() {
        let root = TempDir::new("relink-invalid");
        let manager = manager(&root);
        let file = root.path.join("notes.txt");
        fs::write(&file, "not a client JAR").expect("write fixture");
        match manager.relink(ComponentKind::Assets, &file, &target()) {
            Err(ComponentError::InvalidRequest(_)) => {}
            other => panic!("expected an invalid selection, got {other:?}"),
        }
        assert!(manager.inner.assets.saved_source().source.is_none());
    }

    #[test]
    fn manual_minecraft_root_relinks_to_its_exact_client_jar() {
        let root = TempDir::new("manual-root");
        let manager = manager(&root);
        let (minecraft, version_dir) = manual_installation(&root, "1.21.4", 4189);
        let jar = version_dir.join("1.21.4.jar");

        let status = manager
            .relink(ComponentKind::Assets, &minecraft, &target())
            .expect("relink .minecraft root");
        assert_manual_assets_ready(&manager, &status, &jar, "minecraft_root");

        let resolved = manager
            .resolve(
                ComponentKind::Assets,
                &target(),
                &ComponentRequirements::EDITOR,
            )
            .expect("resolve manual root");
        assert_eq!(resolved.mode, ComponentMode::Manual);
        assert!(same_path(&resolved.path, &jar.display().to_string()));
    }

    #[test]
    fn manual_version_directory_relinks_to_its_client_jar() {
        let root = TempDir::new("manual-version-dir");
        let manager = manager(&root);
        let (_, version_dir) = manual_installation(&root, "1.21.4", 4189);
        let jar = version_dir.join("1.21.4.jar");

        let status = manager
            .relink(ComponentKind::Assets, &version_dir, &target())
            .expect("relink version directory");
        assert_manual_assets_ready(&manager, &status, &jar, "version_directory");
    }

    #[test]
    fn manual_client_jar_relinks_with_its_sibling_metadata() {
        let root = TempDir::new("manual-client-jar");
        let manager = manager(&root);
        let (_, version_dir) = manual_installation(&root, "1.21.4", 4189);
        let jar = version_dir.join("1.21.4.jar");

        let status = manager
            .relink(ComponentKind::Assets, &jar, &target())
            .expect("relink client.jar");
        assert_manual_assets_ready(&manager, &status, &jar, "client_jar");
    }

    #[test]
    fn manual_client_jar_without_launcher_metadata_stays_usable_with_a_warning() {
        let root = TempDir::new("manual-bare-jar");
        let manager = manager(&root);
        let (_, _, jar) = client_jar::test_fixture("1.21.4", 4189);
        let standalone = root.path.join("standalone-client.jar");
        fs::write(&standalone, jar).expect("write standalone jar");

        let status = manager
            .relink(ComponentKind::Assets, &standalone, &target())
            .expect("relink standalone client.jar");
        assert_eq!(status.mode, Some(ComponentMode::Manual));
        assert_eq!(status.state, ComponentState::Ready);
        assert_eq!(status.id.as_deref(), Some("assets-1.21.4"));
        assert!(status
            .notes
            .iter()
            .any(|note| note.contains("was not matched against launcher metadata")));
        status.validate().expect("valid status");
        assert!(manager
            .resolve(
                ComponentKind::Assets,
                &target(),
                &ComponentRequirements::EDITOR,
            )
            .is_ok());
    }

    #[test]
    fn manual_client_jar_with_a_declared_checksum_mismatch_is_refused() {
        let root = TempDir::new("manual-wrong-hash");
        let manager = manager(&root);
        let (_, document, jar) = client_jar::test_fixture("1.21.4", 4189);
        let mut value: serde_json::Value =
            serde_json::from_slice(&document).expect("parse version json");
        value["downloads"]["client"]["sha1"] = serde_json::json!("b".repeat(40));
        let jar_path = root.path.join("client.jar");
        fs::write(&jar_path, jar).expect("write jar");
        fs::write(
            root.path.join("client.json"),
            serde_json::to_vec(&value).expect("serialize metadata"),
        )
        .expect("write metadata");

        match manager.relink(ComponentKind::Assets, &jar_path, &target()) {
            Err(ComponentError::InvalidRequest(message)) => {
                assert!(message.contains("not a usable"), "{message}");
            }
            other => panic!("expected a checksum mismatch to be refused, got {other:?}"),
        }
        assert!(manager.inner.assets.saved_source().source.is_none());
    }

    #[test]
    fn manual_assets_relink_refuses_a_symlinked_root_or_version_directory() {
        let root = TempDir::new("manual-symlink");
        let manager = manager(&root);
        let (minecraft, version_dir) = manual_installation(&root, "1.21.4", 4189);
        for (label, source) in [("root", &minecraft), ("version", &version_dir)] {
            let link = root.path.join(format!("linked-{label}"));
            link_directory(&link, source);
            match manager.relink(ComponentKind::Assets, &link, &target()) {
                Err(ComponentError::InvalidRequest(message)) => {
                    assert!(message.contains("symlink"), "{label}: {message}");
                }
                other => panic!("expected the {label} reparse point to be refused, got {other:?}"),
            }
        }
        assert!(manager.inner.assets.saved_source().source.is_none());
    }

    #[test]
    fn manual_assets_relink_refuses_a_client_jar_for_another_version() {
        let root = TempDir::new("manual-wrong-version");
        let manager = manager(&root);
        let (_, _, jar) = client_jar::test_fixture("1.20.6", 3839);
        let path = root.path.join("client.jar");
        fs::write(&path, jar).expect("write client jar");

        match manager.relink(ComponentKind::Assets, &path, &target()) {
            Err(ComponentError::InvalidRequest(message)) => {
                assert!(message.contains("not a usable Minecraft"), "{message}");
            }
            other => panic!("expected a version mismatch to be refused, got {other:?}"),
        }
        assert!(manager.inner.assets.saved_source().source.is_none());
    }

    #[test]
    fn manual_server_relink_refuses_a_symlinked_jar() {
        let root = TempDir::new("server-symlink");
        let manager = manager(&root);
        let real = root.path.join("server");
        fs::create_dir_all(&real).expect("create server directory");
        let link = root.path.join("linked-server.jar");
        link_directory(&link, &real);

        match manager.relink(ComponentKind::Server, &link, &target()) {
            Err(ComponentError::Server(runtime_manager::ServerError::Invalid(message))) => {
                assert!(message.contains("symlink"), "{message}");
            }
            other => panic!("expected the server reparse point to be refused, got {other:?}"),
        }
        assert!(manager
            .inner
            .server
            .selected(&runtime_manager::ServerTarget::for_profile(
                &minecraft_version::default_profile()
            ))
            .is_none());
    }

    #[test]
    fn manual_java_relink_refuses_a_symlinked_or_directory_path() {
        let root = TempDir::new("java-symlink");
        let manager = manager(&root);
        let real = root.path.join("jdk");
        fs::create_dir_all(&real).expect("create jdk directory");
        let link = root.path.join("linked-jdk");
        link_directory(&link, &real);

        match manager.relink(ComponentKind::Java, &link, &target()) {
            Err(ComponentError::Runtime(runtime_manager::RuntimeError::Invalid(message))) => {
                assert!(message.contains("not a Java executable"), "{message}");
            }
            other => panic!("expected the Java reparse point to be refused, got {other:?}"),
        }
        assert!(manager.inner.java.saved().is_none());
    }

    #[test]
    fn manual_java_relink_refuses_an_unsupported_major_before_probing() {
        let root = TempDir::new("java-major");
        let manager = manager(&root);
        let unsupported = ComponentTarget {
            java_major: 22,
            ..target()
        };
        match manager.relink(
            ComponentKind::Java,
            &root.path.join("missing-java"),
            &unsupported,
        ) {
            Err(ComponentError::UnsupportedTarget { detail, .. }) => {
                assert!(detail.contains("Java 22"), "{detail}");
            }
            other => panic!("expected an unsupported Java major, got {other:?}"),
        }
    }

    #[test]
    fn placeholder_selection_survives_a_fresh_manager_and_stays_unverified() {
        let root = TempDir::new("placeholder-persist");
        manager(&root)
            .inner
            .assets
            .use_placeholder()
            .expect("select placeholder");

        let manager = manager(&root);
        let status = manager.assets_status(&target(), true, false);
        assert_eq!(status.mode, Some(ComponentMode::Placeholder));
        assert_eq!(status.state, ComponentState::Unverified);
        assert!(status.id.is_none());
        assert!(status.path.is_none());
        assert!(status.hash.is_none());
        assert!(status.bytes.is_none());
        assert!(manager
            .resolve(
                ComponentKind::Assets,
                &target(),
                &ComponentRequirements::EDITOR,
            )
            .is_err());
    }

    #[test]
    fn assets_relink_still_refuses_a_target_the_asset_manager_does_not_manage() {
        let root = TempDir::new("relink-target");
        let manager = manager(&root);
        let file = root.path.join("client.jar");
        fs::write(&file, "not a client JAR").expect("write fixture");
        let other_target = ComponentTarget {
            profile_id: "java-classic-1.13.2".to_owned(),
            minecraft_version: "1.13.2".parse().expect("version"),
            data_version: 1519,
            java_major: 8,
        };
        match manager.relink(ComponentKind::Assets, &file, &other_target) {
            Err(ComponentError::UnsupportedTarget {
                minecraft_version, ..
            }) => assert_eq!(minecraft_version, "1.13.2"),
            other => panic!("expected an unsupported target, got {other:?}"),
        }
        assert!(manager.inner.assets.saved_source().source.is_none());
    }

    #[test]
    fn server_relink_supports_an_exact_non_baseline_target() {
        let root = TempDir::new("server-other-version");
        let manager = manager(&root);
        let other_target = ComponentTarget {
            profile_id: "java-classic-1.20.6".to_owned(),
            minecraft_version: "1.20.6".parse().expect("version"),
            data_version: 3839,
            java_major: 21,
        };
        let jar = root.path.join("server.jar");
        write_server_jar(&jar, "1.20.6", 3839, 21);

        let status = manager
            .relink(ComponentKind::Server, &jar, &other_target)
            .expect("relink exact server version");
        assert_eq!(status.mode, Some(ComponentMode::Manual));
        assert_eq!(status.state, ComponentState::Ready);
        assert_eq!(status.id.as_deref(), Some("server-manual-1.20.6"));
        assert_eq!(status.version.as_deref(), Some("1.20.6"));
        status.validate().expect("valid server status");
        assert!(manager
            .inner
            .server
            .has_selection(&other_target.minecraft_version));

        // The baseline target does not silently reuse the 1.20.6 selection.
        let baseline = manager.server_status(&target(), true);
        assert_eq!(baseline.state, ComponentState::Missing);

        let removal = manager
            .remove(ComponentKind::Server, "server-manual-1.20.6")
            .expect("remove manual server selection");
        assert!(!removal.removed_managed);
        assert!(removal.cleared_selection);
        assert!(jar.exists(), "a manual file is never deleted");
        assert!(!manager
            .inner
            .server
            .has_selection(&other_target.minecraft_version));
    }

    #[test]
    fn server_relink_refuses_a_jar_for_another_target() {
        let root = TempDir::new("server-wrong-version");
        let manager = manager(&root);
        let jar = root.path.join("server.jar");
        write_server_jar(&jar, "1.20.6", 3839, 21);
        match manager.relink(ComponentKind::Server, &jar, &target()) {
            Err(ComponentError::Server(_)) => {}
            other => panic!("expected the server JAR to be refused, got {other:?}"),
        }
        assert!(manager
            .inner
            .server
            .selected(&runtime_manager::ServerTarget::for_profile(
                &minecraft_version::default_profile()
            ))
            .is_none());
    }

    #[test]
    fn remove_clears_a_manual_asset_pointer_without_deleting_the_file() {
        let root = TempDir::new("remove-assets");
        let manager = manager(&root);
        let file = root.path.join("client.jar");
        fs::write(&file, "user file").expect("write fixture");
        manager
            .inner
            .assets
            .save(AssetSourceSetting {
                kind: "client_jar".to_owned(),
                path: file.display().to_string(),
            })
            .expect("save source");
        let removal = manager
            .remove(ComponentKind::Assets, "assets-1.21.4")
            .expect("remove");
        assert!(!removal.removed_managed);
        assert!(removal.cleared_selection);
        assert!(manager.inner.assets.saved_source().source.is_none());
        assert!(file.exists(), "a manual file is never deleted");
    }

    #[test]
    fn remove_refuses_an_unknown_component() {
        let root = TempDir::new("remove-unknown");
        match manager(&root).remove(ComponentKind::Java, "unknown") {
            Err(ComponentError::NotFound { kind, .. }) => assert_eq!(kind, "java"),
            other => panic!("expected not-found, got {other:?}"),
        }
    }

    #[test]
    fn managed_assets_are_ready_without_a_saved_pointer() {
        let root = TempDir::new("managed-assets");
        let manager = manager(&root);
        let version: VersionId = "1.21.4".parse().expect("version");
        let (release, document, jar) = client_jar::test_fixture("1.21.4", 4189);
        manager
            .inner
            .client_jars
            .install_reader(
                &version,
                &release,
                &document,
                Cursor::new(jar),
                Some(4189),
                &AtomicBool::new(false),
                &|_| {},
            )
            .expect("seed managed client jar");

        let report = manager
            .status(&target(), &ComponentRequirements::EDITOR)
            .expect("status");
        let assets = report.status(ComponentKind::Assets).expect("assets");
        assert_eq!(assets.mode, Some(ComponentMode::Managed));
        assert_eq!(assets.id.as_deref(), Some("assets-managed-1.21.4"));
        assert_eq!(assets.version.as_deref(), Some("1.21.4"));
        assert_eq!(assets.state, ComponentState::Ready);
        assert!(assets.hash.is_some());
        let inventory = manager.assets_inventory().expect("inventory");
        assert_eq!(
            assets.path.as_deref(),
            Some(inventory.clients[0].jar_path.as_str())
        );

        let resolved = manager
            .resolve(
                ComponentKind::Assets,
                &target(),
                &ComponentRequirements::EDITOR,
            )
            .expect("resolve");
        assert_eq!(resolved.mode, ComponentMode::Managed);
        assert_eq!(resolved.id, "assets-managed-1.21.4");
        assert!(Path::new(&resolved.path).is_file());
        assert_eq!(
            resolved.hash.algorithm,
            component_model::HashAlgorithm::Sha1
        );

        let removal = manager
            .remove(ComponentKind::Assets, "assets-managed-1.21.4")
            .expect("remove");
        assert!(removal.removed_managed);
        assert!(!removal.cleared_selection);
        assert!(manager
            .assets_inventory()
            .expect("inventory")
            .clients
            .is_empty());
    }

    #[test]
    fn flattened_client_without_internal_version_is_verified_and_resolved_from_the_cache() {
        let root = TempDir::new("flattened-assets");
        let manager = manager(&root);
        let profile = minecraft_version::profile_for_id("java-classic-1.13.2").unwrap();
        let version = &profile.minecraft_version;
        let target = ComponentTarget::from_profile(&profile);
        let (release, document, jar) =
            client_jar::test_fixture(version.as_str(), profile.data_version);
        let installed = manager
            .inner
            .client_jars
            .install_reader(
                version,
                &release,
                &document,
                Cursor::new(&jar),
                Some(profile.data_version),
                &AtomicBool::new(false),
                &|_| {},
            )
            .unwrap();
        assert_eq!(installed.data_version, profile.data_version);
        assert_eq!(installed.resource_pack_format, Some(4));
        assert_eq!(installed.datapack_format, Some(4));
        let verified = manager
            .verify_inventory(ComponentKind::Assets, &installed.id)
            .unwrap();
        assert!(verified.verified, "{verified:?}");

        manager.use_placeholder_assets(&target).unwrap();
        let resolved = manager.resolve_managed_assets(&target).unwrap();
        assert_eq!(resolved.hash.value, installed.sha1);

        let directory = root
            .path
            .join("launcher")
            .join("versions")
            .join(version.as_str());
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join(format!("{version}.jar")), &jar).unwrap();
        fs::write(directory.join(format!("{version}.json")), &document).unwrap();
        let status = manager
            .relink(ComponentKind::Assets, &root.path.join("launcher"), &target)
            .unwrap();
        assert_eq!(status.mode, Some(ComponentMode::Manual));
        assert_eq!(status.state, ComponentState::Ready);
        assert_eq!(status.hash.unwrap().value, installed.sha1);

        fs::remove_file(Path::new(&installed.jar_path).with_extension("json")).unwrap();
        assert!(manager.resolve_managed_assets(&target).is_err());
        let repaired = manager
            .inner
            .client_jars
            .install_reader(
                version,
                &release,
                &document,
                Cursor::new(&jar),
                Some(profile.data_version),
                &AtomicBool::new(false),
                &|_| {},
            )
            .unwrap();
        assert_eq!(repaired.sha1, installed.sha1);
        assert!(manager.resolve_managed_assets(&target).is_ok());
    }

    #[test]
    fn inventory_verification_checks_managed_assets_and_server_facts() {
        let root = TempDir::new("inventory-verify");
        let manager = manager(&root);
        let version: VersionId = "1.21.4".parse().expect("version");
        let (release, document, jar) = client_jar::test_fixture("1.21.4", 4189);
        manager
            .inner
            .client_jars
            .install_reader(
                &version,
                &release,
                &document,
                Cursor::new(jar),
                Some(4189),
                &AtomicBool::new(false),
                &|_| {},
            )
            .expect("seed managed client jar");

        let inventory = manager.inventory().expect("inventory");
        let assets_id = inventory.assets.clients[0].id.clone();
        let assets = manager
            .verify_inventory(ComponentKind::Assets, &assets_id)
            .expect("verify assets");
        assert!(assets.verified, "{assets:?}");
        assert_eq!(assets.state, ComponentState::Ready);
        assert_eq!(assets.id, assets_id);

        let server_id = seed_managed_server(&root, "1.21.4", 4189, 21);
        let server = manager
            .verify_inventory(ComponentKind::Server, &server_id)
            .expect("verify server");
        assert!(server.verified, "{server:?}");
        assert_eq!(server.state, ComponentState::Ready);
        assert_eq!(server.id, server_id);
    }

    #[test]
    fn inventory_verification_reports_a_changed_client_jar() {
        let root = TempDir::new("inventory-tampered");
        let manager = manager(&root);
        let version: VersionId = "1.21.4".parse().expect("version");
        let (release, document, jar) = client_jar::test_fixture("1.21.4", 4189);
        let installed = manager
            .inner
            .client_jars
            .install_reader(
                &version,
                &release,
                &document,
                Cursor::new(jar),
                Some(4189),
                &AtomicBool::new(false),
                &|_| {},
            )
            .expect("seed managed client jar");

        let mut bytes = fs::read(&installed.jar_path).expect("read client jar");
        let needle = br#"{"variants":{"":{"model":"minecraft:block/stone"}}}"#;
        let offset = bytes
            .windows(needle.len())
            .position(|window| window == needle)
            .expect("find blockstate payload");
        bytes[offset] = b'!';
        fs::write(&installed.jar_path, &bytes).expect("tamper client jar");

        let verification = manager
            .verify_inventory(ComponentKind::Assets, &installed.id)
            .expect("verify assets");
        assert!(!verification.verified);
        assert_eq!(verification.state, ComponentState::Incompatible);
        assert!(
            verification
                .error
                .as_deref()
                .is_some_and(|error| error.contains("does not match its SHA-1")),
            "{verification:?}"
        );
    }

    #[test]
    fn inventory_verification_refuses_ids_that_are_not_installed() {
        let root = TempDir::new("inventory-unknown");
        let manager = manager(&root);
        for (kind, id) in [
            (ComponentKind::Java, "temurin-21.0.5-11"),
            (ComponentKind::Assets, "assets-managed-1.21.4"),
            (ComponentKind::Server, "server-managed-1.21.4"),
            (ComponentKind::Server, "1.21.4"),
        ] {
            match manager.verify_inventory(kind, id) {
                Err(ComponentError::NotFound { kind: reported, .. }) => {
                    assert_eq!(reported, kind.as_str())
                }
                other => panic!("expected {kind:?} {id} to be refused, got {other:?}"),
            }
        }
    }

    #[test]
    fn assets_release_refuses_a_target_the_managed_cache_does_not_support() {
        let root = TempDir::new("assets-release-target");
        let manager = manager(&root);
        let profile = minecraft_version::profile_for_id("java-split-26.1").unwrap();
        let target = ComponentTarget::from_profile(&profile);
        match manager.assets_release(&target) {
            Err(ComponentError::UnsupportedTarget {
                minecraft_version, ..
            }) => assert_eq!(minecraft_version, "26.1"),
            other => panic!("expected an unsupported target, got {other:?}"),
        }
    }

    #[test]
    fn java_provisioning_accepts_any_supported_project_major() {
        let root = TempDir::new("java-majors");
        let manager = manager(&root);
        let profile = minecraft_version::default_profile();
        let mut target = ComponentTarget::from_profile(&profile);
        for major in [8, 16, 17, 21, 25] {
            target.java_major = major;
            manager
                .ensure_java_major(&target)
                .unwrap_or_else(|error| panic!("Java {major} must be provisionable: {error}"));
        }
        target.java_major = 11;
        match manager.ensure_java_major(&target) {
            Err(ComponentError::UnsupportedTarget { detail, .. }) => {
                assert!(detail.contains("asks for Java 11"), "{detail}");
            }
            other => panic!("expected an unsupported Java major, got {other:?}"),
        }
    }

    #[test]
    fn cancel_is_not_running_when_idle() {
        let root = TempDir::new("cancel-idle");
        assert_eq!(
            manager(&root).cancel(ComponentKind::Java),
            CancelOutcome::NotRunning
        );
    }

    #[test]
    fn targets_come_from_the_version_profile() {
        let profile = minecraft_version::default_profile();
        let target = ComponentTarget::from_profile(&profile);
        assert_eq!(target.profile_id, profile.id);
        assert_eq!(target.minecraft_version, profile.minecraft_version);
        assert_eq!(target.data_version, profile.data_version);
        assert_eq!(target.java_major, profile.java.recommended_major);
        target.validate().expect("valid target");
    }

    #[test]
    fn java_compatibility_uses_the_project_java_major() {
        let profile = minecraft_version::default_profile();
        let target = ComponentTarget {
            java_major: 17,
            ..ComponentTarget::from_profile(&profile)
        };
        let mut report = JavaRuntimeReport {
            path: "java.exe".to_owned(),
            source: runtime_manager::RuntimeSource::Manual,
            status: RuntimeStatus::Outdated,
            version: Some("17.0.10".to_owned()),
            major: Some(17),
            vendor: Some("Eclipse Adoptium".to_owned()),
            arch: Some("x86_64".to_owned()),
            vm: Some("OpenJDK".to_owned()),
            managed: false,
            warnings: Vec::new(),
            error: None,
        };
        assert_eq!(
            java_compatibility(&report, &target),
            (ComponentState::Ready, None)
        );

        let baseline = ComponentTarget::from_profile(&profile);
        let (state, error) = java_compatibility(&report, &baseline);
        assert_eq!(state, ComponentState::Incompatible);
        assert!(error.expect("older Java error").contains("older than"));

        report.major = Some(baseline.java_major + 4);
        let (state, error) = java_compatibility(&report, &baseline);
        assert_eq!(state, ComponentState::Ready);
        assert!(error.is_none());

        report.arch = Some("x86".to_owned());
        assert_eq!(
            java_compatibility(&report, &baseline).0,
            ComponentState::Incompatible
        );
    }

    #[test]
    fn inventory_lists_every_managed_cache_without_downloading() {
        let root = TempDir::new("inventory");
        let manager = manager(&root);
        let inventory = manager.inventory().expect("inventory");
        assert!(inventory.java.runtimes.is_empty());
        assert!(inventory.assets.clients.is_empty());
        assert!(inventory.servers.servers.is_empty());
        assert_eq!(inventory.java.total_bytes, 0);
        assert_eq!(inventory.assets.total_bytes, 0);
        assert_eq!(inventory.servers.total_bytes, 0);
    }

    #[test]
    fn release_previews_fail_offline_before_any_request() {
        let root = TempDir::new("offline-release");
        let manager = ComponentManager::from_paths_with_client(
            root.path.join("runtime.json"),
            root.path.join("runtimes"),
            root.path.join("assets.json"),
            root.path.join("components").join("assets"),
            root.path.join("cache").join("version_manifest_v2.json"),
            root.path.join("server.json"),
            root.path.join("servers"),
            DownloadClient::new(NetworkConfig::offline()),
        );
        let target = target();
        for error in [
            manager.java_release(&target).expect_err("java release"),
            manager.assets_release(&target).expect_err("assets release"),
            manager.server_release(&target).expect_err("server release"),
        ] {
            assert_eq!(error.code(), "offline", "{error:?}");
        }
    }
}
