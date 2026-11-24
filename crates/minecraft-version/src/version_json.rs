//! Mojang version JSON parsing and verified download.
//!
//! A manifest entry points at a version JSON that names the exact Java major
//! and the client/server downloads. The JSON itself is hash-checked against
//! the manifest before any of those downloads are trusted.

use std::fmt::Write as _;

use download_pipeline::{Allowlist, DownloadClient, NetworkConfig, PipelineError};
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use thiserror::Error;

use crate::catalog::{ManifestEntry, ManifestVersionType};
use crate::VersionId;

/// Upper bound for a version JSON document.
const MAX_VERSION_JSON_BYTES: u64 = 8 * 1024 * 1024;

/// The parts of a Mojang version JSON that FuryBuilder relies on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionJson {
    pub id: VersionId,
    #[serde(rename = "type")]
    pub version_type: ManifestVersionType,
    #[serde(default)]
    pub main_class: Option<String>,
    #[serde(default)]
    pub java_version: Option<JavaVersion>,
    #[serde(default)]
    pub downloads: VersionDownloads,
    #[serde(default)]
    pub asset_index: Option<AssetIndex>,
    #[serde(default)]
    pub time: String,
    #[serde(default)]
    pub release_time: String,
    #[serde(default)]
    pub compliance_level: Option<u8>,
}

impl VersionJson {
    /// Parses and validates a version JSON document.
    pub fn from_json(json: &str) -> Result<Self, VersionJsonError> {
        let version: Self = serde_json::from_str(json)?;
        version.validate()?;
        Ok(version)
    }

    /// Parses a version JSON and verifies that it describes the expected id.
    pub fn from_json_for(json: &str, expected: &str) -> Result<Self, VersionJsonError> {
        let version = Self::from_json(json)?;
        if version.id.as_str() != expected {
            return Err(VersionJsonError::IdMismatch {
                expected: expected.to_owned(),
                actual: version.id.to_string(),
            });
        }
        Ok(version)
    }

    /// Checks download URLs, hashes and the Java major.
    pub fn validate(&self) -> Result<(), VersionJsonError> {
        if let Some(java) = &self.java_version {
            if java.major_version == 0 {
                return Err(VersionJsonError::InvalidJavaVersion);
            }
        }

        if let Some(client) = &self.downloads.client {
            client.validate("client")?;
        }
        if let Some(server) = &self.downloads.server {
            server.validate("server")?;
        }
        if let Some(asset_index) = &self.asset_index {
            asset_index.validate()?;
        }

        Ok(())
    }

    /// The required Java major, if this version declares one.
    pub fn java_major(&self) -> Option<u32> {
        self.java_version.as_ref().map(|java| java.major_version)
    }

    pub fn client(&self) -> Option<&VersionDownload> {
        self.downloads.client.as_ref()
    }

    pub fn server(&self) -> Option<&VersionDownload> {
        self.downloads.server.as_ref()
    }
}

/// A declared Java runtime requirement.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaVersion {
    #[serde(default)]
    pub component: Option<String>,
    pub major_version: u32,
}

/// Client and server artifacts referenced by a version JSON.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionDownloads {
    #[serde(default)]
    pub client: Option<VersionDownload>,
    #[serde(default)]
    pub server: Option<VersionDownload>,
}

/// One hash-verified download from Mojang.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionDownload {
    pub sha1: String,
    pub size: u64,
    pub url: String,
}

impl VersionDownload {
    fn validate(&self, field: &'static str) -> Result<(), VersionJsonError> {
        if !Allowlist::MOJANG.is_allowed(&self.url) {
            return Err(VersionJsonError::DisallowedUrl {
                field,
                url: self.url.clone(),
            });
        }
        if !is_sha1_hex(&self.sha1) {
            return Err(VersionJsonError::InvalidSha1 {
                field,
                value: self.sha1.clone(),
            });
        }
        if self.size == 0 {
            return Err(VersionJsonError::InvalidSize { field });
        }
        Ok(())
    }
}

/// The asset index referenced by a version JSON.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetIndex {
    pub id: String,
    pub sha1: String,
    pub size: u64,
    pub total_size: u64,
    pub url: String,
}

impl AssetIndex {
    fn validate(&self) -> Result<(), VersionJsonError> {
        if !Allowlist::MOJANG.is_allowed(&self.url) {
            return Err(VersionJsonError::DisallowedUrl {
                field: "assetIndex",
                url: self.url.clone(),
            });
        }
        if !is_sha1_hex(&self.sha1) {
            return Err(VersionJsonError::InvalidSha1 {
                field: "assetIndex",
                value: self.sha1.clone(),
            });
        }
        if self.size == 0 {
            return Err(VersionJsonError::InvalidSize {
                field: "assetIndex",
            });
        }
        Ok(())
    }
}

/// Fetches, hash-verifies and parses the version JSON behind a manifest entry.
pub fn fetch_version_json(entry: &ManifestEntry) -> Result<VersionJson, VersionJsonError> {
    fetch_version_json_document(entry).map(|(version, _)| version)
}

