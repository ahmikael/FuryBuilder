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

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use asset_source::{
    AssetPathReport, AssetSelectionKind, AssetSettingsError, AssetSettingsStore,
    AssetSourceSetting, VersionReport, VersionStatus,
};
use component_model::{
    ComponentError as StatusError, ComponentHash, ComponentKind, ComponentMode, ComponentState,
    ComponentStatus, HashAlgorithm,
};
use minecraft_version::{VersionId, VersionProfile};
use runtime_manager::{
    JavaRuntimeReport, RuntimeError, RuntimeManager, ServerError, ServerManager, ServerProgress,
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
    #[error("component `{kind}` is not available yet: {detail}")]
    Unavailable { kind: &'static str, detail: String },
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
}

impl ComponentError {
    pub fn code(&self) -> &'static str {
        match self {
            ComponentError::InvalidRequest(_) => "invalid_argument",
            ComponentError::UnsupportedTarget { .. } => "unsupported_version",
            ComponentError::NotRequired { .. } => "invalid_argument",
            ComponentError::NotReady { .. } => "not_ready",
            ComponentError::NotFound { .. } => "not_found",
            ComponentError::Unavailable { .. } => "unsupported_version",
            ComponentError::Busy { .. } => "busy",
            ComponentError::Cancelled { .. } => "cancelled",
            ComponentError::ChecksumMismatch { .. } => "checksum_mismatch",
            ComponentError::Hash { .. } => "io_error",
            ComponentError::Status(_) => "invalid_argument",
            ComponentError::Runtime(error) => error.code(),
            ComponentError::Server(error) => error.code(),
            ComponentError::Assets(error) => error.code(),
        }
    }
}

struct ComponentManagerInner {
    java: RuntimeManager,
    assets: AssetSettingsStore,
    server: ServerManager,
    installs: Mutex<BTreeMap<ComponentKind, Arc<AtomicBool>>>,
}

/// Unified facade. Cloning it shares the managers and the active-install registry.
#[derive(Clone)]
pub struct ComponentManager {
    inner: Arc<ComponentManagerInner>,
}

