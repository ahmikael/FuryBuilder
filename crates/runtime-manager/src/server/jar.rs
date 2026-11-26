use std::fs::File;
use std::io::Read;
use std::path::Path;

use minecraft_version::{profile_for_version, JarPackVersion, JarVersionJson};
use serde::Deserialize;
use sha1::{Digest, Sha1};
use zip::ZipArchive;

use crate::error::ServerError;
use crate::managed::hex;

use super::types::{ServerTarget, MAX_SERVER_JAR_BYTES, MIN_SERVER_JAR_BYTES};

const MAX_METADATA_BYTES: u64 = 64 * 1024;
const MAX_ZIP_ENTRIES: usize = 40_000;
const CLASSIC_MAIN: &str = "net.minecraft.server.MinecraftServer";

// Mojang's exact downloads.server SHA-1 values for the releases without version.json.
// An offline manual selection cannot establish identity from the file name or class alone.
const SERVER_JARS_WITHOUT_VERSION_JSON: [(&str, &str); 3] = [
    ("d0caafb8438ebd206f99930cfaecfa6c9a13dca0", "1.13"),
    ("fe123682e9cb30031eae351764f653500b7396c9", "1.13.1"),
    ("3737db93722a9e39eeada7c27e7aca28b144ffa7", "1.13.2"),
];

#[derive(Debug, Deserialize)]
struct ServerVersionJson {
    #[serde(flatten)]
    version: JarVersionJson,
    #[serde(default)]
    java_version: Option<u32>,
    #[serde(default)]
    protocol_version: Option<i32>,
}

#[derive(Debug, Clone)]
pub struct ServerJarInfo {
    pub version: String,
    pub data_version: i32,
    pub java_major: Option<u32>,
    pub protocol_version: Option<i32>,
    pub resource_pack_format: Option<u32>,
    pub sha1: String,
    pub size: u64,
}

impl ServerJarInfo {
    pub(crate) fn known_official(&self) -> bool {
        version_for_official_hash(&self.sha1).is_some()
    }

    /// Checks the exact version, DataVersion and declared Java major against a project target.
    /// A mismatch is never repaired or substituted; the caller reports the actual facts.
    pub fn validate_for(&self, target: &ServerTarget) -> Result<(), ServerError> {
        let expected = target.minecraft_version.as_str();
        if self.version != expected {
            return Err(ServerError::Unsupported {
                target: expected.to_owned(),
                detail: format!(
                    "the JAR declares Minecraft {}, expected {expected}",
                    self.version
                ),
            });
        }
        if self.data_version != target.data_version {
            return Err(ServerError::Unsupported {
                target: expected.to_owned(),
                detail: format!(
                    "the JAR declares world_version {}, expected {}",
                    self.data_version, target.data_version
                ),
            });
        }
        if let Some(java_major) = self.java_major {
            if java_major != target.java_major {
                return Err(ServerError::Unsupported {
                    target: expected.to_owned(),
                    detail: format!(
                        "the JAR requires Java {java_major}, expected Java {}",
                        target.java_major
                    ),
                });
            }
        }
        Ok(())
    }
}

/// Reads the actual archive instead of trusting a file name. A structurally valid JAR must:
///
/// - be a regular file in the expected size range;
/// - be a readable ZIP;
/// - contain a known server entry point;
/// - contain exactly one root `version.json`, or match a pinned Mojang server JAR without one.
///
/// Whether the declared version, DataVersion and Java major fit a project is checked by
/// [`ServerJarInfo::validate_for`], so callers can report the actual facts of a stale file.
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
    let mut version_count = 0;
    let mut modern_main = false;
    let mut classic_main = false;
    for name in archive.file_names() {
        let name =
            name.map_err(|error| ServerError::Archive(format!("{}: {error}", path.display())))?;
        match name.as_ref() {
            "version.json" => version_count += 1,
            "net/minecraft/bundler/Main.class" | "net/minecraft/server/Main.class" => {
                modern_main = true;
            }
            "net/minecraft/server/MinecraftServer.class" => classic_main = true,
            _ => {}
        }
    }
    if !modern_main && !(classic_main && has_classic_entry_point(&mut archive)?) {
        return Err(ServerError::Archive(format!(
            "{} has no Minecraft server main class; client and library JARs are refused",
            path.display()
        )));
    }
    match version_count {
        1 => {
            let bytes = read_metadata(&mut archive, "version.json")?;
            let metadata: ServerVersionJson = serde_json::from_slice(&bytes)
                .map_err(|error| ServerError::Archive(format!("version.json: {error}")))?;
            let resource_pack_format = match metadata.version.pack_version {
                Some(JarPackVersion::Combined(format)) => Some(format),
                Some(JarPackVersion::Split { resource, .. }) => Some(resource),
                Some(JarPackVersion::Versioned { resource_major, .. }) => Some(resource_major),
                None => None,
            };
            Ok(ServerJarInfo {
                version: metadata.version.id,
                data_version: metadata.version.world_version,
                java_major: metadata.java_version,
                protocol_version: metadata.protocol_version,
                resource_pack_format,
                sha1,
                size,
            })
        }
        0 => {
            let profile = version_for_official_hash(&sha1)
                .and_then(|version| version.parse().ok())
                .and_then(|version| profile_for_version(&version))
                .ok_or_else(|| {
                    ServerError::Archive(format!(
                        "{} has no root version.json and does not match a known official server JAR",
                        path.display()
                    ))
                })?;
            Ok(ServerJarInfo {
                version: profile.minecraft_version.to_string(),
                data_version: profile.data_version,
                java_major: Some(profile.java.recommended_major),
                protocol_version: None,
                resource_pack_format: profile.resource_pack_format,
                sha1,
                size,
            })
        }
        _ => Err(ServerError::Archive(format!(
            "{} must contain at most one root version.json",
            path.display()
        ))),
    }
}

fn version_for_official_hash(sha1: &str) -> Option<&'static str> {
    SERVER_JARS_WITHOUT_VERSION_JSON
        .iter()
        .find(|(hash, _)| hash.eq_ignore_ascii_case(sha1))
        .map(|(_, version)| *version)
}

fn has_classic_entry_point(archive: &mut ZipArchive<File>) -> Result<bool, ServerError> {
    let bytes = read_metadata(archive, "META-INF/MANIFEST.MF")?;
    let manifest = std::str::from_utf8(&bytes)
        .map_err(|error| ServerError::Archive(format!("META-INF/MANIFEST.MF: {error}")))?;
    let mut mains = manifest
        .lines()
        .take_while(|line| !line.is_empty())
        .filter_map(|line| line.split_once(": "))
        .filter(|(key, _)| key.eq_ignore_ascii_case("Main-Class"));
    Ok(mains.next().is_some_and(|(_, value)| value == CLASSIC_MAIN) && mains.next().is_none())
}

fn read_metadata(archive: &mut ZipArchive<File>, name: &str) -> Result<Vec<u8>, ServerError> {
    let entry = archive
        .by_name(name)
        .map_err(|error| ServerError::Archive(format!("{name}: {error}")))?;
    let mut bytes = Vec::new();
    entry
        .take(MAX_METADATA_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| ServerError::Archive(format!("{name}: {error}")))?;
    if bytes.len() as u64 > MAX_METADATA_BYTES {
        return Err(ServerError::Archive(format!(
            "{name} is larger than 64 KiB"
        )));
    }
    Ok(bytes)
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
