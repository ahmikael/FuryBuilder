//! Official Minecraft server JAR provisioning and isolated Vanilla World generation.
//!
//! The JAR is resolved from Mojang's official `version_manifest_v2` → version JSON →
//! `downloads.server`, verified by the announced SHA-1 and size, inspected for the target
//! profile's `world_version` and published into an offline, deletable cache. A user can
//! also select an existing local JAR, which must pass the same internal-version and server
//! entry-point checks.
//!
//! Isolated generation requires an explicit EULA acceptance for that exact JAR and a
//! separate per-run consent. The server is bound to loopback only, runs in a temporary
//! directory, is bounded by memory/time and is placed in a kill-on-close Windows Job Object
//! (or Unix process group), so cancel, crash and normal completion all terminate the whole
//! Java/bundler tree. M9.6 adds the Vanilla World wizard contract: target Java/runtime setup,
//! supported worldgen settings, validated local data packs and an explicit local-run consent.
//! Importing the generated world into the Rust World Core belongs to M9.7.

mod cache;
mod datapack;
mod generation;
mod jar;
mod mojang;
mod process_tree;
mod settings;
mod types;

use cache::{source_for, ServerCache, ServerManifest};
use mojang::{remove_file_if_exists, MojangClient};
use settings::ServerSettingsStore;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use minecraft_version::VersionId;

use crate::probe::probe_java;
use crate::types::RuntimeStatus;

pub use jar::{inspect_server_jar, sha1_file, ServerJarInfo};
pub use mojang::VERSION_MANIFEST_URL;
pub use types::{
    DatapackInspection, DatapackSelection, GeneratedWorld, GenerationDatapack, GenerationPhase,
    GenerationReport, GenerationRequest, GenerationRunInfo, GenerationRunsReport,
    GenerationSnapshot, GenerationStatusReport, GenerationStopReason, InstalledServerJar,
    ManagedServerCacheReport, ManagedServerJar, RunManifest, SavedEula, SavedServerJar,
    ServerDiscoveryReport, ServerJarReport, ServerProgress, ServerReleaseInfo, ServerSource,
    ServerTarget, WorldPreset, EULA_URL, MAX_SERVER_JAR_BYTES, MIN_SERVER_JAR_BYTES,
    RUN_MANIFEST_SCHEMA_VERSION, SERVER_MANIFEST_SCHEMA_VERSION, SERVER_SETTINGS_SCHEMA_VERSION,
};

use crate::error::ServerError;
use crate::target::{target_data_version, target_java_major, target_minecraft_version};

struct ActiveGeneration {
    run_id: String,
    cancel: Arc<AtomicBool>,
    snapshot: Arc<Mutex<GenerationSnapshot>>,
}

#[derive(Default)]
struct GenerationState {
    active: Option<ActiveGeneration>,
    last: Option<GenerationReport>,
}

struct ServerManagerInner {
    settings: ServerSettingsStore,
    cache: ServerCache,
    mojang: MojangClient,
    install_lock: Mutex<()>,
    generation_start_lock: Mutex<()>,
    generation: Mutex<GenerationState>,
    runs_root: PathBuf,
}

/// Application-facing facade. It never installs anything globally; the isolated server
/// process starts only after an explicit EULA acceptance for the exact selected JAR.
#[derive(Clone)]
pub struct ServerManager {
    inner: Arc<ServerManagerInner>,
}

impl ServerManager {
    pub fn new(
        settings_path: impl Into<PathBuf>,
        managed_root: impl Into<PathBuf>,
        manifest_cache: impl Into<PathBuf>,
    ) -> Self {
        Self::with_client(
            settings_path,
            managed_root,
            manifest_cache,
            download_pipeline::DownloadClient::new(download_pipeline::NetworkConfig::default()),
        )
    }

    pub fn with_client(
        settings_path: impl Into<PathBuf>,
        managed_root: impl Into<PathBuf>,
        manifest_cache: impl Into<PathBuf>,
        client: download_pipeline::DownloadClient,
    ) -> Self {
        let settings_path = settings_path.into();
        let runs_root = settings_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("server-runs");
        let last = generation::load_last_report(&runs_root);
        Self {
            inner: Arc::new(ServerManagerInner {
                settings: ServerSettingsStore::new(settings_path),
                cache: ServerCache::new(managed_root),
                mojang: MojangClient::with_client(manifest_cache, client),
                install_lock: Mutex::new(()),
                generation_start_lock: Mutex::new(()),
                generation: Mutex::new(GenerationState { active: None, last }),
                runs_root,
            }),
        }
    }

