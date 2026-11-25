//! Header-only inspection of the Java 26.1+ save layout. This is not a world codec:
//! the shared MCA container reader must not be used to decode split chunk payloads.

use std::collections::BTreeSet;
use std::fs::{self, Metadata};
use std::io::ErrorKind;
use std::path::Path;

use minecraft_version::{profile_for_data_version, WorldLayout};

use crate::anvil::{
    read_file_bounded, LevelDat, RegionFile, MAX_LEVEL_DAT_BYTES, MAX_REGION_FILES,
};
use crate::nbt;
use crate::passthrough::is_link_like;
use crate::world_selection::{relative_folder, SelectionDimension, WorldSelectionReport};

pub(crate) const LAYOUT: &str = "split_26_1_plus";
const BLOCKER: &str =
    "26.1+ split saves can be inspected, but chunk decoding, import and export are not verified";
const DIMENSIONS: &str = "dimensions";
const PLAYERS: &str = "players";
const PLAYER_FOLDERS: [&str; 3] = ["data", "advancements", "stats"];
const WORLD_GEN: &str = "world_gen_settings.dat";
const MAX_STORAGE_ENTRIES: usize = 20_000;
const MAX_DIMENSION_DEPTH: usize = 16;

pub(crate) fn matches_release(level: &LevelDat) -> bool {
    profile_for_data_version(level.data_version)
        .is_some_and(|profile| profile.world_layout == WorldLayout::SplitAnvil)
}

pub(crate) fn has_markers(root: &Path) -> bool {
    let dimensions = root.join(DIMENSIONS);
    let vanilla = dimensions.join("minecraft");
    let players = root.join(PLAYERS);
    let data = root.join("data");
    let shared = data.join("minecraft");
    (is_real_dir(&dimensions)
        && is_real_dir(&vanilla)
        && ["overworld", "the_nether", "the_end"]
            .iter()
            .any(|name| vanilla.join(name).symlink_metadata().is_ok()))
        || (is_real_dir(&players)
            && PLAYER_FOLDERS
                .iter()
                .any(|name| players.join(name).symlink_metadata().is_ok()))
        || (is_real_dir(&data)
            && is_real_dir(&shared)
            && shared.join(WORLD_GEN).symlink_metadata().is_ok())
}

pub(crate) fn inspect(root: &Path, level: Option<&LevelDat>, report: &mut WorldSelectionReport) {
    if let Some(level) = level {
        if let Some(profile) = profile_for_data_version(level.data_version) {
            if profile.world_layout != WorldLayout::SplitAnvil
                || level.version_name.as_deref() != Some(profile.minecraft_version.as_str())
            {
                report.blockers.push(format!(
                    "split layout and level.dat identity disagree: DataVersion {} is Java {} ({:?}), found Version.Name {:?}",
                    level.data_version, profile.minecraft_version, profile.world_layout, level.version_name
                ));
            }
        }
    }
    match declared_dimensions(root) {
        Ok(ids) => report.declared_dimensions = ids,
        Err(error) => report.blockers.push(error),
    }
    match player_folders(root) {
        Ok(folders) if !folders.is_empty() => {
            report.warnings.push(format!(
                "player storage detected: {}; contents were not read",
                folders.join(", ")
            ));
            report.related_folders.extend(folders);
        }
        Ok(_) => {}
        Err(error) => report.blockers.push(error),
    }
    match scan_dimensions(root) {
        Ok(dimensions) => {
            if !dimensions.iter().any(|d| d.id == "minecraft:overworld") {
                report.blockers.push(
                    "split Overworld terrain folder dimensions/minecraft/overworld/region is missing"
                        .to_owned(),
                );
            }
            report.region_files = dimensions.iter().map(|d| d.region_files).sum();
            report.chunks = dimensions.iter().map(|d| d.chunks).sum();
            report.dimensions = dimensions;
        }
        Err(error) => report.blockers.push(error),
    };
    report.complete_save = level.is_some() && report.blockers.is_empty();
    report.blockers.push(BLOCKER.to_owned());
    report.warnings.push(
        "split chunk counts are allocated terrain slots from MCA headers; block, entity, POI and player payloads were not decoded"
            .to_owned(),
    );
}

fn declared_dimensions(root: &Path) -> Result<Vec<String>, String> {
    let data = root.join("data");
    let namespace = data.join("minecraft");
    let path = namespace.join(WORLD_GEN);
    for parent in [data, namespace] {
        if !directory(&parent)? {
            return Err(format!("missing {}", path.display()));
        }
    }
    // The common bounded reader checks size and file type; reject reparse points too.
    if !checked_metadata(&path)?.is_some_and(|metadata| metadata.is_file()) {
        return Err(format!("missing or invalid {}", path.display()));
    }
    let bytes = read_file_bounded(&path, MAX_LEVEL_DAT_BYTES).map_err(|error| error.to_string())?;
    let nbt = nbt::read_gzip(&bytes).map_err(|error| format!("{}: {error}", path.display()))?;
    let data = nbt
        .root
        .as_compound()
        .and_then(|root| root.get("data"))
        .and_then(nbt::NbtValue::as_compound)
        .ok_or_else(|| format!("{} has no data compound", path.display()))?;
    if !matches!(data.get("seed"), Some(nbt::NbtValue::Long(_))) {
        return Err(format!("{} has no long seed field", path.display()));
    }
    // Datapack-defined dimensions need not be listed in this file. This inventory
    // reports only saved declarations and does not resolve dimension types or packs.
    let Some(dimensions) = data.get("dimensions") else {
        return Ok(Vec::new());
    };
    let dimensions = dimensions
        .as_compound()
        .ok_or_else(|| format!("{} data.dimensions is not a compound", path.display()))?;
    if dimensions.len() > MAX_STORAGE_ENTRIES {
        return Err(format!(
            "{} has too many dimension declarations",
            path.display()
        ));
    }
    Ok(dimensions.keys().cloned().collect())
}

