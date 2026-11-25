use std::fs::{self, Metadata};
use std::io::Read;
use std::path::{Path, PathBuf};

use minecraft_version::VersionProfile;
use serde_json::Value;

use crate::archive::{Archive, ArchiveError};
use crate::types::{ResourcePackKind, ResourcePackReport, ResourcePackStatus};

const MAX_PACK_ENTRIES: usize = 20_000;
pub(crate) const MAX_PACK_TOTAL_BYTES: u64 = 512 * 1024 * 1024;
const MAX_PACK_MCMETA_BYTES: u64 = 1024 * 1024;
const MAX_PACK_DEPTH: usize = 32;

/// Validates a resource pack `.zip` or folder without extracting or rendering it.
pub(crate) fn inspect_resource_pack(path: &Path, profile: &VersionProfile) -> ResourcePackReport {
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
    if is_link(&metadata) {
        return unreadable(
            path,
            ResourcePackKind::Folder,
            "resource pack is a symlink, junction or reparse point".to_owned(),
        );
    }
    if metadata.is_file() {
        inspect_zip(path, metadata.len(), Archive::open(path), profile)
    } else if metadata.is_dir() {
        inspect_folder(path, profile)
    } else {
        unreadable(
            path,
            ResourcePackKind::Folder,
            "resource pack is not a regular file or directory".to_owned(),
        )
    }
}

fn inspect_zip(
    path: &Path,
    bytes: u64,
    archive: Result<Archive, ArchiveError>,
    profile: &VersionProfile,
) -> ResourcePackReport {
    match archive {
        Ok(mut archive) => inspect_open_zip(path, bytes, &mut archive, profile),
        Err(error) => {
            let mut report = unreadable(path, ResourcePackKind::Zip, error.to_string());
            report.bytes = bytes;
            if error.is_unsafe_name() {
                report.status = ResourcePackStatus::UnsafeContents;
            }
            report
        }
    }
}

pub(crate) fn inspect_open_zip(
    path: &Path,
    bytes: u64,
    archive: &mut Archive,
    profile: &VersionProfile,
) -> ResourcePackReport {
    let mut report = empty_report(path, ResourcePackKind::Zip);
    report.bytes = bytes;
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
        profile,
    );
    report
}

fn inspect_folder(path: &Path, profile: &VersionProfile) -> ResourcePackReport {
    let mut report = empty_report(path, ResourcePackKind::Folder);
    let listing = match folder_files(path) {
        Ok(entries) => entries,
        Err(error) => {
            report.errors.push(error.to_string());
            return report;
        }
    };
    report.entry_count = listing.entry_count;
    report.bytes = listing.bytes;
    let mut metadata_bytes = None;
    for (name, path) in listing.files {
        if name.starts_with("assets/minecraft/blockstates/") && name.ends_with(".json") {
            report.has_blockstates = true;
        }
        if name.starts_with("assets/minecraft/textures/") {
            report.has_textures = true;
        }
        if name == "pack.mcmeta" {
            metadata_bytes = fs::File::open(path).ok().and_then(|file| {
                let mut bytes = Vec::new();
                file.take(MAX_PACK_MCMETA_BYTES + 1)
                    .read_to_end(&mut bytes)
                    .ok()?;
                (bytes.len() as u64 <= MAX_PACK_MCMETA_BYTES).then_some(bytes)
            });
        }
    }
    inspect_metadata(&mut report, metadata_bytes, profile);
    report
}

pub(crate) struct FolderListing {
    pub files: Vec<(String, PathBuf)>,
    entry_count: usize,
    bytes: u64,
}

/// Bounded, deterministic walk used for both inspection and local folder imports.
pub(crate) fn folder_files(path: &Path) -> std::io::Result<FolderListing> {
    let mut stack: Vec<(PathBuf, String, usize)> = vec![(path.to_path_buf(), String::new(), 0)];
    let mut files = Vec::new();
    let mut count = 0;
    let mut total = 0;
    while let Some((folder, relative, depth)) = stack.pop() {
        if depth > MAX_PACK_DEPTH {
            return Err(std::io::Error::other(
                "resource pack directory nesting is too deep",
            ));
        }
        if is_link(&fs::symlink_metadata(&folder)?) {
            return Err(std::io::Error::other(
                "resource pack contains a linked directory",
            ));
        }
        for entry in fs::read_dir(&folder)? {
            let entry = entry?;
            let name = entry.file_name().into_string().map_err(|_| {
                std::io::Error::other("resource pack contains a non-UTF-8 entry name")
            })?;
            if name.contains(['\\', ':']) {
                return Err(std::io::Error::other(
                    "resource pack contains an unsafe entry name",
                ));
            }
            let child_relative = if relative.is_empty() {
                name
            } else {
                format!("{relative}/{name}")
            };
            let child_path = entry.path();
            let child_metadata = fs::symlink_metadata(&child_path)?;
            if is_link(&child_metadata) {
                return Err(std::io::Error::other(format!(
                    "resource pack contains a symlink or junction: {child_relative}"
                )));
            }
            count += 1;
            if count > MAX_PACK_ENTRIES {
                return Err(std::io::Error::other(
                    "resource pack contains too many entries",
                ));
            }
            if child_metadata.is_dir() {
                stack.push((child_path, child_relative, depth + 1));
            } else if child_metadata.is_file() {
                total += child_metadata.len().min(MAX_PACK_TOTAL_BYTES + 1);
                if total > MAX_PACK_TOTAL_BYTES {
                    return Err(std::io::Error::other(
                        "resource pack exceeds the total size limit",
                    ));
                }
                files.push((child_relative, child_path));
            } else {
                return Err(std::io::Error::other(
                    "resource pack contains a non-regular entry",
                ));
            }
        }
    }
    files.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    Ok(FolderListing {
        files,
        entry_count: count,
        bytes: total,
    })
}