    pub fn settings_path(&self) -> &Path {
        self.inner.settings.path()
    }

    pub fn managed_root(&self) -> &Path {
        self.inner.cache.root()
    }

    pub fn runs_root(&self) -> &Path {
        &self.inner.runs_root
    }

    /// Validates selected local data packs without starting anything. The result is per-path
    /// so the wizard can show why a pack is rejected.
    pub fn inspect_datapacks(&self, paths: &[String]) -> Vec<DatapackInspection> {
        datapack::inspect_all(paths)
    }

    pub fn discover(&self) -> ServerDiscoveryReport {
        let target = ServerTarget::baseline();
        let (managed, managed_bytes, mut problems) = match self.inner.cache.list() {
            Ok((managed, bytes, problems)) => (managed, bytes, problems),
            Err(error) => (
                Vec::new(),
                0,
                vec![format!("cannot read the managed server cache: {error}")],
            ),
        };
        if let Some(warning) = self.inner.settings.load_warning() {
            problems.push(warning.to_owned());
        }
        let selected = self.selected(&target);
        let eula = selected
            .as_ref()
            .filter(|report| report.ready)
            .and_then(|report| {
                let version = report.version.as_deref()?;
                let sha1 = report.sha1.as_deref()?;
                self.inner.settings.eula_for(version, sha1)
            });
        let eula_matches_selected = eula.is_some();
        ServerDiscoveryReport {
            target_minecraft_version: target_minecraft_version().to_owned(),
            target_data_version: target_data_version(),
            target_java_major: target_java_major(),
            selected,
            eula,
            eula_matches_selected,
            managed_root: self.inner.cache.root().display().to_string(),
            managed,
            managed_bytes,
            problems,
            manual_hint: manual_hint(&target),
        }
    }

    /// Returns the remembered selection for exactly this target after re-reading the file.
    /// A selection for another Minecraft version is never substituted.
    pub fn selected(&self, target: &ServerTarget) -> Option<ServerJarReport> {
        let saved = self.inner.settings.selected(&target.minecraft_version)?;
        let official = self.official_sha1_for(Path::new(&saved.path));
        Some(self.report_for(
            Path::new(&saved.path),
            Some(&saved),
            official.as_deref(),
            target,
        ))
    }

    /// Validates a manual `server.jar` path against the exact project target and records it
    /// only after the internal `version.json`, DataVersion, Java major, server entry point and
    /// hash have been checked.
    pub fn select_manual(
        &self,
        path: &Path,
        target: &ServerTarget,
    ) -> Result<ServerJarReport, ServerError> {
        let metadata = fs::symlink_metadata(path)
            .map_err(|error| ServerError::Invalid(format!("{}: {error}", path.display())))?;
        if metadata.file_type().is_symlink() {
            return Err(ServerError::Invalid(format!(
                "{} is a symlink or junction; choose the real file",
                path.display()
            )));
        }
        let canonical = fs::canonicalize(path)
            .map_err(|error| ServerError::Invalid(format!("{}: {error}", path.display())))?;
        let info = inspect_server_jar(&canonical)?;
        info.validate_for(target)?;
        self.inner.settings.save_selected(SavedServerJar {
            minecraft_version: info.version.clone(),
            path: canonical.display().to_string(),
            sha1: info.sha1.clone(),
            size: info.size,
            data_version: info.data_version,
        })?;
        let official = self.official_sha1_for(&canonical);
        Ok(self.report_for(&canonical, None, official.as_deref(), target))
    }

    pub fn clear_selection(&self, version: &VersionId) -> Result<(), ServerError> {
        self.inner.settings.clear_selection(version)
    }

    pub fn has_selection(&self, version: &VersionId) -> bool {
        self.inner.settings.selected(version).is_some()
    }

    pub fn release(&self, target: &ServerTarget) -> Result<ServerReleaseInfo, ServerError> {
        self.inner.mojang.release(target)
    }

    pub fn managed_list(&self) -> Result<ManagedServerCacheReport, ServerError> {
        let (servers, total_bytes, problems) = self.inner.cache.list()?;
        Ok(ManagedServerCacheReport {
            root: self.inner.cache.root().display().to_string(),
            servers,
            total_bytes,
            problems,
        })
    }

