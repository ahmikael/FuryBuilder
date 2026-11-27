use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::jar::{inspect_client_jar, sha1_hex, ClientJarInspection, ExpectedClient};
use crate::resource_pack::inspect_resource_pack;
use crate::target::{target_data_version, target_minecraft_version, target_resource_pack_format};
use crate::types::{
    AssetDiscoveryReport, AssetPathReport, AssetSelectionKind, InstallationReport,
    InstallationSource, ResourcePackReport, VersionReport, VersionStatus,
};

const MAX_VERSION_ENTRIES: usize = 512;
const MAX_RESOURCE_PACKS: usize = 64;
const MAX_METADATA_BYTES: u64 = 8 * 1024 * 1024;
const MAX_ASSET_INDEX_BYTES: u64 = 32 * 1024 * 1024;
const MAX_PROFILES: usize = 32;

/// The resource pack format of the current asset target.
pub fn default_resource_pack_format() -> i32 {
    target_resource_pack_format()
}

#[derive(Debug, Deserialize)]
pub(crate) struct VersionMetadata {
    pub(crate) id: Option<String>,
    #[serde(rename = "type")]
    version_type: Option<String>,
    #[serde(rename = "releaseTime")]
    release_time: Option<String>,
    #[serde(rename = "inheritsFrom")]
    inherits_from: Option<String>,
    #[serde(rename = "javaVersion")]
    java_version: Option<JavaVersionMetadata>,
    #[serde(rename = "assetIndex")]
    asset_index: Option<AssetIndexMetadata>,
    pub(crate) downloads: Option<DownloadsMetadata>,
}

