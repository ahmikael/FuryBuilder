//! Classification of a user selection before import or export (M8.11).
//!
//! A region `.mca` file is a 32x32 chunk container. A `region/` directory is a folder of
//! such containers. Neither is a complete Java world save: a save also needs `level.dat`,
//! its dimensions and any related sidecar folders. This module answers that question
//! explicitly instead of letting a region file or a region folder look like an empty world.
//!
//! The classifier is read-only and never decodes chunk payloads. It validates `level.dat`,
//! the layout markers and every region header so the wizard can block an unsupported
//! selection with a concrete reason. Full chunk validation remains the job of
//! [`crate::analyze_java_world`].

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::anvil::{parse_region_file_name, JavaWorld, LevelDat, RegionFile};
use crate::version_support;

/// Layout identifier used by the supported classic Anvil reader.
pub const CLASSIC_ANVIL_LAYOUT: &str = "classic_anvil";

/// Known folders that a Java save may contain next to `level.dat`.
const KNOWN_FOLDERS: &[&str] = &[
    "region",
    "DIM-1",
    "DIM1",
    "dimensions",
    "poi",
    "entities",
    "data",
    "playerdata",
    "players",
    "datapacks",
    "stats",
    "advancements",
    "structures",
    "serverconfig",
];

/// What the selected path actually is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorldSelectionKind {
    /// A directory that contains `level.dat` or classic/split world markers.
    WorldFolder,
    /// A single `r.<x>.<z>.mca` region container.
    RegionFile,
    /// A directory of region files without `level.dat`.
    RegionDirectory,
    /// The path does not exist.
    NotFound,
    /// The path is a symlink, junction or other reparse point.
    UnsafeLink,
    /// A file or directory that is not recognisable as any of the above.
    Unknown,
}

impl WorldSelectionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            WorldSelectionKind::WorldFolder => "world_folder",
            WorldSelectionKind::RegionFile => "region_file",
            WorldSelectionKind::RegionDirectory => "region_directory",
            WorldSelectionKind::NotFound => "not_found",
            WorldSelectionKind::UnsafeLink => "unsafe_link",
            WorldSelectionKind::Unknown => "unknown",
        }
    }
}

/// One region container found inside a selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionRegionFile {
    pub file_name: String,
    pub region_x: i32,
    pub region_z: i32,
    pub bytes: u64,
    pub chunks: usize,
}

/// One dimension folder found inside a selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionDimension {
    pub id: String,
    pub folder: String,
    pub region_files: usize,
    pub chunks: usize,
}

/// Complete, read-only description of what the user selected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldSelectionReport {
    pub path: String,
    pub kind: WorldSelectionKind,
    pub layout: String,
    /// True only for a folder that has `level.dat` and classic/split world markers.
    ///
    /// This is structural completeness, not support. A complete 26.1+ split save still
    /// reports `supported = false`.
    pub complete_save: bool,
    /// True only for a complete, structurally valid save of a codec-backed version.
    pub supported: bool,
    pub has_level_dat: bool,
    pub data_version: Option<i32>,
    pub version_name: Option<String>,
    pub level_name: Option<String>,
    pub dimensions: Vec<SelectionDimension>,
    pub related_folders: Vec<String>,
    pub region_file: Option<SelectionRegionFile>,
    pub region_files: usize,
    pub chunks: usize,
    pub blockers: Vec<String>,
    pub warnings: Vec<String>,
}

