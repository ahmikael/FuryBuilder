use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;

use serde::Deserialize;
use sha1::{Digest, Sha1};

use crate::error::{unsupported_server, ServerError};
use crate::managed::hex;
use crate::net;
use crate::types::{TARGET_JAVA_MAJOR, TARGET_MINECRAFT_VERSION};

use super::jar::is_sha1;
use super::types::{
    ServerProgress, ServerReleaseInfo, MAX_SERVER_JAR_BYTES, MIN_SERVER_JAR_BYTES,
    TARGET_DATA_VERSION,
};

pub const VERSION_MANIFEST_URL: &str =
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const MOJANG_HOSTS: &[&str] = &[
    "piston-meta.mojang.com",
    "launchermeta.mojang.com",
    "piston-data.mojang.com",
];
const MAX_METADATA_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Deserialize)]
struct VersionManifest {
    versions: Vec<ManifestEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ManifestEntry {
    pub id: String,
    #[serde(rename = "type")]
    pub version_type: String,
    pub url: String,
    pub sha1: String,
}

#[derive(Debug, Deserialize)]
struct VersionDetails {
    id: String,
    #[serde(rename = "type")]
    version_type: String,
    #[serde(rename = "releaseTime")]
    release_time: String,
    #[serde(rename = "javaVersion")]
    java_version: Option<JavaVersion>,
    downloads: Option<Downloads>,
}

#[derive(Debug, Deserialize)]
struct JavaVersion {
    #[serde(rename = "majorVersion")]
    major_version: u32,
}

#[derive(Debug, Deserialize)]
struct Downloads {
    server: Option<DownloadInfo>,
}

#[derive(Debug, Deserialize)]
struct DownloadInfo {
    sha1: String,
    size: u64,
    url: String,
}

pub struct MojangClient {
    agent: ureq::Agent,
}

impl Default for MojangClient {
    fn default() -> Self {
        Self::new()
    }
}

impl MojangClient {
    pub fn new() -> Self {
        Self {
            agent: net::agent(),
        }
    }

    /// Resolves exactly `1.21.4` through the official manifest and that version's JSON.
    /// The version JSON body is checked against the manifest's SHA-1 before it is parsed.
    pub fn release(&self) -> Result<ServerReleaseInfo, ServerError> {
        let manifest_body = self.fetch(VERSION_MANIFEST_URL)?;
        let entry = parse_manifest(&manifest_body, TARGET_MINECRAFT_VERSION)?;
        let version_body = self.fetch(&entry.url)?;
        let actual = sha1_bytes(&version_body);
        if !actual.eq_ignore_ascii_case(&entry.sha1) {
            return Err(ServerError::Checksum(format!(
                "Mojang version JSON SHA-1 mismatch: manifest announced {}, received {actual}",
                entry.sha1
            )));
        }
        parse_version_json(&version_body, &entry)
    }

    /// Downloads the announced server JAR with a hard size cap and streaming SHA-1. The file
    /// is written once to the caller's temporary path; it is not published here.
    pub fn download(
        &self,
        release: &ServerReleaseInfo,
        destination: &Path,
        progress: &dyn Fn(ServerProgress),
    ) -> Result<(), ServerError> {
        net::check_url(&release.download_url, MOJANG_HOSTS).map_err(ServerError::Network)?;
        let response = net::open(&self.agent, &release.download_url, MOJANG_HOSTS)
            .map_err(ServerError::Network)?;
        if response.status() != 200 {
            return Err(ServerError::Network(format!(
                "server JAR download returned HTTP {}",
                response.status()
            )));
        }
        if let Some(length) = response.header("content-length") {
            if let Ok(length) = length.parse::<u64>() {
                if length != release.size {
                    return Err(ServerError::Checksum(format!(
                        "announced size {} does not match response size {length}",
                        release.size
                    )));
                }
            }
        }
        let mut reader = response.into_reader();
        let mut file = File::create(destination).map_err(|error| {
            ServerError::Io(format!("cannot create {}: {error}", destination.display()))
        })?;
        let mut hasher = Sha1::new();
        let mut buffer = [0u8; 64 * 1024];
        let mut downloaded = 0u64;
        let mut last_report = 0u64;
        loop {
            let read = reader.read(&mut buffer).map_err(|error| {
                ServerError::Network(format!("server JAR download interrupted: {error}"))
            })?;
            if read == 0 {
                break;
            }
            downloaded = downloaded.saturating_add(read as u64);
            if downloaded > MAX_SERVER_JAR_BYTES {
                return Err(ServerError::Limit(format!(
                    "server JAR download exceeds {MAX_SERVER_JAR_BYTES} bytes"
                )));
            }
            hasher.update(&buffer[..read]);
            file.write_all(&buffer[..read])
                .map_err(|error| ServerError::Io(format!("cannot write download: {error}")))?;
            if downloaded - last_report >= 512 * 1024 || downloaded == release.size {
                last_report = downloaded;
                progress(ServerProgress {
                    phase: "downloading".to_owned(),
                    downloaded,
                    total: release.size,
                });
            }
        }
        file.sync_all()
            .map_err(|error| ServerError::Io(format!("cannot flush download: {error}")))?;
        if downloaded != release.size {
            return Err(ServerError::Checksum(format!(
                "received {downloaded} bytes, expected {}",
                release.size
            )));
        }
        let actual = hex(&hasher.finalize());
        if !actual.eq_ignore_ascii_case(&release.sha1) {
            return Err(ServerError::Checksum(format!(
                "server JAR SHA-1 mismatch: Mojang announced {}, received {actual}",
                release.sha1
            )));
        }
        progress(ServerProgress {
            phase: "verified".to_owned(),
            downloaded,
            total: release.size,
        });
        Ok(())
    }

