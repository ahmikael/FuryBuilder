use std::fs;
use std::path::Path;

use download_pipeline::{
    Allowlist, Checksum, DownloadClient, DownloadProgress, DownloadSpec, NetworkConfig,
};
use serde::Deserialize;

use crate::error::RuntimeError;
use crate::types::{RuntimeProgress, RuntimeReleaseInfo};

const API_BASE: &str = "https://api.adoptium.net/v3";
const MAX_METADATA_BYTES: u64 = 1024 * 1024;
const MAX_DOWNLOAD_BYTES: u64 = 200 * 1024 * 1024;
const MIN_DOWNLOAD_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Debug, Deserialize)]
struct AdoptiumRelease {
    release_name: String,
    vendor: String,
    version: AdoptiumVersion,
    binary: AdoptiumBinary,
}

#[derive(Debug, Deserialize)]
struct AdoptiumVersion {
    major: u32,
    semver: String,
}

#[derive(Debug, Deserialize)]
struct AdoptiumBinary {
    os: String,
    architecture: String,
    image_type: String,
    package: AdoptiumPackage,
}

#[derive(Debug, Deserialize)]
struct AdoptiumPackage {
    link: String,
    checksum: String,
    size: u64,
    name: String,
}

/// Client for the official Adoptium API. Only metadata and the package URL announced by
/// that API are accepted; the download host is allowlisted separately.
pub struct AdoptiumClient {
    client: DownloadClient,
}

impl Default for AdoptiumClient {
    fn default() -> Self {
        Self::new()
    }
}

impl AdoptiumClient {
    pub fn new() -> Self {
        Self::with_client(DownloadClient::new(NetworkConfig::default()))
    }

    pub fn with_client(client: DownloadClient) -> Self {
        Self { client }
    }

    pub fn latest(&self, major: u32) -> Result<RuntimeReleaseInfo, RuntimeError> {
        crate::validate_java_major(major)?;
        let (os, architecture) = platform()?;
        let image_type = temurin_image_type(major);
        let url = format!(
            "{API_BASE}/assets/latest/{major}/hotspot?architecture={architecture}&image_type={image_type}&os={os}&vendor=eclipse&project=jdk"
        );
        let body = self
            .client
            .get_bytes(&url, &Allowlist::ADOPTIUM, MAX_METADATA_BYTES)?;
        parse_latest_release(&body, &os, &architecture, major)
    }

    /// Streams the announced archive with a hard size cap and SHA-256 verification. The
    /// destination is replaced atomically after the checksum matches.
    pub fn download(
        &self,
        release: &RuntimeReleaseInfo,
        destination: &Path,
        cancel: &std::sync::atomic::AtomicBool,
        progress: &dyn Fn(RuntimeProgress),
    ) -> Result<(), RuntimeError> {
        self.client.download(
            &DownloadSpec {
                url: &release.download_url,
                allowlist: &Allowlist::GITHUB_RELEASES,
                destination,
                expected_size: release.size,
                checksum: Checksum::Sha256(&release.sha256),
                max_size: MAX_DOWNLOAD_BYTES,
            },
            cancel,
            &|event: DownloadProgress| {
                progress(RuntimeProgress {
                    phase: event.phase.as_str().to_owned(),
                    downloaded: event.downloaded,
                    total: event.total,
                });
            },
        )?;
        Ok(())
    }
}

pub fn release_id(release: &RuntimeReleaseInfo) -> String {
    let raw = format!(
        "temurin-{}-{}-{}",
        release.release_name.trim_start_matches("jdk-"),
        release.os,
        release.arch
    );
    raw.chars()
        .map(|character| {
            let character = character.to_ascii_lowercase();
            if character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '-' | '_' | '.' | '+')
            {
                character
            } else {
                '-'
            }
        })
        .collect()
}

fn platform() -> Result<(String, String), RuntimeError> {
    let os = if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "mac"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        return Err(RuntimeError::Unsupported {
            target: crate::target::target_minecraft_version(),
            detail: "managed Temurin downloads are not implemented for this operating system"
                .to_owned(),
        });
    };
    let architecture = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "aarch64",
        other => {
            return Err(RuntimeError::Unsupported {
                target: crate::target::target_minecraft_version(),
                detail: format!("managed Temurin downloads are not available for {other}"),
            })
        }
    };
    Ok((os.to_owned(), architecture.to_owned()))
}

