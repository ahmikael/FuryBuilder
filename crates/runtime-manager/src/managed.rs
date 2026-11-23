use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::RuntimeError;
use crate::probe::JavaProbe;
use crate::types::{ManagedCacheReport, ManagedRuntime, RuntimeProgress};

const MAX_ENTRIES: usize = 20_000;
const MAX_ENTRY_BYTES: u64 = 200 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 512 * 1024 * 1024;
const MANIFEST_NAME: &str = ".furybuilder-runtime.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredManifest {
    schema_version: u32,
    id: String,
    version: String,
    release_name: String,
    sha256: String,
    source_url: String,
    vendor: String,
    os: String,
    arch: String,
    installed_at: u64,
    java_relative_path: String,
}

pub struct ManagedRuntimeCache {
    root: PathBuf,
}

#[derive(Debug)]
pub struct StagedRuntime {
    staging: PathBuf,
    install: PathBuf,
    java_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct RuntimeManifest {
    pub id: String,
    pub version: String,
    pub release_name: String,
    pub sha256: String,
    pub source_url: String,
    pub vendor: String,
    pub os: String,
    pub arch: String,
}

impl ManagedRuntimeCache {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// True when `path` is inside the managed root. Used to label/clean managed selections.
    pub fn contains(&self, path: &Path) -> bool {
        let Ok(root) = std::fs::canonicalize(&self.root) else {
            return false;
        };
        let candidate = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        candidate.starts_with(root)
    }

