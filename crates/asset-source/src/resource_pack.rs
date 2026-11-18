use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::archive::Archive;
use crate::types::{
    ResourcePackKind, ResourcePackReport, ResourcePackStatus, TARGET_RESOURCE_PACK_FORMAT,
};

const MAX_PACK_ENTRIES: usize = 20_000;
const MAX_PACK_TOTAL_BYTES: u64 = 512 * 1024 * 1024;
const MAX_PACK_MCMETA_BYTES: u64 = 1024 * 1024;
const MAX_PACK_DEPTH: usize = 32;

/// Validates a resource pack `.zip` or folder without extracting or rendering it.
pub(crate) fn inspect_resource_pack(path: &Path) -> ResourcePackReport {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) => {
            return unreadable(
                path,
                ResourcePackKind::Folder,
                format!("resource pack cannot be read: {error}"),
            )
        }
    };
    if metadata.file_type().is_symlink() {
        return unreadable(
            path,
            ResourcePackKind::Folder,
            "resource pack is a symlink, junction or reparse point".to_owned(),
        );
    }
    if metadata.is_file() {
        inspect_zip(path, metadata.len())
    } else if metadata.is_dir() {
        inspect_folder(path)
    } else {
        unreadable(
            path,
            ResourcePackKind::Folder,
            "resource pack is not a regular file or directory".to_owned(),
        )
    }
}

fn inspect_zip(path: &Path, bytes: u64) -> ResourcePackReport {
    let name = pack_name(path);
    let mut report = ResourcePackReport {
        path: path.display().to_string(),
        name,
        kind: ResourcePackKind::Zip,
        bytes,
        pack_format: None,
        supported_formats: None,
        description: None,
        compatible: false,
        has_blockstates: false,
        has_textures: false,
        entry_count: 0,
        status: ResourcePackStatus::Unreadable,
        errors: Vec::new(),
        warnings: Vec::new(),
    };
    let mut archive = match Archive::open(path) {
        Ok(archive) => archive,
        Err(error) => {
            report.status = if error.is_unsafe_name() {
                ResourcePackStatus::UnsafeContents
            } else {
                ResourcePackStatus::Unreadable
            };
            report.errors.push(error.to_string());
            return report;
        }
    };
    report.entry_count = archive.entries().len();
    if archive.total_uncompressed() > MAX_PACK_TOTAL_BYTES {
        report.status = ResourcePackStatus::Unreadable;
        report
            .errors
            .push("resource pack expands beyond the uncompressed size limit".to_owned());
        return report;
    }
    if report.entry_count > MAX_PACK_ENTRIES {
        report.status = ResourcePackStatus::Unreadable;
        report
            .errors
            .push("resource pack contains too many entries".to_owned());
        return report;
    }
    for entry in archive.entries() {
        if entry.name.starts_with("assets/minecraft/blockstates/") && entry.name.ends_with(".json")
        {
            report.has_blockstates = true;
        }
        if entry.name.starts_with("assets/minecraft/textures/") {
            report.has_textures = true;
        }
    }
    inspect_metadata(
        &mut report,
        archive
            .read_entry("pack.mcmeta", MAX_PACK_MCMETA_BYTES)
            .ok(),
    );
    report
}

fn inspect_folder(path: &Path) -> ResourcePackReport {
    let name = pack_name(path);
    let mut report = ResourcePackReport {
        path: path.display().to_string(),
        name,
        kind: ResourcePackKind::Folder,
        bytes: 0,
        pack_format: None,
        supported_formats: None,
        description: None,
        compatible: false,
        has_blockstates: false,
        has_textures: false,
        entry_count: 0,
        status: ResourcePackStatus::Unreadable,
        errors: Vec::new(),
        warnings: Vec::new(),
    };
    let mut stack: Vec<(PathBuf, String, usize)> = vec![(path.to_path_buf(), String::new(), 0)];
    let mut metadata_bytes: Option<Vec<u8>> = None;
    while let Some((folder, relative, depth)) = stack.pop() {
        if depth > MAX_PACK_DEPTH {
            report
                .errors
                .push("resource pack directory nesting is too deep".to_owned());
            break;
        }
        let entries = match fs::read_dir(&folder) {
            Ok(entries) => entries,
            Err(error) => {
                report
                    .errors
                    .push(format!("cannot read {}: {error}", folder.display()));
                break;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    report
                        .errors
                        .push(format!("cannot read resource pack entry: {error}"));
                    break;
                }
            };
            let child_relative = if relative.is_empty() {
                entry.file_name().to_string_lossy().into_owned()
            } else {
                format!("{relative}/{}", entry.file_name().to_string_lossy())
            };
            let child_path = entry.path();
            let child_metadata = match fs::symlink_metadata(&child_path) {
                Ok(metadata) => metadata,
                Err(error) => {
                    report
                        .errors
                        .push(format!("cannot inspect {}: {error}", child_path.display()));
                    continue;
                }
            };
            if child_metadata.file_type().is_symlink() {
                report.warnings.push(format!(
                    "skipped symlink or junction inside the resource pack: {child_relative}"
                ));
                continue;
            }
            report.entry_count += 1;
            if report.entry_count > MAX_PACK_ENTRIES {
                report
                    .errors
                    .push("resource pack contains too many entries".to_owned());
                report.status = ResourcePackStatus::Unreadable;
                return report;
            }
            if child_metadata.is_dir() {
                stack.push((child_path, child_relative, depth + 1));
            } else if child_metadata.is_file() {
                report.bytes = report.bytes.saturating_add(child_metadata.len());
                if report.bytes > MAX_PACK_TOTAL_BYTES {
                    report
                        .errors
                        .push("resource pack exceeds the total size limit".to_owned());
                    report.status = ResourcePackStatus::Unreadable;
                    return report;
                }
                if child_relative.starts_with("assets/minecraft/blockstates/")
                    && child_relative.ends_with(".json")
                {
                    report.has_blockstates = true;
                }
                if child_relative.starts_with("assets/minecraft/textures/") {
                    report.has_textures = true;
                }
                if child_relative == "pack.mcmeta" && child_metadata.len() <= MAX_PACK_MCMETA_BYTES
                {
                    metadata_bytes = fs::read(&child_path).ok();
                }
            }
        }
    }
    inspect_metadata(&mut report, metadata_bytes);
    report
}