fn player_folders(root: &Path) -> Result<Vec<String>, String> {
    let players = root.join(PLAYERS);
    let mut folders = Vec::new();
    if directory(&players)? {
        for name in PLAYER_FOLDERS {
            let folder = players.join(name);
            if directory(&folder)? {
                folders.push(relative_folder(root, &folder));
            }
        }
    }
    Ok(folders)
}

fn scan_dimensions(root: &Path) -> Result<Vec<SelectionDimension>, String> {
    let dimensions = root.join(DIMENSIONS);
    if !directory(&dimensions)? {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    let mut pending = vec![(dimensions, 0)];
    let mut budget = MAX_STORAGE_ENTRIES;
    let mut regions = 0;
    while let Some((folder, depth)) = pending.pop() {
        for entry in entries(&folder)? {
            spend_entry(&mut budget)?;
            let path = entry
                .map_err(|error| format!("{}: {error}", folder.display()))?
                .path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !name
                .bytes()
                .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || b"_.-".contains(&ch))
            {
                continue;
            }
            if depth >= 2 && name == "region" {
                if !directory(&path)? {
                    return Err(format!("missing {}", path.display()));
                }
                let (count, chunks) = scan_regions(&path, &mut budget, &mut regions)?;
                let relative = relative_folder(root, &folder);
                let (namespace, dimension) = relative
                    .strip_prefix("dimensions/")
                    .and_then(|path| path.split_once('/'))
                    .expect("dimension path comes from the dimensions directory");
                result.push(SelectionDimension {
                    id: format!("{namespace}:{dimension}"),
                    folder: relative,
                    region_files: count,
                    chunks,
                });
            } else if !(depth >= 2 && matches!(name, "entities" | "poi" | "data"))
                && checked_metadata(&path)?.is_some_and(|metadata| metadata.is_dir())
            {
                if depth >= MAX_DIMENSION_DEPTH {
                    return Err(format!(
                        "split dimension path is too deep: {}",
                        path.display()
                    ));
                }
                pending.push((path, depth + 1));
            }
        }
    }
    result.sort_unstable_by(|a, b| a.id.cmp(&b.id));
    Ok(result)
}

fn scan_regions(
    folder: &Path,
    budget: &mut usize,
    regions: &mut usize,
) -> Result<(usize, usize), String> {
    let mut coords = BTreeSet::new();
    let mut chunks = 0;
    for entry in entries(folder)? {
        spend_entry(budget)?;
        let path = entry
            .map_err(|error| format!("{}: {error}", folder.display()))?
            .path();
        let extension = path.extension().and_then(|value| value.to_str());
        if extension.is_some_and(|ext| ext.eq_ignore_ascii_case("mcr")) {
            return Err(format!(
                "legacy region in split terrain folder: {}",
                path.display()
            ));
        }
        if !extension.is_some_and(|ext| ext.eq_ignore_ascii_case("mca")) {
            continue;
        }
        *regions += 1;
        if *regions > MAX_REGION_FILES {
            return Err(format!(
                "split save exceeds the {MAX_REGION_FILES} terrain region header limit"
            ));
        }
        if !checked_metadata(&path)?.is_some_and(|metadata| metadata.is_file()) {
            return Err(format!("not a regular region file: {}", path.display()));
        }
        let region = RegionFile::open(&path).map_err(|error| error.to_string())?;
        if !coords.insert(region.region_coordinates()) {
            return Err(format!("duplicate region coordinates: {}", path.display()));
        }
        chunks += region.entries().len();
    }
    Ok((coords.len(), chunks))
}

fn spend_entry(budget: &mut usize) -> Result<(), String> {
    *budget = budget
        .checked_sub(1)
        .ok_or_else(|| format!("split storage exceeds the {MAX_STORAGE_ENTRIES} entry limit"))?;
    Ok(())
}

fn entries(path: &Path) -> Result<fs::ReadDir, String> {
    fs::read_dir(path).map_err(|error| format!("{}: {error}", path.display()))
}

fn checked_metadata(path: &Path) -> Result<Option<Metadata>, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if is_link_like(&metadata) => Err(format!(
            "refusing to follow link or reparse point: {}",
            path.display()
        )),
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("{}: {error}", path.display())),
    }
}

fn directory(path: &Path) -> Result<bool, String> {
    match checked_metadata(path)? {
        Some(metadata) if metadata.is_dir() => Ok(true),
        None => Ok(false),
        Some(_) => Err(format!("not a directory: {}", path.display())),
    }
}

fn is_real_dir(path: &Path) -> bool {
    matches!(directory(path), Ok(true))
}