    pub fn list(&self) -> Result<ManagedCacheReport, RuntimeError> {
        let mut report = ManagedCacheReport {
            root: self.root.display().to_string(),
            runtimes: Vec::new(),
            total_bytes: 0,
            problems: Vec::new(),
        };
        if !self.root.exists() {
            return Ok(report);
        }
        let entries = fs::read_dir(&self.root)
            .map_err(|error| RuntimeError::Io(format!("{}: {error}", self.root.display())))?;
        for entry in entries.flatten() {
            let path = entry.path();
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) => {
                    report
                        .problems
                        .push(format!("cannot inspect {}: {error}", path.display()));
                    continue;
                }
            };
            if metadata.file_type().is_symlink() {
                report
                    .problems
                    .push(format!("skipped symlink {}", path.display()));
                continue;
            }
            if !metadata.is_dir()
                || path.file_name().and_then(|name| name.to_str()) == Some(".staging")
            {
                continue;
            }
            let manifest_path = path.join(MANIFEST_NAME);
            if !manifest_path.is_file() {
                report
                    .problems
                    .push(format!("{} has no FuryBuilder manifest", path.display()));
                continue;
            }
            let manifest = match read_manifest(&manifest_path) {
                Ok(manifest) => manifest,
                Err(error) => {
                    report.problems.push(error);
                    continue;
                }
            };
            let Some(relative) = safe_relative(&manifest.java_relative_path) else {
                report.problems.push(format!(
                    "{} contains an unsafe java path",
                    manifest_path.display()
                ));
                continue;
            };
            let java_path = path.join(relative);
            if !java_path.is_file() {
                report.problems.push(format!(
                    "{} is missing {}",
                    path.display(),
                    manifest.java_relative_path
                ));
                continue;
            }
            let bytes = directory_bytes(&path).unwrap_or(0);
            report.total_bytes = report.total_bytes.saturating_add(bytes);
            report
                .runtimes
                .push(to_managed(&path, &java_path, &manifest, bytes));
        }
        report
            .runtimes
            .sort_by(|left, right| left.id.cmp(&right.id));
        Ok(report)
    }

    /// Extracts a verified Adoptium archive into a staging directory. The caller must probe
    /// the binary and then either `commit` or `discard` the staged runtime.
    pub fn stage(
        &self,
        archive: &Path,
        manifest: &RuntimeManifest,
        cancel: &std::sync::atomic::AtomicBool,
        progress: &dyn Fn(RuntimeProgress),
    ) -> Result<StagedRuntime, RuntimeError> {
        use std::sync::atomic::Ordering;
        if cancel.load(Ordering::Relaxed) {
            return Err(RuntimeError::Cancelled);
        }
        validate_id(&manifest.id)?;
        fs::create_dir_all(&self.root)
            .map_err(|error| RuntimeError::Io(format!("{}: {error}", self.root.display())))?;
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let staging = self
            .root
            .join(format!(".staging-{}-{suffix}", std::process::id()));
        let raw = staging.join("raw");
        fs::create_dir_all(&raw)
            .map_err(|error| RuntimeError::Io(format!("{}: {error}", staging.display())))?;
        let result = extract_zip(archive, &raw, cancel, progress).and_then(|_| {
            let java_root = find_java_root(&raw)?;
            let install = staging.join("install");
            fs::rename(&java_root, &install).map_err(|error| {
                RuntimeError::Io(format!("cannot stage {}: {error}", java_root.display()))
            })?;
            let java_path = if cfg!(windows) {
                install.join("bin").join("java.exe")
            } else {
                install.join("bin").join("java")
            };
            if !java_path.is_file() {
                return Err(RuntimeError::Archive(format!(
                    "{} has no executable Java binary",
                    install.display()
                )));
            }
            progress(RuntimeProgress {
                phase: "extracted".to_owned(),
                downloaded: 0,
                total: 0,
            });
            Ok(StagedRuntime {
                staging: staging.clone(),
                install,
                java_path,
            })
        });
        if result.is_err() {
            let _ = fs::remove_dir_all(&staging);
        }
        result
    }

    pub fn probe_staged(staged: &StagedRuntime) -> std::path::PathBuf {
        staged.java_path.clone()
    }

    pub fn commit(
        &self,
        staged: StagedRuntime,
        manifest: RuntimeManifest,
        probe: &JavaProbe,
    ) -> Result<ManagedRuntime, RuntimeError> {
        let expected_java = crate::target::target_java_major();
        if probe.major != Some(expected_java) {
            self.discard(staged);
            return Err(RuntimeError::Archive(format!(
                "downloaded Java reports {:?}, expected {expected_java}",
                probe.version
            )));
        }
        let destination = self.root.join(&manifest.id);
        if destination.exists() {
            let existing = self
                .list()?
                .runtimes
                .into_iter()
                .find(|runtime| runtime.id == manifest.id);
            self.discard(staged);
            if let Some(existing) = existing {
                if existing.sha256.eq_ignore_ascii_case(&manifest.sha256) {
                    return Ok(existing);
                }
                return Err(RuntimeError::Invalid(format!(
                    "{} is already installed with different content",
                    manifest.id
                )));
            }
            return Err(RuntimeError::Invalid(format!(
                "{} already exists and is not a valid managed runtime",
                destination.display()
            )));
        }
        let stored = StoredManifest {
            schema_version: 1,
            id: manifest.id.clone(),
            version: manifest.version.clone(),
            release_name: manifest.release_name.clone(),
            sha256: manifest.sha256.to_ascii_lowercase(),
            source_url: manifest.source_url.clone(),
            vendor: manifest.vendor.clone(),
            os: manifest.os.clone(),
            arch: manifest.arch.clone(),
            installed_at: now_secs(),
            java_relative_path: if cfg!(windows) {
                "bin/java.exe".to_owned()
            } else {
                "bin/java".to_owned()
            },
        };
        let bytes = serde_json::to_vec_pretty(&stored)
            .map_err(|error| RuntimeError::Io(error.to_string()))?;
        fs::write(staged.install.join(MANIFEST_NAME), bytes)
            .map_err(|error| RuntimeError::Io(format!("cannot write runtime manifest: {error}")))?;
        if let Err(error) = fs::rename(&staged.install, &destination) {
            let _ = fs::remove_dir_all(&staged.staging);
            return Err(RuntimeError::Io(format!(
                "cannot publish managed runtime {}: {error}",
                destination.display()
            )));
        }
        let java_path = if cfg!(windows) {
            destination.join("bin").join("java.exe")
        } else {
            destination.join("bin").join("java")
        };
        let _ = fs::remove_dir_all(&staged.staging);
        let size = directory_bytes(&destination).unwrap_or(0);
        Ok(to_managed(&destination, &java_path, &stored, size))
    }

    pub fn discard(&self, staged: StagedRuntime) {
        let _ = fs::remove_dir_all(&staged.staging);
    }

    /// Removes only directories that carry a FuryBuilder runtime manifest. Nothing else in the
    /// cache root is touched; this is the UI's "clear managed Java" action.
    /// Removes exactly one FuryBuilder-owned runtime directory. The id is validated as a
    /// cache directory name, and the resolved path must stay inside the managed root.
    pub fn remove(&self, id: &str) -> Result<bool, RuntimeError> {
        validate_id(id)?;
        let directory = self.root.join(id);
        if !directory.exists() {
            return Ok(false);
        }
        let root = fs::canonicalize(&self.root).map_err(|error| {
            RuntimeError::Io(format!("cannot resolve {}: {error}", self.root.display()))
        })?;
        let target = fs::canonicalize(&directory).map_err(|error| {
            RuntimeError::Io(format!("cannot resolve {}: {error}", directory.display()))
        })?;
        if target == root || !target.starts_with(&root) {
            return Err(RuntimeError::Invalid(format!(
                "{} is not a managed runtime directory",
                directory.display()
            )));
        }
        fs::remove_dir_all(&target).map_err(|error| {
            RuntimeError::Io(format!("cannot remove {}: {error}", target.display()))
        })?;
        Ok(true)
    }

    pub fn clear(&self) -> Result<ManagedCacheReport, RuntimeError> {
        let before = self.list()?;
        for runtime in &before.runtimes {
            let path = Path::new(&runtime.path);
            if !self.contains(path) {
                continue;
            }
            let metadata = fs::symlink_metadata(path)
                .map_err(|error| RuntimeError::Io(format!("{}: {error}", path.display())))?;
            if metadata.file_type().is_symlink() {
                continue;
            }
            fs::remove_dir_all(path)
                .map_err(|error| RuntimeError::Io(format!("{}: {error}", path.display())))?;
        }
        self.list()
    }
}

