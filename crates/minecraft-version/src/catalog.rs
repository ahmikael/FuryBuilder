//! Official Minecraft version manifest loading and caching.
//!
//! The catalog is the only place that talks to Mojang's version manifest. It
//! validates the manifest, keeps a timestamped cache, serves a stale cache
//! when the network is unavailable, and refuses download URLs outside the
//! official host allowlist.

use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use download_pipeline::{Allowlist, DownloadClient, NetworkConfig, PipelineError};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::VersionId;

/// Official Mojang version manifest used by the launcher and FuryBuilder.
pub const OFFICIAL_MANIFEST_URL: &str =
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
/// Version of FuryBuilder's on-disk manifest cache.
pub const MANIFEST_CACHE_SCHEMA_VERSION: u32 = 1;
/// How long a cached manifest is considered fresh.
pub const DEFAULT_MANIFEST_TTL: Duration = Duration::from_secs(6 * 60 * 60);

const MAX_MANIFEST_BYTES: u64 = 8 * 1024 * 1024;

/// The official version manifest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionManifest {
    pub latest: LatestVersions,
    pub versions: Vec<ManifestEntry>,
}

impl VersionManifest {
    /// Parses and validates a manifest document.
    pub fn from_json(json: &str) -> Result<Self, CatalogError> {
        let manifest: Self = serde_json::from_str(json)?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Checks invariants the rest of the application relies on.
    pub fn validate(&self) -> Result<(), CatalogError> {
        if self.versions.is_empty() {
            return Err(CatalogError::EmptyManifest);
        }

        let mut ids = HashSet::new();
        for entry in &self.versions {
            if !ids.insert(entry.id.as_str()) {
                return Err(CatalogError::DuplicateVersionId(entry.id.to_string()));
            }
        }

        if !self
            .versions
            .iter()
            .any(|entry| entry.id == self.latest.release)
        {
            return Err(CatalogError::MissingLatestVersion(
                self.latest.release.to_string(),
            ));
        }
        if !self
            .versions
            .iter()
            .any(|entry| entry.id == self.latest.snapshot)
        {
            return Err(CatalogError::MissingLatestVersion(
                self.latest.snapshot.to_string(),
            ));
        }

        Ok(())
    }
}

/// The newest release and snapshot ids advertised by Mojang.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LatestVersions {
    pub release: VersionId,
    pub snapshot: VersionId,
}

/// One version in the official manifest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestEntry {
    pub id: VersionId,
    #[serde(rename = "type")]
    pub version_type: ManifestVersionType,
    /// Absolute URL of the version JSON; validated against the host allowlist.
    pub url: String,
    pub time: String,
    pub release_time: String,
    #[serde(default)]
    pub sha1: String,
    #[serde(default)]
    pub compliance_level: Option<u8>,
}

/// The channel labels used by the official manifest.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManifestVersionType {
    Release,
    Snapshot,
    OldBeta,
    OldAlpha,
}

/// Where the active catalog came from.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum CatalogSource {
    Network,
    Cache,
}

/// A validated manifest plus cache provenance.
#[derive(Clone, Debug)]
pub struct VersionCatalog {
    manifest: VersionManifest,
    fetched_at_unix: u64,
    source: CatalogSource,
}

impl VersionCatalog {
    /// Builds a catalog from an already parsed manifest.
    pub fn from_manifest(
        manifest: VersionManifest,
        fetched_at_unix: u64,
        source: CatalogSource,
    ) -> Result<Self, CatalogError> {
        manifest.validate()?;
        Ok(Self {
            manifest,
            fetched_at_unix,
            source,
        })
    }

    /// Loads the manifest cache or fetches the official manifest.
    ///
    /// A fresh cache is used without network access. A stale cache is refreshed
    /// when possible; if the fetch fails, the stale cache is served instead.
    pub fn load(cache_path: impl AsRef<Path>, ttl: Duration) -> Result<Self, CatalogError> {
        let client = DownloadClient::new(NetworkConfig::default());
        Self::load_with_client(cache_path.as_ref(), ttl, &client)
    }

    /// Loads through a caller-owned client so proxy/offline policy is shared with the
    /// component that requested the manifest.
    pub fn load_with_client(
        cache_path: &Path,
        ttl: Duration,
        client: &DownloadClient,
    ) -> Result<Self, CatalogError> {
        Self::load_with(cache_path, ttl, unix_now(), || fetch_manifest_with(client))
    }

    /// Reads the cache without any network request, regardless of its age.
    pub fn load_offline(cache_path: impl AsRef<Path>) -> Result<Self, CatalogError> {
        let cached = read_cache(cache_path.as_ref()).ok_or(CatalogError::Offline)?;
        Self::from_manifest(
            cached.manifest,
            cached.fetched_at_unix,
            CatalogSource::Cache,
        )
    }

