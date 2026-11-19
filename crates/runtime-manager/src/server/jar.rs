use std::fs::File;
use std::io::Read;
use std::path::Path;

use serde::Deserialize;
use sha1::{Digest, Sha1};
use zip::ZipArchive;

use crate::error::{unsupported_server, ServerError};
use crate::managed::hex;
use crate::types::TARGET_MINECRAFT_VERSION;

use super::types::{MAX_SERVER_JAR_BYTES, MIN_SERVER_JAR_BYTES, TARGET_DATA_VERSION};

const MAX_VERSION_JSON_BYTES: u64 = 64 * 1024;
const MAX_ZIP_ENTRIES: usize = 10_000;

#[derive(Debug, Deserialize)]
struct ServerVersionJson {
    id: String,
    world_version: i32,
    #[serde(default)]
    java_version: Option<u32>,
    #[serde(default)]
    protocol_version: Option<i32>,
    #[serde(default)]
    pack_version: Option<PackVersion>,
}

#[derive(Debug, Deserialize)]
struct PackVersion {
    #[serde(default)]
    resource: Option<i32>,
}

#[derive(Debug, Clone)]
pub struct ServerJarInfo {
    pub version: String,
    pub data_version: i32,
    pub java_major: Option<u32>,
    pub protocol_version: Option<i32>,
    pub resource_pack_format: Option<i32>,
    pub sha1: String,
    pub size: u64,
}

/// Reads the actual archive instead of trusting a file name. A valid target JAR must:
///
/// - be a regular file in the expected size range;
/// - be a readable ZIP;
/// - contain exactly one root `version.json` declaring `id` 1.21.4 and `world_version` 4189;
/// - declare Java 21 when `java_version` is present;
/// - contain a known server entry point (`bundler/Main` or `server/Main`).
pub fn inspect_server_jar(path: &Path) -> Result<ServerJarInfo, ServerError> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| ServerError::Invalid(format!("{}: {error}", path.display())))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(ServerError::Invalid(format!(
            "{} is not a regular file",
            path.display()
        )));
    }
    let size = metadata.len();
    if !(MIN_SERVER_JAR_BYTES..=MAX_SERVER_JAR_BYTES).contains(&size) {
        return Err(ServerError::Limit(format!(
            "{} is {size} bytes; a Minecraft server JAR must be between {MIN_SERVER_JAR_BYTES} and {MAX_SERVER_JAR_BYTES} bytes",
            path.display()
        )));
    }
    let sha1 = sha1_file(path)?;
    let file = File::open(path)
        .map_err(|error| ServerError::Io(format!("{}: {error}", path.display())))?;
    let mut archive = ZipArchive::new(file)
        .map_err(|error| ServerError::Archive(format!("{}: {error}", path.display())))?;
    if archive.len() > MAX_ZIP_ENTRIES {
        return Err(ServerError::Limit(format!(
            "{} contains {} ZIP entries (limit {MAX_ZIP_ENTRIES})",
            path.display(),
            archive.len()
        )));
    }
    let mut version_names: Vec<String> = Vec::new();
    for name in archive.file_names() {
        let name =
            name.map_err(|error| ServerError::Archive(format!("{}: {error}", path.display())))?;
        if name == "version.json" {
            version_names.push(name.into_owned());
        }
    }
    if version_names.len() != 1 {
        return Err(ServerError::Archive(format!(
            "{} must contain exactly one root version.json",
            path.display()
        )));
    }
    let version_json = {
        let mut entry = archive
            .by_name("version.json")
            .map_err(|error| ServerError::Archive(format!("version.json: {error}")))?;
        let mut bytes = Vec::new();
        entry
            .by_ref()
            .take(MAX_VERSION_JSON_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| ServerError::Archive(format!("version.json: {error}")))?;
        if bytes.len() as u64 > MAX_VERSION_JSON_BYTES {
            return Err(ServerError::Archive(
                "version.json is larger than 64 KiB".to_owned(),
            ));
        }
        serde_json::from_slice::<ServerVersionJson>(&bytes)
            .map_err(|error| ServerError::Archive(format!("version.json: {error}")))?
    };
    if version_json.id != TARGET_MINECRAFT_VERSION {
        return Err(unsupported_server(format!(
            "the JAR declares Minecraft {}, expected {}",
            version_json.id, TARGET_MINECRAFT_VERSION
        )));
    }
    if version_json.world_version != TARGET_DATA_VERSION {
        return Err(unsupported_server(format!(
            "the JAR declares world_version {}, expected {TARGET_DATA_VERSION}",
            version_json.world_version
        )));
    }
    if let Some(java_major) = version_json.java_version {
        if java_major != crate::types::TARGET_JAVA_MAJOR {
            return Err(unsupported_server(format!(
                "the JAR requires Java {java_major}, expected Java {}",
                crate::types::TARGET_JAVA_MAJOR
            )));
        }
    }
    let mut has_server_main = false;
    for name in archive.file_names() {
        let name =
            name.map_err(|error| ServerError::Archive(format!("{}: {error}", path.display())))?;
        if name == "net/minecraft/bundler/Main.class" || name == "net/minecraft/server/Main.class" {
            has_server_main = true;
            break;
        }
    }
    if !has_server_main {
        return Err(ServerError::Archive(format!(
            "{} has no Minecraft server main class; client and library JARs are refused",
            path.display()
        )));
    }
    Ok(ServerJarInfo {
        version: version_json.id,
        data_version: version_json.world_version,
        java_major: version_json.java_version,
        protocol_version: version_json.protocol_version,
        resource_pack_format: version_json.pack_version.and_then(|pack| pack.resource),
        sha1,
        size,
    })
}

pub fn sha1_file(path: &Path) -> Result<String, ServerError> {
    let mut file = File::open(path)
        .map_err(|error| ServerError::Io(format!("{}: {error}", path.display())))?;
    let mut hasher = Sha1::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| ServerError::Io(format!("{}: {error}", path.display())))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex(&hasher.finalize()))
}

pub(crate) fn is_sha1(value: &str) -> bool {
    value.len() == 40 && value.chars().all(|character| character.is_ascii_hexdigit())
}