/// Classifies a path without modifying it.
pub fn classify_world_selection(path: impl AsRef<Path>) -> WorldSelectionReport {
    let path = path.as_ref();
    let mut report = WorldSelectionReport {
        path: path.display().to_string(),
        kind: WorldSelectionKind::Unknown,
        layout: "unknown".to_owned(),
        complete_save: false,
        supported: false,
        has_level_dat: false,
        data_version: None,
        version_name: None,
        level_name: None,
        dimensions: Vec::new(),
        related_folders: Vec::new(),
        region_file: None,
        region_files: 0,
        chunks: 0,
        blockers: Vec::new(),
        warnings: Vec::new(),
    };

    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            report.kind = WorldSelectionKind::NotFound;
            report.blockers.push(format!(
                "the selected path does not exist: {}",
                path.display()
            ));
            return report;
        }
        Err(error) => {
            report
                .blockers
                .push(format!("cannot read {}: {error}", path.display()));
            return report;
        }
    };
    if metadata.file_type().is_symlink() {
        report.kind = WorldSelectionKind::UnsafeLink;
        report.blockers.push(
            "the selection is a symlink, junction or reparse point; select the real world folder"
                .to_owned(),
        );
        return report;
    }
    if metadata.is_file() {
        classify_file(path, metadata.len(), &mut report);
        return report;
    }
    if !metadata.is_dir() {
        report
            .blockers
            .push("the selection is not a regular file or directory".to_owned());
        return report;
    }
    classify_directory(path, &mut report);
    report
}

fn classify_file(path: &Path, bytes: u64, report: &mut WorldSelectionReport) {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let name = path
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    if extension == "mca" {
        report.kind = WorldSelectionKind::RegionFile;
        report.layout = "region_file".to_owned();
        let coordinates = parse_region_file_name(path);
        let chunks = match RegionFile::open(path) {
            Ok(region) => region.entries().len(),
            Err(error) => {
                report
                    .blockers
                    .push(format!("region file {name} is unreadable: {error}"));
                0
            }
        };
        let (region_x, region_z) = match coordinates {
            Ok((x, z)) => (x, z),
            Err(error) => {
                report
                    .blockers
                    .push(format!("region file name {name} is invalid: {error}"));
                (0, 0)
            }
        };
        report.region_files = 1;
        report.chunks = chunks;
        report.region_file = Some(SelectionRegionFile {
            file_name: name.clone(),
            region_x,
            region_z,
            bytes,
            chunks,
        });
        report.blockers.push(format!(
            "{name} is a single region container, not a complete world save: it has no level.dat, dimensions or sidecar folders. Select the world folder that contains it."
        ));
        report.warnings.push(
            "a region file can be inspected as a container, but it cannot be opened as a project"
                .to_owned(),
        );
        return;
    }
    if extension == "mcr" {
        report.kind = WorldSelectionKind::RegionFile;
        report.layout = "legacy_mcr".to_owned();
        report.region_files = 1;
        report.blockers.push(format!(
            "{name} is a legacy McRegion file, not a complete world save; the legacy format is not supported"
        ));
        return;
    }
    report.blockers.push(format!(
        "{name} is not a Java world folder or region container"
    ));
}

