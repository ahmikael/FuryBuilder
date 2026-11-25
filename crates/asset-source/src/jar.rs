use std::fs::File;
use std::io::Read;
use std::path::Path;

use serde_json::Value;
use sha1::{Digest, Sha1};

use crate::archive::Archive;

const MAX_VERSION_JSON_BYTES: u64 = 4 * 1024 * 1024;
const HASH_BUFFER_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ExpectedClient {
    pub sha1: Option<String>,
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ClientJarInspection {
    pub bytes: u64,
    pub sha1: Option<String>,
    pub checksum_verified: bool,
    pub internal_id: Option<String>,
    pub world_version: Option<i32>,
    pub resource_pack_format: Option<i32>,
    pub java_version: Option<u32>,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
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

    match Archive::open(path) {
        Ok(mut archive) => {
            if let Some(entry) = archive
                .entries()
                .iter()
                .find(|entry| entry.name == "version.json")
            {
                if entry.uncompressed_size > MAX_VERSION_JSON_BYTES {
                    report
                        .errors
                        .push("the client JAR version.json is too large".to_owned());
                } else {
                    match archive.read_entry("version.json", MAX_VERSION_JSON_BYTES) {
                        Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
                            Ok(value) => {
                                report.internal_id = value
                                    .get("id")
                                    .and_then(Value::as_str)
                                    .map(ToOwned::to_owned);
                                report.world_version = value
                                    .get("world_version")
                                    .and_then(Value::as_i64)
                                    .and_then(|value| i32::try_from(value).ok());
                                report.resource_pack_format = value
                                    .get("pack_version")
                                    .and_then(|pack| pack.get("resource"))
                                    .and_then(Value::as_i64)
                                    .and_then(|value| i32::try_from(value).ok());
                                report.java_version = value
                                    .get("java_version")
                                    .and_then(Value::as_i64)
                                    .and_then(|value| u32::try_from(value).ok());
                                if report.internal_id.is_none() {
                                    report
                                        .warnings
                                        .push("client JAR version.json has no id".to_owned());
                                }
                                if report.world_version.is_none() {
                                    report.warnings.push(
                                        "client JAR version.json has no world_version".to_owned(),
                                    );
                                }
                            }
                            Err(error) => report.errors.push(format!(
                                "client JAR version.json is not valid JSON: {error}"
                            )),
                        },
                        Err(error) => report.errors.push(format!(
                            "client JAR version.json cannot be decompressed: {error}"
                        )),
                    }
                }
            } else {
                report.errors.push(
                    "the JAR has no root version.json; it is not a Minecraft client JAR".to_owned(),
                );
            }
        }
        Err(error) => report
            .errors
            .push(format!("the JAR is not a readable zip archive: {error}")),
    }
    report
}