#[derive(Debug, Deserialize)]
struct JavaVersionMetadata {
    #[serde(rename = "majorVersion")]
    major_version: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct AssetIndexMetadata {
    id: Option<String>,
    sha1: Option<String>,
    size: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DownloadsMetadata {
    pub(crate) client: Option<ClientDownload>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ClientDownload {
    pub(crate) sha1: Option<String>,
    pub(crate) size: Option<u64>,
}

/// Conventional launcher roots for the current platform.
pub fn default_minecraft_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    #[cfg(target_os = "windows")]
    {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            roots.push(PathBuf::from(appdata).join(".minecraft"));
        }
        if let Some(profile) = std::env::var_os("USERPROFILE") {
            roots.push(
                PathBuf::from(profile)
                    .join("AppData")
                    .join("Roaming")
                    .join(".minecraft"),
            );
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            roots.push(
                PathBuf::from(home)
                    .join("Library")
                    .join("Application Support")
                    .join("minecraft"),
            );
        }
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        if let Some(home) = std::env::var_os("HOME") {
            roots.push(PathBuf::from(home).join(".minecraft"));
        }
    }
    dedupe_paths(roots)
}

/// Scans conventional launcher roots and their launcher-profile game directories.
pub fn discover_assets() -> AssetDiscoveryReport {
    let default_roots = default_minecraft_roots();
    let default_root_present = default_roots.iter().any(|root| is_real_directory(root));
    let mut problems = Vec::new();
    let mut roots: Vec<(PathBuf, InstallationSource)> = Vec::new();
    for root in &default_roots {
        if is_real_directory(root) {
            roots.push((root.clone(), InstallationSource::DefaultRoot));
            for profile_root in launcher_game_dirs(root) {
                roots.push((profile_root, InstallationSource::LauncherProfile));
            }
        } else {
            problems.push(format!(
                "no Minecraft Java launcher folder at {}",
                root.display()
            ));
        }
    }
    let mut report = discover_roots(roots);
    report.default_root_present = default_root_present;
    report.problems.splice(0..0, problems);
    report.manual_hint = manual_hint();
    report
}

/// Discovers installations from an explicit root list. Read-only.
pub fn discover_roots(roots: Vec<(PathBuf, InstallationSource)>) -> AssetDiscoveryReport {
    let mut seen = BTreeSet::new();
    let mut searched_roots = Vec::new();
    let mut installations = Vec::new();
    let mut problems = Vec::new();
    for (root, source) in roots {
        let absolute = absolute_path(&root);
        let key = path_key(&absolute);
        if !seen.insert(key) {
            continue;
        }
        searched_roots.push(absolute.display().to_string());
        match fs::symlink_metadata(&absolute) {
            Ok(metadata) if metadata.file_type().is_symlink() => problems.push(format!(
                "refusing to scan a symlink or junction: {}",
                absolute.display()
            )),
            Ok(metadata) if metadata.is_dir() => {
                installations.push(scan_root(&absolute, source, crate::target_profile()));
            }
            Ok(_) => problems.push(format!(
                "the Minecraft root is not a directory: {}",
                absolute.display()
            )),
            Err(error) => problems.push(format!(
                "the Minecraft root cannot be read: {} ({error})",
                absolute.display()
            )),
        }
    }
    AssetDiscoveryReport {
        target_minecraft_version: target_minecraft_version().to_owned(),
        target_data_version: target_data_version(),
        target_resource_pack_format: target_resource_pack_format(),
        default_root_present: false,
        searched_roots,
        installations,
        problems,
        manual_hint: manual_hint(),
    }
}

/// Scans one explicit root. Used by manual selection and by `discover_roots`.
pub fn discover_root(root: impl AsRef<Path>) -> InstallationReport {
    scan_root(
        root.as_ref(),
        InstallationSource::Manual,
        crate::target_profile(),
    )
}

/// Classifies and validates an explicitly selected file or folder without writing it.
pub fn inspect_asset_path(path: impl AsRef<Path>) -> AssetPathReport {
    inspect_asset_path_for(path, crate::target_profile())
}

/// Inspects a source against the exact project profile.
pub fn inspect_asset_path_for(
    path: impl AsRef<Path>,
    profile: &minecraft_version::VersionProfile,
) -> AssetPathReport {
    let target_version = profile.minecraft_version.as_str();
    let path = path.as_ref();
    let display = path.display().to_string();
    let mut report = AssetPathReport {
        path: display,
        kind: AssetSelectionKind::Unknown,
        valid: false,
        target_match: false,
        installation: None,
        version: None,
        resource_pack: None,
        errors: Vec::new(),
        warnings: Vec::new(),
    };
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            report.kind = AssetSelectionKind::NotFound;
            report.errors.push(format!(
                "the selected path does not exist: {}",
                path.display()
            ));
            return report;
        }
        Err(error) => {
            report
                .errors
                .push(format!("cannot read {}: {error}", path.display()));
            return report;
        }
    };
    if metadata.file_type().is_symlink() {
        report.kind = AssetSelectionKind::UnsafeLink;
        report.errors.push(
            "the selection is a symlink, junction or reparse point; select the real path"
                .to_owned(),
        );
        return report;
    }
    if metadata.is_file() {
        match extension(path).as_str() {
            "jar" => inspect_client_jar_selection(path, profile, &mut report),
            "zip" => inspect_pack_selection(path, profile, &mut report),
            _ => report.errors.push(
                "the selected file is neither a client JAR nor a resource pack .zip".to_owned(),
            ),
        }
        return report;
    }
    if !metadata.is_dir() {
        report
            .errors
            .push("the selection is not a regular file or directory".to_owned());
        return report;
    }
    if is_file_no_follow(&path.join("pack.mcmeta")) {
        inspect_pack_selection(path, profile, &mut report);
        return report;
    }
    if is_real_directory(&path.join("versions"))
        || is_real_directory(&path.join("assets").join("indexes"))
        || is_real_directory(&path.join("assets").join("objects"))
        || is_file_no_follow(&path.join("launcher_profiles.json"))
        || is_file_no_follow(&path.join("launcher_profiles_microsoft_store.json"))
    {
        report.kind = AssetSelectionKind::MinecraftRoot;
        let installation = scan_root(path, InstallationSource::Manual, profile);
        report.target_match = !installation.compatible_versions.is_empty();
        report.valid = report.target_match;
        if let Some(version) = installation
            .versions
            .iter()
            .find(|version| is_compatible(version.status))
            .cloned()
        {
            report.version = Some(version);
        }
        if !report.target_match {
            report.errors.push(format!(
                "no compatible Minecraft {target_version} client JAR was found in this root"
            ));
            if !installation.versions_dir_present {
                report.errors.push(
                    "the folder has no versions/ directory with installed clients".to_owned(),
                );
            }
        }
        for warning in &installation.warnings {
            report.warnings.push(warning.clone());
        }
        report.installation = Some(installation);
        return report;
    }
    if is_real_directory(&path.join("assets")) {
        inspect_pack_selection(path, profile, &mut report);
        return report;
    }
    if looks_like_version_directory(path) {
        report.kind = AssetSelectionKind::VersionDirectory;
        let root = version_root(path);
        let version = scan_version(path, root.as_deref(), profile);
        report.target_match = is_compatible(version.status);
        report.valid = report.target_match;
        if !report.target_match {
            report.errors.extend(version.notes.iter().cloned());
            report.errors.push(format!(
                "this version is not a compatible Minecraft {target_version} client"
            ));
        }
        report.version = Some(version);
        return report;
    }
    report.errors.push(
        "the selected folder is not a Minecraft root, version directory or resource pack"
            .to_owned(),
    );
    report
}