    /// Downloads, verifies, inspects and publishes exactly the requested release. A cached
    /// JAR for the same version is reused only after the same internal checks.
    pub fn install(
        &self,
        target: &ServerTarget,
        cancel: &AtomicBool,
        progress: &dyn Fn(ServerProgress),
    ) -> Result<InstalledServerJar, ServerError> {
        let _guard =
            self.inner.install_lock.try_lock().map_err(|_| {
                ServerError::Busy("another server JAR install is running".to_owned())
            })?;
        if cancel.load(Ordering::Relaxed) {
            return Err(ServerError::Cancelled);
        }
        progress(ServerProgress {
            phase: "metadata".to_owned(),
            downloaded: 0,
            total: 0,
        });
        let release = self.inner.mojang.release(target)?;
        if release.minecraft_version != target.minecraft_version.as_str() {
            return Err(ServerError::Invalid(format!(
                "Mojang now offers Minecraft {}, not {}; refresh the download preview",
                release.minecraft_version, target.minecraft_version
            )));
        }
        if release.expected_data_version != target.data_version
            || release.java_major != target.java_major
        {
            return Err(ServerError::Invalid(format!(
                "Mojang metadata for Minecraft {} no longer matches DataVersion {} / Java {}; refresh the download preview",
                target.minecraft_version, target.data_version, target.java_major
            )));
        }

        if let Some(existing) = self
            .inner
            .cache
            .list()?
            .0
            .into_iter()
            .find(|server| server.version == target.minecraft_version.as_str())
        {
            if existing.sha1.eq_ignore_ascii_case(&release.sha1) {
                let info = inspect_server_jar(Path::new(&existing.jar_path))?;
                info.validate_for(target)?;
                if info.sha1.eq_ignore_ascii_case(&release.sha1) {
                    self.inner.settings.save_selected(SavedServerJar {
                        minecraft_version: info.version.clone(),
                        path: existing.jar_path.clone(),
                        sha1: info.sha1.clone(),
                        size: info.size,
                        data_version: info.data_version,
                    })?;
                    let report = self.report_for(
                        Path::new(&existing.jar_path),
                        None,
                        Some(&release.sha1),
                        target,
                    );
                    return Ok(InstalledServerJar {
                        server: existing,
                        report,
                        selected: true,
                    });
                }
            }
            return Err(ServerError::Invalid(format!(
                "a managed server JAR for {} already exists with a different SHA-1",
                release.minecraft_version
            )));
        }

        let root = self.inner.cache.root().to_path_buf();
        fs::create_dir_all(&root)
            .map_err(|error| ServerError::Io(format!("{}: {error}", root.display())))?;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let archive = root.join(format!(".download-{}-{stamp}.jar", std::process::id()));
        let staging = root.join(format!(".staging-{}-{stamp}", std::process::id()));
        let result = (|| -> Result<ManagedServerJar, ServerError> {
            self.inner
                .mojang
                .download(&release, &archive, cancel, progress)?;
            if cancel.load(Ordering::Relaxed) {
                return Err(ServerError::Cancelled);
            }
            progress(ServerProgress {
                phase: "inspecting".to_owned(),
                downloaded: release.size,
                total: release.size,
            });
            let info = inspect_server_jar(&archive)?;
            info.validate_for(target)?;
            if !info.sha1.eq_ignore_ascii_case(&release.sha1) {
                return Err(ServerError::Checksum(format!(
                    "server JAR SHA-1 changed between download and inspection: expected {}, found {}",
                    release.sha1, info.sha1
                )));
            }
            if info.size != release.size {
                return Err(ServerError::Checksum(format!(
                    "server JAR size changed between download and inspection: expected {}, found {}",
                    release.size, info.size
                )));
            }
            fs::create_dir_all(&staging)
                .map_err(|error| ServerError::Io(format!("{}: {error}", staging.display())))?;
            let staged_jar = staging.join("server.jar");
            fs::rename(&archive, &staged_jar).map_err(|error| {
                ServerError::Io(format!(
                    "cannot move the verified server JAR into staging: {error}"
                ))
            })?;
            if cancel.load(Ordering::Relaxed) {
                return Err(ServerError::Cancelled);
            }
            let manifest = ServerManifest {
                version: release.minecraft_version.clone(),
                data_version: info.data_version,
                java_major: info.java_major.unwrap_or(release.java_major),
                sha1: info.sha1.clone(),
                size: info.size,
                release_time: release.release_time.clone(),
                source_url: release.download_url.clone(),
                version_json_sha1: release.version_json_sha1.clone(),
            };
            self.inner.cache.publish(staging.clone(), manifest)
        })();
        remove_file_if_exists(&archive);
        if result.is_err() {
            let _ = fs::remove_dir_all(&staging);
        }
        let server = result?;
        let saved = SavedServerJar {
            minecraft_version: server.version.clone(),
            path: server.jar_path.clone(),
            sha1: server.sha1.clone(),
            size: server.size,
            data_version: server.data_version,
        };
        self.inner.settings.save_selected(saved)?;
        let report = self.report_for(
            Path::new(&server.jar_path),
            None,
            Some(&release.sha1),
            target,
        );
        progress(ServerProgress {
            phase: "complete".to_owned(),
            downloaded: release.size,
            total: release.size,
        });
        Ok(InstalledServerJar {
            server,
            report,
            selected: true,
        })
    }