fn classify_directory(root: &Path, report: &mut WorldSelectionReport) {
    report.has_level_dat = is_file(&root.join("level.dat"));
    report.related_folders = KNOWN_FOLDERS
        .iter()
        .filter(|name| is_dir(&root.join(name)))
        .map(|name| (*name).to_owned())
        .collect();
    let direct_mca = count_direct_files(root, "mca");
    let direct_mcr = count_direct_files(root, "mcr");
    let layout = detect_layout(root);

    if !report.has_level_dat && direct_mca > 0 && direct_mcr == 0 {
        report.kind = WorldSelectionKind::RegionDirectory;
        report.layout = "region_directory".to_owned();
        report.region_files = direct_mca;
        report.blockers.push(format!(
            "the selected folder contains {direct_mca} region file(s) but no level.dat; it looks like a region/ directory. Select its parent world folder instead."
        ));
        report.warnings.push(
            "a region directory is only a part of a save; metadata, dimensions and sidecars live next to it"
                .to_owned(),
        );
        return;
    }
    if !report.has_level_dat && direct_mcr > 0 {
        report.kind = WorldSelectionKind::RegionDirectory;
        report.layout = "legacy_mcr".to_owned();
        report.region_files = direct_mcr;
        report.blockers.push(
            "the selected folder contains legacy .mcr region files and no level.dat; the legacy format is not supported"
                .to_owned(),
        );
        return;
    }

    if !report.has_level_dat && !has_any_world_marker(root) {
        report
            .blockers
            .push("the selected folder is not a Java world save: no level.dat, region/ or split dimension markers were found".to_owned());
        return;
    }

    report.kind = WorldSelectionKind::WorldFolder;
    report.layout = layout.to_owned();

    match LevelDat::read(root.join("level.dat")) {
        Ok(level) => {
            report.data_version = Some(level.data_version);
            report.version_name = level.version_name.clone();
            report.level_name = level.level_name.clone();
        }
        Err(error) => report
            .blockers
            .push(format!("level.dat is missing or unreadable: {error}")),
    }

    if layout == CLASSIC_ANVIL_LAYOUT {
        match JavaWorld::open(root) {
            Ok(world) => {
                report.complete_save = report.has_level_dat;
                inventory_classic(&world, root, report);
            }
            Err(error) => report
                .blockers
                .push(format!("the classic world cannot be opened: {error}")),
        }
    } else {
        report.complete_save =
            report.has_level_dat && matches!(layout, "split_26_1_plus" | "mixed" | "legacy_mcr");
        match layout {
            "split_26_1_plus" => report
                .blockers
                .push("26.1+ split world layout is not supported".to_owned()),
            "mixed" => report.blockers.push(
                "the world mixes classic region/ files with 26.1+ split layout markers".to_owned(),
            ),
            "legacy_mcr" => report
                .blockers
                .push("the legacy McRegion layout is detected but not supported".to_owned()),
            "level_dat_only" => report.blockers.push(
                "level.dat exists but no classic region/ or split dimension folders were found; this is not a complete save"
                    .to_owned(),
            ),
            _ => report
                .blockers
                .push("the world layout is not recognised".to_owned()),
        }
        inventory_best_effort(root, report);
    }

    if !report.complete_save {
        report.warnings.push(
            "an incomplete selection is never copied or imported; choose a folder that contains level.dat and its dimensions"
                .to_owned(),
        );
    }
    report.supported =
        report.complete_save && report.layout == CLASSIC_ANVIL_LAYOUT && report.blockers.is_empty();
    if report.supported {
        report.warnings.push(format!(
            "the selection is a complete classic {} save; full chunk validation runs before import",
            version_support::default_version_profile().minecraft_version
        ));
    }
}

fn inventory_classic(world: &JavaWorld, root: &Path, report: &mut WorldSelectionReport) {
    for dimension in &world.dimensions {
        let coordinates = match world.region_coordinates(&dimension.id) {
            Ok(coordinates) => coordinates,
            Err(error) => {
                report.blockers.push(format!(
                    "dimension {} cannot be listed: {error}",
                    dimension.id
                ));
                continue;
            }
        };
        let mut region_files = 0usize;
        let mut chunks = 0usize;
        for (region_x, region_z) in coordinates {
            match world.read_region(&dimension.id, region_x, region_z) {
                Ok(region) => {
                    region_files += 1;
                    chunks += region.entries().len();
                }
                Err(error) => report.blockers.push(format!(
                    "region ({region_x}, {region_z}) in {} is unreadable: {error}",
                    dimension.id
                )),
            }
        }
        report.region_files += region_files;
        report.chunks += chunks;
        report.dimensions.push(SelectionDimension {
            id: dimension.id.clone(),
            folder: relative_folder(root, &dimension.folder),
            region_files,
            chunks,
        });
    }
}

