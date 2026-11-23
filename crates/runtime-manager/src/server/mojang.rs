use std::fs;
use std::path::{Path, PathBuf};

use download_pipeline::{Allowlist, Checksum, DownloadClient, DownloadProgress, DownloadSpec};
use minecraft_version::{
    fetch_version_json_document_with, is_allowed_download_url, CatalogError, ManifestEntry,
    ManifestVersionType, VersionCatalog, VersionJson, VersionJsonError, DEFAULT_MANIFEST_TTL,
};

use crate::error::ServerError;

use super::jar::is_sha1;
use super::types::{
    ServerProgress, ServerReleaseInfo, ServerTarget, MAX_SERVER_JAR_BYTES, MIN_SERVER_JAR_BYTES,
};

pub const VERSION_MANIFEST_URL: &str = minecraft_version::OFFICIAL_MANIFEST_URL;

pub struct MojangClient {
    client: DownloadClient,
    manifest_cache: PathBuf,
}

impl MojangClient {
    pub fn with_client(manifest_cache: impl Into<PathBuf>, client: DownloadClient) -> Self {
        Self {
            client,
            manifest_cache: manifest_cache.into(),
        }
    }

    /// Resolves exactly the requested version through the official manifest and that
    /// version's JSON. The version JSON body is checked against the manifest's SHA-1 before
    /// it is parsed. A snapshot or a version without a server download is refused.
    pub fn release(&self, target: &ServerTarget) -> Result<ServerReleaseInfo, ServerError> {
        let catalog = VersionCatalog::load_with_client(
            &self.manifest_cache,
            DEFAULT_MANIFEST_TTL,
            &self.client,
        )
        .map_err(catalog_error)?;
        let entry = catalog
            .entry(target.minecraft_version.as_str())
            .ok_or_else(|| {
                unsupported_server(
                    target,
                    format!(
                        "Minecraft {} is not present in the Mojang version manifest",
                        target.minecraft_version
                    ),
                )
            })?;
        let (version_json, _) =
            fetch_version_json_document_with(entry, &self.client).map_err(version_json_error)?;
        release_from_version_json(entry, &version_json, target)
    }

    /// Downloads the announced server JAR with a hard size cap and streaming SHA-1. The
    /// destination is replaced atomically only after the checksum matches.
    pub fn download(
        &self,
        release: &ServerReleaseInfo,
        destination: &Path,
        cancel: &std::sync::atomic::AtomicBool,
        progress: &dyn Fn(ServerProgress),
    ) -> Result<(), ServerError> {
        self.client.download(
            &DownloadSpec {
                url: &release.download_url,
                allowlist: &Allowlist::MOJANG,
                destination,
                expected_size: release.size,
                checksum: Checksum::Sha1(&release.sha1),
                max_size: MAX_SERVER_JAR_BYTES,
            },
            cancel,
            &|event: DownloadProgress| {
                progress(ServerProgress {
                    phase: event.phase.as_str().to_owned(),
                    downloaded: event.downloaded,
                    total: event.total,
                });
            },
        )?;
        Ok(())
    }
}