pub(crate) fn is_link(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    metadata.file_type().is_symlink()
}

fn inspect_metadata(
    report: &mut ResourcePackReport,
    bytes: Option<Vec<u8>>,
    profile: &VersionProfile,
) {
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
    report.description = pack.get("description").map(describe);
    let Some(major) = profile.resource_pack_format else {
        report
            .errors
            .push("project version has no known resource pack format".to_owned());
        report.status = ResourcePackStatus::IncompatibleFormat;
        return;
    };
    let (range, target) = if let Some(minors) = profile.pack_format_minors {
        let range = format_bound(pack.get("min_format"), 0)
            .zip(format_bound(pack.get("max_format"), u32::MAX));
        (range, (major, minors.resource))
    } else {
        let range = if pack.get("supported_formats").is_some() {
            parse_format_range(pack.get("supported_formats"))
                .and_then(|(min, max)| Some(((min.try_into().ok()?, 0), (max.try_into().ok()?, 0))))
        } else {
            report.pack_format.and_then(|format| {
                let format = u32::try_from(format).ok()?;
                Some(((format, 0), (format, 0)))
            })
        };
        (range, (major, 0))
    };
    let Some((min, max)) = range.filter(|(min, max)| min.0 > 0 && min <= max) else {
        report.errors.push(
            "pack.mcmeta has no valid resource pack format declaration for this version".to_owned(),
        );
        report.status = ResourcePackStatus::InvalidMetadata;
        return;
    };
    if profile.pack_format_minors.is_some() {
        let upper = if max.1 == u32::MAX {
            max.0.to_string()
        } else {
            format!("{}.{}", max.0, max.1)
        };
        report.supported_formats = Some(format!("{}.{}-{upper}", min.0, min.1));
    } else if pack.get("supported_formats").is_some() {
        report.supported_formats = Some(format!("{}-{}", min.0, max.0));
    }
    let compatible = (min..=max).contains(&target);
    report.compatible = compatible;
    report.status = if compatible {
        ResourcePackStatus::Compatible
    } else {
        ResourcePackStatus::IncompatibleFormat
    };
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

fn format_bound(value: Option<&Value>, default_minor: u32) -> Option<(u32, u32)> {
    fn number(value: &Value) -> Option<u32> {
        value.as_u64()?.try_into().ok()
    }
    let value = value?;
    match value.as_array().map(Vec::as_slice) {
        Some([major, minor]) => Some((number(major)?, number(minor)?)),
        Some([major]) => Some((number(major)?, default_minor)),
        Some(_) => None,
        None => Some((number(value)?, default_minor)),
    }
}

fn parse_format_range(value: Option<&Value>) -> Option<(i32, i32)> {
    let value = value?;
    if let Some(format) = value.as_i64().and_then(|value| i32::try_from(value).ok()) {
        return Some((format, format));
    }
    if let Some(items) = value.as_array() {
        if items.len() == 2 {
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
    let mut report = empty_report(path, kind);
    report.errors.push(error);
    report
}

fn empty_report(path: &Path, kind: ResourcePackKind) -> ResourcePackReport {
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
        errors: Vec::new(),
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

    #[test]
    fn modern_pack_bounds_compare_the_exact_major_and_minor() {
        let profile = minecraft_version::profile_for_version(&"26.3".parse().unwrap()).unwrap();
        let major = profile.resource_pack_format.unwrap();
        let minor = profile.pack_format_minors.unwrap().resource;
        let from_bounds = |min, max| {
            let mut report = empty_report(Path::new("test.zip"), ResourcePackKind::Zip);
            let bytes = serde_json::to_vec(&serde_json::json!({
                "pack": { "min_format": min, "max_format": max },
            }))
            .unwrap();
            inspect_metadata(&mut report, Some(bytes), &profile);
            report.status
        };
        assert_eq!(
            from_bounds(serde_json::json!([major, minor]), serde_json::json!(major)),
            ResourcePackStatus::Compatible,
        );
        assert_eq!(
            from_bounds(
                serde_json::json!([major, minor + 1]),
                serde_json::json!(major)
            ),
            ResourcePackStatus::IncompatibleFormat,
        );
        assert_eq!(
            from_bounds(
                serde_json::json!([major, minor + 1]),
                serde_json::json!([major, minor])
            ),
            ResourcePackStatus::InvalidMetadata,
        );
    }
}