    fn fetch(&self, url: &str) -> Result<Vec<u8>, ServerError> {
        net::check_url(url, MOJANG_HOSTS).map_err(ServerError::Network)?;
        let response = net::open(&self.agent, url, MOJANG_HOSTS).map_err(ServerError::Network)?;
        if response.status() != 200 {
            return Err(ServerError::Network(format!(
                "Mojang metadata returned HTTP {}",
                response.status()
            )));
        }
        let mut body = Vec::new();
        response
            .into_reader()
            .take(MAX_METADATA_BYTES + 1)
            .read_to_end(&mut body)
            .map_err(|error| {
                ServerError::Network(format!("cannot read Mojang metadata: {error}"))
            })?;
        if body.len() as u64 > MAX_METADATA_BYTES {
            return Err(ServerError::Limit(
                "Mojang metadata is larger than 8 MiB".to_owned(),
            ));
        }
        Ok(body)
    }
}

pub(crate) fn parse_manifest(body: &[u8], target: &str) -> Result<ManifestEntry, ServerError> {
    let manifest: VersionManifest = serde_json::from_slice(body).map_err(|error| {
        ServerError::Network(format!("invalid Mojang version manifest: {error}"))
    })?;
    let entry = manifest
        .versions
        .into_iter()
        .find(|entry| entry.id == target)
        .ok_or_else(|| {
            unsupported_server(format!(
                "Minecraft {target} is not present in the Mojang version manifest"
            ))
        })?;
    if entry.version_type != "release" {
        return Err(unsupported_server(format!(
            "Minecraft {target} is a {} build, not a release",
            entry.version_type
        )));
    }
    if !is_sha1(&entry.sha1) {
        return Err(ServerError::Checksum(
            "Mojang manifest announced an invalid version JSON SHA-1".to_owned(),
        ));
    }
    net::check_url(&entry.url, MOJANG_HOSTS).map_err(ServerError::Network)?;
    Ok(entry)
}

pub(crate) fn parse_version_json(
    body: &[u8],
    entry: &ManifestEntry,
) -> Result<ServerReleaseInfo, ServerError> {
    let details: VersionDetails = serde_json::from_slice(body)
        .map_err(|error| ServerError::Network(format!("invalid Mojang version JSON: {error}")))?;
    if details.id != TARGET_MINECRAFT_VERSION || entry.id != TARGET_MINECRAFT_VERSION {
        return Err(unsupported_server(format!(
            "Mojang returned version {}, expected {}",
            details.id, TARGET_MINECRAFT_VERSION
        )));
    }
    if details.version_type != "release" {
        return Err(unsupported_server(format!(
            "Minecraft {} is a {} build, not a release",
            details.id, details.version_type
        )));
    }
    let java_major = details
        .java_version
        .map(|java| java.major_version)
        .ok_or_else(|| {
            unsupported_server("the Mojang version JSON declares no Java version".to_owned())
        })?;
    if java_major != TARGET_JAVA_MAJOR {
        return Err(unsupported_server(format!(
            "the version requires Java {java_major}, expected Java {TARGET_JAVA_MAJOR}"
        )));
    }
    let server = details
        .downloads
        .and_then(|downloads| downloads.server)
        .ok_or_else(|| {
            unsupported_server("the Mojang version JSON has no server download".to_owned())
        })?;
    if !is_sha1(&server.sha1) {
        return Err(ServerError::Checksum(
            "Mojang announced an invalid server JAR SHA-1".to_owned(),
        ));
    }
    if !(MIN_SERVER_JAR_BYTES..=MAX_SERVER_JAR_BYTES).contains(&server.size) {
        return Err(ServerError::Limit(format!(
            "Mojang announced server JAR size {}, outside the accepted range",
            server.size
        )));
    }
    net::check_url(&server.url, MOJANG_HOSTS).map_err(ServerError::Network)?;
    Ok(ServerReleaseInfo {
        minecraft_version: details.id,
        version_type: details.version_type,
        release_time: details.release_time,
        manifest_url: VERSION_MANIFEST_URL.to_owned(),
        version_json_sha1: entry.sha1.to_ascii_lowercase(),
        download_url: server.url,
        sha1: server.sha1.to_ascii_lowercase(),
        size: server.size,
        java_major,
        expected_data_version: TARGET_DATA_VERSION,
        source: "Mojang version_manifest_v2".to_owned(),
    })
}

fn sha1_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(bytes);
    hex(&hasher.finalize())
}