fn inspect_client_jar_selection(
    path: &Path,
    profile: &minecraft_version::VersionProfile,
    report: &mut AssetPathReport,
) {
    let target_version = profile.minecraft_version.as_str();
    report.kind = AssetSelectionKind::ClientJar;
    let metadata = match read_version_metadata(&path.with_extension("json")) {
        Ok(metadata) => metadata,
        Err(error) => {
            report.errors.push(error);
            return;
        }
    };
    if metadata
        .as_ref()
        .is_some_and(|metadata| metadata.inherits_from.is_some())
    {
        report
            .errors
            .push("the selected client JAR belongs to a modded profile".to_owned());
        return;
    }
    let expected = expected_client_from_metadata(metadata.as_ref());
    let inspection = inspect_client_jar(path, &expected);
    for error in &inspection.errors {
        report.errors.push(error.clone());
    }
    for warning in &inspection.warnings {
        report.warnings.push(warning.clone());
    }
    let version =
        version_report_from_jar(path, &inspection, &expected, metadata.is_some(), profile);
    report.target_match = is_compatible(version.status);
    report.valid = report.target_match && report.errors.is_empty();
    if !report.target_match && report.errors.is_empty() {
        report.errors.push(format!(
            "this client JAR is not a compatible Minecraft {target_version} client"
        ));
    }
    report.version = Some(version);
}

fn inspect_pack_selection(
    path: &Path,
    profile: &minecraft_version::VersionProfile,
    report: &mut AssetPathReport,
) {
    report.kind = AssetSelectionKind::ResourcePack;
    let pack = inspect_resource_pack(path, profile);
    report.target_match = pack.compatible;
    report.valid = pack.compatible && pack.errors.is_empty();
    for error in &pack.errors {
        report.errors.push(error.clone());
    }
    for warning in &pack.warnings {
        report.warnings.push(warning.clone());
    }
    if !pack.compatible {
        report.errors.push(format!(
            "the resource pack does not declare support for Minecraft {}",
            profile.minecraft_version
        ));
    }
    report.resource_pack = Some(pack);
}

