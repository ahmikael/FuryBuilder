//! Official Minecraft server JAR provisioning for the future Vanilla World flow.
//!
//! The JAR is resolved from Mojang's official `version_manifest_v2` → version JSON →
//! `downloads.server`, verified by the announced SHA-1 and size, inspected for the exact
//! 1.21.4 `world_version` 4189 and published into an offline, deletable cache. A user can
//! also select an existing local JAR, which must pass the same internal-version and server
//! entry-point checks. Nothing is executed here and no EULA is accepted.

mod cache;
mod jar;
mod mojang;
mod settings;
mod types;

use cache::{source_for, ServerCache, ServerManifest};
use mojang::{remove_file_if_exists, MojangClient};
use settings::ServerSettingsStore;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

pub use jar::{inspect_server_jar, sha1_file, ServerJarInfo};
pub use mojang::VERSION_MANIFEST_URL;
pub use types::{
    InstalledServerJar, ManagedServerCacheReport, ManagedServerJar, SavedServerJar,
    ServerDiscoveryReport, ServerJarReport, ServerProgress, ServerReleaseInfo, ServerSource,
    MAX_SERVER_JAR_BYTES, MIN_SERVER_JAR_BYTES, SERVER_MANIFEST_SCHEMA_VERSION,
    SERVER_SETTINGS_SCHEMA_VERSION, TARGET_DATA_VERSION,
};

use crate::error::ServerError;
use crate::types::{TARGET_JAVA_MAJOR, TARGET_MINECRAFT_VERSION};

struct ServerManagerInner {
    settings: ServerSettingsStore,
    cache: ServerCache,
    mojang: MojangClient,
    install_lock: Mutex<()>,
}

/// Application-facing facade. It never installs anything globally, never starts the server
/// and never accepts the Minecraft EULA.
#[derive(Clone)]
pub struct ServerManager {
    inner: Arc<ServerManagerInner>,
}

impl ServerManager {
    pub fn new(settings_path: impl Into<PathBuf>, managed_root: impl Into<PathBuf>) -> Self {
        Self {
            inner: Arc::new(ServerManagerInner {
                settings: ServerSettingsStore::new(settings_path),
                cache: ServerCache::new(managed_root),
                mojang: MojangClient::new(),
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
        ServerDiscoveryReport {
            target_minecraft_version: TARGET_MINECRAFT_VERSION.to_owned(),
            target_data_version: types::TARGET_DATA_VERSION,
            target_java_major: TARGET_JAVA_MAJOR,
            selected,
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
