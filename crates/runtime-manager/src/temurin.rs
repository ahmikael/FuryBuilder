use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::error::RuntimeError;
use crate::managed::hex;
use crate::net;
use crate::types::{RuntimeProgress, RuntimeReleaseInfo};

const API_BASE: &str = "https://api.adoptium.net/v3";
const MAX_METADATA_BYTES: u64 = 1024 * 1024;
const MAX_DOWNLOAD_BYTES: u64 = 200 * 1024 * 1024;
const MIN_DOWNLOAD_BYTES: u64 = 10 * 1024 * 1024;

const DOWNLOAD_HOSTS: &[&str] = &[
    "github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
    "github-releases.githubusercontent.com",
];

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
    agent: ureq::Agent,
}

impl Default for AdoptiumClient {
    fn default() -> Self {
        Self::new()
    }
}

impl AdoptiumClient {
    pub fn new() -> Self {
        let agent = net::agent();
        Self { agent }
    }

    pub fn latest_jre(&self, major: u32) -> Result<RuntimeReleaseInfo, RuntimeError> {
        crate::validate_java_major(major)?;
        let (os, architecture) = platform()?;
        let url = format!(
            "{API_BASE}/assets/latest/{major}/hotspot?architecture={architecture}&image_type=jre&os={os}&vendor=eclipse&project=jdk"
        );
        let response = self.agent.get(&url).call().map_err(map_http_error)?;
        if response.status() != 200 {
            return Err(RuntimeError::Network(format!(
                "Adoptium API returned HTTP {}",
                response.status()
            )));
        }
        let mut body = Vec::new();
        response
            .into_reader()
            .take(MAX_METADATA_BYTES + 1)
            .read_to_end(&mut body)
            .map_err(|error| {
                RuntimeError::Network(format!("cannot read Adoptium response: {error}"))
            })?;
        if body.len() as u64 > MAX_METADATA_BYTES {
            return Err(RuntimeError::Limit(
                "Adoptium response is larger than 1 MiB".to_owned(),
            ));
        }
        parse_latest_release(&body, &os, &architecture, major)
    }

    /// Downloads with manual redirect validation, a hard size limit and streaming SHA-256.
    /// The destination is written exactly once; the caller supplies a new temporary path.
    pub fn download(
        &self,
        release: &RuntimeReleaseInfo,
        destination: &Path,
        cancel: &std::sync::atomic::AtomicBool,
        progress: &dyn Fn(RuntimeProgress),
    ) -> Result<(), RuntimeError> {
        use std::sync::atomic::Ordering;
        if cancel.load(Ordering::Relaxed) {
            return Err(RuntimeError::Cancelled);
        }
        require_download_url(&release.download_url)?;
        let response = net::open(&self.agent, &release.download_url, DOWNLOAD_HOSTS)
            .map_err(RuntimeError::Network)?;
        if response.status() != 200 {
            return Err(RuntimeError::Network(format!(
                "download returned HTTP {}",
                response.status()
            )));
        }
        if let Some(length) = response.header("content-length") {
            if let Ok(length) = length.parse::<u64>() {
                if length != release.size {
                    return Err(RuntimeError::Checksum(format!(
                        "announced size {} does not match response size {length}",
                        release.size
                    )));
                }
            }
        }
        let mut reader = response.into_reader();
        let mut file = File::create(destination).map_err(|error| {
            RuntimeError::Io(format!("cannot create {}: {error}", destination.display()))
        })?;
        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 64 * 1024];
        let mut downloaded = 0u64;
        let mut last_report = 0u64;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(RuntimeError::Cancelled);
            }
            let read = reader
                .read(&mut buffer)
                .map_err(|error| RuntimeError::Network(format!("download interrupted: {error}")))?;
            if read == 0 {
                break;
            }
            downloaded = downloaded.saturating_add(read as u64);
            if downloaded > MAX_DOWNLOAD_BYTES {
                return Err(RuntimeError::Limit(format!(
                    "download exceeds {MAX_DOWNLOAD_BYTES} bytes"
                )));
            }
            hasher.update(&buffer[..read]);
            file.write_all(&buffer[..read])
                .map_err(|error| RuntimeError::Io(format!("cannot write download: {error}")))?;
            if downloaded - last_report >= 512 * 1024 || downloaded == release.size {
                last_report = downloaded;
                progress(RuntimeProgress {
                    phase: "downloading".to_owned(),
                    downloaded,
                    total: release.size,
                });
            }
        }
        file.sync_all()
            .map_err(|error| RuntimeError::Io(format!("cannot flush download: {error}")))?;
        if downloaded != release.size {
            return Err(RuntimeError::Checksum(format!(
                "received {downloaded} bytes, expected {}",
                release.size
            )));
        }
        if cancel.load(Ordering::Relaxed) {
            return Err(RuntimeError::Cancelled);
        }
        let actual = hex(&hasher.finalize());
        if !actual.eq_ignore_ascii_case(&release.sha256) {
            return Err(RuntimeError::Checksum(format!(
                "SHA-256 mismatch: announced {}, received {actual}",
                release.sha256
            )));
        }
        progress(RuntimeProgress {
            phase: "verified".to_owned(),
            downloaded,
            total: release.size,
        });
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

/// Selects exactly the requested Eclipse Temurin JRE. Kept pure so the response contract is
/// tested without network access.
pub fn parse_latest_release(
    body: &[u8],
    os: &str,
    architecture: &str,
    major: u32,
) -> Result<RuntimeReleaseInfo, RuntimeError> {
    let releases: Vec<AdoptiumRelease> = serde_json::from_slice(body)
        .map_err(|error| RuntimeError::Network(format!("invalid Adoptium JSON: {error}")))?;
    let release = releases
        .into_iter()
        .find(|release| {
            release.vendor == "eclipse"
                && release.binary.os == os
                && release.binary.architecture == architecture
                && release.binary.image_type == "jre"
                && release.version.major == major
        })
        .ok_or_else(|| {
            RuntimeError::Network(format!(
                "Adoptium has no Java {major} JRE for {os}/{architecture}"
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
        image_type: "jre".to_owned(),
        source: "Adoptium API v3".to_owned(),
    })
}

fn require_download_url(url: &str) -> Result<(), RuntimeError> {
    net::check_url(url, DOWNLOAD_HOSTS).map_err(RuntimeError::Network)
}

fn map_http_error(error: ureq::Error) -> RuntimeError {
    RuntimeError::Network(net::http_error(error))
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