    /// Deterministic loader used by tests and callers with their own HTTP stack.
    pub fn load_with(
        cache_path: &Path,
        ttl: Duration,
        now_unix: u64,
        fetch: impl FnOnce() -> Result<String, CatalogError>,
    ) -> Result<Self, CatalogError> {
        let cached = read_cache(cache_path);

        if let Some(cached) = cached.as_ref() {
            if now_unix.saturating_sub(cached.fetched_at_unix) <= ttl.as_secs() {
                return Self::from_manifest(
                    cached.manifest.clone(),
                    cached.fetched_at_unix,
                    CatalogSource::Cache,
                );
            }
        }

        match fetch() {
            Ok(json) => {
                let manifest = VersionManifest::from_json(&json)?;
                write_cache(cache_path, now_unix, &manifest)?;
                Self::from_manifest(manifest, now_unix, CatalogSource::Network)
            }
            Err(error) => match cached {
                Some(cached) => Self::from_manifest(
                    cached.manifest,
                    cached.fetched_at_unix,
                    CatalogSource::Cache,
                ),
                None => Err(error),
            },
        }
    }

    pub fn manifest(&self) -> &VersionManifest {
        &self.manifest
    }

    /// Unix timestamp of the moment the manifest was fetched from Mojang.
    pub fn fetched_at_unix(&self) -> u64 {
        self.fetched_at_unix
    }

    pub fn source(&self) -> CatalogSource {
        self.source
    }

    pub fn entry(&self, id: &str) -> Option<&ManifestEntry> {
        self.manifest
            .versions
            .iter()
            .find(|entry| entry.id.as_str() == id)
    }

    pub fn latest_release(&self) -> Option<&ManifestEntry> {
        self.entry(self.manifest.latest.release.as_str())
    }

    pub fn latest_snapshot(&self) -> Option<&ManifestEntry> {
        self.entry(self.manifest.latest.snapshot.as_str())
    }

    /// Release entries in manifest order (newest first).
    pub fn releases(&self) -> impl Iterator<Item = &ManifestEntry> {
        self.manifest
            .versions
            .iter()
            .filter(|entry| entry.version_type == ManifestVersionType::Release)
    }

    /// Snapshot entries in manifest order (newest first).
    pub fn snapshots(&self) -> impl Iterator<Item = &ManifestEntry> {
        self.manifest
            .versions
            .iter()
            .filter(|entry| entry.version_type == ManifestVersionType::Snapshot)
    }
}

/// Fetches the official manifest without touching the cache.
pub fn fetch_official_manifest() -> Result<String, CatalogError> {
    let client = DownloadClient::new(NetworkConfig::default());
    fetch_manifest_with(&client)
}

/// Fetches the official manifest through a caller-owned network policy.
pub fn fetch_manifest_with(client: &DownloadClient) -> Result<String, CatalogError> {
    let bytes = client
        .get_bytes(
            OFFICIAL_MANIFEST_URL,
            &Allowlist::MOJANG,
            MAX_MANIFEST_BYTES,
        )
        .map_err(catalog_pipeline_error)?;
    String::from_utf8(bytes)
        .map_err(|error| CatalogError::Http(format!("the manifest is not valid UTF-8: {error}")))
}

/// Returns whether a URL points at an allowed official download host.
pub fn is_allowed_download_url(url: &str) -> bool {
    Allowlist::MOJANG.is_allowed(url)
}

fn catalog_pipeline_error(error: PipelineError) -> CatalogError {
    match error {
        PipelineError::Offline => CatalogError::Offline,
        PipelineError::DisallowedUrl(url) => CatalogError::DisallowedUrl(url),
        PipelineError::InsufficientSpace { .. } => {
            CatalogError::Io(std::io::Error::other(error.to_string()))
        }
        other => CatalogError::Http(other.to_string()),
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CachedManifest {
    schema_version: u32,
    fetched_at_unix: u64,
    manifest: VersionManifest,
}

fn read_cache(path: &Path) -> Option<CachedManifest> {
    let bytes = fs::read(path).ok()?;
    let cached: CachedManifest = serde_json::from_slice(&bytes).ok()?;
    (cached.schema_version == MANIFEST_CACHE_SCHEMA_VERSION).then_some(cached)
}

fn write_cache(
    path: &Path,
    fetched_at_unix: u64,
    manifest: &VersionManifest,
) -> Result<(), CatalogError> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }

    let cached = CachedManifest {
        schema_version: MANIFEST_CACHE_SCHEMA_VERSION,
        fetched_at_unix,
        manifest: manifest.clone(),
    };
    let bytes = serde_json::to_vec_pretty(&cached)?;
    download_pipeline::write_file(path, &bytes).map_err(catalog_pipeline_error)?;
    Ok(())
}