fn normalize_sha256(value: &str) -> Result<String, RuntimeError> {
    let value = value.trim().to_ascii_lowercase();
    if value.len() != 64 || !value.chars().all(|character| character.is_ascii_hexdigit()) {
        return Err(RuntimeError::Checksum(
            "Adoptium announced an invalid SHA-256 checksum".to_owned(),
        ));
    }
    Ok(value)
}

fn temurin_image_type(major: u32) -> &'static str {
    // Adoptium publishes Java 16 as a JDK but does not offer a standalone JRE.
    if major == 16 {
        "jdk"
    } else {
        "jre"
    }
}

/// Selects exactly the requested Eclipse Temurin runtime. Kept pure so the response contract is
/// tested without network access.
pub fn parse_latest_release(
    body: &[u8],
    os: &str,
    architecture: &str,
    major: u32,
) -> Result<RuntimeReleaseInfo, RuntimeError> {
    let image_type = temurin_image_type(major);
    let releases: Vec<AdoptiumRelease> = serde_json::from_slice(body)
        .map_err(|error| RuntimeError::Network(format!("invalid Adoptium JSON: {error}")))?;
    let release = releases
        .into_iter()
        .find(|release| {
            release.vendor == "eclipse"
                && release.binary.os == os
                && release.binary.architecture == architecture
                && release.binary.image_type == image_type
                && release.version.major == major
        })
        .ok_or_else(|| {
            RuntimeError::Network(format!(
                "Adoptium has no Java {major} {image_type} for {os}/{architecture}"
            ))
        })?;
    let sha256 = normalize_sha256(&release.binary.package.checksum)?;
    let size = release.binary.package.size;
    if !(MIN_DOWNLOAD_BYTES..=MAX_DOWNLOAD_BYTES).contains(&size) {
        return Err(RuntimeError::Limit(format!(
            "Adoptium package size {size} is outside the accepted range"
        )));
    }
    require_download_url(&release.binary.package.link)?;
    if release.binary.package.name.trim().is_empty() {
        return Err(RuntimeError::Network(
            "Adoptium package has no file name".to_owned(),
        ));
    }
    Ok(RuntimeReleaseInfo {
        major,
        version: release.version.semver,
        release_name: release.release_name,
        download_url: release.binary.package.link,
        sha256,
        size,
        vendor: "Eclipse Adoptium".to_owned(),
        os: os.to_owned(),
        arch: architecture.to_owned(),
        image_type: release.binary.image_type,
        source: "Adoptium API v3".to_owned(),
    })
}

fn require_download_url(url: &str) -> Result<(), RuntimeError> {
    Allowlist::GITHUB_RELEASES
        .check(url)
        .map_err(RuntimeError::from)
}

