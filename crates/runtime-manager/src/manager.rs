use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::discovery;
use crate::error::RuntimeError;
use crate::managed::{ManagedRuntimeCache, RuntimeManifest};
use crate::probe::{probe_compatible, probe_java, report_for};
use crate::settings::RuntimeSettingsStore;
use crate::temurin::{release_id, remove_file_if_exists, AdoptiumClient};
use crate::types::{
    InstalledRuntime, JavaRuntimeReport, ManagedCacheReport, RuntimeDiscoveryReport,
    RuntimeProgress, RuntimeReleaseInfo, RuntimeSource,
};

struct RuntimeManagerInner {
    settings: RuntimeSettingsStore,
    cache: ManagedRuntimeCache,
    adoptium: AdoptiumClient,
    install_lock: Mutex<()>,
}

/// Application-facing facade. It owns only settings, a managed cache and an HTTPS client;
/// it never installs Java globally and never downloads without an explicit call.
#[derive(Clone)]
pub struct RuntimeManager {
    inner: Arc<RuntimeManagerInner>,
}

impl RuntimeManager {
    pub fn new(settings_path: impl Into<PathBuf>, managed_root: impl Into<PathBuf>) -> Self {
        Self {
            inner: Arc::new(RuntimeManagerInner {
                settings: RuntimeSettingsStore::new(settings_path),
                cache: ManagedRuntimeCache::new(managed_root),
                adoptium: AdoptiumClient::new(),
                install_lock: Mutex::new(()),
            }),
        }
    }

    pub fn settings_path(&self) -> &Path {
        self.inner.settings.path()
    }

    pub fn managed_root(&self) -> &Path {
        self.inner.cache.root()
    }

    pub fn discover(&self) -> RuntimeDiscoveryReport {
        let mut report =
            discovery::discover(&self.inner.cache, self.inner.settings.selected().as_deref());
        if let Some(warning) = self.inner.settings.load_warning() {
            report.problems.push(warning.to_owned());
        }
        report
    }

    pub fn saved(&self) -> Option<JavaRuntimeReport> {
        let path = self.inner.settings.selected()?;
        let managed = self.inner.cache.contains(&path);
        Some(report_for(&path, RuntimeSource::Manual, managed))
    }

    /// Validates and persists a manually chosen `java` executable. Incompatible runtimes are
    /// rejected rather than silently stored for a later Vanilla World failure.
    pub fn select(&self, path: &Path) -> Result<JavaRuntimeReport, RuntimeError> {
        let canonical = std::fs::canonicalize(path)
            .map_err(|error| RuntimeError::Invalid(format!("{}: {error}", path.display())))?;
        if !canonical.is_file() {
            return Err(RuntimeError::Invalid(format!(
                "{} is not a Java executable",
                canonical.display()
            )));
        }
        let managed = self.inner.cache.contains(&canonical);
        let report = probe_compatible(
            &canonical,
            if managed {
                RuntimeSource::Managed
            } else {
                RuntimeSource::Manual
            },
            managed,
        )?;
        self.inner.settings.save_selected(&canonical)?;
        Ok(report)
    }

    pub fn clear_selection(&self) -> Result<(), RuntimeError> {
        self.inner.settings.clear()
    }

    pub fn latest_temurin(&self) -> Result<RuntimeReleaseInfo, RuntimeError> {
        self.inner.adoptium.latest_jre()
    }

    pub fn managed_list(&self) -> Result<ManagedCacheReport, RuntimeError> {
        self.inner.cache.list()
    }