    /// Removes one FuryBuilder-owned server JAR by Minecraft version. A manual selection
    /// outside the cache is never removed; only a stale pointer is cleared.
    pub fn remove_managed(&self, version: &str) -> Result<ManagedServerCacheReport, ServerError> {
        let _guard = self
            .inner
            .install_lock
            .try_lock()
            .map_err(|_| ServerError::Busy("a server JAR install is running".to_owned()))?;
        let saved = self.inner.settings.selections();
        let removed = self.inner.cache.remove_version(version)?;
        if removed {
            for selection in saved {
                if !Path::new(&selection.path).exists() {
                    if let Ok(version) = selection.minecraft_version.parse::<VersionId>() {
                        self.inner.settings.clear_selection(&version)?;
                    }
                }
            }
        }
        self.managed_list()
    }

    /// Deletes only FuryBuilder-owned server directories and temporary entries. A manual
    /// selection outside the cache is never removed; only a stale pointer is cleared.
    pub fn clear_managed(&self) -> Result<ManagedServerCacheReport, ServerError> {
        let _guard = self
            .inner
            .install_lock
            .try_lock()
            .map_err(|_| ServerError::Busy("a server JAR install is running".to_owned()))?;
        let saved = self.inner.settings.selections();
        let managed_paths: Vec<String> = saved
            .iter()
            .filter(|selection| self.inner.cache.contains(Path::new(&selection.path)))
            .map(|selection| selection.minecraft_version.clone())
            .collect();
        self.inner.cache.clear()?;
        for selection in saved {
            if managed_paths.contains(&selection.minecraft_version)
                || !Path::new(&selection.path).exists()
            {
                if let Ok(version) = selection.minecraft_version.parse::<VersionId>() {
                    self.inner.settings.clear_selection(&version)?;
                }
            }
        }
        self.managed_list()
    }

    /// Records an explicit EULA acceptance for the exact selected server JAR.
    /// Passing `false` is rejected; this method never accepts the EULA implicitly.
    pub fn accept_eula(
        &self,
        accepted: bool,
        target: &ServerTarget,
    ) -> Result<SavedEula, ServerError> {
        if !accepted {
            return Err(ServerError::EulaRequired(
                "the Minecraft EULA must be accepted explicitly by the user".to_owned(),
            ));
        }
        let saved = self
            .inner
            .settings
            .selected(&target.minecraft_version)
            .ok_or_else(|| {
                ServerError::Invalid(format!(
                    "choose the Minecraft {} server JAR first",
                    target.minecraft_version
                ))
            })?;
        let info = inspect_server_jar(Path::new(&saved.path))?;
        info.validate_for(target)?;
        if !info.sha1.eq_ignore_ascii_case(&saved.sha1) || info.size != saved.size {
            return Err(ServerError::Invalid(
                "the selected server JAR changed on disk; choose it again before accepting the EULA"
                    .to_owned(),
            ));
        }
        let eula = SavedEula {
            accepted_at: now_secs(),
            minecraft_version: info.version,
            jar_sha1: info.sha1,
            eula_url: EULA_URL.to_owned(),
        };
        self.inner.settings.save_eula(eula.clone())?;
        Ok(eula)
    }

    pub fn clear_eula(&self) -> Result<(), ServerError> {
        let target = ServerTarget::baseline();
        match self.inner.settings.selected(&target.minecraft_version) {
            Some(saved) => self
                .inner
                .settings
                .clear_eula(&saved.minecraft_version, &saved.sha1),
            None => self.inner.settings.clear_eulas(),
        }
    }