fn inventory_best_effort(root: &Path, report: &mut WorldSelectionReport) {
    let candidates = vec![
        ("minecraft:overworld", PathBuf::from("region")),
        (
            "minecraft:the_nether",
            PathBuf::from("DIM-1").join("region"),
        ),
        ("minecraft:the_end", PathBuf::from("DIM1").join("region")),
        (
            "minecraft:overworld",
            PathBuf::from("dimensions")
                .join("minecraft")
                .join("overworld")
                .join("region"),
        ),
        (
            "minecraft:the_nether",
            PathBuf::from("dimensions")
                .join("minecraft")
                .join("the_nether")
                .join("region"),
        ),
        (
            "minecraft:the_end",
            PathBuf::from("dimensions")
                .join("minecraft")
                .join("the_end")
                .join("region"),
        ),
    ];
    for (id, relative) in candidates {
        let folder = root.join(&relative);
        if !is_dir(&folder) {
            continue;
        }
        let (region_files, chunks) = count_region_folder(&folder, report);
        report.region_files += region_files;
        report.chunks += chunks;
        report.dimensions.push(SelectionDimension {
            id: id.to_owned(),
            folder: relative_folder(root, &folder),
            region_files,
            chunks,
        });
    }
}

fn count_region_folder(folder: &Path, report: &mut WorldSelectionReport) -> (usize, usize) {
    let mut files = 0usize;
    let mut chunks = 0usize;
    let Ok(entries) = fs::read_dir(folder) else {
        return (0, 0);
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if extension != "mca" {
            continue;
        }
        files += 1;
        match RegionFile::open(&path) {
            Ok(region) => chunks += region.entries().len(),
            Err(error) => report.blockers.push(format!(
                "region file {} is unreadable: {error}",
                path.display()
            )),
        }
    }
    (files, chunks)
}

fn relative_folder(root: &Path, folder: &Path) -> String {
    let relative = folder.strip_prefix(root).unwrap_or(folder);
    if relative.as_os_str().is_empty() {
        return ".".to_owned();
    }
    relative.to_string_lossy().replace('\\', "/")
}

fn count_direct_files(root: &Path, extension: &str) -> usize {
    let Ok(entries) = fs::read_dir(root) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|entry| {
            entry
                .path()
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case(extension))
        })
        .count()
}

/// Detects the world layout from folder markers only; no `level.dat` is decoded.
pub(crate) fn detect_layout(root: &Path) -> &'static str {
    let region_dir = is_dir(&root.join("region"));
    let classic = region_dir && has_region_file(&root.join("region"), "mca");
    let split = has_split_marker(root);
    let legacy = region_dir && has_legacy_region_file(&root.join("region"));
    if classic && split {
        "mixed"
    } else if split {
        "split_26_1_plus"
    } else if legacy && !classic {
        "legacy_mcr"
    } else if region_dir {
        CLASSIC_ANVIL_LAYOUT
    } else if is_file(&root.join("level.dat")) {
        "level_dat_only"
    } else {
        "unknown"
    }
}

fn has_any_world_marker(root: &Path) -> bool {
    is_dir(&root.join("region")) || has_split_marker(root)
}

fn has_split_marker(root: &Path) -> bool {
    is_dir(&root.join("dimensions").join("minecraft").join("overworld"))
        || (is_dir(&root.join("players"))
            && is_file(
                &root
                    .join("data")
                    .join("minecraft")
                    .join("world_gen_settings.dat"),
            ))
}

fn is_dir(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.is_dir())
        .unwrap_or(false)
}

fn is_file(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.is_file())
        .unwrap_or(false)
}

fn has_legacy_region_file(region_dir: &Path) -> bool {
    has_region_file(region_dir, "mcr")
}

fn has_region_file(region_dir: &Path, extension: &str) -> bool {
    let Ok(entries) = fs::read_dir(region_dir) else {
        return false;
    };
    entries.flatten().any(|entry| {
        entry
            .path()
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case(extension))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_paths_are_classified_without_panicking() {
        let path = std::env::temp_dir().join("furybuilder-selection-missing");
        let report = classify_world_selection(&path);
        assert_eq!(report.kind, WorldSelectionKind::NotFound);
        assert!(!report.complete_save);
        assert!(!report.supported);
        assert!(!report.blockers.is_empty());
    }
}
