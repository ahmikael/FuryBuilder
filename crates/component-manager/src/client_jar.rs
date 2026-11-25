//! Managed Minecraft client JAR provisioning.
//!
//! The client JAR is the source of block states, models, textures, language files and the
//! version's own `version.json`. FuryBuilder downloads it only after an explicit request,
//! from the official version JSON, and verifies the announced SHA-1 and size before the
//! directory becomes visible in the cache.
//!
//! Cache layout:
//!
//! ```text
//! <app-data>/components/assets/<version>/client.jar
//! <app-data>/components/assets/<version>/client.json          (official launcher metadata)
//! <app-data>/components/assets/<version>/.furybuilder-assets.json
//! ```
//!
//! `client.json` is the exact verified version JSON. It lets the read-only asset inspector
//! verify the JAR against the same metadata a launcher would use, without introducing a
//! second metadata format.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use download_pipeline::{
    Allowlist, Checksum, DownloadClient, DownloadProgress, DownloadSpec, NetworkConfig,
    PipelineError,
};
use minecraft_version::{
    fetch_version_json_document_with, read_client_jar_version_json, CatalogError, ManifestEntry,
    ManifestVersionType, ProfileBuildError, VersionCatalog, VersionId, VersionJson,
    VersionJsonError, DEFAULT_MANIFEST_TTL,
};
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use thiserror::Error;

/// Version of the cache manifest stored next to a managed client JAR. This is internal
/// cache metadata, not a user project format; only the current schema is read.
pub const ASSET_MANIFEST_SCHEMA_VERSION: u32 = 1;
/// Hard cap for a downloaded client JAR. Modern JARs are roughly 20–80 MiB.
pub const MAX_CLIENT_JAR_BYTES: u64 = 512 * 1024 * 1024;
/// A real client JAR is larger than this; smaller downloads are refused early.
pub const MIN_CLIENT_JAR_BYTES: u64 = 1024 * 1024;

const MANIFEST_NAME: &str = ".furybuilder-assets.json";
const CLIENT_JAR_NAME: &str = "client.jar";
const VERSION_METADATA_NAME: &str = "client.json";
const MAX_CACHE_ENTRIES: usize = 128;
const PARTIAL_MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);

/// Download metadata shown before any client JAR bytes are requested.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientJarReleaseInfo {
    pub minecraft_version: String,
    pub version_type: ManifestVersionType,
    pub release_time: String,
    pub version_json_url: String,
    pub version_json_sha1: String,
    pub download_url: String,
    pub sha1: String,
    pub size: u64,
    pub source: String,
}

/// One verified client JAR owned by FuryBuilder under its application-data directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedClientJar {
    pub id: String,
    pub version: String,
    pub path: String,
    pub jar_path: String,
    pub sha1: String,
    pub size: u64,
    pub data_version: i32,
    pub resource_pack_format: Option<u32>,
    pub datapack_format: Option<u32>,
    pub source_url: String,
    pub installed_at: u64,
}

impl ManagedClientJar {
    /// Logical component id used by the facade and project manifest.
    pub fn component_id(&self) -> String {
        format!("assets-managed-{}", self.version)
    }
}

/// Inventory of FuryBuilder-owned client JARs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedClientJarReport {
    pub root: String,
    pub clients: Vec<ManagedClientJar>,
    pub total_bytes: u64,
    pub problems: Vec<String>,
}

/// Progress from a managed client JAR download.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientJarProgress {
    pub phase: String,
    pub downloaded: u64,
    pub total: u64,
}

