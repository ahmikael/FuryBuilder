//! Official Minecraft server JAR provisioning and isolated Vanilla World generation.
//!
//! The JAR is resolved from Mojang's official `version_manifest_v2` → version JSON →
//! `downloads.server`, verified by the announced SHA-1 and size, inspected for the exact
//! 1.21.4 `world_version` 4189 and published into an offline, deletable cache. A user can
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

use crate::probe::probe_java;
use crate::types::RuntimeStatus;

pub use datapack::TARGET_DATAPACK_FORMAT;
pub use jar::{inspect_server_jar, sha1_file, ServerJarInfo};
pub use mojang::VERSION_MANIFEST_URL;
pub use types::{
    DatapackInspection, DatapackSelection, GenerationDatapack, GenerationPhase, GenerationReport,
    GenerationRequest, GenerationRunInfo, GenerationRunsReport, GenerationSnapshot,
    GenerationStatusReport, GenerationStopReason, InstalledServerJar, ManagedServerCacheReport,
    ManagedServerJar, RunManifest, SavedEula, SavedServerJar, ServerDiscoveryReport,
    ServerJarReport, ServerProgress, ServerReleaseInfo, ServerSource, WorldPreset, EULA_URL,
    MAX_SERVER_JAR_BYTES, MIN_SERVER_JAR_BYTES, RUN_MANIFEST_SCHEMA_VERSION,
    SERVER_MANIFEST_SCHEMA_VERSION, SERVER_SETTINGS_SCHEMA_VERSION, TARGET_DATA_VERSION,
};