fn scan_root(
    root: &Path,
    source: InstallationSource,
    profile: &minecraft_version::VersionProfile,
) -> InstallationReport {
    let target_version = profile.minecraft_version.as_str();
    let versions_dir = root.join("versions");
    let assets_dir = root.join("assets");
    let versions_dir_present = is_real_directory(&versions_dir);
    let assets_dir_present = is_real_directory(&assets_dir);
    let asset_indexes = count_files(&assets_dir.join("indexes"), "json");
    let mut warnings = Vec::new();
    let mut versions = Vec::new();
    if versions_dir_present {
        match fs::read_dir(&versions_dir) {
            Ok(entries) => {
                let mut count = 0usize;
                for entry in entries.flatten() {
                    count += 1;
                    if count > MAX_VERSION_ENTRIES {
                        warnings.push(format!(
                            "only the first {MAX_VERSION_ENTRIES} version entries were inspected"
                        ));
                        break;
                    }
                    let entry_path = entry.path();
                    let entry_metadata = match fs::symlink_metadata(&entry_path) {
                        Ok(metadata) => metadata,
                        Err(_) => continue,
                    };
                    if entry_metadata.file_type().is_symlink() {
                        warnings.push(format!(
                            "skipped symlinked version folder: {}",
                            entry_path.display()
                        ));
                        continue;
                    }
                    if !entry_metadata.is_dir() {
                        continue;
                    }
                    versions.push(scan_version(&entry_path, Some(root), profile));
                }
            }
            Err(error) => warnings.push(format!("cannot read {}: {error}", versions_dir.display())),
        }
    } else {
        warnings.push(
            "the root has no versions/ directory; no client JAR can be selected from it".to_owned(),
        );
    }
    versions.sort_by(|left, right| left.id.cmp(&right.id));
    let compatible_versions = versions
        .iter()
        .filter(|version| is_compatible(version.status))
        .map(|version| version.id.clone())
        .collect::<Vec<_>>();
    if !compatible_versions.iter().any(|id| id == target_version) {
        warnings.push(format!(
            "Minecraft {target_version} was not found with a verified client JAR in this root"
        ));
    }
    let resource_packs = scan_resource_packs(&root.join("resourcepacks"), profile, &mut warnings);
    InstallationReport {
        root: root.display().to_string(),
        source,
        versions_dir_present,
        assets_dir_present,
        asset_indexes,
        versions,
        compatible_versions,
        resource_packs,
        warnings,
    }
}