#[derive(Debug, Error)]
pub enum ClientJarError {
    #[error("Minecraft version catalog: {0}")]
    Catalog(#[from] CatalogError),
    #[error("Minecraft version JSON: {0}")]
    VersionJson(#[from] VersionJsonError),
    #[error("client JAR metadata is invalid: {0}")]
    Metadata(#[from] ProfileBuildError),
    #[error("client JAR download failed: {0}")]
    Network(String),
    #[error("network access is disabled (offline mode)")]
    Offline,
    #[error("client JAR I/O error: {0}")]
    Io(String),
    #[error("not enough free disk space: {0}")]
    DiskSpace(String),
    #[error("client JAR checksum mismatch: expected {expected}, got {actual}")]
    Checksum { expected: String, actual: String },
    #[error("client JAR size mismatch: expected {expected} bytes, got {actual}")]
    Size { expected: u64, actual: u64 },
    #[error("client JAR download exceeds the allowed limit: {0}")]
    Limit(String),
    #[error("client JAR download was cancelled")]
    Cancelled,
    #[error("{0}")]
    Unavailable(String),
    #[error("invalid client JAR request: {0}")]
    Invalid(String),
}

impl ClientJarError {
    pub fn code(&self) -> &'static str {
        match self {
            ClientJarError::Catalog(_) | ClientJarError::VersionJson(_) => "network_error",
            ClientJarError::Metadata(_) | ClientJarError::Invalid(_) => "invalid_argument",
            ClientJarError::Network(_) => "network_error",
            ClientJarError::Offline => "offline",
            ClientJarError::Io(_) => "io_error",
            ClientJarError::DiskSpace(_) => "disk_space",
            ClientJarError::Checksum { .. } => "checksum_mismatch",
            ClientJarError::Size { .. } | ClientJarError::Limit(_) => "checksum_mismatch",
            ClientJarError::Cancelled => "cancelled",
            ClientJarError::Unavailable(_) => "unsupported_version",
        }
    }
}

impl From<PipelineError> for ClientJarError {
    fn from(error: PipelineError) -> Self {
        let message = error.to_string();
        match error {
            PipelineError::Offline => ClientJarError::Offline,
            PipelineError::Cancelled => ClientJarError::Cancelled,
            PipelineError::Checksum { expected, actual } => {
                ClientJarError::Checksum { expected, actual }
            }
            PipelineError::Size { expected, actual } => ClientJarError::Size { expected, actual },
            PipelineError::Limit(_) => ClientJarError::Limit(message),
            PipelineError::InsufficientSpace { .. } => ClientJarError::DiskSpace(message),
            PipelineError::Io(_) => ClientJarError::Io(message),
            PipelineError::DisallowedUrl(_)
            | PipelineError::Proxy(_)
            | PipelineError::Http { .. }
            | PipelineError::Transport(_) => ClientJarError::Network(message),
        }
    }
}

/// Resolves and installs official Minecraft client JARs.
pub struct ClientJarManager {
    cache: ManagedClientJarCache,
    manifest_cache: PathBuf,
    client: DownloadClient,
    install_lock: Mutex<()>,
}

impl ClientJarManager {
    pub fn new(root: impl Into<PathBuf>, manifest_cache: impl Into<PathBuf>) -> Self {
        Self::with_client(
            root,
            manifest_cache,
            DownloadClient::new(NetworkConfig::default()),
        )
    }

    pub fn with_client(
        root: impl Into<PathBuf>,
        manifest_cache: impl Into<PathBuf>,
        client: DownloadClient,
    ) -> Self {
        Self {
            cache: ManagedClientJarCache::new(root),
            manifest_cache: manifest_cache.into(),
            client,
            install_lock: Mutex::new(()),
        }
    }

    pub fn root(&self) -> &Path {
        self.cache.root()
    }

    /// Loads the official version manifest through the same cache and network policy as
    /// client JAR downloads. No JAR bytes are fetched.
    pub fn version_catalog(&self) -> Result<VersionCatalog, ClientJarError> {
        VersionCatalog::load_with_client(&self.manifest_cache, DEFAULT_MANIFEST_TTL, &self.client)
            .map_err(client_catalog_error)
    }

    pub fn contains(&self, path: &Path) -> bool {
        self.cache.contains(path)
    }

    pub fn managed_list(&self) -> Result<ManagedClientJarReport, ClientJarError> {
        self.cache.list()
    }

    /// Returns the cached client JAR for exactly this version, if its manifest and size
    /// still agree with the files on disk.
    pub fn find(&self, version: &VersionId) -> Result<Option<ManagedClientJar>, ClientJarError> {
        Ok(self
            .cache
            .list()?
            .clients
            .into_iter()
            .find(|client| client.version == version.as_str()))
    }

    /// Resolves the official version JSON and returns the client JAR preview. No JAR bytes
    /// are downloaded here.
    pub fn release(&self, version: &VersionId) -> Result<ClientJarReleaseInfo, ClientJarError> {
        Ok(self.resolve(version)?.info)
    }

    /// Downloads and publishes the exact client JAR named by the official version JSON.
    ///
    /// An already installed, size-consistent JAR is returned without touching the network
    /// when it matches the reviewed checksum and DataVersion. Every other path performs a
    /// fresh hash-checked download. Partial files are removed on error or cancellation.
    pub fn install(
        &self,
        version: &VersionId,
        expected_sha1: Option<&str>,
        expected_data_version: Option<i32>,
        cancel: &AtomicBool,
        progress: &dyn Fn(ClientJarProgress),
    ) -> Result<ManagedClientJar, ClientJarError> {
        let _guard = self.install_lock.try_lock().map_err(|_| {
            ClientJarError::Invalid("another client JAR install is running".to_owned())
        })?;
        if cancel.load(Ordering::Relaxed) {
            return Err(ClientJarError::Cancelled);
        }
        // Best-effort removal of leftovers from a previous crash; it never fails the install.
        let _ = self.cache.cleanup_partial(PARTIAL_MAX_AGE);

        if let Some(existing) = self.find(version)? {
            accept_existing(&existing, expected_sha1, expected_data_version)?;
            if sha1_file(Path::new(&existing.jar_path))
                .is_ok_and(|actual| actual.eq_ignore_ascii_case(&existing.sha1))
            {
                return Ok(existing);
            }
            // The cache no longer matches its own verified manifest. Remove it and
            // download a fresh copy rather than publishing corrupt bytes.
            let _ = self.cache.remove(version);
        }

        let resolved = self.resolve(version)?;
        if let Some(expected) = expected_sha1 {
            if !expected.eq_ignore_ascii_case(&resolved.info.sha1) {
                return Err(ClientJarError::Checksum {
                    expected: expected.to_owned(),
                    actual: resolved.info.sha1.clone(),
                });
            }
        }
        if cancel.load(Ordering::Relaxed) {
            return Err(ClientJarError::Cancelled);
        }

        progress(ClientJarProgress {
            phase: "downloading".to_owned(),
            downloaded: 0,
            total: resolved.info.size,
        });
        let staged = self.cache.create_staging()?;
        self.client.download(
            &DownloadSpec {
                url: &resolved.info.download_url,
                allowlist: &Allowlist::MOJANG,
                destination: &staged.jar_path(),
                expected_size: resolved.info.size,
                checksum: Checksum::Sha1(&resolved.info.sha1),
                max_size: MAX_CLIENT_JAR_BYTES,
            },
            cancel,
            &|event: DownloadProgress| {
                progress(ClientJarProgress {
                    phase: event.phase.as_str().to_owned(),
                    downloaded: event.downloaded,
                    total: event.total,
                });
            },
        )?;
        self.finish_staged(
            version,
            &resolved.info,
            &resolved.version_json,
            staged,
            expected_data_version,
            cancel,
        )
    }

    /// Removes one FuryBuilder-owned client JAR.
    pub fn remove(&self, version: &VersionId) -> Result<bool, ClientJarError> {
        self.cache.remove(version)
    }

    /// Removes abandoned staging directories and partial downloads older than the given
    /// age. Only FuryBuilder-owned names are touched.
    pub fn cleanup_partial(&self, older_than: Duration) -> Result<u64, ClientJarError> {
        self.cache.cleanup_partial(older_than)
    }

    fn resolve(&self, version: &VersionId) -> Result<ResolvedClientJarRelease, ClientJarError> {
        let catalog = VersionCatalog::load_with_client(
            &self.manifest_cache,
            DEFAULT_MANIFEST_TTL,
            &self.client,
        )
        .map_err(client_catalog_error)?;
        let entry = catalog.entry(version.as_str()).ok_or_else(|| {
            ClientJarError::Unavailable(format!(
                "Minecraft {version} is not present in the Mojang version manifest"
            ))
        })?;
        let (version_json, version_json_bytes) =
            fetch_version_json_document_with(entry, &self.client)
                .map_err(client_version_json_error)?;
        let info = release_from_document(entry, &version_json)?;
        Ok(ResolvedClientJarRelease {
            info,
            version_json: version_json_bytes,
        })
    }

    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn install_reader(
        &self,
        version: &VersionId,
        release: &ClientJarReleaseInfo,
        version_json: &[u8],
        mut reader: impl Read,
        expected_data_version: Option<i32>,
        cancel: &AtomicBool,
        progress: &dyn Fn(ClientJarProgress),
    ) -> Result<ManagedClientJar, ClientJarError> {
        let staged = self.cache.create_staging()?;
        copy_verified(
            &mut reader,
            &staged.jar_path(),
            release.size,
            &release.sha1,
            cancel,
            progress,
        )?;
        self.finish_staged(
            version,
            release,
            version_json,
            staged,
            expected_data_version,
            cancel,
        )
    }

    fn finish_staged(
        &self,
        version: &VersionId,
        release: &ClientJarReleaseInfo,
        version_json: &[u8],
        staged: StagedClientJar,
        expected_data_version: Option<i32>,
        cancel: &AtomicBool,
    ) -> Result<ManagedClientJar, ClientJarError> {
        if cancel.load(Ordering::Relaxed) {
            return Err(ClientJarError::Cancelled);
        }
        let internal = read_client_jar_version_json(staged.jar_path(), version)?;
        if let Some(expected) = expected_data_version {
            if internal.world_version != expected {
                return Err(ClientJarError::Invalid(format!(
                    "the downloaded client JAR for Minecraft {version} declares DataVersion {}, not {expected}",
                    internal.world_version
                )));
            }
        }
        let (resource_pack_format, datapack_format) = internal.integer_pack_formats()?;
        if cancel.load(Ordering::Relaxed) {
            return Err(ClientJarError::Cancelled);
        }
        self.cache.publish(
            staged,
            ClientJarManifest {
                version: version.as_str().to_owned(),
                sha1: release.sha1.clone(),
                size: release.size,
                data_version: internal.world_version,
                resource_pack_format,
                datapack_format,
                source_url: release.download_url.clone(),
                version_json_url: release.version_json_url.clone(),
                version_json_sha1: release.version_json_sha1.clone(),
            },
            version_json,
        )
    }
}

struct ResolvedClientJarRelease {
    info: ClientJarReleaseInfo,
    version_json: Vec<u8>,
}

fn client_catalog_error(error: CatalogError) -> ClientJarError {
    match error {
        CatalogError::Offline => ClientJarError::Offline,
        other => ClientJarError::Catalog(other),
    }
}

fn client_version_json_error(error: VersionJsonError) -> ClientJarError {
    match error {
        VersionJsonError::Offline => ClientJarError::Offline,
        other => ClientJarError::VersionJson(other),
    }
}

/// Builds the preview from an already verified version JSON document.
fn release_from_document(
    entry: &ManifestEntry,
    version_json: &VersionJson,
) -> Result<ClientJarReleaseInfo, ClientJarError> {
    if version_json.id.as_str() != entry.id {
        return Err(ClientJarError::Invalid(format!(
            "version JSON describes {}, not {}",
            version_json.id, entry.id
        )));
    }
    let client = version_json.client().ok_or_else(|| {
        ClientJarError::Unavailable(format!(
            "Minecraft {} does not publish an official client JAR",
            entry.id
        ))
    })?;
    if client.size < MIN_CLIENT_JAR_BYTES {
        return Err(ClientJarError::Size {
            expected: MIN_CLIENT_JAR_BYTES,
            actual: client.size,
        });
    }
    if client.size > MAX_CLIENT_JAR_BYTES {
        return Err(ClientJarError::Limit(format!(
            "announced size {} exceeds {MAX_CLIENT_JAR_BYTES} bytes",
            client.size
        )));
    }
    Allowlist::MOJANG
        .check(&client.url)
        .map_err(ClientJarError::from)?;
    Allowlist::MOJANG
        .check(&entry.url)
        .map_err(ClientJarError::from)?;
    Ok(ClientJarReleaseInfo {
        minecraft_version: entry.id.to_string(),
        version_type: entry.version_type,
        release_time: entry.release_time.clone(),
        version_json_url: entry.url.clone(),
        version_json_sha1: entry.sha1.clone(),
        download_url: client.url.clone(),
        sha1: client.sha1.to_ascii_lowercase(),
        size: client.size,
        source: "Mojang version_manifest_v2".to_owned(),
    })
}

fn accept_existing(
    existing: &ManagedClientJar,
    expected_sha1: Option<&str>,
    expected_data_version: Option<i32>,
) -> Result<(), ClientJarError> {
    if let Some(expected) = expected_sha1 {
        if !expected.eq_ignore_ascii_case(&existing.sha1) {
            return Err(ClientJarError::Checksum {
                expected: expected.to_owned(),
                actual: existing.sha1.clone(),
            });
        }
    }
    if let Some(expected) = expected_data_version {
        if existing.data_version != expected {
            return Err(ClientJarError::Invalid(format!(
                "the cached client JAR for Minecraft {} declares DataVersion {}, not {expected}",
                existing.version, existing.data_version
            )));
        }
    }
    Ok(())
}

/// Streams a cache file through SHA-1 without loading it into memory.
fn sha1_file(path: &Path) -> Result<String, ClientJarError> {
    let mut file = File::open(path)
        .map_err(|error| ClientJarError::Io(format!("cannot read {}: {error}", path.display())))?;
    let mut hasher = Sha1::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| {
            ClientJarError::Io(format!("cannot hash {}: {error}", path.display()))
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex(&hasher.finalize()))
}

/// Streams one response into `destination`, enforcing the announced size and SHA-1.
#[cfg(test)]
fn copy_verified(
    reader: &mut impl Read,
    destination: &Path,
    expected_size: u64,
    expected_sha1: &str,
    cancel: &AtomicBool,
    progress: &dyn Fn(ClientJarProgress),
) -> Result<(), ClientJarError> {
    download_pipeline::write_verified(
        reader,
        destination,
        expected_size,
        Checksum::Sha1(expected_sha1),
        MAX_CLIENT_JAR_BYTES,
        cancel,
        &|event: DownloadProgress| {
            progress(ClientJarProgress {
                phase: event.phase.as_str().to_owned(),
                downloaded: event.downloaded,
                total: event.total,
            });
        },
    )?;
    Ok(())
}

struct ManagedClientJarCache {
    root: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredClientJarManifest {
    schema_version: u32,
    version: String,
    sha1: String,
    size: u64,
    data_version: i32,
    resource_pack_format: Option<u32>,
    datapack_format: Option<u32>,
    source_url: String,
    version_json_url: String,
    version_json_sha1: String,
    installed_at: u64,
}

struct ClientJarManifest {
    version: String,
    sha1: String,
    size: u64,
    data_version: i32,
    resource_pack_format: Option<u32>,
    datapack_format: Option<u32>,
    source_url: String,
    version_json_url: String,
    version_json_sha1: String,
}

struct StagedClientJar {
    directory: PathBuf,
}

impl StagedClientJar {
    fn directory(&self) -> &Path {
        &self.directory
    }

    fn jar_path(&self) -> PathBuf {
        self.directory.join(CLIENT_JAR_NAME)
    }
}

impl Drop for StagedClientJar {
    fn drop(&mut self) {
        // A staged client JAR is private until `publish` renames it into the cache. Every
        // error and cancellation path must leave no partial bytes behind.
        let _ = fs::remove_dir_all(&self.directory);
    }
}

impl ManagedClientJarCache {
    fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn root(&self) -> &Path {
        &self.root
    }

    fn contains(&self, path: &Path) -> bool {
        let Ok(root) = fs::canonicalize(&self.root) else {
            return false;
        };
        let candidate = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        candidate.starts_with(root)
    }

    fn list(&self) -> Result<ManagedClientJarReport, ClientJarError> {
        let mut report = ManagedClientJarReport {
            root: self.root.display().to_string(),
            clients: Vec::new(),
            total_bytes: 0,
            problems: Vec::new(),
        };
        if !self.root.exists() {
            return Ok(report);
        }
        let entries = fs::read_dir(&self.root)
            .map_err(|error| ClientJarError::Io(format!("{}: {error}", self.root.display())))?;
        for (index, entry) in entries.flatten().enumerate() {
            if index >= MAX_CACHE_ENTRIES {
                report.problems.push(format!(
                    "{} contains more than {MAX_CACHE_ENTRIES} entries; the rest was ignored",
                    self.root.display()
                ));
                break;
            }
            let path = entry.path();
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if name == component_model::RESOURCE_PACK_CACHE_DIR
                || name.starts_with(".staging-")
                || name.starts_with(".download-")
            {
                continue;
            }
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
            if !metadata.is_dir() {
                continue;
            }
            let Some(version) = name.parse::<VersionId>().ok() else {
                report.problems.push(format!(
                    "{} is not a Minecraft version directory",
                    path.display()
                ));
                continue;
            };
            let manifest_path = path.join(MANIFEST_NAME);
            if !manifest_path.is_file() {
                report.problems.push(format!(
                    "{} has no FuryBuilder assets manifest",
                    path.display()
                ));
                continue;
            }
            let manifest = match read_manifest(&manifest_path) {
                Ok(manifest) => manifest,
                Err(error) => {
                    report.problems.push(error);
                    continue;
                }
            };
            if manifest.version != version.as_str() {
                report.problems.push(format!(
                    "{} declares Minecraft {}, not {}",
                    manifest_path.display(),
                    manifest.version,
                    version
                ));
                continue;
            }
            let jar_path = path.join(CLIENT_JAR_NAME);
            let jar_metadata = match fs::symlink_metadata(&jar_path) {
                Ok(metadata) => metadata,
                Err(error) => {
                    report
                        .problems
                        .push(format!("{}: {error}", jar_path.display()));
                    continue;
                }
            };
            if jar_metadata.file_type().is_symlink() || !jar_metadata.is_file() {
                report
                    .problems
                    .push(format!("{} is not a regular file", jar_path.display()));
                continue;
            }
            if jar_metadata.len() != manifest.size {
                report.problems.push(format!(
                    "{} changed on disk (expected {} bytes, found {})",
                    jar_path.display(),
                    manifest.size,
                    jar_metadata.len()
                ));
                continue;
            }
            let metadata_path = path.join(VERSION_METADATA_NAME);
            if !metadata_path.is_file() {
                report.problems.push(format!(
                    "{} is missing the launcher metadata client.json",
                    path.display()
                ));
                continue;
            }
            let bytes = jar_metadata
                .len()
                .saturating_add(manifest_path.metadata().map(|meta| meta.len()).unwrap_or(0))
                .saturating_add(metadata_path.metadata().map(|meta| meta.len()).unwrap_or(0));
            report.total_bytes = report.total_bytes.saturating_add(bytes);
            report.clients.push(ManagedClientJar {
                id: format!("assets-managed-{}", manifest.version),
                version: manifest.version.clone(),
                path: path.display().to_string(),
                jar_path: jar_path.display().to_string(),
                sha1: manifest.sha1.clone(),
                size: jar_metadata.len(),
                data_version: manifest.data_version,
                resource_pack_format: manifest.resource_pack_format,
                datapack_format: manifest.datapack_format,
                source_url: manifest.source_url.clone(),
                installed_at: manifest.installed_at,
            });
        }
        report
            .clients
            .sort_by(|left, right| left.version.cmp(&right.version));
        Ok(report)
    }

    fn create_staging(&self) -> Result<StagedClientJar, ClientJarError> {
        fs::create_dir_all(&self.root)
            .map_err(|error| ClientJarError::Io(format!("{}: {error}", self.root.display())))?;
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let directory = self
            .root
            .join(format!(".staging-{}-{suffix}", std::process::id()));
        fs::create_dir_all(&directory)
            .map_err(|error| ClientJarError::Io(format!("{}: {error}", directory.display())))?;
        Ok(StagedClientJar { directory })
    }

    fn publish(
        &self,
        staged: StagedClientJar,
        manifest: ClientJarManifest,
        version_json: &[u8],
    ) -> Result<ManagedClientJar, ClientJarError> {
        validate_version_name(&manifest.version)?;
        let destination = self.root.join(&manifest.version);
        if destination.exists() {
            let existing = self
                .list()?
                .clients
                .into_iter()
                .find(|client| client.version == manifest.version);
            return match existing {
                Some(existing) if existing.sha1.eq_ignore_ascii_case(&manifest.sha1) => {
                    Ok(existing)
                }
                Some(_) => Err(ClientJarError::Invalid(format!(
                    "managed assets for Minecraft {} are already installed with a different checksum",
                    manifest.version
                ))),
                None => Err(ClientJarError::Invalid(format!(
                    "{} already exists and is not valid managed assets",
                    destination.display()
                ))),
            };
        }
        let jar_path = staged.jar_path();
        let jar_size = fs::symlink_metadata(&jar_path)
            .map_err(|error| ClientJarError::Io(format!("{}: {error}", jar_path.display())))?
            .len();
        if jar_size != manifest.size {
            return Err(ClientJarError::Size {
                expected: manifest.size,
                actual: jar_size,
            });
        }
        fs::write(staged.directory().join(VERSION_METADATA_NAME), version_json).map_err(
            |error| ClientJarError::Io(format!("cannot write the verified version JSON: {error}")),
        )?;
        let stored = StoredClientJarManifest {
            schema_version: ASSET_MANIFEST_SCHEMA_VERSION,
            version: manifest.version.clone(),
            sha1: manifest.sha1.to_ascii_lowercase(),
            size: manifest.size,
            data_version: manifest.data_version,
            resource_pack_format: manifest.resource_pack_format,
            datapack_format: manifest.datapack_format,
            source_url: manifest.source_url.clone(),
            version_json_url: manifest.version_json_url.clone(),
            version_json_sha1: manifest.version_json_sha1.clone(),
            installed_at: now_secs(),
        };
        let bytes = serde_json::to_vec_pretty(&stored)
            .map_err(|error| ClientJarError::Io(error.to_string()))?;
        fs::write(staged.directory().join(MANIFEST_NAME), bytes).map_err(|error| {
            ClientJarError::Io(format!("cannot write the assets manifest: {error}"))
        })?;
        download_pipeline::publish_directory(staged.directory(), &destination)
            .map_err(ClientJarError::from)?;
        Ok(ManagedClientJar {
            id: format!("assets-managed-{}", stored.version),
            version: stored.version,
            path: destination.display().to_string(),
            jar_path: destination.join(CLIENT_JAR_NAME).display().to_string(),
            sha1: stored.sha1,
            size: stored.size,
            data_version: stored.data_version,
            resource_pack_format: stored.resource_pack_format,
            datapack_format: stored.datapack_format,
            source_url: stored.source_url,
            installed_at: stored.installed_at,
        })
    }

    fn remove(&self, version: &VersionId) -> Result<bool, ClientJarError> {
        validate_version_name(version.as_str())?;
        let directory = self.root.join(version.as_str());
        if !directory.exists() {
            return Ok(false);
        }
        let manifest_path = directory.join(MANIFEST_NAME);
        if !manifest_path.is_file() {
            return Err(ClientJarError::Invalid(format!(
                "{} is not a FuryBuilder-managed assets directory",
                directory.display()
            )));
        }
        let manifest = read_manifest(&manifest_path).map_err(ClientJarError::Invalid)?;
        if manifest.version != version.as_str() {
            return Err(ClientJarError::Invalid(format!(
                "{} declares Minecraft {}, not {version}",
                manifest_path.display(),
                manifest.version
            )));
        }
        let root = fs::canonicalize(&self.root).map_err(|error| {
            ClientJarError::Io(format!("cannot resolve {}: {error}", self.root.display()))
        })?;
        let target = fs::canonicalize(&directory).map_err(|error| {
            ClientJarError::Io(format!("cannot resolve {}: {error}", directory.display()))
        })?;
        if target == root || !target.starts_with(&root) {
            return Err(ClientJarError::Invalid(format!(
                "{} is not a managed assets directory",
                directory.display()
            )));
        }
        fs::remove_dir_all(&target).map_err(|error| {
            ClientJarError::Io(format!("cannot remove {}: {error}", target.display()))
        })?;
        Ok(true)
    }

    fn cleanup_partial(&self, older_than: Duration) -> Result<u64, ClientJarError> {
        if !self.root.exists() {
            return Ok(0);
        }
        let mut removed = 0;
        for entry in fs::read_dir(&self.root)
            .map_err(|error| ClientJarError::Io(format!("{}: {error}", self.root.display())))?
            .flatten()
        {
            let path = entry.path();
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            let is_partial = name.starts_with(".staging-") || name.starts_with(".download-");
            if !is_partial {
                continue;
            }
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(_) => continue,
            };
            if metadata.file_type().is_symlink() {
                continue;
            }
            let old_enough = metadata
                .modified()
                .ok()
                .and_then(|modified| modified.elapsed().ok())
                .is_some_and(|age| age >= older_than);
            if !old_enough {
                continue;
            }
            let removed_entry = if metadata.is_dir() {
                fs::remove_dir_all(&path)
            } else {
                fs::remove_file(&path)
            };
            if removed_entry.is_ok() {
                removed += 1;
            }
        }
        Ok(removed)
    }
}

fn read_manifest(path: &Path) -> Result<StoredClientJarManifest, String> {
    let bytes =
        fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let manifest: StoredClientJarManifest = serde_json::from_slice(&bytes)
        .map_err(|error| format!("cannot parse {}: {error}", path.display()))?;
    if manifest.schema_version != ASSET_MANIFEST_SCHEMA_VERSION {
        return Err(format!(
            "{} uses unsupported assets manifest schema {}; the current managed cache layout is required",
            path.display(),
            manifest.schema_version
        ));
    }
    Ok(manifest)
}

fn validate_version_name(version: &str) -> Result<(), ClientJarError> {
    let valid = !version.is_empty()
        && version != "."
        && version != ".."
        && !version.starts_with('.')
        && version.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_')
        });
    if valid {
        Ok(())
    } else {
        Err(ClientJarError::Invalid(format!(
            "`{version}` is not a safe Minecraft version directory name"
        )))
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        let _ = write!(text, "{byte:02x}");
    }
    text
}