use crate::error::ServerError;
use crate::types::{TARGET_JAVA_MAJOR, TARGET_MINECRAFT_VERSION};

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
    pub fn new(settings_path: impl Into<PathBuf>, managed_root: impl Into<PathBuf>) -> Self {
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
                mojang: MojangClient::new(),
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
        let selected = self.selected();
        let eula = self.inner.settings.eula();
        let eula_matches_selected = match (
            eula.as_ref(),
            selected
                .as_ref()
                .and_then(|selected| selected.sha1.as_deref()),
        ) {
            (Some(eula), Some(sha1)) => eula.jar_sha1.eq_ignore_ascii_case(sha1),
            _ => false,
        };
        ServerDiscoveryReport {
            target_minecraft_version: TARGET_MINECRAFT_VERSION.to_owned(),
            target_data_version: types::TARGET_DATA_VERSION,
            target_java_major: TARGET_JAVA_MAJOR,
            selected,
            eula,
            eula_matches_selected,
            managed_root: self.inner.cache.root().display().to_string(),
            managed,
            managed_bytes,
            problems,
            manual_hint: manual_hint(),
        }
    }

    pub fn selected(&self) -> Option<ServerJarReport> {
        let saved = self.inner.settings.selected()?;
        let official = self.official_sha1_for(Path::new(&saved.path));
        Some(self.report_for(Path::new(&saved.path), Some(&saved), official.as_deref()))
    }

    /// Validates a manual `server.jar` path and records it only after the internal
    /// `version.json`, DataVersion, server entry point and hash have been checked.
    pub fn select_manual(&self, path: &Path) -> Result<ServerJarReport, ServerError> {
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
        self.inner.settings.save_selected(SavedServerJar {
            path: canonical.display().to_string(),
            sha1: info.sha1.clone(),
            size: info.size,
            data_version: info.data_version,
        })?;
        let official = self.official_sha1_for(&canonical);
        Ok(self.report_for(&canonical, None, official.as_deref()))
    }

    pub fn clear_selection(&self) -> Result<(), ServerError> {
        self.inner.settings.clear()
    }

    pub fn release(&self) -> Result<ServerReleaseInfo, ServerError> {
        self.inner.mojang.release()
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

    /// Downloads, verifies, inspects and publishes the exact release the user previewed.
    pub fn install(
        &self,
        expected_version: &str,
        progress: &dyn Fn(ServerProgress),
    ) -> Result<InstalledServerJar, ServerError> {
        let _guard =
            self.inner.install_lock.try_lock().map_err(|_| {
                ServerError::Busy("another server JAR install is running".to_owned())
            })?;
        progress(ServerProgress {
            phase: "metadata".to_owned(),
            downloaded: 0,
            total: 0,
        });
        let release = self.inner.mojang.release()?;
        if release.minecraft_version != expected_version {
            return Err(ServerError::Invalid(format!(
                "Mojang now offers Minecraft {}, not {expected_version}; refresh the download preview",
                release.minecraft_version
            )));
        }

        if let Some(existing) = self
            .inner
            .cache
            .list()?
            .0
            .into_iter()
            .find(|server| server.version == release.minecraft_version)
        {
            if existing.sha1.eq_ignore_ascii_case(&release.sha1) {
                let info = inspect_server_jar(Path::new(&existing.jar_path))?;
                if info.sha1.eq_ignore_ascii_case(&release.sha1) {
                    self.inner.settings.save_selected(SavedServerJar {
                        path: existing.jar_path.clone(),
                        sha1: info.sha1.clone(),
                        size: info.size,
                        data_version: info.data_version,
                    })?;
                    let report =
                        self.report_for(Path::new(&existing.jar_path), None, Some(&release.sha1));
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
            self.inner.mojang.download(&release, &archive, progress)?;
            progress(ServerProgress {
                phase: "inspecting".to_owned(),
                downloaded: release.size,
                total: release.size,
            });
            let info = inspect_server_jar(&archive)?;
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
            let manifest = ServerManifest {
                version: release.minecraft_version.clone(),
                data_version: info.data_version,
                java_major: info.java_major.unwrap_or(release.java_major),
                sha1: info.sha1.clone(),
                size: info.size,
                release_time: release.release_time.clone(),
                source_url: release.download_url.clone(),
            };
            self.inner.cache.publish(staging.clone(), manifest)
        })();
        remove_file_if_exists(&archive);
        if result.is_err() {
            let _ = fs::remove_dir_all(&staging);
        }
        let server = result?;
        let saved = SavedServerJar {
            path: server.jar_path.clone(),
            sha1: server.sha1.clone(),
            size: server.size,
            data_version: server.data_version,
        };
        self.inner.settings.save_selected(saved)?;
        let report = self.report_for(Path::new(&server.jar_path), None, Some(&release.sha1));
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

    /// Deletes only FuryBuilder-owned server directories and temporary entries. A manual
    /// selection outside the cache is never removed; only a stale pointer is cleared.
    pub fn clear_managed(&self) -> Result<ManagedServerCacheReport, ServerError> {
        let _guard = self
            .inner
            .install_lock
            .try_lock()
            .map_err(|_| ServerError::Busy("a server JAR install is running".to_owned()))?;
        let saved = self.inner.settings.selected();
        let was_managed = saved
            .as_ref()
            .map(|saved| self.inner.cache.contains(Path::new(&saved.path)))
            .unwrap_or(false);
        self.inner.cache.clear()?;
        if let Some(saved) = saved {
            if was_managed || !Path::new(&saved.path).exists() {
                self.inner.settings.clear()?;
            }
        }
        self.managed_list()
    }

    /// Records an explicit EULA acceptance for the exact selected 1.21.4 server JAR.
    /// Passing `false` is rejected; this method never accepts the EULA implicitly.
    pub fn accept_eula(&self, accepted: bool) -> Result<SavedEula, ServerError> {
        if !accepted {
            return Err(ServerError::EulaRequired(
                "the Minecraft EULA must be accepted explicitly by the user".to_owned(),
            ));
        }
        let saved = self.inner.settings.selected().ok_or_else(|| {
            ServerError::Invalid("choose the Minecraft 1.21.4 server JAR first".to_owned())
        })?;
        let info = inspect_server_jar(Path::new(&saved.path))?;
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
        self.inner.settings.clear_eula()
    }

    pub fn eula(&self) -> Option<SavedEula> {
        self.inner.settings.eula()
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
        let saved = self.inner.settings.selected().ok_or_else(|| {
            ServerError::Invalid("choose the Minecraft 1.21.4 server JAR first".to_owned())
        })?;
        let server_path = Path::new(&saved.path);
        let info = inspect_server_jar(server_path)?;
        if !info.sha1.eq_ignore_ascii_case(&saved.sha1) || info.size != saved.size {
            return Err(ServerError::Invalid(
                "the selected server JAR changed on disk; choose it again to re-verify".to_owned(),
            ));
        }
        let eula = self.inner.settings.eula().ok_or_else(|| {
            ServerError::EulaRequired(
                "read and accept https://aka.ms/MinecraftEULA before generation".to_owned(),
            )
        })?;
        if !eula.jar_sha1.eq_ignore_ascii_case(&info.sha1) {
            return Err(ServerError::EulaRequired(
                "the EULA was accepted for a different server JAR; confirm it for this file"
                    .to_owned(),
            ));
        }
        let probe = probe_java(java).map_err(|error| ServerError::UnsupportedJava {
            target: TARGET_MINECRAFT_VERSION,
            detail: error.to_string(),
        })?;
        let (status, warnings) = probe.classify();
        if status != RuntimeStatus::Compatible {
            return Err(ServerError::UnsupportedJava {
                target: TARGET_MINECRAFT_VERSION,
                detail: format!(
                    "Java {} is not the required compatible Java {TARGET_JAVA_MAJOR}: {}",
                    probe
                        .version
                        .clone()
                        .unwrap_or_else(|| "unknown".to_owned()),
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
                eula_accepted_at: eula.accepted_at,
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
        report.ready = true;
        report
    }
}

fn manual_hint() -> String {
    "Choose the official Minecraft 1.21.4 server.jar, or download it explicitly from Mojang's version manifest. Client JARs and other versions are rejected."
        .to_owned()
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
    fn target_constants_match_the_fixed_version() {
        assert_eq!(TARGET_MINECRAFT_VERSION, "1.21.4");
        assert_eq!(types::TARGET_DATA_VERSION, 4189);
        assert_eq!(TARGET_JAVA_MAJOR, 21);
        assert!(!manual_hint().is_empty());
    }
}