fn inspect_metadata(report: &mut ResourcePackReport, bytes: Option<Vec<u8>>) {
    let Some(bytes) = bytes else {
        report.status = ResourcePackStatus::MissingMetadata;
        report
            .errors
            .push("resource pack has no readable pack.mcmeta".to_owned());
        return;
    };
    let value: Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(error) => {
            report.status = ResourcePackStatus::InvalidMetadata;
            report
                .errors
                .push(format!("pack.mcmeta is not valid JSON: {error}"));
            return;
        }
    };
    let pack = match value.get("pack") {
        Some(pack) if pack.is_object() => pack,
        _ => {
            report.status = ResourcePackStatus::InvalidMetadata;
            report
                .errors
                .push("pack.mcmeta has no pack object".to_owned());
            return;
        }
    };
    report.pack_format = pack
        .get("pack_format")
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok());
    let range = parse_format_range(pack.get("supported_formats"));
    if let Some((min, max)) = range {
        report.supported_formats = Some(format!("{min}-{max}"));
    }
    report.description = pack.get("description").map(describe);
    let compatible = match range {
        Some((min, max)) => (min..=max).contains(&TARGET_RESOURCE_PACK_FORMAT),
        None => report.pack_format == Some(TARGET_RESOURCE_PACK_FORMAT),
    };
    report.compatible = compatible;
    report.status = if compatible {
        ResourcePackStatus::Compatible
    } else {
        ResourcePackStatus::IncompatibleFormat
    };
    if report.pack_format.is_none() && range.is_none() {
        report
            .errors
            .push("pack.mcmeta declares neither pack_format nor supported_formats".to_owned());
        report.status = ResourcePackStatus::InvalidMetadata;
        report.compatible = false;
    }
    if !report.has_blockstates {
        report
            .warnings
            .push("resource pack has no assets/minecraft/blockstates/*.json".to_owned());
    }
    if !report.has_textures {
        report
            .warnings
            .push("resource pack has no assets/minecraft/textures/**".to_owned());
    }
}

fn parse_format_range(value: Option<&Value>) -> Option<(i32, i32)> {
    let value = value?;
    if let Some(format) = value.as_i64().and_then(|value| i32::try_from(value).ok()) {
        return Some((format, format));
    }
    if let Some(items) = value.as_array() {
        if items.len() >= 2 {
            let min = items[0]
                .as_i64()
                .and_then(|value| i32::try_from(value).ok());
            let max = items[1]
                .as_i64()
                .and_then(|value| i32::try_from(value).ok());
            if let (Some(min), Some(max)) = (min, max) {
                if min <= max {
                    return Some((min, max));
                }
            }
        }
        return None;
    }
    let min = value
        .get("min_inclusive")
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok());
    let max = value
        .get("max_inclusive")
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok());
    match (min, max) {
        (Some(min), Some(max)) if min <= max => Some((min, max)),
        _ => None,
    }
}

fn describe(value: &Value) -> String {
    let text = match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    };
    let mut text = text.replace(['\r', '\n'], " ");
    if text.chars().count() > 200 {
        text = text.chars().take(200).collect::<String>() + "...";
    }
    text
}

fn pack_name(path: &Path) -> String {
    if path.is_dir() {
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string())
    } else {
        path.file_stem()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string())
    }
}

fn unreadable(path: &Path, kind: ResourcePackKind, error: String) -> ResourcePackReport {
    ResourcePackReport {
        path: path.display().to_string(),
        name: pack_name(path),
        kind,
        bytes: 0,
        pack_format: None,
        supported_formats: None,
        description: None,
        compatible: false,
        has_blockstates: false,
        has_textures: false,
        entry_count: 0,
        status: ResourcePackStatus::Unreadable,
        errors: vec![error],
        warnings: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_ranges_are_parsed_in_all_three_shapes() {
        assert_eq!(parse_format_range(Some(&Value::from(46))), Some((46, 46)));
        assert_eq!(
            parse_format_range(Some(&serde_json::json!([34, 60]))),
            Some((34, 60))
        );
        assert_eq!(
            parse_format_range(Some(&serde_json::json!({
                "min_inclusive": 40,
                "max_inclusive": 50
            }))),
            Some((40, 50))
        );
        assert_eq!(parse_format_range(Some(&serde_json::json!([60, 34]))), None);
    }

    #[test]
    fn descriptions_are_bounded_and_single_line() {
        let description = describe(&Value::String("line one\nline two".to_owned()));
        assert_eq!(description, "line one line two");
        let long = describe(&Value::String("x".repeat(300)));
        assert!(long.chars().count() <= 203);
        assert!(long.ends_with("..."));
    }
}