fn read_manifest(path: &Path) -> Result<StoredManifest, String> {
    let bytes =
        fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let manifest: StoredManifest = serde_json::from_slice(&bytes)
        .map_err(|error| format!("cannot parse {}: {error}", path.display()))?;
    if manifest.schema_version != 1 {
        return Err(format!(
            "{} uses unsupported manifest schema {}",
            path.display(),
            manifest.schema_version
        ));
    }
    Ok(manifest)
}

fn to_managed(
    path: &Path,
    java_path: &Path,
    manifest: &StoredManifest,
    bytes: u64,
) -> ManagedRuntime {
    ManagedRuntime {
        id: manifest.id.clone(),
        path: path.display().to_string(),
        java_path: java_path.display().to_string(),
        version: manifest.version.clone(),
        release_name: manifest.release_name.clone(),
        sha256: manifest.sha256.clone(),
        source_url: manifest.source_url.clone(),
        vendor: manifest.vendor.clone(),
        os: manifest.os.clone(),
        arch: manifest.arch.clone(),
        installed_at: manifest.installed_at,
        bytes,
    }
}

fn validate_id(id: &str) -> Result<(), RuntimeError> {
    if id.is_empty()
        || id.len() > 96
        || !id.chars().all(|c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '.' | '+')
        })
    {
        return Err(RuntimeError::Invalid(format!(
            "managed runtime id is not safe: {id}"
        )));
    }
    Ok(())
}

fn safe_relative(path: &str) -> Option<PathBuf> {
    let path = Path::new(path);
    if path.is_absolute() {
        return None;
    }
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::Normal(name) => result.push(name),
            std::path::Component::CurDir => {}
            _ => return None,
        }
    }
    (!result.as_os_str().is_empty()).then_some(result)
}