fn scan_version(
    version_dir: &Path,
    root: Option<&Path>,
    profile: &minecraft_version::VersionProfile,
) -> VersionReport {
    let target_version = profile.minecraft_version.as_str();
    let id = version_dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| version_dir.display().to_string());
    let metadata_path = version_dir.join(format!("{id}.json"));
    let jar_path = version_dir.join(format!("{id}.jar"));
    let metadata_present = is_file_no_follow(&metadata_path);
    let jar_present = is_file_no_follow(&jar_path);
    let mut notes = Vec::new();
    let (metadata, metadata_error) = match read_version_metadata(&metadata_path) {
        Ok(metadata) => (metadata, None),
        Err(error) => (None, Some(error)),
    };
    if let Some(error) = &metadata_error {
        notes.push(format!("launcher metadata is unreadable: {error}"));
    }
    let modded = metadata
        .as_ref()
        .and_then(|metadata| metadata.inherits_from.as_ref())
        .is_some();
    let effective_id = metadata
        .as_ref()
        .and_then(|metadata| metadata.id.clone())
        .unwrap_or_else(|| id.clone());
    if modded {
        notes.push(
            "modded profile: FuryBuilder never substitutes modded classes for vanilla assets"
                .to_owned(),
        );
    }
    let expected = if modded {
        ExpectedClient::default()
    } else {
        expected_client_from_metadata(metadata.as_ref())
    };
    let target_candidate = effective_id == target_version || (!metadata_present && jar_present);
    let mut inspection = ClientJarInspection::default();
    if jar_present && target_candidate && !modded {
        inspection = inspect_client_jar(&jar_path, &expected);
    }
    for error in &inspection.errors {
        notes.push(error.clone());
    }
    for warning in &inspection.warnings {
        notes.push(warning.clone());
    }

    let mut status = if modded {
        VersionStatus::Modded
    } else if !metadata_present {
        VersionStatus::MissingMetadata
    } else if effective_id != target_version {
        VersionStatus::WrongVersion
    } else if !jar_present {
        VersionStatus::MissingClientJar
    } else if metadata_error.is_some() {
        VersionStatus::InvalidMetadata
    } else {
        VersionStatus::CompatibleUnverified
    };
    if jar_present && !modded && target_candidate && metadata_error.is_none() {
        status = status_from_jar(&inspection, &expected, profile);
    }
    if metadata.is_none() && jar_present && !modded {
        notes
            .push("launcher metadata is missing; the client JAR was inspected directly".to_owned());
    }
    if !target_candidate && jar_present {
        notes.push(format!(
            "client JAR was not hashed because this folder is not a {target_version} candidate"
        ));
    }

    let asset_index = metadata
        .as_ref()
        .and_then(|metadata| metadata.asset_index.as_ref())
        .and_then(|index| index.id.clone());
    let asset_index_path = root.zip(asset_index.as_ref()).map(|(root, id)| {
        root.join("assets")
            .join("indexes")
            .join(format!("{id}.json"))
    });
    let asset_index_present = asset_index_path.as_deref().is_some_and(is_file_no_follow);
    let asset_index_checksum_verified = if is_compatible(status) {
        verify_asset_index(
            asset_index_path.as_deref(),
            metadata
                .as_ref()
                .and_then(|metadata| metadata.asset_index.as_ref()),
            &mut notes,
        )
    } else {
        false
    };

    VersionReport {
        id: effective_id,
        version_type: metadata
            .as_ref()
            .and_then(|metadata| metadata.version_type.clone()),
        release_time: metadata
            .as_ref()
            .and_then(|metadata| metadata.release_time.clone()),
        java_major: metadata
            .as_ref()
            .and_then(|metadata| metadata.java_version.as_ref())
            .and_then(|java| java.major_version),
        inherits_from: metadata
            .as_ref()
            .and_then(|metadata| metadata.inherits_from.clone()),
        metadata_path: metadata_path.display().to_string(),
        metadata_present,
        jar_path: jar_path.display().to_string(),
        jar_present,
        jar_bytes: if inspection.bytes > 0 {
            Some(inspection.bytes)
        } else {
            None
        },
        expected_sha1: expected.sha1.clone(),
        actual_sha1: inspection.sha1.clone(),
        checksum_verified: inspection.checksum_verified,
        internal_version_id: inspection.internal_id.clone(),
        world_version: inspection.world_version,
        resource_pack_format: inspection
            .pack_formats
            .and_then(|packs| packs.resource.try_into().ok()),
        java_version_from_jar: inspection.java_version,
        asset_index,
        asset_index_present,
        asset_index_checksum_verified,
        status,
        notes,
    }
}

fn version_report_from_jar(
    path: &Path,
    inspection: &ClientJarInspection,
    expected: &ExpectedClient,
    metadata_present: bool,
    profile: &minecraft_version::VersionProfile,
) -> VersionReport {
    let status = status_from_jar(inspection, expected, profile);
    VersionReport {
        id: inspection
            .id
            .clone()
            .or_else(|| expected.id.clone())
            .unwrap_or_else(|| {
                path.file_stem()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string())
            }),
        version_type: None,
        release_time: None,
        java_major: inspection.java_version,
        inherits_from: None,
        metadata_path: path.display().to_string(),
        metadata_present,
        jar_path: path.display().to_string(),
        jar_present: true,
        jar_bytes: (inspection.bytes > 0).then_some(inspection.bytes),
        expected_sha1: expected.sha1.clone(),
        actual_sha1: inspection.sha1.clone(),
        checksum_verified: inspection.checksum_verified,
        internal_version_id: inspection.internal_id.clone(),
        world_version: inspection.world_version,
        resource_pack_format: inspection
            .pack_formats
            .and_then(|packs| packs.resource.try_into().ok()),
        java_version_from_jar: inspection.java_version,
        asset_index: None,
        asset_index_present: false,
        asset_index_checksum_verified: false,
        status,
        notes: inspection
            .errors
            .iter()
            .chain(inspection.warnings.iter())
            .cloned()
            .collect(),
    }
}