/// Removes a partial server JAR after a failed install.
pub fn remove_file_if_exists(path: &Path) {
    let _ = fs::remove_file(path);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest_body() -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "latest": { "release": "1.21.4", "snapshot": "26.3" },
            "versions": [
                {
                    "id": "26.3",
                    "type": "snapshot",
                    "url": "https://piston-meta.mojang.com/v1/packages/aaa/26.3.json",
                    "time": "2026-11-18T10:13:31+00:00",
                    "releaseTime": "2026-11-18T10:13:31+00:00",
                    "sha1": "a".repeat(40),
                    "complianceLevel": 1
                },
                {
                    "id": "1.21.4",
                    "type": "release",
                    "url": "https://piston-meta.mojang.com/v1/packages/c887/1.21.4.json",
                    "time": "2026-11-18T10:13:31+00:00",
                    "releaseTime": "2024-12-03T10:12:57+00:00",
                    "sha1": "c88775961e316f2be09859c54e13017660ee7770",
                    "complianceLevel": 1
                }
            ]
        }))
        .expect("manifest json")
    }

    fn version_body() -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "id": "1.21.4",
            "type": "release",
            "releaseTime": "2024-12-03T10:12:57+00:00",
            "javaVersion": { "component": "java-runtime-delta", "majorVersion": 21 },
            "downloads": {
                "server": {
                    "sha1": "4707d00eb834b446575d89a61a11b5d548d8c001",
                    "size": 56880250,
                    "url": "https://piston-data.mojang.com/v1/objects/4707d00eb834b446575d89a61a11b5d548d8c001/server.jar"
                }
            }
        }))
        .expect("version json")
    }

    #[test]
    fn selects_the_exact_release_and_server_download() {
        let entry = parse_manifest(&manifest_body(), "1.21.4").expect("manifest entry");
        let release = parse_version_json(&version_body(), &entry).expect("release");
        assert_eq!(release.minecraft_version, "1.21.4");
        assert_eq!(release.sha1, "4707d00eb834b446575d89a61a11b5d548d8c001");
        assert_eq!(release.size, 56_880_250);
        assert_eq!(release.java_major, 21);
        assert_eq!(release.expected_data_version, 4189);
        assert_eq!(release.version_type, "release");
        assert!(release
            .download_url
            .starts_with("https://piston-data.mojang.com/"));
    }

    #[test]
    fn refuses_wrong_versions_hosts_sizes_and_checksums() {
        assert!(parse_manifest(&manifest_body(), "1.20.1").is_err());

        let mut wrong_type = manifest_body();
        let text = String::from_utf8(wrong_type.clone()).expect("utf8");
        wrong_type = text
            .replace("\"type\":\"release\"", "\"type\":\"snapshot\"")
            .into_bytes();
        assert!(parse_manifest(&wrong_type, "1.21.4").is_err());

        let mut wrong_host = version_body();
        let text = String::from_utf8(wrong_host.clone()).expect("utf8");
        wrong_host = text
            .replace("https://piston-data.mojang.com", "https://example.com")
            .into_bytes();
        let entry = parse_manifest(&manifest_body(), "1.21.4").expect("entry");
        assert!(parse_version_json(&wrong_host, &entry).is_err());

        let mut tiny = version_body();
        let text = String::from_utf8(tiny.clone()).expect("utf8");
        tiny = text.replace("56880250", "1024").into_bytes();
        assert!(parse_version_json(&tiny, &entry).is_err());

        let mut invalid_hash = manifest_body();
        let text = String::from_utf8(invalid_hash.clone()).expect("utf8");
        invalid_hash = text
            .replace("c88775961e316f2be09859c54e13017660ee7770", "not-a-sha1")
            .into_bytes();
        assert!(parse_manifest(&invalid_hash, "1.21.4").is_err());
    }

    #[test]
    fn computes_the_known_version_json_sha1() {
        // The real official manifest entry hashes the real version JSON body.
        let body = version_body();
        assert_eq!(sha1_bytes(&body).len(), 40);
        assert!(is_sha1(&sha1_bytes(&body)));
    }
}