/// Fetches the version JSON and also returns the exact verified bytes. The bytes are
/// useful as launcher metadata next to a downloaded client JAR.
pub fn fetch_version_json_document(
    entry: &ManifestEntry,
) -> Result<(VersionJson, Vec<u8>), VersionJsonError> {
    let client = DownloadClient::new(NetworkConfig::default());
    fetch_version_json_document_with(entry, &client)
}

/// Fetches the version JSON through a caller-owned network policy.
pub fn fetch_version_json_document_with(
    entry: &ManifestEntry,
    client: &DownloadClient,
) -> Result<(VersionJson, Vec<u8>), VersionJsonError> {
    let bytes = fetch_bytes(entry, client)?;

    if !entry.sha1.is_empty() {
        let actual = sha1_hex(&bytes);
        if !actual.eq_ignore_ascii_case(&entry.sha1) {
            return Err(VersionJsonError::Sha1Mismatch {
                expected: entry.sha1.clone(),
                actual,
            });
        }
    }

    let json = std::str::from_utf8(&bytes)?;
    let version = VersionJson::from_json_for(json, &entry.id)?;
    Ok((version, bytes))
}

fn fetch_bytes(
    entry: &ManifestEntry,
    client: &DownloadClient,
) -> Result<Vec<u8>, VersionJsonError> {
    client
        .get_bytes(&entry.url, &Allowlist::MOJANG, MAX_VERSION_JSON_BYTES)
        .map_err(version_pipeline_error)
}

fn version_pipeline_error(error: PipelineError) -> VersionJsonError {
    let message = error.to_string();
    match error {
        PipelineError::Offline => VersionJsonError::Offline,
        PipelineError::DisallowedUrl(url) => VersionJsonError::DisallowedUrl {
            field: "versionJson",
            url,
        },
        PipelineError::Limit(_) | PipelineError::Size { .. } => VersionJsonError::TooLarge,
        PipelineError::InsufficientSpace { .. } | PipelineError::Io(_) => {
            VersionJsonError::Io(std::io::Error::other(message))
        }
        other => VersionJsonError::Http(other.to_string()),
    }
}