fn status_from_jar(
    inspection: &ClientJarInspection,
    expected: &ExpectedClient,
    profile: &minecraft_version::VersionProfile,
) -> VersionStatus {
    if expected.sha1.is_some() && !inspection.checksum_verified
        || expected.size.is_some_and(|size| size != inspection.bytes)
    {
        return VersionStatus::ChecksumMismatch;
    }
    if inspection.sha1.is_none() || !inspection.errors.is_empty() {
        return VersionStatus::InvalidClientJar;
    }
    if inspection.id.as_deref() != Some(profile.minecraft_version.as_str()) {
        return VersionStatus::WrongVersion;
    }
    if inspection.world_version != Some(profile.data_version) {
        return VersionStatus::WorldVersionMismatch;
    }
    if profile.family == minecraft_version::VersionFamily::Flattened
        && !inspection.pack_formats.is_some_and(|formats| {
            Some(formats.resource) == profile.resource_pack_format
                && Some(formats.data) == profile.datapack_format
                && formats.minors == profile.pack_format_minors
        })
    {
        return VersionStatus::InvalidClientJar;
    }
    if inspection.checksum_verified {
        VersionStatus::Compatible
    } else {
        VersionStatus::CompatibleUnverified
    }
}

fn expected_client_from_metadata(metadata: Option<&VersionMetadata>) -> ExpectedClient {
    let client = metadata
        .and_then(|metadata| metadata.downloads.as_ref())
        .and_then(|downloads| downloads.client.as_ref());
    ExpectedClient {
        id: metadata.and_then(|metadata| metadata.id.clone()),
        sha1: client.and_then(|client| client.sha1.clone()),
        size: client.and_then(|client| client.size),
    }
}

fn verify_asset_index(
    path: Option<&Path>,
    metadata: Option<&AssetIndexMetadata>,
    notes: &mut Vec<String>,
) -> bool {
    let (Some(path), Some(metadata)) = (path, metadata) else {
        return false;
    };
    let Some(expected_sha1) = &metadata.sha1 else {
        return false;
    };
    let size = match fs::metadata(path) {
        Ok(metadata) => metadata.len(),
        Err(error) => {
            notes.push(format!("asset index cannot be read: {error}"));
            return false;
        }
    };
    if let Some(expected_size) = metadata.size {
        if expected_size != size {
            notes.push(format!(
                "asset index size is {size} but metadata declares {expected_size}"
            ));
            return false;
        }
    }
    if size > MAX_ASSET_INDEX_BYTES {
        notes.push("asset index is larger than the inspection limit".to_owned());
        return false;
    }
    match fs::read(path) {
        Ok(bytes) => {
            let actual = sha1_hex(&bytes);
            if actual.eq_ignore_ascii_case(expected_sha1) {
                true
            } else {
                notes.push(format!(
                    "asset index SHA-1 is {actual} but metadata declares {expected_sha1}"
                ));
                false
            }
        }
        Err(error) => {
            notes.push(format!("asset index cannot be read: {error}"));
            false
        }
    }
}

pub(crate) fn read_version_metadata(path: &Path) -> Result<Option<VersionMetadata>, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("{} cannot be read: {error}", path.display())),
    };
    if !metadata.is_file() {
        return Ok(None);
    }
    if metadata.file_type().is_symlink() {
        return Err("launcher metadata is a symlink".to_owned());
    }
    if metadata.len() > MAX_METADATA_BYTES {
        return Err("launcher metadata is larger than the inspection limit".to_owned());
    }
    let bytes = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|error| format!("{} is not valid JSON: {error}", path.display()))
}

