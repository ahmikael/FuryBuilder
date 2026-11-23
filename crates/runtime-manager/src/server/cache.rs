use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::ServerError;
use crate::target::{target_data_version, target_java_major, target_minecraft_version};

use super::types::{ManagedServerJar, ServerSource, SERVER_MANIFEST_SCHEMA_VERSION};

pub(crate) const MANIFEST_NAME: &str = ".furybuilder-server.json";
const MAX_ENTRIES: usize = 10_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredServerManifest {
    schema_version: u32,
    version: String,
    data_version: i32,
    java_major: u32,
    sha1: String,
    size: u64,
    release_time: String,
    source_url: String,
    installed_at: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct ServerManifest {
    pub version: String,
    pub data_version: i32,
    pub java_major: u32,
    pub sha1: String,
    pub size: u64,
    pub release_time: String,
    pub source_url: String,
}

pub(crate) struct ServerCache {
    root: PathBuf,
}

impl ServerCache {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn contains(&self, path: &Path) -> bool {
        let Ok(root) = fs::canonicalize(&self.root) else {
            return false;
        };
        let candidate = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        candidate.starts_with(root)
    }

    pub fn list(&self) -> Result<(Vec<ManagedServerJar>, u64, Vec<String>), ServerError> {
        let mut runtimes = Vec::new();
        let mut total_bytes = 0u64;
        let mut problems = Vec::new();
        if !self.root.exists() {
            return Ok((runtimes, total_bytes, problems));
        }
        let entries = fs::read_dir(&self.root)
            .map_err(|error| ServerError::Io(format!("{}: {error}", self.root.display())))?;
        for (index, entry) in entries.flatten().enumerate() {
            if index >= MAX_ENTRIES {
                problems.push(format!(
                    "{} contains more than {MAX_ENTRIES} entries; the rest was ignored",
                    self.root.display()
                ));
                break;
            }
            let path = entry.path();
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) => {
                    problems.push(format!("cannot inspect {}: {error}", path.display()));
                    continue;
                }
            };
            if metadata.file_type().is_symlink() {
                problems.push(format!("skipped symlink {}", path.display()));
                continue;
            }
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if name.starts_with(".staging-") || name.starts_with(".download-") {
                continue;
            }
            if !metadata.is_dir() {
                continue;
            }
            let manifest_path = path.join(MANIFEST_NAME);
            if !manifest_path.is_file() {
                problems.push(format!("{} has no FuryBuilder manifest", path.display()));
                continue;
            }
            let manifest = match read_manifest(&manifest_path) {
                Ok(manifest) => manifest,
                Err(error) => {
                    problems.push(error);
                    continue;
                }
            };
            if manifest.version != target_minecraft_version()
                || manifest.data_version != target_data_version()
                || manifest.java_major != target_java_major()
            {
                problems.push(format!(
                    "{} declares an unsupported server version/data version",
                    path.display()
                ));
                continue;
            }
            let jar_path = path.join("server.jar");
            let jar_metadata = match fs::symlink_metadata(&jar_path) {
                Ok(metadata) => metadata,
                Err(error) => {
                    problems.push(format!("{}: {error}", jar_path.display()));
                    continue;
                }
            };
            if jar_metadata.file_type().is_symlink() || !jar_metadata.is_file() {
                problems.push(format!("{} is not a regular file", jar_path.display()));
                continue;
            }
            if jar_metadata.len() != manifest.size {
                problems.push(format!(
                    "{} changed on disk (expected {} bytes, found {})",
                    jar_path.display(),
                    manifest.size,
                    jar_metadata.len()
                ));
                continue;
            }
            let bytes = jar_metadata
                .len()
                .saturating_add(manifest_path.metadata().map(|meta| meta.len()).unwrap_or(0));
            total_bytes = total_bytes.saturating_add(bytes);
            runtimes.push(ManagedServerJar {
                version: manifest.version.clone(),
                path: path.display().to_string(),
                jar_path: jar_path.display().to_string(),
                sha1: manifest.sha1.clone(),
                size: jar_metadata.len(),
                data_version: manifest.data_version,
                java_major: manifest.java_major,
                release_time: manifest.release_time.clone(),
                source_url: manifest.source_url.clone(),
                installed_at: manifest.installed_at,
            });
        }
        runtimes.sort_by(|left, right| left.version.cmp(&right.version));
        Ok((runtimes, total_bytes, problems))
    }

    /// Publishes a fully verified `server.jar` (already inside `staging`) as the managed
    /// target runtime. The directory becomes visible with one rename; the JAR bytes are never
    /// modified after verification.
    pub fn publish(
        &self,
        staging: PathBuf,
        manifest: ServerManifest,
    ) -> Result<ManagedServerJar, ServerError> {
        let destination = self
            .root
            .join(format!("minecraft-{}", target_minecraft_version()));
        if destination.exists() {
            if let Some(existing) = self
                .list()?
                .0
                .into_iter()
                .find(|runtime| runtime.version == manifest.version)
            {
                if existing.sha1.eq_ignore_ascii_case(&manifest.sha1) {
                    let _ = fs::remove_dir_all(&staging);
                    return Ok(existing);
                }
                return Err(ServerError::Invalid(format!(
                    "{} is already installed with a different SHA-1",
                    destination.display()
                )));
            }
            return Err(ServerError::Invalid(format!(
                "{} already exists and is not a valid managed server JAR",
                destination.display()
            )));
        }
        let stored = StoredServerManifest {
            schema_version: SERVER_MANIFEST_SCHEMA_VERSION,
            version: manifest.version.clone(),
            data_version: manifest.data_version,
            java_major: manifest.java_major,
            sha1: manifest.sha1.to_ascii_lowercase(),
            size: manifest.size,
            release_time: manifest.release_time.clone(),
            source_url: manifest.source_url.clone(),
            installed_at: now_secs(),
        };
        let bytes = serde_json::to_vec_pretty(&stored)
            .map_err(|error| ServerError::Io(error.to_string()))?;
        fs::write(staging.join(MANIFEST_NAME), bytes)
            .map_err(|error| ServerError::Io(format!("cannot write server manifest: {error}")))?;
        if let Err(error) = fs::rename(&staging, &destination) {
            let _ = fs::remove_dir_all(&staging);
            return Err(ServerError::Io(format!(
                "cannot publish managed server JAR {}: {error}",
                destination.display()
            )));
        }
        let jar_path = destination.join("server.jar");
        let size = fs::symlink_metadata(&jar_path)
            .map(|metadata| metadata.len())
            .unwrap_or(stored.size);
        Ok(ManagedServerJar {
            version: stored.version,
            path: destination.display().to_string(),
            jar_path: jar_path.display().to_string(),
            sha1: stored.sha1,
            size,
            data_version: stored.data_version,
            java_major: stored.java_major,
            release_time: stored.release_time,
            source_url: stored.source_url,
            installed_at: stored.installed_at,
        })
    }

    /// Removes exactly one FuryBuilder-owned server directory after checking its manifest.
    pub fn remove_version(&self, version: &str) -> Result<bool, ServerError> {
        if version.is_empty()
            || !version.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_')
            })
        {
            return Err(ServerError::Invalid(format!(
                "invalid server version directory `{version}`"
            )));
        }
        let directory = self.root.join(format!("minecraft-{version}"));
        if !directory.exists() {
            return Ok(false);
        }
        let manifest_path = directory.join(MANIFEST_NAME);
        if !manifest_path.is_file() {
            return Err(ServerError::Invalid(format!(
                "{} is not a FuryBuilder-managed server directory",
                directory.display()
            )));
        }
        let manifest = read_manifest(&manifest_path).map_err(ServerError::Invalid)?;
        if manifest.version != version {
            return Err(ServerError::Invalid(format!(
                "{} declares Minecraft {}, not {version}",
                directory.display(),
                manifest.version
            )));
        }
        let root = fs::canonicalize(&self.root)
            .map_err(|error| ServerError::Io(format!("{}: {error}", self.root.display())))?;
        let target = fs::canonicalize(&directory)
            .map_err(|error| ServerError::Io(format!("{}: {error}", directory.display())))?;
        if target == root || !target.starts_with(&root) {
            return Err(ServerError::Invalid(format!(
                "{} is not inside the managed server cache",
                directory.display()
            )));
        }
        fs::remove_dir_all(&target).map_err(|error| {
            ServerError::Io(format!("cannot remove {}: {error}", target.display()))
        })?;
        Ok(true)
    }

    /// Removes only FuryBuilder-owned server directories (identified by our manifest) and
    /// our temporary download/staging entries. A user-selected external JAR is never touched.
    pub fn clear(&self) -> Result<(), ServerError> {
        if !self.root.exists() {
            return Ok(());
        }
        let entries = fs::read_dir(&self.root)
            .map_err(|error| ServerError::Io(format!("{}: {error}", self.root.display())))?;
        let mut removed = 0usize;
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                continue;
            };
            if metadata.file_type().is_symlink() {
                continue;
            }
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_owned();
            if name.starts_with(".staging-") || name.starts_with(".download-") {
                if metadata.is_dir() {
                    let _ = fs::remove_dir_all(&path);
                } else {
                    let _ = fs::remove_file(&path);
                }
                removed += 1;
                continue;
            }
            if metadata.is_dir() && path.join(MANIFEST_NAME).is_file() {
                fs::remove_dir_all(&path)
                    .map_err(|error| ServerError::Io(format!("{}: {error}", path.display())))?;
                removed += 1;
            }
        }
        let _ = removed;
        Ok(())
    }
}

