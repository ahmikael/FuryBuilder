//! Local data pack validation for isolated Vanilla World generation.
//!
//! A data pack is untrusted input: it is a ZIP selected by the user and later loaded by the
//! official server. Before a run, FuryBuilder checks the archive structure, entry paths,
//! declared size, data pack format and the root `pack.mcmeta`. The file is copied into the
//! run-local `world/datapacks/` directory only after its SHA-256 and size have been recorded
//! and re-verified, so a changed file cannot silently replace the accepted one.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use zip::ZipArchive;

use crate::error::ServerError;

use super::types::{DatapackInspection, GenerationDatapack, GenerationTarget};

pub const MAX_DATAPACKS: usize = 16;
pub const MAX_DATAPACK_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_DATAPACK_TOTAL_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_DATAPACK_UNCOMPRESSED_BYTES: u64 = 512 * 1024 * 1024;
pub const MAX_DATAPACK_ENTRIES: usize = 20_000;
const MAX_MCMETA_BYTES: u64 = 64 * 1024;

#[derive(Debug, Deserialize)]
struct PackMeta {
    pack: PackSection,
}

#[derive(Debug, Deserialize)]
struct PackSection {
    pack_format: Option<serde_json::Value>,
    supported_formats: Option<serde_json::Value>,
}

fn inspect_path(path: &Path, target: &GenerationTarget) -> Result<GenerationDatapack, ServerError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| ServerError::Invalid(format!("{}: {error}", path.display())))?;
    if metadata.file_type().is_symlink() {
        return Err(ServerError::Invalid(format!(
            "{} is a symbolic link; choose the real data pack file",
            path.display()
        )));
    }
    if !metadata.is_file() {
        return Err(ServerError::Invalid(format!(
            "{} is not a regular data pack file",
            path.display()
        )));
    }
    if metadata.len() == 0 {
        return Err(ServerError::Invalid(format!("{} is empty", path.display())));
    }
    if metadata.len() > MAX_DATAPACK_BYTES {
        return Err(ServerError::Limit(format!(
            "{} is larger than {} MiB",
            path.display(),
            MAX_DATAPACK_BYTES / (1024 * 1024)
        )));
    }
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| ServerError::Invalid(format!("{} has no file name", path.display())))?;
    if !file_name.to_ascii_lowercase().ends_with(".zip") {
        return Err(ServerError::Invalid(format!(
            "{} is not a .zip data pack",
            path.display()
        )));
    }
    let sha256 = sha256_file(path)?;
    let file = File::open(path)
        .map_err(|error| ServerError::Io(format!("cannot open {}: {error}", path.display())))?;
    let mut archive = ZipArchive::new(BufReader::new(file)).map_err(|error| {
        ServerError::Archive(format!("{} is not a valid ZIP: {error}", path.display()))
    })?;
    if archive.len() > MAX_DATAPACK_ENTRIES {
        return Err(ServerError::Limit(format!(
            "{} contains more than {MAX_DATAPACK_ENTRIES} entries",
            path.display()
        )));
    }
    let mut uncompressed = 0u64;
    let mut has_meta = false;
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|error| {
            ServerError::Archive(format!(
                "{} has an unreadable entry: {error}",
                path.display()
            ))
        })?;
        let name = entry
            .name()
            .map_err(|error| {
                ServerError::Archive(format!(
                    "{} has an entry with an unreadable name: {error}",
                    path.display()
                ))
            })?
            .into_owned();
        validate_entry_name(&name, path)?;
        if entry
            .unix_mode()
            .map(|mode| mode & 0o170000 == 0o120000)
            .unwrap_or(false)
        {
            return Err(ServerError::Archive(format!(
                "{} contains a symbolic link entry: {name}",
                path.display()
            )));
        }
        uncompressed = uncompressed.saturating_add(entry.size());
        if uncompressed > MAX_DATAPACK_UNCOMPRESSED_BYTES {
            return Err(ServerError::Limit(format!(
                "{} expands beyond {} MiB",
                path.display(),
                MAX_DATAPACK_UNCOMPRESSED_BYTES / (1024 * 1024)
            )));
        }
        if name == "pack.mcmeta" && !entry.is_dir() {
            has_meta = true;
        }
    }
    if !has_meta {
        return Err(ServerError::Archive(format!(
            "{} has no root pack.mcmeta",
            path.display()
        )));
    }
    let mut meta_entry = archive.by_name("pack.mcmeta").map_err(|error| {
        ServerError::Archive(format!(
            "{} pack.mcmeta is unreadable: {error}",
            path.display()
        ))
    })?;
    if meta_entry.size() > MAX_MCMETA_BYTES {
        return Err(ServerError::Limit(format!(
            "{} pack.mcmeta is too large",
            path.display()
        )));
    }
    let mut meta_bytes = Vec::new();
    meta_entry
        .by_ref()
        .take(MAX_MCMETA_BYTES)
        .read_to_end(&mut meta_bytes)
        .map_err(|error| {
            ServerError::Archive(format!(
                "cannot read {} pack.mcmeta: {error}",
                path.display()
            ))
        })?;
    let meta: PackMeta = serde_json::from_slice(&meta_bytes).map_err(|error| {
        ServerError::Archive(format!(
            "{} pack.mcmeta is not valid JSON: {error}",
            path.display()
        ))
    })?;
    let pack_format = meta
        .pack
        .pack_format
        .as_ref()
        .and_then(serde_json::Value::as_u64)
        .and_then(|format| u32::try_from(format).ok())
        .ok_or_else(|| {
            ServerError::Archive(format!(
                "{} pack.mcmeta has no numeric pack.pack_format",
                path.display()
            ))
        })?;
    let expected_format = target.datapack_format;
    let supported = supports_format(&meta.pack, expected_format);
    if pack_format > expected_format && !supported {
        return Err(ServerError::Unsupported {
            target: target.minecraft_version.to_string(),
            detail: format!(
                "{} declares data pack format {pack_format}, newer than {expected_format}",
                path.display()
            ),
        });
    }
    let mut warnings = Vec::new();
    if pack_format < expected_format && !supported {
        warnings.push(format!(
            "data pack format {pack_format} predates {expected_format}; the server may upgrade or reject it"
        ));
    }
    Ok(GenerationDatapack {
        name: file_name.to_owned(),
        path: path.display().to_string(),
        sha256,
        size: metadata.len(),
        pack_format,
        warnings,
    })
}