impl ComponentManager {
    pub fn new(java: RuntimeManager, assets: AssetSettingsStore, server: ServerManager) -> Self {
        Self {
            inner: Arc::new(ComponentManagerInner {
                java,
                assets,
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
        server_settings: impl Into<PathBuf>,
        server_root: impl Into<PathBuf>,
    ) -> Self {
        Self::new(
            RuntimeManager::new(java_settings, java_root),
            AssetSettingsStore::new(assets_settings),
            ServerManager::new(server_settings, server_root),
        )
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
                self.ensure_managed_target(target)?;
                let report = asset_source::inspect_asset_path(path);
                if !report.valid || !report.target_match {
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
                self.ensure_managed_target(target)?;
                self.inner.server.select_manual(path)?;
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
                if id.starts_with("assets-") && self.inner.assets.saved_source().source.is_some() {
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
                } else if id.starts_with("server-manual-") {
                    if let Some(report) = self.inner.server.selected() {
                        if server_id(&report) == id {
                            self.inner.server.clear_selection()?;
                            cleared_selection = true;
                        }
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
            ComponentKind::Assets => Err(ComponentError::Unavailable {
                kind: ComponentKind::Assets.as_str(),
                detail:
                    "client JAR downloading is not enabled in this build; choose a local .minecraft root or client JAR"
                        .to_owned(),
            }),
            ComponentKind::Java => {
                self.ensure_java_major(&request.target)?;
                let guarded = self.begin_install(ComponentKind::Java)?;
                let cancel = guarded.token();
                let release = self.inner.java.latest_temurin(request.target.java_major)?;
                let expected_version = request.expected_version.as_deref().ok_or_else(|| {
                    ComponentError::InvalidRequest(
                        "a Java install requires the reviewed Adoptium release version"
                            .to_owned(),
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
                self.ensure_managed_target(&request.target)?;
                let guarded = self.begin_install(ComponentKind::Server)?;
                let cancel = guarded.token();
                let release = self.inner.server.release()?;
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
                    .install(expected_version, &cancel, &|update: ServerProgress| {
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
        let saved = self.inner.assets.saved_source();
        let Some(source) = saved.source else {
            if placeholder {
                let mut status =
                    ComponentStatus::new(ComponentKind::Assets, ComponentState::Unverified);
                status.mode = Some(ComponentMode::Placeholder);
                status.notes.push(
                    "placeholder textures are enabled; there is no client JAR catalog for this project"
                        .to_owned(),
                );
                return finalize(status);
            }
            let mut status = ComponentStatus::new(ComponentKind::Assets, ComponentState::Missing);
            status.notes.push(
                "no Minecraft client assets are selected; choose a .minecraft root, version folder or client JAR"
                    .to_owned(),
            );
            return finalize(status);
        };
        let Some(report) = saved.report else {
            let mut status = ComponentStatus::new(ComponentKind::Assets, ComponentState::Error);
            status.mode = Some(ComponentMode::Manual);
            status.path = Some(source.path);
            status.error = Some("the selected asset source cannot be inspected".to_owned());
            return finalize(status);
        };

        let mut status = ComponentStatus::new(ComponentKind::Assets, ComponentState::Missing);
        status.mode = Some(ComponentMode::Manual);
        status.path = Some(report.path.clone());
        status.notes.extend(report.warnings.clone());
        if let Some(warning) = saved.load_warning {
            status.notes.push(warning);
        }
        let Some(version) = version_for_target(&report, target) else {
            status.state = ComponentState::Incompatible;
            if let Some(version) = report.version.as_ref() {
                status.id = Some(format!("assets-{}", version.id));
                status.version = Some(version.id.clone());
            }
            status.error = Some(match report.kind {
                AssetSelectionKind::ResourcePack => {
                    "a resource pack is not a client JAR; select a Minecraft installation or client JAR"
                        .to_owned()
                }
                _ => format!(
                    "the selected asset source has no client JAR for Minecraft {}",
                    target.minecraft_version
                ),
            });
            return finalize(status);
        };

        status.id = Some(format!("assets-{}", version.id));
        status.version = Some(version.id.clone());
        status.path = Some(version.jar_path.clone());
        status.bytes = version.jar_bytes;
        status.notes.extend(version.notes.clone());
        if !version.jar_present {
            status.state = ComponentState::Missing;
            status.error = Some(format!(
                "the client JAR for Minecraft {} is missing from disk",
                version.id
            ));
            return finalize(status);
        }

        let id_matches = version.id == target.minecraft_version.as_str()
            || version.internal_version_id.as_deref() == Some(target.minecraft_version.as_str());
        if !id_matches {
            status.state = ComponentState::Incompatible;
            status.error = Some(format!(
                "the selected client JAR is Minecraft {}, not {}",
                version.id, target.minecraft_version
            ));
            return finalize(status);
        }
        match version.world_version {
            Some(data_version) if data_version != target.data_version => {
                status.state = ComponentState::Incompatible;
                status.error = Some(format!(
                    "the client JAR declares DataVersion {data_version}, not {}",
                    target.data_version
                ));
                return finalize(status);
            }
            None => {
                status.state = ComponentState::Unverified;
                status
                    .notes
                    .push("the client JAR did not declare a DataVersion".to_owned());
            }
            Some(_) => {}
        }
        match version.status {
            VersionStatus::ChecksumMismatch => {
                status.state = ComponentState::Incompatible;
                status.error = Some("the client JAR checksum does not match".to_owned());
                return finalize(status);
            }
            VersionStatus::InvalidClientJar
            | VersionStatus::InvalidMetadata
            | VersionStatus::MissingMetadata => {
                status.state = ComponentState::Error;
                status.error = Some(format!(
                    "the selected client JAR metadata is invalid: {}",
                    first_reason(&report)
                ));
                return finalize(status);
            }
            _ => {}
        }
        status.hash = version
            .actual_sha1
            .clone()
            .or_else(|| version.expected_sha1.clone())
            .map(ComponentHash::sha1);
        if status.hash.is_none() {
            status.state = ComponentState::Unverified;
            status
                .notes
                .push("no SHA-1 is available for the client JAR".to_owned());
        } else if version.checksum_verified {
            status.state = ComponentState::Ready;
        } else if status.state != ComponentState::Unverified {
            status.state = ComponentState::Unverified;
            status
                .notes
                .push("the client JAR checksum could not be verified".to_owned());
        }
        finalize(status)
    }

    fn server_status(&self, target: &ComponentTarget, required: bool) -> ComponentStatus {
        if !required {
            return ComponentStatus::not_required(ComponentKind::Server);
        }
        let Some(report) = self.inner.server.selected() else {
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
        status.id = Some(server_id(&report));
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

    fn ensure_managed_target(&self, target: &ComponentTarget) -> Result<(), ComponentError> {
        let managed_version = runtime_manager::target_minecraft_version();
        let managed_data_version = runtime_manager::target_data_version();
        if target.minecraft_version.as_str() != managed_version
            || target.data_version != managed_data_version
        {
            return Err(ComponentError::UnsupportedTarget {
                minecraft_version: target.minecraft_version.to_string(),
                data_version: target.data_version,
                detail: format!(
                    "component validation and downloads are implemented for Minecraft {managed_version} (DataVersion {managed_data_version}); this build cannot provision or validate components for the requested target yet"
                ),
            });
        }
        Ok(())
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

fn version_for_target<'a>(
    report: &'a AssetPathReport,
    target: &ComponentTarget,
) -> Option<&'a VersionReport> {
    let wanted = target.minecraft_version.as_str();
    report
        .version
        .iter()
        .chain(
            report
                .installation
                .iter()
                .flat_map(|installation| installation.versions.iter()),
        )
        .find(|version| {
            version.id == wanted || version.internal_version_id.as_deref() == Some(wanted)
        })
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

fn server_id(report: &runtime_manager::ServerJarReport) -> String {
    let mode = if report.managed { "managed" } else { "manual" };
    format!(
        "server-{mode}-{}",
        report.version.as_deref().unwrap_or("unknown")
    )
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
            root.path.join("server.json"),
            root.path.join("servers"),
        )
    }

    fn target() -> ComponentTarget {
        ComponentTarget::from_profile(&minecraft_version::default_profile())
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
    fn relink_refuses_a_target_the_managers_do_not_manage() {
        let root = TempDir::new("relink-target");
        let manager = manager(&root);
        let file = root.path.join("server.jar");
        fs::write(&file, "not a server JAR").expect("write fixture");
        let other_target = ComponentTarget {
            profile_id: "java-classic-1.20.6".to_owned(),
            minecraft_version: "1.20.6".parse().expect("version"),
            data_version: 3839,
            java_major: 21,
        };
        match manager.relink(ComponentKind::Server, &file, &other_target) {
            Err(ComponentError::UnsupportedTarget {
                minecraft_version, ..
            }) => assert_eq!(minecraft_version, "1.20.6"),
            other => panic!("expected an unsupported target, got {other:?}"),
        }
        assert!(manager.inner.server.selected().is_none());
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
}