/// Errors from manifest loading, validation or caching.
#[derive(Debug, Error)]
pub enum CatalogError {
    #[error("failed to fetch the Minecraft version manifest: {0}")]
    Http(String),
    #[error("network access is disabled and no cached Minecraft version manifest is available")]
    Offline,
    #[error("manifest cache I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid Minecraft version manifest: {0}")]
    Json(#[from] serde_json::Error),
    #[error("the Minecraft version manifest has no versions")]
    EmptyManifest,
    #[error("duplicate version id in the manifest: {0}")]
    DuplicateVersionId(String),
    #[error("the manifest does not contain its latest version: {0}")]
    MissingLatestVersion(String),
    #[error("download host is not allowed: {0}")]
    DisallowedUrl(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const MANIFEST_JSON: &str = r#"{
      "latest": { "release": "1.21.4", "snapshot": "26.1-snapshot-3" },
      "versions": [
        {
          "id": "1.21.4",
          "type": "release",
          "url": "https://piston-meta.mojang.com/v1/packages/aaa/1.21.4.json",
          "time": "2024-12-03T10:12:57+00:00",
          "releaseTime": "2024-12-03T10:12:57+00:00",
          "sha1": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
          "complianceLevel": 1
        },
        {
          "id": "26.1-snapshot-3",
          "type": "snapshot",
          "url": "https://piston-meta.mojang.com/v1/packages/bbb/26.1-snapshot-3.json",
          "time": "2025-12-10T09:00:00+00:00",
          "releaseTime": "2025-12-10T09:00:00+00:00",
          "sha1": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        },
        {
          "id": "b1.7.3",
          "type": "old_beta",
          "url": "https://piston-meta.mojang.com/v1/packages/ccc/b1.7.3.json",
          "time": "2011-07-08T00:00:00+00:00",
          "releaseTime": "2011-07-08T00:00:00+00:00",
          "sha1": "cccccccccccccccccccccccccccccccccccccccc"
        },
        {
          "id": "a1.2.6",
          "type": "old_alpha",
          "url": "https://piston-meta.mojang.com/v1/packages/ddd/a1.2.6.json",
          "time": "2010-12-21T00:00:00+00:00",
          "releaseTime": "2010-12-21T00:00:00+00:00",
          "sha1": "dddddddddddddddddddddddddddddddddddddddd"
        }
      ]
    }"#;

    fn temp_cache(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "furybuilder-version-catalog-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        root.join("version_manifest_v2.json")
    }

    #[test]
    fn parses_and_filters_the_official_manifest() {
        let manifest = VersionManifest::from_json(MANIFEST_JSON).expect("valid manifest");
        assert_eq!(manifest.latest.release.as_str(), "1.21.4");
        assert_eq!(manifest.latest.snapshot.as_str(), "26.1-snapshot-3");
        assert_eq!(manifest.versions.len(), 4);

        let catalog =
            VersionCatalog::from_manifest(manifest, 100, CatalogSource::Network).expect("catalog");
        assert_eq!(catalog.releases().count(), 1);
        assert_eq!(catalog.snapshots().count(), 1);
        assert_eq!(
            catalog.entry("1.21.4").expect("release").version_type,
            ManifestVersionType::Release
        );
        assert_eq!(
            catalog.entry("b1.7.3").expect("beta").version_type,
            ManifestVersionType::OldBeta
        );
        assert_eq!(
            catalog
                .latest_release()
                .expect("latest release")
                .id
                .as_str(),
            "1.21.4"
        );
    }

    #[test]
    fn rejects_empty_duplicate_and_incomplete_manifests() {
        let empty = r#"{"latest":{"release":"1.21.4","snapshot":"1.21.4"},"versions":[]}"#;
        assert!(matches!(
            VersionManifest::from_json(empty),
            Err(CatalogError::EmptyManifest)
        ));

        let duplicate = MANIFEST_JSON.replace("\"id\": \"26.1-snapshot-3\"", "\"id\": \"1.21.4\"");
        assert!(matches!(
            VersionManifest::from_json(&duplicate),
            Err(CatalogError::DuplicateVersionId(id)) if id == "1.21.4"
        ));

        let missing_latest =
            MANIFEST_JSON.replace("\"release\": \"1.21.4\"", "\"release\": \"1.21.5\"");
        assert!(matches!(
            VersionManifest::from_json(&missing_latest),
            Err(CatalogError::MissingLatestVersion(id)) if id == "1.21.5"
        ));
    }

    #[test]
    fn fresh_cache_is_used_without_fetching() {
        let cache = temp_cache("fresh");
        let first = VersionCatalog::load_with(&cache, Duration::from_secs(100), 1_000, || {
            Ok(MANIFEST_JSON.to_owned())
        })
        .expect("initial fetch");
        assert_eq!(first.source(), CatalogSource::Network);
        assert_eq!(first.fetched_at_unix(), 1_000);

        let second = VersionCatalog::load_with(&cache, Duration::from_secs(100), 1_050, || {
            panic!("a fresh cache must not trigger a fetch")
        })
        .expect("cache hit");
        assert_eq!(second.source(), CatalogSource::Cache);
        assert_eq!(second.fetched_at_unix(), 1_000);
        assert_eq!(second.manifest(), first.manifest());

        let _ = fs::remove_dir_all(cache.parent().expect("cache parent"));
    }

    #[test]
    fn stale_cache_is_served_when_the_network_is_offline() {
        let cache = temp_cache("offline");
        VersionCatalog::load_with(&cache, Duration::from_secs(100), 1_000, || {
            Ok(MANIFEST_JSON.to_owned())
        })
        .expect("initial fetch");

        let offline = VersionCatalog::load_with(&cache, Duration::from_secs(100), 2_000, || {
            Err(CatalogError::Http("offline".to_owned()))
        })
        .expect("stale fallback");
        assert_eq!(offline.source(), CatalogSource::Cache);
        assert_eq!(offline.fetched_at_unix(), 1_000);

        let _ = fs::remove_dir_all(cache.parent().expect("cache parent"));
    }

    #[test]
    fn stale_cache_is_replaced_after_a_successful_fetch() {
        let cache = temp_cache("refresh");
        VersionCatalog::load_with(&cache, Duration::from_secs(100), 1_000, || {
            Ok(MANIFEST_JSON.to_owned())
        })
        .expect("initial fetch");

        let refreshed = VersionCatalog::load_with(&cache, Duration::from_secs(100), 2_000, || {
            Ok(MANIFEST_JSON.to_owned())
        })
        .expect("refresh");
        assert_eq!(refreshed.source(), CatalogSource::Network);
        assert_eq!(refreshed.fetched_at_unix(), 2_000);

        let cached = VersionCatalog::load_with(&cache, Duration::from_secs(100), 2_050, || {
            panic!("refresh must have written a fresh cache")
        })
        .expect("refreshed cache hit");
        assert_eq!(cached.source(), CatalogSource::Cache);
        assert_eq!(cached.fetched_at_unix(), 2_000);

        let _ = fs::remove_dir_all(cache.parent().expect("cache parent"));
    }

    #[test]
    fn missing_cache_and_offline_fetch_is_an_error() {
        let cache = temp_cache("missing");
        let result = VersionCatalog::load_with(&cache, Duration::from_secs(100), 1_000, || {
            Err(CatalogError::Http("offline".to_owned()))
        });
        assert!(matches!(result, Err(CatalogError::Http(message)) if message == "offline"));
    }

    #[test]
    fn explicit_offline_loading_uses_the_cache_regardless_of_age() {
        let cache = temp_cache("explicit-offline");
        assert!(matches!(
            VersionCatalog::load_offline(&cache),
            Err(CatalogError::Offline)
        ));
        VersionCatalog::load_with(&cache, Duration::from_secs(1), 1_000, || {
            Ok(MANIFEST_JSON.to_owned())
        })
        .expect("initial fetch");

        let offline = VersionCatalog::load_offline(&cache).expect("offline cache");
        assert_eq!(offline.source(), CatalogSource::Cache);
        assert_eq!(offline.fetched_at_unix(), 1_000);
        assert_eq!(
            offline.entry("1.21.4").expect("cached version").id.as_str(),
            "1.21.4"
        );
        let _ = fs::remove_dir_all(cache.parent().expect("cache parent"));
    }

    #[test]
    fn only_official_https_hosts_are_allowed() {
        assert!(is_allowed_download_url(
            "https://piston-meta.mojang.com/v1/packages/aaa/1.21.4.json"
        ));
        assert!(is_allowed_download_url(
            "https://launchermeta.mojang.com/mc/game/version_manifest_v2.json"
        ));
        for url in [
            "http://piston-meta.mojang.com/v1/packages/aaa/1.21.4.json",
            "https://piston-meta.mojang.com.evil.example/1.21.4.json",
            "https://evil.example/1.21.4.json",
            "https://user@piston-meta.mojang.com/1.21.4.json",
            "file:///tmp/1.21.4.json",
        ] {
            assert!(!is_allowed_download_url(url), "{url} must be rejected");
        }
    }
}