fn scan_resource_packs(
    folder: &Path,
    profile: &minecraft_version::VersionProfile,
    warnings: &mut Vec<String>,
) -> Vec<ResourcePackReport> {
    if !is_real_directory(folder) {
        return Vec::new();
    }
    let entries = match fs::read_dir(folder) {
        Ok(entries) => entries,
        Err(error) => {
            warnings.push(format!("cannot read {}: {error}", folder.display()));
            return Vec::new();
        }
    };
    let mut packs = Vec::new();
    for (index, entry) in entries.flatten().enumerate() {
        if index >= MAX_RESOURCE_PACKS {
            warnings.push(format!(
                "only the first {MAX_RESOURCE_PACKS} resource packs were inspected"
            ));
            break;
        }
        let path = entry.path();
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        if metadata.file_type().is_symlink() {
            warnings.push(format!(
                "skipped symlinked resource pack: {}",
                path.display()
            ));
            continue;
        }
        let extension = extension(&path);
        if metadata.is_dir() || (metadata.is_file() && extension == "zip") {
            packs.push(inspect_resource_pack(&path, profile));
        }
    }
    packs.sort_by(|left, right| left.name.cmp(&right.name));
    packs
}

fn launcher_game_dirs(root: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for name in [
        "launcher_profiles.json",
        "launcher_profiles_microsoft_store.json",
    ] {
        let path = root.join(name);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => metadata,
            _ => continue,
        };
        if metadata.len() > MAX_METADATA_BYTES {
            continue;
        }
        let Ok(bytes) = fs::read(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            continue;
        };
        let Some(profiles) = value
            .get("profiles")
            .and_then(|profiles| profiles.as_object())
        else {
            continue;
        };
        for profile in profiles.values().take(MAX_PROFILES) {
            let Some(game_dir) = profile.get("gameDir").and_then(|value| value.as_str()) else {
                continue;
            };
            let candidate = PathBuf::from(game_dir);
            let candidate = if candidate.is_absolute() {
                candidate
            } else {
                root.join(candidate)
            };
            if is_real_directory(&candidate) {
                roots.push(candidate);
            }
        }
    }
    dedupe_paths(roots)
}

fn looks_like_version_directory(path: &Path) -> bool {
    let Some(name) = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
    else {
        return false;
    };
    path.join(format!("{name}.json")).exists() || path.join(format!("{name}.jar")).exists()
}

fn version_root(path: &Path) -> Option<PathBuf> {
    let versions = path.parent()?;
    if versions.file_name().is_some_and(|name| name == "versions") {
        return versions.parent().map(Path::to_path_buf);
    }
    None
}

fn is_compatible(status: VersionStatus) -> bool {
    matches!(
        status,
        VersionStatus::Compatible | VersionStatus::CompatibleUnverified
    )
}

fn dedupe_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = BTreeSet::new();
    paths
        .into_iter()
        .filter(|path| seen.insert(path_key(path)))
        .collect()
}

fn path_key(path: &Path) -> String {
    let absolute = absolute_path(path);
    let text = absolute.display().to_string();
    if cfg!(windows) {
        text.to_ascii_lowercase()
    } else {
        text
    }
}

fn absolute_path(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

fn extension(path: &Path) -> String {
    path.extension()
        .map(|extension| extension.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

fn count_files(folder: &Path, extension_filter: &str) -> usize {
    let Ok(entries) = fs::read_dir(folder) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|entry| extension(&entry.path()) == extension_filter)
        .count()
}

fn is_real_directory(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
        .unwrap_or(false)
}

fn is_file_no_follow(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
        .unwrap_or(false)
}

fn manual_hint() -> String {
    let target_version = target_minecraft_version();
    format!(
        "If automatic detection finds nothing, choose the folder that contains versions/ (often .minecraft), a Minecraft {target_version} client JAR, or a resource pack. FuryBuilder only reads these files and never downloads or redistributes them."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_predicate_accepts_only_verified_or_unverified_targets() {
        assert!(is_compatible(VersionStatus::Compatible));
        assert!(is_compatible(VersionStatus::CompatibleUnverified));
        for status in [
            VersionStatus::WrongVersion,
            VersionStatus::Modded,
            VersionStatus::MissingMetadata,
            VersionStatus::MissingClientJar,
            VersionStatus::ChecksumMismatch,
            VersionStatus::WorldVersionMismatch,
            VersionStatus::InvalidMetadata,
            VersionStatus::InvalidClientJar,
        ] {
            assert!(!is_compatible(status), "{status:?}");
        }
    }
}