/// Builds a small but valid client JAR, its official version JSON and the matching
/// release preview. Shared by cache, facade and future mock-HTTP tests.
#[cfg(test)]
pub(crate) fn test_fixture(
    version: &str,
    data_version: i32,
) -> (ClientJarReleaseInfo, Vec<u8>, Vec<u8>) {
    use std::io::{Cursor, Write};
    use zip::write::SimpleFileOptions;

    let mut buffer = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buffer);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        zip.start_file("version.json", options)
            .expect("start version.json");
        let json = format!(
            r#"{{"id":"{version}","world_version":{data_version},"pack_version":{{"resource":46,"data":61}},"stable":true}}"#
        );
        zip.write_all(json.as_bytes()).expect("write version.json");
        zip.start_file("assets/minecraft/blockstates/stone.json", options)
            .expect("start blockstate");
        zip.write_all(br#"{"variants":{"":{"model":"minecraft:block/stone"}}}"#)
            .expect("write blockstate");
        zip.finish().expect("finish jar");
    }
    let jar = buffer.into_inner();
    let sha1 = hex(&Sha1::digest(&jar));
    let release = ClientJarReleaseInfo {
        minecraft_version: version.to_owned(),
        version_type: ManifestVersionType::Release,
        release_time: "2024-12-03T10:12:57+00:00".to_owned(),
        version_json_url: "https://piston-meta.mojang.com/v1/packages/aaa/1.21.4.json".to_owned(),
        version_json_sha1: "a".repeat(40),
        download_url: format!("https://piston-data.mojang.com/v1/objects/{sha1}/client.jar"),
        sha1,
        size: jar.len() as u64,
        source: "Mojang version_manifest_v2".to_owned(),
    };
    let document = serde_json::to_vec(&serde_json::json!({
        "id": version,
        "type": "release",
        "mainClass": "net.minecraft.client.main.Main",
        "javaVersion": { "component": "java-runtime-delta", "majorVersion": 21 },
        "time": "2024-12-03T10:12:57+00:00",
        "releaseTime": "2024-12-03T10:12:57+00:00",
        "downloads": {
            "client": {
                "sha1": release.sha1,
                "size": release.size,
                "url": release.download_url
            }
        }
    }))
    .expect("serialize version JSON");
    (release, document, jar)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    use zip::write::SimpleFileOptions;

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "furybuilder-client-jar-{label}-{}-{}",
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

    fn jar_bytes(version: &str, data_version: i32) -> Vec<u8> {
        let mut buffer = Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buffer);
            let options =
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
            zip.start_file("version.json", options)
                .expect("start version.json");
            let json = format!(
                r#"{{"id":"{version}","world_version":{data_version},"pack_version":{{"resource":46,"data":61}},"stable":true}}"#
            );
            zip.write_all(json.as_bytes()).expect("write version.json");
            zip.start_file("assets/minecraft/blockstates/stone.json", options)
                .expect("start blockstate");
            zip.write_all(br#"{"variants":{"":{"model":"minecraft:block/stone"}}}"#)
                .expect("write blockstate");
            zip.finish().expect("finish jar");
        }
        buffer.into_inner()
    }

    fn version_json_document(version: &str, sha1: &str, size: u64) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "id": version,
            "type": "release",
            "mainClass": "net.minecraft.client.main.Main",
            "javaVersion": { "component": "java-runtime-delta", "majorVersion": 21 },
            "time": "2024-12-03T10:12:57+00:00",
            "releaseTime": "2024-12-03T10:12:57+00:00",
            "downloads": {
                "client": {
                    "sha1": sha1,
                    "size": size,
                    "url": format!("https://piston-data.mojang.com/v1/objects/{sha1}/client.jar")
                }
            }
        }))
        .expect("serialize version JSON")
    }

    fn release(version: &str, jar: &[u8]) -> ClientJarReleaseInfo {
        let sha1 = hex(&Sha1::digest(jar));
        ClientJarReleaseInfo {
            minecraft_version: version.to_owned(),
            version_type: ManifestVersionType::Release,
            release_time: "2024-12-03T10:12:57+00:00".to_owned(),
            version_json_url: "https://piston-meta.mojang.com/v1/packages/aaa/1.21.4.json"
                .to_owned(),
            version_json_sha1: "a".repeat(40),
            download_url: format!("https://piston-data.mojang.com/v1/objects/{sha1}/client.jar"),
            sha1,
            size: jar.len() as u64,
            source: "Mojang version_manifest_v2".to_owned(),
        }
    }

    fn manifest_entry(version: &str) -> ManifestEntry {
        ManifestEntry {
            id: version.to_owned(),
            version_type: ManifestVersionType::Release,
            url: "https://piston-meta.mojang.com/v1/packages/aaa/1.21.4.json".to_owned(),
            time: "2024-12-03T10:12:57+00:00".to_owned(),
            release_time: "2024-12-03T10:12:57+00:00".to_owned(),
            sha1: "a".repeat(40),
            compliance_level: Some(1),
        }
    }

    #[test]
    fn install_publishes_the_versioned_client_jar_and_manifest() {
        let dir = TempDir::new("publish");
        let manager = ClientJarManager::new(
            dir.path.join("components").join("assets"),
            dir.path.join("cache").join("version_manifest_v2.json"),
        );
        let version: VersionId = "1.21.4".parse().expect("version");
        let jar = jar_bytes("1.21.4", 4189);
        let release = release("1.21.4", &jar);
        let document = version_json_document("1.21.4", &release.sha1, release.size);

        let installed = manager
            .install_reader(
                &version,
                &release,
                &document,
                Cursor::new(jar),
                Some(4189),
                &AtomicBool::new(false),
                &|_| {},
            )
            .expect("install client jar");

        assert_eq!(installed.id, "assets-managed-1.21.4");
        assert_eq!(installed.version, "1.21.4");
        assert_eq!(installed.data_version, 4189);
        assert_eq!(installed.resource_pack_format, Some(46));
        assert_eq!(installed.datapack_format, Some(61));
        let directory = manager.root().join("1.21.4");
        assert!(directory.join(CLIENT_JAR_NAME).is_file());
        assert!(directory.join(VERSION_METADATA_NAME).is_file());
        assert!(directory.join(MANIFEST_NAME).is_file());
        assert!(manager.contains(Path::new(&installed.jar_path)));

        let list = manager.managed_list().expect("list managed assets");
        assert_eq!(list.clients.len(), 1);
        assert_eq!(list.clients[0].version, "1.21.4");
        assert!(list.total_bytes > 0);
        assert!(list.problems.is_empty(), "{:?}", list.problems);
        assert_eq!(
            manager.find(&version).expect("find").as_ref(),
            Some(&installed)
        );

        let inspection = asset_source::inspect_asset_path(&installed.jar_path);
        assert!(inspection.valid, "{:?}", inspection.errors);
        assert_eq!(
            inspection.version.expect("version report").status,
            asset_source::VersionStatus::Compatible
        );

        assert!(manager.remove(&version).expect("remove"));
        assert!(manager.managed_list().expect("list").clients.is_empty());
        assert!(!manager.remove(&version).expect("second remove"));
    }

    #[test]
    fn install_rejects_a_data_version_mismatch_and_cleans_the_staging_directory() {
        let dir = TempDir::new("wrong-data-version");
        let manager = ClientJarManager::new(
            dir.path.join("components").join("assets"),
            dir.path.join("cache").join("version_manifest_v2.json"),
        );
        let version: VersionId = "1.21.4".parse().expect("version");
        let jar = jar_bytes("1.21.4", 4189);
        let release = release("1.21.4", &jar);
        let document = version_json_document("1.21.4", &release.sha1, release.size);

        let error = manager
            .install_reader(
                &version,
                &release,
                &document,
                Cursor::new(jar),
                Some(999),
                &AtomicBool::new(false),
                &|_| {},
            )
            .expect_err("mismatched DataVersion must fail");
        assert!(matches!(error, ClientJarError::Invalid(_)), "{error:?}");
        assert!(!manager.root().join("1.21.4").exists());
        assert!(staging_entries(manager.root()) == 0);
    }

    #[test]
    fn copy_verified_enforces_size_hash_and_cancellation() {
        let dir = TempDir::new("copy-verified");
        let destination = dir.path.join("client.jar");
        let data = b"a real client jar payload";
        let expected = hex(&Sha1::digest(data));

        copy_verified(
            &mut Cursor::new(data),
            &destination,
            data.len() as u64,
            &expected,
            &AtomicBool::new(false),
            &|_| {},
        )
        .expect("verified copy");
        assert_eq!(fs::read(&destination).expect("read"), data);

        let error = copy_verified(
            &mut Cursor::new(data),
            &destination,
            data.len() as u64,
            &"b".repeat(40),
            &AtomicBool::new(false),
            &|_| {},
        )
        .expect_err("wrong hash");
        assert!(
            matches!(error, ClientJarError::Checksum { .. }),
            "{error:?}"
        );

        let error = copy_verified(
            &mut Cursor::new(data),
            &destination,
            data.len() as u64 + 1,
            &expected,
            &AtomicBool::new(false),
            &|_| {},
        )
        .expect_err("wrong size");
        assert!(matches!(error, ClientJarError::Size { .. }), "{error:?}");

        let error = copy_verified(
            &mut Cursor::new(data),
            &destination,
            data.len() as u64,
            &expected,
            &AtomicBool::new(true),
            &|_| {},
        )
        .expect_err("cancelled");
        assert!(matches!(error, ClientJarError::Cancelled), "{error:?}");
    }

    #[test]
    fn a_tampered_jar_is_reported_and_not_listed_as_usable() {
        let dir = TempDir::new("tampered");
        let manager = ClientJarManager::new(
            dir.path.join("components").join("assets"),
            dir.path.join("cache").join("version_manifest_v2.json"),
        );
        let version: VersionId = "1.21.4".parse().expect("version");
        let jar = jar_bytes("1.21.4", 4189);
        let release = release("1.21.4", &jar);
        let document = version_json_document("1.21.4", &release.sha1, release.size);
        let installed = manager
            .install_reader(
                &version,
                &release,
                &document,
                Cursor::new(jar),
                Some(4189),
                &AtomicBool::new(false),
                &|_| {},
            )
            .expect("install");

        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(&installed.jar_path)
            .expect("open jar");
        file.write_all(b"tampered").expect("append");
        drop(file);

        let list = manager.managed_list().expect("list");
        assert!(list.clients.is_empty());
        assert!(list
            .problems
            .iter()
            .any(|problem| problem.contains("changed on disk")));
        assert!(manager.find(&version).expect("find").is_none());
    }

    #[test]
    fn version_directory_names_cannot_escape_the_cache() {
        for value in ["..", ".hidden", "a/b", "a\\b", ""] {
            assert!(
                validate_version_name(value).is_err(),
                "{value} must be rejected"
            );
        }
        for value in ["1.21.4", "26.1-snapshot-3", "1.20.6"] {
            assert!(
                validate_version_name(value).is_ok(),
                "{value} must be accepted"
            );
        }
    }

    #[test]
    fn release_preview_requires_an_official_client_download() {
        let document = serde_json::json!({
            "id": "1.21.4",
            "type": "release",
            "time": "2024-12-03T10:12:57+00:00",
            "releaseTime": "2024-12-03T10:12:57+00:00",
            "downloads": {}
        });
        let version = VersionJson::from_json(&document.to_string()).expect("version json");
        let error = release_from_document(&manifest_entry("1.21.4"), &version)
            .expect_err("no client download");
        assert!(matches!(error, ClientJarError::Unavailable(_)), "{error:?}");
    }

    #[test]
    fn release_preview_carries_the_reviewed_hash_and_size() {
        let jar = jar_bytes("1.21.4", 4189);
        let mut expected = release("1.21.4", &jar);
        expected.size = MIN_CLIENT_JAR_BYTES;
        let document = version_json_document("1.21.4", &expected.sha1, expected.size);
        let version = VersionJson::from_json(std::str::from_utf8(&document).expect("utf8"))
            .expect("version json");
        let preview = release_from_document(&manifest_entry("1.21.4"), &version).expect("preview");
        assert_eq!(preview.minecraft_version, "1.21.4");
        assert_eq!(preview.sha1, expected.sha1);
        assert_eq!(preview.size, expected.size);
        assert_eq!(preview.version_json_sha1, "a".repeat(40));
    }

    #[test]
    fn offline_client_jar_resolution_is_refused_without_network() {
        let dir = TempDir::new("offline");
        let manager = ClientJarManager::with_client(
            dir.path.join("components").join("assets"),
            dir.path.join("cache").join("version_manifest_v2.json"),
            DownloadClient::new(NetworkConfig::offline()),
        );
        let version: VersionId = "1.21.4".parse().expect("version");
        let error = manager.release(&version).expect_err("offline");
        assert!(matches!(error, ClientJarError::Offline), "{error:?}");
        assert_eq!(error.code(), "offline");
    }

    fn staging_entries(root: &Path) -> usize {
        if !root.exists() {
            return 0;
        }
        fs::read_dir(root)
            .expect("cache root")
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(".staging-"))
            .count()
    }
}