    /// Downloads, checksum-verifies, safely extracts, probes and selects the exact Adoptium
    /// release the user previewed. The expected version prevents a stale UI request from
    /// silently installing a different release if Adoptium publishes a new latest build.
    pub fn install_temurin(
        &self,
        expected_version: &str,
        progress: &dyn Fn(RuntimeProgress),
    ) -> Result<InstalledRuntime, RuntimeError> {
        let _guard = self.inner.install_lock.try_lock().map_err(|_| {
            RuntimeError::Busy("another managed Java install is running".to_owned())
        })?;
        progress(RuntimeProgress {
            phase: "metadata".to_owned(),
            downloaded: 0,
            total: 0,
        });
        let release = self.inner.adoptium.latest_jre()?;
        if release.version != expected_version {
            return Err(RuntimeError::Invalid(format!(
                "Adoptium now offers {}, not {expected_version}; refresh the download preview",
                release.version
            )));
        }
        let id = release_id(&release);

        let existing = self
            .inner
            .cache
            .list()?
            .runtimes
            .into_iter()
            .find(|runtime| runtime.id == id);
        if let Some(runtime) = existing {
            if runtime.sha256.eq_ignore_ascii_case(&release.sha256) {
                let report =
                    probe_compatible(Path::new(&runtime.java_path), RuntimeSource::Managed, true)?;
                self.inner
                    .settings
                    .save_selected(Path::new(&runtime.java_path))?;
                return Ok(InstalledRuntime {
                    runtime,
                    report,
                    selected: true,
                });
            }
            return Err(RuntimeError::Invalid(format!(
                "managed runtime {id} already exists with a different checksum"
            )));
        }

        std::fs::create_dir_all(self.inner.cache.root()).map_err(|error| {
            RuntimeError::Io(format!(
                "cannot create managed runtime cache {}: {error}",
                self.inner.cache.root().display()
            ))
        })?;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let archive = self
            .inner
            .cache
            .root()
            .join(format!(".download-{}-{stamp}.zip", std::process::id()));
        let manifest = RuntimeManifest {
            id: id.clone(),
            version: release.version.clone(),
            release_name: release.release_name.clone(),
            sha256: release.sha256.clone(),
            source_url: release.download_url.clone(),
            vendor: release.vendor.clone(),
            os: release.os.clone(),
            arch: release.arch.clone(),
        };
        let result = self.install_archive(&release, &archive, &manifest, progress);
        remove_file_if_exists(&archive);
        let (runtime, report) = result?;
        self.inner
            .settings
            .save_selected(Path::new(&runtime.java_path))?;
        Ok(InstalledRuntime {
            runtime,
            report,
            selected: true,
        })
    }

    fn install_archive(
        &self,
        release: &RuntimeReleaseInfo,
        archive: &Path,
        manifest: &RuntimeManifest,
        progress: &dyn Fn(RuntimeProgress),
    ) -> Result<(crate::types::ManagedRuntime, JavaRuntimeReport), RuntimeError> {
        self.inner.adoptium.download(release, archive, progress)?;
        let staged = self.inner.cache.stage(archive, manifest, progress)?;
        progress(RuntimeProgress {
            phase: "probing".to_owned(),
            downloaded: 0,
            total: 0,
        });
        let probe = match probe_java(&ManagedRuntimeCache::probe_staged(&staged)) {
            Ok(probe) => probe,
            Err(error) => {
                self.inner.cache.discard(staged);
                return Err(error);
            }
        };
        let expected_java = crate::target::target_java_major();
        if probe.major != Some(expected_java) {
            self.inner.cache.discard(staged);
            return Err(RuntimeError::Archive(format!(
                "downloaded Java reports {:?}, expected Java {expected_java}",
                probe.version
            )));
        }
        let runtime = self.inner.cache.commit(staged, manifest.clone(), &probe)?;
        let report = probe_compatible(Path::new(&runtime.java_path), RuntimeSource::Managed, true)?;
        progress(RuntimeProgress {
            phase: "complete".to_owned(),
            downloaded: release.size,
            total: release.size,
        });
        Ok((runtime, report))
    }

    /// Deletes FuryBuilder-owned runtimes only. A selected managed runtime is unselected
    /// because its executable no longer exists; user-selected external Java is untouched.
    pub fn clear_managed(&self) -> Result<ManagedCacheReport, RuntimeError> {
        let _guard = self
            .inner
            .install_lock
            .try_lock()
            .map_err(|_| RuntimeError::Busy("a managed Java install is running".to_owned()))?;
        let selected = self.inner.settings.selected();
        let report = self.inner.cache.clear()?;
        if let Some(selected) = selected {
            if self.inner.cache.contains(&selected) || !selected.exists() {
                let still_managed = report
                    .runtimes
                    .iter()
                    .any(|runtime| Path::new(&runtime.java_path) == selected);
                if !still_managed {
                    self.inner.settings.clear()?;
                }
            }
        }
        Ok(report)
    }
}