fn read_manifest(path: &Path) -> Result<StoredServerManifest, String> {
    let bytes =
        fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let manifest: StoredServerManifest = serde_json::from_slice(&bytes)
        .map_err(|error| format!("cannot parse {}: {error}", path.display()))?;
    if manifest.schema_version != SERVER_MANIFEST_SCHEMA_VERSION {
        return Err(format!(
            "{} uses unsupported manifest schema {}",
            path.display(),
            manifest.schema_version
        ));
    }
    Ok(manifest)
}

pub(crate) fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

pub(crate) fn source_for(cache: &ServerCache, path: &Path) -> (ServerSource, bool) {
    if cache.contains(path) {
        (ServerSource::Managed, true)
    } else {
        (ServerSource::Manual, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_cache_lists_and_clears_only_manifest_directories() {
        let dir =
            std::env::temp_dir().join(format!("furybuilder-server-cache-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let cache = ServerCache::new(dir.join("servers"));
        fs::create_dir_all(&cache.root).expect("cache root");
        fs::write(cache.root.join("keep.txt"), b"not ours").expect("unrelated file");
        let staging = cache.root.join(".staging-test");
        fs::create_dir_all(&staging).expect("staging");
        fs::write(staging.join("server.jar"), vec![0u8; 32]).expect("jar");
        let manifest = ServerManifest {
            version: target_minecraft_version().to_owned(),
            data_version: target_data_version(),
            java_major: target_java_major(),
            sha1: "4707d00eb834b446575d89a61a11b5d548d8c001".to_owned(),
            size: 32,
            release_time: "2024-12-03T10:12:57+00:00".to_owned(),
            source_url: "https://piston-data.mojang.com/x".to_owned(),
        };
        let runtime = cache.publish(staging, manifest).expect("publish");
        assert!(Path::new(&runtime.jar_path).is_file());
        let (runtimes, bytes, problems) = cache.list().expect("list");
        assert_eq!(runtimes.len(), 1);
        assert_eq!(runtimes[0].version, "1.21.4");
        assert!(bytes > 0);
        assert!(problems.is_empty(), "{problems:?}");
        cache.clear().expect("clear");
        let (runtimes, _, _) = cache.list().expect("list after clear");
        assert!(runtimes.is_empty());
        assert!(cache.root.join("keep.txt").is_file());
    }
}