fn supports_format(pack: &PackSection, target: u32) -> bool {
    let Some(value) = pack.supported_formats.as_ref() else {
        return false;
    };
    if let Some(format) = value.as_u64() {
        return u32::try_from(format)
            .map(|format| format == target)
            .unwrap_or(false);
    }
    let Some(object) = value.as_object() else {
        return false;
    };
    let min = object
        .get("min_inclusive")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| u32::try_from(value).ok());
    let max = object
        .get("max_inclusive")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| u32::try_from(value).ok());
    matches!((min, max), (Some(min), Some(max)) if (min..=max).contains(&target))
}

fn validate_entry_name(name: &str, archive: &Path) -> Result<(), ServerError> {
    let unsafe_entry = || {
        ServerError::Archive(format!(
            "{} contains an unsafe entry path: {name}",
            archive.display()
        ))
    };
    if name.is_empty() || name.starts_with('/') || name.starts_with('\\') || name.contains('\\') {
        return Err(unsafe_entry());
    }
    if name.len() >= 2 && name.as_bytes()[1] == b':' {
        return Err(unsafe_entry());
    }
    if name
        .split('/')
        .any(|component| component == ".." || component == ".")
    {
        return Err(unsafe_entry());
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, ServerError> {
    let mut file = File::open(path)
        .map_err(|error| ServerError::Io(format!("cannot read {}: {error}", path.display())))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| ServerError::Io(format!("cannot read {}: {error}", path.display())))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(DIGITS[(byte >> 4) as usize] as char);
        output.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    output
}

/// Inspects every selected path without starting anything. Invalid entries are reported
/// individually so the wizard can show the exact reason before the user tries to run.
pub(crate) fn inspect_all(target: &GenerationTarget, paths: &[String]) -> Vec<DatapackInspection> {
    paths
        .iter()
        .map(|path| match inspect_path(Path::new(path), target) {
            Ok(info) => DatapackInspection {
                path: info.path,
                name: info.name,
                sha256: Some(info.sha256),
                size: Some(info.size),
                pack_format: Some(info.pack_format),
                valid: true,
                error: None,
                warnings: info.warnings,
            },
            Err(error) => DatapackInspection {
                path: path.clone(),
                name: Path::new(path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(path)
                    .to_owned(),
                sha256: None,
                size: None,
                pack_format: None,
                valid: false,
                error: Some(error.to_string()),
                warnings: Vec::new(),
            },
        })
        .collect()
}

/// Validates the complete selection, rejects duplicate names and returns the accepted pack
/// records. The SHA-256 and size are stored in the run manifest and report.
pub(crate) fn validate_selection(
    target: &GenerationTarget,
    paths: &[String],
) -> Result<Vec<GenerationDatapack>, ServerError> {
    if paths.len() > MAX_DATAPACKS {
        return Err(ServerError::Limit(format!(
            "at most {MAX_DATAPACKS} data packs can be used in one run"
        )));
    }
    let mut seen = HashSet::new();
    let mut total = 0u64;
    let mut accepted = Vec::with_capacity(paths.len());
    for path in paths {
        let info = inspect_path(Path::new(path), target)?;
        let key = info.name.to_ascii_lowercase();
        if !seen.insert(key) {
            return Err(ServerError::Invalid(format!(
                "two selected data packs are named {}; rename one of them",
                info.name
            )));
        }
        total = total.saturating_add(info.size);
        if total > MAX_DATAPACK_TOTAL_BYTES {
            return Err(ServerError::Limit(format!(
                "selected data packs exceed {} MiB in total",
                MAX_DATAPACK_TOTAL_BYTES / (1024 * 1024)
            )));
        }
        accepted.push(info);
    }
    Ok(accepted)
}

/// Copies the accepted packs into the run-local world. The source is inspected again so a
/// file changed between selection and start is refused instead of silently replaced.
pub(crate) fn install(
    world_dir: &Path,
    datapacks: &[GenerationDatapack],
    target: &GenerationTarget,
) -> Result<(), ServerError> {
    if datapacks.is_empty() {
        return Ok(());
    }
    let target_dir = world_dir.join("datapacks");
    fs::create_dir_all(&target_dir).map_err(|error| {
        ServerError::Io(format!(
            "cannot create {} for data packs: {error}",
            target_dir.display()
        ))
    })?;
    for datapack in datapacks {
        let source = PathBuf::from(&datapack.path);
        let current = inspect_path(&source, target)?;
        if current.sha256 != datapack.sha256 || current.size != datapack.size {
            return Err(ServerError::Invalid(format!(
                "{} changed after it was validated; add it again",
                datapack.name
            )));
        }
        let target = target_dir.join(&datapack.name);
        fs::copy(&source, &target).map_err(|error| {
            ServerError::Io(format!(
                "cannot copy data pack {} into the isolated run: {error}",
                datapack.name
            ))
        })?;
        let copied = fs::metadata(&target).map_err(|error| {
            ServerError::Io(format!("cannot verify {}: {error}", target.display()))
        })?;
        if copied.len() != datapack.size {
            return Err(ServerError::Io(format!(
                "copied data pack {} has an unexpected size",
                datapack.name
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::time::{SystemTime, UNIX_EPOCH};
    use zip::write::SimpleFileOptions;

    fn unique() -> u128 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or(0)
    }

    fn write_zip(path: &Path, entries: &[(&str, &[u8], bool)]) {
        let file = File::create(path).expect("zip file");
        let mut zip = zip::ZipWriter::new(file);
        for (name, bytes, directory) in entries {
            let options =
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            if *directory {
                zip.add_directory(*name, options).expect("zip directory");
            } else {
                zip.start_file(*name, options).expect("zip entry");
                zip.write_all(bytes).expect("zip bytes");
            }
        }
        zip.finish().expect("zip finish");
    }

    fn meta(format: u64) -> Vec<u8> {
        format!(r#"{{"pack":{{"pack_format":{format},"description":"test"}}}}"#).into_bytes()
    }

    fn target() -> GenerationTarget {
        GenerationTarget::for_profile(&minecraft_version::default_profile())
            .expect("the default profile supports generation")
    }

    #[test]
    fn accepts_a_valid_target_format_datapack() {
        let dir = std::env::temp_dir().join(format!(
            "furybuilder-datapack-{}-{}",
            std::process::id(),
            unique()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("valid.zip");
        write_zip(
            &path,
            &[
                ("pack.mcmeta", &meta(61), false),
                ("data/example/function/hello.mcfunction", b"say hi\n", false),
            ],
        );
        let info = inspect_path(&path, &target()).expect("valid datapack");
        assert_eq!(info.pack_format, 61);
        assert_eq!(info.size, fs::metadata(&path).unwrap().len());
        assert_eq!(info.sha256.len(), 64);
        assert!(info.warnings.is_empty());
        let selected =
            validate_selection(&target(), &[path.display().to_string()]).expect("selection");
        assert_eq!(selected.len(), 1);
        let world = dir.join("world");
        install(&world, &selected, &target()).expect("install");
        assert!(world.join("datapacks").join("valid.zip").is_file());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_newer_format_unsafe_paths_and_missing_meta() {
        let dir = std::env::temp_dir().join(format!(
            "furybuilder-datapack-bad-{}-{}",
            std::process::id(),
            unique()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("dir");
        let newer = dir.join("newer.zip");
        write_zip(&newer, &[("pack.mcmeta", &meta(99), false)]);
        assert!(inspect_path(&newer, &target()).is_err());
        let traversal = dir.join("traversal.zip");
        write_zip(
            &traversal,
            &[
                ("pack.mcmeta", &meta(61), false),
                ("../escape.txt", b"bad", false),
            ],
        );
        let error = inspect_path(&traversal, &target()).expect_err("traversal");
        assert!(error.to_string().contains("unsafe entry"));
        let missing = dir.join("missing.zip");
        write_zip(
            &missing,
            &[("data/example/function/a.mcfunction", b"say hi", false)],
        );
        let error = inspect_path(&missing, &target()).expect_err("missing meta");
        assert!(error.to_string().contains("pack.mcmeta"));
        let link = dir.join("link.zip");
        std::fs::write(&link, b"not a zip").expect("fake zip");
        assert!(inspect_path(&link, &target()).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn allows_older_format_with_a_warning_and_supported_formats_range() {
        let dir = std::env::temp_dir().join(format!(
            "furybuilder-datapack-old-{}-{}",
            std::process::id(),
            unique()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("dir");
        let old = dir.join("old.zip");
        write_zip(&old, &[("pack.mcmeta", &meta(48), false)]);
        let info = inspect_path(&old, &target()).expect("older pack is accepted");
        assert!(!info.warnings.is_empty());
        let ranged = dir.join("ranged.zip");
        let meta = br#"{"pack":{"pack_format":80,"supported_formats":{"min_inclusive":50,"max_inclusive":70}}}"#;
        write_zip(&ranged, &[("pack.mcmeta", meta, false)]);
        let info = inspect_path(&ranged, &target()).expect("supported range");
        assert_eq!(info.pack_format, 80);
        let _ = fs::remove_dir_all(&dir);
    }
}
