use std::fs::File;
use std::io::Read;
use std::path::Path;

use minecraft_version::{profile_for_version, JarPackFormats, JarVersionJson, VersionFamily};
use serde::Deserialize;
use sha1::{Digest, Sha1};

use crate::archive::Archive;

const MAX_VERSION_JSON_BYTES: u64 = 4 * 1024 * 1024;
const HASH_BUFFER_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ExpectedClient {
    pub id: Option<String>,
    pub sha1: Option<String>,
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ClientJarInspection {
    pub bytes: u64,
    pub sha1: Option<String>,
    pub checksum_verified: bool,
    pub id: Option<String>,
    pub internal_id: Option<String>,
    pub world_version: Option<i32>,
    pub pack_formats: Option<JarPackFormats>,
    pub java_version: Option<u32>,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

pub(crate) struct ClientJarVersion {
    pub id: String,
    pub world_version: i32,
    pub pack_formats: Option<JarPackFormats>,
    /// Resource pack format even when the release has no data pack format (1.8–1.12.2).
    pub resource_pack_format: Option<u32>,
    pub datapack_format: Option<u32>,
    pub java_version: Option<u32>,
    pub internal_id: Option<String>,
}

#[derive(Deserialize)]
struct InternalVersion {
    #[serde(flatten)]
    version: JarVersionJson,
    java_version: Option<u32>,
}

pub(crate) fn client_jar_version(
    path: &Path,
    launcher_id: Option<&str>,
    checksum_verified: bool,
) -> Result<ClientJarVersion, String> {
    let mut archive =
        Archive::open(path).map_err(|error| format!("cannot read the client JAR: {error}"))?;
    let bytes = match archive.read_entry("version.json", MAX_VERSION_JSON_BYTES) {
        Ok(bytes) => bytes,
        Err(crate::archive::ArchiveError::Missing(_)) => {
            let id = launcher_id.ok_or("the client JAR has no internal version.json and no adjacent launcher metadata naming the version")?;
            let profile = id.parse().ok().and_then(|version| profile_for_version(&version))
                .filter(|profile| {
                    matches!(
                        profile.family,
                        VersionFamily::Flattened | VersionFamily::Numeric
                    )
                })
                .ok_or_else(|| format!("Minecraft {id} has no exact built-in profile that supports a client JAR without version.json"))?;
            if !checksum_verified {
                return Err("a client JAR without version.json must match the adjacent launcher metadata SHA-1".to_owned());
            }
            return Ok(ClientJarVersion {
                id: profile.minecraft_version.to_string(),
                world_version: profile.data_version,
                pack_formats: profile
                    .resource_pack_format
                    .zip(profile.datapack_format)
                    .map(|(resource, data)| JarPackFormats {
                        resource,
                        data,
                        minors: profile.pack_format_minors,
                    }),
                resource_pack_format: profile.resource_pack_format,
                datapack_format: profile.datapack_format,
                java_version: None,
                internal_id: None,
            });
        }
        Err(error) => return Err(format!("cannot read the client JAR version.json: {error}")),
    };
    let InternalVersion {
        version,
        java_version,
    } = serde_json::from_slice(&bytes)
        .map_err(|error| format!("client JAR version.json is invalid: {error}"))?;
    let id = version
        .minecraft_version()
        .map_err(|error| error.to_string())?;
    if let Some(launcher_id) = launcher_id {
        if launcher_id != id.as_str() {
            return Err(format!(
                "version.json declares `{}`, not the launcher metadata id `{launcher_id}`",
                version.id
            ));
        }
    }
    if version.world_version <= 0 {
        return Err("client JAR version.json has no positive world_version".to_owned());
    }
    let pack_formats = version.pack_formats().map_err(|error| error.to_string())?;
    Ok(ClientJarVersion {
        id: id.to_string(),
        world_version: version.world_version,
        pack_formats,
        resource_pack_format: pack_formats.map(|formats| formats.resource),
        datapack_format: pack_formats.map(|formats| formats.data),
        java_version,
        internal_id: Some(version.id),
    })
}

/// Hashes a file with SHA-1, streaming and without loading it into memory.
pub(crate) fn file_sha1(path: &Path) -> std::io::Result<String> {
    reader_sha1(File::open(path)?)
}

pub(crate) fn reader_sha1(mut file: impl Read) -> std::io::Result<String> {
    let mut hasher = Sha1::new();
    let mut buffer = [0u8; HASH_BUFFER_BYTES];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex(&hasher.finalize()))
}

pub(crate) fn sha1_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(bytes);
    hex(&hasher.finalize())
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut output, "{byte:02x}");
    }
    output
}

/// Inspects a client JAR: size, SHA-1, internal `version.json` and the expected
/// checksum from the launcher metadata when available.
pub(crate) fn inspect_client_jar(path: &Path, expected: &ExpectedClient) -> ClientJarInspection {
    let mut report = ClientJarInspection::default();
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => report.bytes = metadata.len(),
        Ok(_) => {
            report
                .errors
                .push("the client JAR path is not a regular file".to_owned());
            return report;
        }
        Err(error) => {
            report
                .errors
                .push(format!("the client JAR cannot be read: {error}"));
            return report;
        }
    }
    match file_sha1(path) {
        Ok(hash) => report.sha1 = Some(hash),
        Err(error) => report
            .errors
            .push(format!("the client JAR cannot be hashed: {error}")),
    }

    if let Some(expected_size) = expected.size {
        if expected_size != report.bytes {
            report.errors.push(format!(
                "client JAR size is {} bytes but launcher metadata declares {expected_size}",
                report.bytes
            ));
        }
    }
    if let (Some(expected_hash), Some(actual_hash)) = (&expected.sha1, &report.sha1) {
        if !expected_hash.eq_ignore_ascii_case(actual_hash) {
            report.errors.push(format!(
                "client JAR SHA-1 is {actual_hash} but launcher metadata declares {expected_hash}"
            ));
        } else if expected.size.is_none_or(|size| size == report.bytes) {
            report.checksum_verified = true;
        }
    } else if expected.sha1.is_none() {
        report
            .warnings
            .push("launcher metadata does not declare a client JAR SHA-1".to_owned());
    }

    match client_jar_version(path, expected.id.as_deref(), report.checksum_verified) {
        Ok(version) => {
            report.id = Some(version.id.clone());
            report.world_version = Some(version.world_version);
            report.pack_formats = version.pack_formats;
            report.java_version = version.java_version;
            if version.internal_id.is_none() {
                report.warnings.push(format!(
                    "the client JAR has no internal version.json; using the exact Minecraft {} profile and matching launcher checksum",
                    version.id
                ));
            }
            report.internal_id = version.internal_id;
        }
        Err(error) => report.errors.push(error),
    }
    report
}