fn extract_zip(
    archive: &Path,
    destination: &Path,
    cancel: &std::sync::atomic::AtomicBool,
    progress: &dyn Fn(RuntimeProgress),
) -> Result<(), RuntimeError> {
    use std::sync::atomic::Ordering;
    let file = File::open(archive)
        .map_err(|error| RuntimeError::Io(format!("{}: {error}", archive.display())))?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|error| RuntimeError::Archive(format!("{}: {error}", archive.display())))?;
    if zip.len() > MAX_ENTRIES {
        return Err(RuntimeError::Limit(format!(
            "archive contains {} entries (limit {MAX_ENTRIES})",
            zip.len()
        )));
    }
    let mut total = 0u64;
    for index in 0..zip.len() {
        if cancel.load(Ordering::Relaxed) {
            return Err(RuntimeError::Cancelled);
        }
        let mut entry = zip
            .by_index(index)
            .map_err(|error| RuntimeError::Archive(error.to_string()))?;
        let name = entry
            .name()
            .map_err(|error| RuntimeError::Archive(format!("archive entry name: {error}")))?
            .into_owned();
        if name.contains('\\') {
            return Err(RuntimeError::Archive(format!(
                "archive entry uses a backslash: {name}"
            )));
        }
        let relative = entry
            .enclosed_name()
            .ok_or_else(|| RuntimeError::Archive(format!("unsafe archive entry: {name}")))?;
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(RuntimeError::Archive(format!(
                "archive entry is a symlink: {name}"
            )));
        }
        let target = destination.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(&target)
                .map_err(|error| RuntimeError::Io(format!("{}: {error}", target.display())))?;
            continue;
        }
        let size = entry.size();
        if size > MAX_ENTRY_BYTES {
            return Err(RuntimeError::Limit(format!(
                "{name} is {size} bytes (limit {MAX_ENTRY_BYTES})"
            )));
        }
        total = total.saturating_add(size);
        if total > MAX_TOTAL_BYTES {
            return Err(RuntimeError::Limit(format!(
                "archive expands beyond {MAX_TOTAL_BYTES} bytes"
            )));
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| RuntimeError::Io(format!("{}: {error}", parent.display())))?;
        }
        let mut output = File::create(&target)
            .map_err(|error| RuntimeError::Io(format!("{}: {error}", target.display())))?;
        let mut buffer = [0u8; 64 * 1024];
        let mut written = 0u64;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(RuntimeError::Cancelled);
            }
            let read = entry
                .read(&mut buffer)
                .map_err(|error| RuntimeError::Archive(format!("{name}: {error}")))?;
            if read == 0 {
                break;
            }
            written += read as u64;
            if written > MAX_ENTRY_BYTES {
                return Err(RuntimeError::Limit(format!(
                    "{name} expands beyond {MAX_ENTRY_BYTES} bytes"
                )));
            }
            output
                .write_all(&buffer[..read])
                .map_err(|error| RuntimeError::Io(format!("{}: {error}", target.display())))?;
        }
        progress(RuntimeProgress {
            phase: "extracting".to_owned(),
            downloaded: total,
            total: 0,
        });
    }
    Ok(())
}

fn find_java_root(raw: &Path) -> Result<PathBuf, RuntimeError> {
    if java_binary(raw).is_file() {
        return Ok(raw.to_path_buf());
    }
    let entries = fs::read_dir(raw)
        .map_err(|error| RuntimeError::Io(format!("{}: {error}", raw.display())))?;
    let mut roots = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            continue;
        }
        if java_binary(&path).is_file() {
            roots.push(path);
            continue;
        }
        if let Ok(children) = fs::read_dir(&path) {
            for child in children.flatten() {
                let child = child.path();
                let Ok(metadata) = std::fs::symlink_metadata(&child) else {
                    continue;
                };
                if metadata.is_dir()
                    && !metadata.file_type().is_symlink()
                    && java_binary(&child).is_file()
                {
                    roots.push(child);
                }
            }
        }
    }
    match roots.len() {
        0 => Err(RuntimeError::Archive(
            "archive has no bin/java executable".to_owned(),
        )),
        1 => Ok(roots.remove(0)),
        _ => Err(RuntimeError::Archive(
            "archive contains more than one Java installation".to_owned(),
        )),
    }
}

fn java_binary(root: &Path) -> PathBuf {
    if cfg!(windows) {
        root.join("bin").join("java.exe")
    } else {
        root.join("bin").join("java")
    }
}

fn directory_bytes(root: &Path) -> Result<u64, RuntimeError> {
    let mut total = 0u64;
    let mut stack = vec![root.to_path_buf()];
    let mut entries = 0usize;
    while let Some(path) = stack.pop() {
        entries += 1;
        if entries > MAX_ENTRIES * 4 {
            return Err(RuntimeError::Limit(
                "managed runtime has too many files".to_owned(),
            ));
        }
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| RuntimeError::Io(format!("{}: {error}", path.display())))?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            if let Ok(children) = fs::read_dir(&path) {
                stack.extend(children.flatten().map(|entry| entry.path()));
            }
        } else {
            total = total.saturating_add(metadata.len());
        }
    }
    Ok(total)
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

/// SHA-256 of a file, used for pre-existing archive checks and tests.
pub fn sha256_file(path: &Path) -> Result<String, RuntimeError> {
    let mut file = File::open(path)
        .map_err(|error| RuntimeError::Io(format!("{}: {error}", path.display())))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| RuntimeError::Io(format!("{}: {error}", path.display())))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex(&hasher.finalize()))
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut text, "{byte:02x}");
    }
    text
}