fn sha1_hex(bytes: &[u8]) -> String {
    let digest = Sha1::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn is_sha1_hex(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Errors from version JSON parsing, validation or download.
#[derive(Debug, Error)]
pub enum VersionJsonError {
    #[error("failed to fetch the Minecraft version JSON: {0}")]
    Http(String),
    #[error("network access is disabled (offline mode)")]
    Offline,
    #[error("version JSON I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid version JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("version JSON is not valid UTF-8: {0}")]
    Utf8(#[from] std::str::Utf8Error),
    #[error("download URL for {field} is not allowed: {url}")]
    DisallowedUrl { field: &'static str, url: String },
    #[error("{field} download has an invalid SHA-1: {value}")]
    InvalidSha1 { field: &'static str, value: String },
    #[error("{field} download has an invalid size")]
    InvalidSize { field: &'static str },
    #[error("version JSON SHA-1 mismatch: expected {expected}, got {actual}")]
    Sha1Mismatch { expected: String, actual: String },
    #[error("version JSON id mismatch: expected {expected}, got {actual}")]
    IdMismatch { expected: String, actual: String },
    #[error("version JSON declares an invalid Java major")]
    InvalidJavaVersion,
    #[error("version JSON is larger than the allowed limit")]
    TooLarge,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::VersionManifest;

    const VERSION_JSON: &str = r#"{
      "id": "1.21.4",
      "type": "release",
      "mainClass": "net.minecraft.client.main.Main",
      "javaVersion": {
        "component": "java-runtime-delta",
        "majorVersion": 21
      },
      "time": "2024-12-03T10:12:57+00:00",
      "releaseTime": "2024-12-03T10:12:57+00:00",
      "complianceLevel": 1,
      "downloads": {
        "client": {
          "sha1": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
          "size": 123456,
          "url": "https://piston-data.mojang.com/v1/objects/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/client.jar"
        },
        "server": {
          "sha1": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
          "size": 654321,
          "url": "https://piston-data.mojang.com/v1/objects/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb/server.jar"
        }
      },
      "assetIndex": {
        "id": "17",
        "sha1": "cccccccccccccccccccccccccccccccccccccccc",
        "size": 789,
        "totalSize": 123456789,
        "url": "https://piston-meta.mojang.com/v1/packages/cccccccccccccccccccccccccccccccccccccccc/17.json"
      }
    }"#;

    const OLD_VERSION_JSON: &str = r#"{
      "id": "a1.2.6",
      "type": "old_alpha",
      "time": "2010-12-21T00:00:00+00:00",
      "releaseTime": "2010-12-21T00:00:00+00:00",
      "downloads": {}
    }"#;

    #[test]
    fn parses_java_and_verified_downloads() {
        let json = VersionJson::from_json(VERSION_JSON).expect("valid version json");

        assert_eq!(json.id.as_str(), "1.21.4");
        assert_eq!(json.version_type, ManifestVersionType::Release);
        assert_eq!(json.java_major(), Some(21));
        assert_eq!(
            json.java_version
                .as_ref()
                .and_then(|java| java.component.as_deref()),
            Some("java-runtime-delta")
        );
        assert_eq!(json.client().expect("client").size, 123456);
        assert_eq!(json.server().expect("server").size, 654321);
        assert_eq!(json.asset_index.as_ref().expect("asset index").id, "17");
    }

    #[test]
    fn verifies_the_expected_version_id() {
        let mismatched = VERSION_JSON.replace("\"id\": \"1.21.4\"", "\"id\": \"1.21.5\"");
        let result = VersionJson::from_json_for(&mismatched, "1.21.4");
        assert!(matches!(
            result,
            Err(VersionJsonError::IdMismatch { expected, actual })
                if expected == "1.21.4" && actual == "1.21.5"
        ));
    }

    #[test]
    fn allows_old_versions_without_java_or_downloads() {
        let json = VersionJson::from_json(OLD_VERSION_JSON).expect("old version json");

        assert_eq!(json.id.as_str(), "a1.2.6");
        assert_eq!(json.version_type, ManifestVersionType::OldAlpha);
        assert_eq!(json.java_major(), None);
        assert_eq!(json.client(), None);
        assert_eq!(json.server(), None);
        assert_eq!(json.asset_index, None);
    }

    #[test]
    fn offline_mode_refuses_version_json_before_any_request() {
        let entry = ManifestEntry {
            id: "1.21.4".to_owned(),
            version_type: ManifestVersionType::Release,
            url: "https://piston-meta.mojang.com/v1/packages/aaa/1.21.4.json".to_owned(),
            time: "2024-12-03T10:12:57+00:00".to_owned(),
            release_time: "2024-12-03T10:12:57+00:00".to_owned(),
            sha1: "a".repeat(40),
            compliance_level: Some(1),
        };
        let client = DownloadClient::new(NetworkConfig::offline());
        let error = fetch_version_json_document_with(&entry, &client)
            .expect_err("offline mode must refuse the request");
        assert!(matches!(error, VersionJsonError::Offline));
    }

    #[test]
    fn rejects_download_urls_outside_the_allowlist() {
        let bad_url = VERSION_JSON.replace(
            "https://piston-data.mojang.com/v1/objects/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/client.jar",
            "https://evil.example/client.jar",
        );
        let result = VersionJson::from_json(&bad_url);
        assert!(matches!(
            result,
            Err(VersionJsonError::DisallowedUrl {
                field: "client",
                ..
            })
        ));

        let bad_asset_url = VERSION_JSON.replace(
            "https://piston-meta.mojang.com/v1/packages/cccccccccccccccccccccccccccccccccccccccc/17.json",
            "https://evil.example/17.json",
        );
        let result = VersionJson::from_json(&bad_asset_url);
        assert!(matches!(
            result,
            Err(VersionJsonError::DisallowedUrl {
                field: "assetIndex",
                ..
            })
        ));
    }

    #[test]
    fn rejects_invalid_hashes_sizes_and_java_major() {
        let short_hash = VERSION_JSON.replace(
            "cccccccccccccccccccccccccccccccccccccccc",
            "ccccccccccccccccccccccccccccccccccccccc",
        );
        assert!(matches!(
            VersionJson::from_json(&short_hash),
            Err(VersionJsonError::InvalidSha1 {
                field: "assetIndex",
                ..
            })
        ));

        let zero_size = VERSION_JSON.replace("\"size\": 123456", "\"size\": 0");
        assert!(matches!(
            VersionJson::from_json(&zero_size),
            Err(VersionJsonError::InvalidSize { field: "client" })
        ));

        let zero_java = VERSION_JSON.replace("\"majorVersion\": 21", "\"majorVersion\": 0");
        assert!(matches!(
            VersionJson::from_json(&zero_java),
            Err(VersionJsonError::InvalidJavaVersion)
        ));
    }

    #[test]
    fn allows_official_download_hosts() {
        assert!(Allowlist::MOJANG
            .is_allowed("https://piston-data.mojang.com/v1/objects/aaa/client.jar"));
        assert!(
            Allowlist::MOJANG.is_allowed("https://launcher.mojang.com/v1/objects/aaa/server.jar")
        );
        assert!(Allowlist::MOJANG
            .is_allowed("https://piston-meta.mojang.com/v1/packages/aaa/1.21.4.json"));
        assert!(
            !Allowlist::MOJANG.is_allowed("https://piston-data.mojang.com.evil.example/client.jar")
        );
    }

    #[test]
    fn manifest_and_version_json_share_the_version_model() {
        let manifest = VersionManifest::from_json(
            r#"{
              "latest": { "release": "1.21.4", "snapshot": "1.21.4" },
              "versions": [{
                "id": "1.21.4",
                "type": "release",
                "url": "https://piston-meta.mojang.com/v1/packages/aaa/1.21.4.json",
                "time": "2024-12-03T10:12:57+00:00",
                "releaseTime": "2024-12-03T10:12:57+00:00",
                "sha1": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
              }]
            }"#,
        )
        .expect("manifest");
        let entry = &manifest.versions[0];
        let parsed = VersionJson::from_json_for(VERSION_JSON, &entry.id).expect("version json");
        assert_eq!(parsed.id.as_str(), entry.id);
    }
}