    /// Returns the explicit acceptance for exactly this target's selected JAR, if any.
    pub fn eula_for(&self, target: &ServerTarget) -> Option<SavedEula> {
        let saved = self.inner.settings.selected(&target.minecraft_version)?;
        self.inner
            .settings
            .eula_for(&saved.minecraft_version, &saved.sha1)
    }

    /// Starts exactly one isolated generation run. The caller supplies a Java executable
    /// that was selected in the runtime manager; it is probed again here before use.
    pub fn start_generation(
        &self,
        java: &Path,
        request: GenerationRequest,
    ) -> Result<GenerationSnapshot, ServerError> {
        let _start = self
            .inner
            .generation_start_lock
            .try_lock()
            .map_err(|_| ServerError::Busy("another generation run is starting".to_owned()))?;
        {
            let state = self
                .inner
                .generation
                .lock()
                .map_err(|_| ServerError::Busy("generation state lock poisoned".to_owned()))?;
            if state.active.is_some() {
                return Err(ServerError::Busy(
                    "an isolated generation run is already active".to_owned(),
                ));
            }
        }
        generation::validate_request(&request)?;
        let datapack_paths: Vec<String> = request
            .datapacks
            .iter()
            .map(|datapack| datapack.path.clone())
            .collect();
        let datapacks = datapack::validate_selection(&datapack_paths)?;
        let consent_at = now_secs();
        let target = ServerTarget::baseline();
        let saved = self
            .inner
            .settings
            .selected(&target.minecraft_version)
            .ok_or_else(|| {
                ServerError::Invalid(format!(
                    "choose the Minecraft {} server JAR first",
                    target_minecraft_version()
                ))
            })?;
        let server_path = Path::new(&saved.path);
        let info = inspect_server_jar(server_path)?;
        info.validate_for(&target)?;
        if !info.sha1.eq_ignore_ascii_case(&saved.sha1) || info.size != saved.size {
            return Err(ServerError::Invalid(
                "the selected server JAR changed on disk; choose it again to re-verify".to_owned(),
            ));
        }
        let eula = self
            .inner
            .settings
            .eula_for(&info.version, &info.sha1)
            .ok_or_else(|| {
                ServerError::EulaRequired(format!(
                    "read and accept {EULA_URL} for the Minecraft {} server JAR with SHA-1 {} before generation",
                    info.version, info.sha1
                ))
            })?;
        let probe = probe_java(java).map_err(|error| ServerError::UnsupportedJava {
            target: target_minecraft_version(),
            detail: error.to_string(),
        })?;
        let (status, warnings) = probe.classify();
        if !matches!(status, RuntimeStatus::Compatible | RuntimeStatus::Newer) {
            return Err(ServerError::UnsupportedJava {
                target: target_minecraft_version(),
                detail: format!(
                    "Java {} is not usable for the required Java {}: {}",
                    probe
                        .version
                        .clone()
                        .unwrap_or_else(|| "unknown".to_owned()),
                    target_java_major(),
                    warnings.join("; ")
                ),
            });
        }
        let java_version = probe.version.clone().unwrap_or_default();
        let plan = generation::prepare(
            &self.inner.runs_root,
            generation::GenerationRuntime {
                java_exe: java,
                java_version: &java_version,
                server_jar: server_path,
                server: &info,
            },
            &request,
            generation::RunApproval {
                eula: &eula,
                consent_at,
            },
            &datapacks,
        )?;
        let snapshot = generation::initial_snapshot(&plan);
        let run_id = plan.run_id.clone();
        let shared = Arc::new(Mutex::new(snapshot.clone()));
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut state = self
                .inner
                .generation
                .lock()
                .map_err(|_| ServerError::Busy("generation state lock poisoned".to_owned()))?;
            state.active = Some(ActiveGeneration {
                run_id: run_id.clone(),
                cancel: Arc::clone(&cancel),
                snapshot: Arc::clone(&shared),
            });
        }
        let manager = self.clone();
        std::thread::spawn(move || {
            let report = generation::execute(plan, cancel, Arc::clone(&shared));
            if let Ok(mut state) = manager.inner.generation.lock() {
                let is_current = state
                    .active
                    .as_ref()
                    .map(|active| active.run_id == report.snapshot.run_id)
                    .unwrap_or(false);
                if is_current {
                    state.active = None;
                    state.last = Some(report);
                }
            }
        });
        Ok(snapshot)
    }

    pub fn generation_status(&self) -> GenerationStatusReport {
        let state = self.inner.generation.lock().ok();
        let active = state.as_ref().and_then(|state| {
            state
                .active
                .as_ref()
                .and_then(|active| active.snapshot.lock().ok().map(|snapshot| snapshot.clone()))
        });
        let last = state.as_ref().and_then(|state| state.last.clone());
        GenerationStatusReport { active, last }
    }

    pub fn cancel_generation(&self) -> bool {
        let Ok(state) = self.inner.generation.lock() else {
            return false;
        };
        let Some(active) = state.active.as_ref() else {
            return false;
        };
        active.cancel.store(true, Ordering::SeqCst);
        true
    }

    pub fn generation_runs(&self) -> GenerationRunsReport {
        generation::scan_runs(&self.inner.runs_root)
    }

    /// Resolves a completed generation run into an importable world. The run must still be
    /// manifest-owned and have a completed terminal report; the returned path is read-only
    /// input for project import.
    pub fn generated_world(&self, run_id: &str) -> Result<GeneratedWorld, ServerError> {
        generation::load_completed_world(&self.inner.runs_root, run_id)
    }

    pub fn clear_generation_runs(&self) -> Result<GenerationRunsReport, ServerError> {
        let _start = self
            .inner
            .generation_start_lock
            .try_lock()
            .map_err(|_| ServerError::Busy("a generation run is starting".to_owned()))?;
        {
            let mut state = self
                .inner
                .generation
                .lock()
                .map_err(|_| ServerError::Busy("generation state lock poisoned".to_owned()))?;
            if state.active.is_some() {
                return Err(ServerError::Busy(
                    "cancel the active generation before clearing its runs".to_owned(),
                ));
            }
            state.last = None;
        }
        generation::clear_runs(&self.inner.runs_root)
    }

    fn official_sha1_for(&self, path: &Path) -> Option<String> {
        let canonical = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        self.inner.cache.list().ok().and_then(|(servers, _, _)| {
            servers
                .into_iter()
                .find(|server| {
                    fs::canonicalize(&server.jar_path)
                        .map(|candidate| candidate == canonical)
                        .unwrap_or(false)
                })
                .map(|server| server.sha1)
        })
    }

    fn report_for(
        &self,
        path: &Path,
        expected: Option<&SavedServerJar>,
        official_sha1: Option<&str>,
        target: &ServerTarget,
    ) -> ServerJarReport {
        let (source, managed) = source_for(&self.inner.cache, path);
        let mut report = ServerJarReport {
            path: path.display().to_string(),
            source,
            managed,
            ready: false,
            version: None,
            data_version: None,
            java_major: None,
            protocol_version: None,
            resource_pack_format: None,
            sha1: None,
            size: None,
            matches_official: None,
            warnings: Vec::new(),
            error: None,
        };
        let info = match inspect_server_jar(path) {
            Ok(info) => info,
            Err(error) => {
                report.error = Some(error.to_string());
                return report;
            }
        };
        report.version = Some(info.version.clone());
        report.data_version = Some(info.data_version);
        report.java_major = info.java_major;
        report.protocol_version = info.protocol_version;
        report.resource_pack_format = info.resource_pack_format;
        report.sha1 = Some(info.sha1.clone());
        report.size = Some(info.size);
        if let Some(expected) = expected {
            if expected.size != info.size || !expected.sha1.eq_ignore_ascii_case(&info.sha1) {
                report.error = Some(
                    "the selected server JAR changed on disk; choose it again to re-verify"
                        .to_owned(),
                );
                return report;
            }
        }
        if let Some(official) = official_sha1 {
            report.matches_official = Some(official.eq_ignore_ascii_case(&info.sha1));
        }
        if source == ServerSource::Manual {
            report.warnings.push(
                "manual JAR: exact version and DataVersion are verified from its version.json; Mojang metadata was not consulted"
                    .to_owned(),
            );
        }
        if let Err(error) = info.validate_for(target) {
            report.warnings.push(error.to_string());
            return report;
        }
        report.ready = true;
        report
    }
}

fn manual_hint(target: &ServerTarget) -> String {
    format!(
        "Choose the official Minecraft {} server.jar, or download it explicitly from Mojang's version manifest. Client JARs and other versions are rejected.",
        target.minecraft_version
    )
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_comes_from_the_version_profile() {
        assert_eq!(target_minecraft_version(), "1.21.4");
        assert_eq!(target_data_version(), 4189);
        assert_eq!(target_java_major(), 21);
        let target = ServerTarget::baseline();
        assert_eq!(target.minecraft_version.as_str(), "1.21.4");
        assert!(!manual_hint(&target).is_empty());
    }
}