/// Removes a partially downloaded archive after a failed install.
pub fn remove_file_if_exists(path: &Path) {
    let _ = fs::remove_file(path);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_checksum_and_download_hosts() {
        assert!(normalize_sha256(&"a".repeat(64)).is_ok());
        assert!(normalize_sha256("abc").is_err());
        assert!(normalize_sha256(&"g".repeat(64)).is_err());

        assert!(require_download_url(
            "https://github.com/adoptium/temurin21-binaries/releases/download/x/java.zip"
        )
        .is_ok());
        assert!(require_download_url(
            "https://objects.githubusercontent.com/github-production-release-asset/x"
        )
        .is_ok());
        assert!(require_download_url("https://evil.example.com/java.zip").is_err());
        assert!(require_download_url("http://github.com/java.zip").is_err());
        assert!(require_download_url("https://github.com.evil.example/java.zip").is_err());
        assert!(require_download_url("https://user@github.com/java.zip").is_err());
    }

    #[test]
    fn provisionable_java_majors_are_explicit() {
        assert!(crate::is_supported_java_major(8));
        assert!(crate::is_supported_java_major(16));
        assert!(crate::is_supported_java_major(17));
        assert!(crate::is_supported_java_major(21));
        assert!(crate::is_supported_java_major(25));
        assert!(!crate::is_supported_java_major(11));
        assert!(!crate::is_supported_java_major(0));
        assert!(matches!(
            crate::validate_java_major(11),
            Err(RuntimeError::Unsupported { .. })
        ));
    }

    #[test]
    fn release_ids_are_safe_directory_names() {
        let release = RuntimeReleaseInfo {
            major: 21,
            version: "21.0.13+9.0.LTS".to_owned(),
            release_name: "jdk-21.0.13+9".to_owned(),
            download_url: "https://github.com/x".to_owned(),
            sha256: "a".repeat(64),
            size: 49_106_397,
            vendor: "Eclipse Adoptium".to_owned(),
            os: "windows".to_owned(),
            arch: "x64".to_owned(),
            image_type: "jre".to_owned(),
            source: "Adoptium API v3".to_owned(),
        };
        let id = release_id(&release);
        assert_eq!(id, "temurin-21.0.13+9-windows-x64");
        assert!(!id.contains('/') && !id.contains('\\'));
    }

    fn adoptium_body(link: &str, size: u64, checksum: &str, major: u32) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!([
            {
                "release_name": "jdk-21.0.13+9",
                "vendor": "eclipse",
                "version": { "major": 21, "semver": "21.0.13+9.0.LTS" },
                "binary": {
                    "os": "windows",
                    "architecture": "x64",
                    "image_type": "jdk",
                    "package": {
                        "link": "https://github.com/adoptium/jdk.zip",
                        "checksum": checksum,
                        "size": size,
                        "name": "jdk.zip"
                    }
                }
            },
            {
                "release_name": "jdk-21.0.13+9",
                "vendor": "eclipse",
                "version": { "major": major, "semver": "21.0.13+9.0.LTS" },
                "binary": {
                    "os": "windows",
                    "architecture": "x64",
                    "image_type": "jre",
                    "package": {
                        "link": link,
                        "checksum": checksum,
                        "size": size,
                        "name": "jre.zip"
                    }
                }
            }
        ]))
        .expect("adoptium json")
    }

    #[test]
    fn java_16_selects_the_official_jdk_archive() {
        let body = serde_json::json!([{
            "release_name": "jdk-16.0.2+7",
            "vendor": "eclipse",
            "version": { "major": 16, "semver": "16.0.2+7" },
            "binary": {
                "os": "windows",
                "architecture": "x64",
                "image_type": "jdk",
                "package": {
                    "link": "https://github.com/adoptium/temurin16-binaries/releases/download/jdk-16.0.2%2B7/OpenJDK16U-jdk_x64_windows_hotspot_16.0.2_7.zip",
                    "checksum": "a".repeat(64),
                    "size": 203_448_494,
                    "name": "OpenJDK16U-jdk_x64_windows_hotspot_16.0.2_7.zip"
                }
            }
        }]);
        let release = parse_latest_release(body.to_string().as_bytes(), "windows", "x64", 16)
            .expect("official Java 16 package");
        assert_eq!(release.major, 16);
        assert_eq!(release.image_type, "jdk");
        assert_eq!(release.size, 203_448_494);
    }

    #[test]
    fn selects_only_the_matching_temurin_jre() {
        let checksum = "AB".repeat(32);
        let body = adoptium_body(
            "https://github.com/adoptium/temurin21-binaries/releases/download/x.zip",
            49_106_397,
            &checksum,
            21,
        );
        let release = parse_latest_release(&body, "windows", "x64", 21).expect("release");
        assert_eq!(release.major, 21);
        assert_eq!(release.version, "21.0.13+9.0.LTS");
        assert_eq!(release.sha256, "ab".repeat(32));
        assert_eq!(release.size, 49_106_397);
        assert_eq!(release.image_type, "jre");

        let wrong_major = adoptium_body(
            "https://github.com/adoptium/temurin21-binaries/releases/download/x.zip",
            49_106_397,
            &checksum,
            17,
        );
        assert!(parse_latest_release(&wrong_major, "windows", "x64", 21).is_err());
        assert!(parse_latest_release(&body, "windows", "x64", 17).is_err());

        let wrong_host = adoptium_body("https://example.com/x.zip", 49_106_397, &checksum, 21);
        assert!(matches!(
            parse_latest_release(&wrong_host, "windows", "x64", 21),
            Err(RuntimeError::Network(_))
        ));

        let tiny = adoptium_body(
            "https://github.com/adoptium/temurin21-binaries/releases/download/x.zip",
            1024,
            &checksum,
            21,
        );
        assert!(matches!(
            parse_latest_release(&tiny, "windows", "x64", 21),
            Err(RuntimeError::Limit(_))
        ));
    }
}