/// Turns one verified version JSON into the exact server download preview. This is a pure
/// function so exact-version selection can be tested without HTTP.
pub(crate) fn release_from_version_json(
    entry: &ManifestEntry,
    version: &VersionJson,
    target: &ServerTarget,
) -> Result<ServerReleaseInfo, ServerError> {
    version.validate().map_err(version_json_error)?;
    let expected = target.minecraft_version.as_str();
    if entry.version_type != ManifestVersionType::Release
        || version.version_type != ManifestVersionType::Release
    {
        return Err(unsupported_server(
            target,
            format!("Minecraft {expected} is not a release build; server generation is read-only for snapshots"),
        ));
    }
    if entry.id.as_str() != expected || version.id.as_str() != expected {
        return Err(unsupported_server(
            target,
            format!(
                "Mojang returned version {} / {}, expected {expected}",
                entry.id, version.id
            ),
        ));
    }
    let java_major = version.java_major().ok_or_else(|| {
        unsupported_server(
            target,
            "the Mojang version JSON declares no Java version".to_owned(),
        )
    })?;
    if java_major != target.java_major {
        return Err(unsupported_server(
            target,
            format!(
                "the version requires Java {java_major}, expected Java {}",
                target.java_major
            ),
        ));
    }
    let server = version.server().ok_or_else(|| {
        unsupported_server(
            target,
            "the Mojang version JSON has no server download".to_owned(),
        )
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
    if !is_allowed_download_url(&server.url) {
        return Err(ServerError::Network(format!(
            "server JAR URL is not an allowed Mojang host: {}",
            server.url
        )));
    }
    Ok(ServerReleaseInfo {
        minecraft_version: version.id.to_string(),
        version_type: version_type_label(version.version_type),
        release_time: version.release_time.clone(),
        manifest_url: VERSION_MANIFEST_URL.to_owned(),
        version_json_sha1: entry.sha1.to_ascii_lowercase(),
        download_url: server.url.clone(),
        sha1: server.sha1.to_ascii_lowercase(),
        size: server.size,
        java_major,
        expected_data_version: target.data_version,
        source: "Mojang version_manifest_v2".to_owned(),
    })
}

fn version_type_label(kind: ManifestVersionType) -> String {
    match kind {
        ManifestVersionType::Release => "release",
        ManifestVersionType::Snapshot => "snapshot",
        ManifestVersionType::OldBeta => "old_beta",
        ManifestVersionType::OldAlpha => "old_alpha",
    }
    .to_owned()
}

fn unsupported_server(target: &ServerTarget, detail: impl Into<String>) -> ServerError {
    ServerError::Unsupported {
        target: target.minecraft_version.to_string(),
        detail: detail.into(),
    }
}

fn catalog_error(error: CatalogError) -> ServerError {
    match error {
        CatalogError::Offline => ServerError::Offline,
        other => ServerError::Network(format!("Mojang version manifest: {other}")),
    }
}

fn version_json_error(error: VersionJsonError) -> ServerError {
    match error {
        VersionJsonError::Offline => ServerError::Offline,
        VersionJsonError::Sha1Mismatch { .. }
        | VersionJsonError::InvalidSha1 { .. }
        | VersionJsonError::InvalidSize { .. } => ServerError::Checksum(error.to_string()),
        other => ServerError::Network(format!("Mojang version JSON: {other}")),
    }
}

/// Removes a partial server JAR after a failed install.
pub fn remove_file_if_exists(path: &Path) {
    let _ = fs::remove_file(path);
}

#[cfg(test)]
mod tests {
    use super::*;
    use minecraft_version::VersionId;

    fn target(raw: &str, data_version: i32, java_major: u32) -> ServerTarget {
        ServerTarget::new(
            raw.parse::<VersionId>().expect("version"),
            data_version,
            java_major,
        )
        .expect("target")
    }

    fn entry(raw: &str, kind: ManifestVersionType) -> ManifestEntry {
        ManifestEntry {
            id: raw.parse().expect("version"),
            version_type: kind,
            url: format!("https://piston-meta.mojang.com/v1/packages/{raw}.json"),
            time: "2024-12-03T10:12:57+00:00".to_owned(),
            release_time: "2024-12-03T10:12:57+00:00".to_owned(),
            sha1: "c88775961e316f2be09859c54e13017660ee7770".to_owned(),
            compliance_level: Some(1),
        }
    }

    fn version(raw: &str, java_major: u32, server: bool) -> VersionJson {
        let downloads = if server {
            serde_json::json!({
                "server": {
                    "sha1": "4707d00eb834b446575d89a61a11b5d548d8c001",
                    "size": 56_880_250,
                    "url": "https://piston-data.mojang.com/v1/objects/4707d00eb834b446575d89a61a11b5d548d8c001/server.jar"
                }
            })
        } else {
            serde_json::json!({})
        };
        VersionJson::from_json(
            &serde_json::json!({
                "id": raw,
                "type": "release",
                "releaseTime": "2024-12-03T10:12:57+00:00",
                "javaVersion": { "component": "java-runtime-delta", "majorVersion": java_major },
                "downloads": downloads
            })
            .to_string(),
        )
        .expect("version json")
    }

    #[test]
    fn selects_the_exact_version_and_server_download() {
        let release = release_from_version_json(
            &entry("1.21.4", ManifestVersionType::Release),
            &version("1.21.4", 21, true),
            &target("1.21.4", 4189, 21),
        )
        .expect("release");
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
    fn resolves_a_second_exact_version_without_using_the_baseline() {
        let release = release_from_version_json(
            &entry("1.20.6", ManifestVersionType::Release),
            &version("1.20.6", 21, true),
            &target("1.20.6", 3839, 21),
        )
        .expect("release");
        assert_eq!(release.minecraft_version, "1.20.6");
        assert_eq!(release.expected_data_version, 3839);
    }

    #[test]
    fn refuses_a_snapshot_a_missing_server_a_wrong_version_and_wrong_java() {
        let snapshot_target = target("26.1-snapshot-3", 4500, 21);
        assert!(release_from_version_json(
            &entry("26.1-snapshot-3", ManifestVersionType::Snapshot),
            &version("26.1-snapshot-3", 21, true),
            &snapshot_target,
        )
        .is_err());

        assert!(release_from_version_json(
            &entry("1.21.4", ManifestVersionType::Release),
            &version("1.21.4", 21, false),
            &target("1.21.4", 4189, 21),
        )
        .is_err());

        assert!(release_from_version_json(
            &entry("1.21.4", ManifestVersionType::Release),
            &version("1.21.4", 21, true),
            &target("1.20.6", 3839, 21),
        )
        .is_err());

        assert!(release_from_version_json(
            &entry("1.21.4", ManifestVersionType::Release),
            &version("1.21.4", 17, true),
            &target("1.21.4", 4189, 21),
        )
        .is_err());
    }

    #[test]
    fn refuses_an_announced_size_outside_the_server_range() {
        let mut json = version("1.21.4", 21, true);
        json.downloads.server.as_mut().expect("server").size = 1024;
        assert!(release_from_version_json(
            &entry("1.21.4", ManifestVersionType::Release),
            &json,
            &target("1.21.4", 4189, 21),
        )
        .is_err());
    }
}
