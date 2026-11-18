use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::archive::Archive;
use crate::bundle::BundleError;
use crate::discovery::inspect_asset_path;
use crate::resource_pack::inspect_resource_pack;
use crate::settings::{AssetSourceSetting, MAX_RESOURCE_PACKS};
use crate::types::{ResourcePackStatus, VersionStatus};

const MAX_JSON_BYTES: u64 = 2 * 1024 * 1024;
const MAX_TEXTURE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_TREE_ENTRIES: usize = 4096;
const MAX_TREE_DEPTH: usize = 16;

/// One user resource pack in the validated stack. Priority 0 is the highest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BundlePack {
    pub priority: usize,
    pub path: String,
    pub name: String,
    pub pack_format: Option<i32>,
}

enum PackSource {
    Zip { path: PathBuf, archive: Archive },
    Folder { path: PathBuf },
}

impl PackSource {
    fn _unused(&self) {}
}

/// Read-only view over a verified client JAR and an ordered list of resource packs.
/// Packs are searched from highest to lowest priority; the base client JAR is last.
pub struct ResourceStack {
    base_jar_path: Option<PathBuf>,
    base_jar_sha1: Option<String>,
    base_jar: Option<Archive>,
    packs: Vec<PackSource>,
    pub packs_info: Vec<BundlePack>,
    pub warnings: Vec<String>,
    pub fingerprint: String,
}

impl ResourceStack {
    pub fn open(
        source: Option<&AssetSourceSetting>,
        pack_paths: &[String],
    ) -> Result<Self, BundleError> {
        if pack_paths.len() > MAX_RESOURCE_PACKS {
            return Err(BundleError::InvalidArgument(format!(
                "at most {MAX_RESOURCE_PACKS} resource packs can be stacked"
            )));
        }
        let mut stack = Self {
            base_jar_path: None,
            base_jar_sha1: None,
            base_jar: None,
            packs: Vec::new(),
            packs_info: Vec::new(),
            warnings: Vec::new(),
            fingerprint: String::new(),
        };
        let mut source_pack: Option<PathBuf> = None;
        if let Some(source) = source {
            let report = inspect_asset_path(&source.path);
            match source.kind.as_str() {
                "minecraft_root" => {
                    if !report.valid {
                        return Err(BundleError::UnsupportedFormat(format!(
                            "the selected Minecraft root is not usable: {}",
                            report
                                .errors
                                .first()
                                .cloned()
                                .unwrap_or_else(|| "no verified 1.21.4 client".to_owned())
                        )));
                    }
                    let version = report.version.as_ref().ok_or_else(|| {
                        BundleError::UnsupportedFormat(
                            "the selected Minecraft root has no verified 1.21.4 client JAR"
                                .to_owned(),
                        )
                    })?;
                    if !matches!(
                        version.status,
                        VersionStatus::Compatible | VersionStatus::CompatibleUnverified
                    ) {
                        return Err(BundleError::UnsupportedFormat(format!(
                            "the selected root's client JAR status is {}",
                            version.status.as_str()
                        )));
                    }
                    if version.status == VersionStatus::CompatibleUnverified {
                        stack.warnings.push(format!(
                            "client JAR {} is internally consistent but its SHA-1 was not verified",
                            version.jar_path
                        ));
                    }
                    stack.base_jar_path = Some(PathBuf::from(&version.jar_path));
                    stack.base_jar_sha1 = version.actual_sha1.clone();
                }
                "client_jar" => {
                    if !report.valid {
                        return Err(BundleError::UnsupportedFormat(format!(
                            "the selected client JAR is not usable: {}",
                            report
                                .errors
                                .first()
                                .cloned()
                                .unwrap_or_else(|| "unknown reason".to_owned())
                        )));
                    }
                    let version = report.version.as_ref().ok_or_else(|| {
                        BundleError::UnsupportedFormat(
                            "the selected client JAR has no version report".to_owned(),
                        )
                    })?;
                    stack.base_jar_path = Some(PathBuf::from(&version.jar_path));
                    stack.base_jar_sha1 = version.actual_sha1.clone();
                    if !version.checksum_verified {
                        stack.warnings.push(
                            "client JAR SHA-1 was not verified against launcher metadata"
                                .to_owned(),
                        );
                    }
                }
                "resource_pack" => {
                    if !report.valid {
                        return Err(BundleError::UnsupportedFormat(format!(
                            "the selected resource pack is not usable: {}",
                            report
                                .errors
                                .first()
                                .cloned()
                                .unwrap_or_else(|| "unknown reason".to_owned())
                        )));
                    }
                    source_pack = Some(PathBuf::from(&source.path));
                }
                other => {
                    return Err(BundleError::InvalidArgument(format!(
                        "unsupported asset source kind: {other}"
                    )))
                }
            }
        }

        if let Some(path) = source_pack {
            stack.push_pack(&path)?;
        }
        for path in pack_paths {
            stack.push_pack(Path::new(path))?;
        }
        if let Some(base_path) = &stack.base_jar_path {
            let archive = Archive::open(base_path).map_err(|error| {
                BundleError::UnsupportedFormat(format!(
                    "cannot read the client JAR {}: {error}",
                    base_path.display()
                ))
            })?;
            stack.base_jar = Some(archive);
        }
        if stack.base_jar.is_none() && stack.packs.is_empty() {
            return Err(BundleError::InvalidArgument(
                "select a Minecraft client JAR or at least one resource pack".to_owned(),
            ));
        }
        stack.fingerprint = stack.compute_fingerprint()?;
        Ok(stack)
    }

    fn push_pack(&mut self, path: &Path) -> Result<(), BundleError> {
        let report = inspect_resource_pack(path);
        if report.status != ResourcePackStatus::Compatible || !report.errors.is_empty() {
            return Err(BundleError::UnsupportedFormat(format!(
                "resource pack {} is not compatible with format 46: {}",
                path.display(),
                report
                    .errors
                    .first()
                    .cloned()
                    .unwrap_or_else(|| format!("{:?}", report.status))
            )));
        }
        for warning in &report.warnings {
            self.warnings.push(format!("{}: {warning}", path.display()));
        }
        let priority = self.packs_info.len();
        let metadata = fs::symlink_metadata(path).map_err(|error| {
            BundleError::Io(format!("cannot inspect {}: {error}", path.display()))
        })?;
        if metadata.file_type().is_symlink() {
            return Err(BundleError::UnsupportedFormat(format!(
                "resource pack is a symlink or junction: {}",
                path.display()
            )));
        }
        if metadata.is_file() {
            let archive = Archive::open(path).map_err(|error| {
                BundleError::UnsupportedFormat(format!(
                    "cannot read resource pack {}: {error}",
                    path.display()
                ))
            })?;
            self.packs.push(PackSource::Zip {
                path: path.to_path_buf(),
                archive,
            });
        } else if metadata.is_dir() {
            self.packs.push(PackSource::Folder {
                path: path.to_path_buf(),
            });
        } else {
            return Err(BundleError::UnsupportedFormat(format!(
                "resource pack is not a zip archive or directory: {}",
                path.display()
            )));
        }
        self.packs_info.push(BundlePack {
            priority,
            path: path.display().to_string(),
            name: report.name,
            pack_format: report.pack_format,
        });
        Ok(())
    }

    pub fn base_jar_path(&self) -> Option<&Path> {
        self.base_jar_path.as_deref()
    }

    pub fn base_jar_sha1(&self) -> Option<&str> {
        self.base_jar_sha1.as_deref()
    }

    pub fn read_json(&mut self, logical_path: &str) -> Result<Option<Value>, BundleError> {
        let Some(bytes) = self.read_file(logical_path, MAX_JSON_BYTES)? else {
            return Ok(None);
        };
        serde_json::from_slice(&bytes).map(Some).map_err(|error| {
            BundleError::UnsupportedFormat(format!("{logical_path} is not valid JSON: {error}"))
        })
    }

    pub fn read_texture(&mut self, logical_path: &str) -> Result<Option<Vec<u8>>, BundleError> {
        self.read_file(logical_path, MAX_TEXTURE_BYTES)
    }

    fn read_file(
        &mut self,
        logical_path: &str,
        max_bytes: u64,
    ) -> Result<Option<Vec<u8>>, BundleError> {
        let components = safe_components(logical_path).ok_or_else(|| {
            BundleError::InvalidArgument(format!("unsafe resource path: {logical_path}"))
        })?;
        let zip_name = components.join("/");
        for pack in &mut self.packs {
            match pack {
                PackSource::Zip { path, archive } => {
                    if !archive.entries().iter().any(|entry| entry.name == zip_name) {
                        continue;
                    }
                    return archive
                        .read_entry(&zip_name, max_bytes)
                        .map(Some)
                        .map_err(|error| {
                            BundleError::UnsupportedFormat(format!(
                                "cannot read {zip_name} from {}: {error}",
                                path.display()
                            ))
                        });
                }
                PackSource::Folder { path } => {
                    if let Some(bytes) = read_folder_file(path, &components, max_bytes)? {
                        return Ok(Some(bytes));
                    }
                }
            }
        }
        if let Some(archive) = &mut self.base_jar {
            if archive.entries().iter().any(|entry| entry.name == zip_name) {
                return archive
                    .read_entry(&zip_name, max_bytes)
                    .map(Some)
                    .map_err(|error| {
                        BundleError::UnsupportedFormat(format!(
                            "cannot read {zip_name} from the client JAR: {error}"
                        ))
                    });
            }
        }
        Ok(None)
    }

    fn compute_fingerprint(&self) -> Result<String, BundleError> {
        let mut hasher = Sha256::new();
        hasher.update(b"furybuilder-asset-stack-v2");
        if let Some(path) = &self.base_jar_path {
            hasher.update(b"base");
            hash_path_metadata(&mut hasher, path)?;
            if let Some(sha1) = &self.base_jar_sha1 {
                hasher.update(sha1.as_bytes());
            }
        }
        for pack in &self.packs {
            hasher.update(b"pack");
            match pack {
                PackSource::Zip { path, .. } => hash_path_metadata(&mut hasher, path)?,
                PackSource::Folder { path } => hash_tree(&mut hasher, path, 0)?,
            }
        }
        Ok(hex(&hasher.finalize()))
    }
}

fn safe_components(path: &str) -> Option<Vec<&str>> {
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path.contains('\0')
        || path.contains(':')
    {
        return None;
    }
    let mut components = Vec::new();
    for part in path.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return None;
        }
        components.push(part);
    }
    Some(components)
}

fn read_folder_file(
    root: &Path,
    components: &[&str],
    max_bytes: u64,
) -> Result<Option<Vec<u8>>, BundleError> {
    let mut current = root.to_path_buf();
    for component in components {
        current.push(component);
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(BundleError::Io(format!(
                    "cannot inspect {}: {error}",
                    current.display()
                )))
            }
        };
        if metadata.file_type().is_symlink() {
            return Err(BundleError::UnsupportedFormat(format!(
                "resource pack contains a symlink or junction: {}",
                current.display()
            )));
        }
    }
    let metadata = fs::symlink_metadata(&current).map_err(|error| {
        BundleError::Io(format!("cannot inspect {}: {error}", current.display()))
    })?;
    if !metadata.is_file() {
        return Ok(None);
    }
    if metadata.len() > max_bytes {
        return Err(BundleError::LimitExceeded(format!(
            "{} is larger than the resource limit",
            current.display()
        )));
    }
    fs::read(&current)
        .map(Some)
        .map_err(|error| BundleError::Io(format!("cannot read {}: {error}", current.display())))
}

fn hash_path_metadata(hasher: &mut Sha256, path: &Path) -> Result<(), BundleError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| BundleError::Io(format!("cannot inspect {}: {error}", path.display())))?;
    hasher.update(path.display().to_string().as_bytes());
    hasher.update(metadata.len().to_le_bytes());
    if let Ok(modified) = metadata.modified() {
        if let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH) {
            hasher.update(duration.as_nanos().to_le_bytes());
        }
    }
    Ok(())
}

fn hash_tree(hasher: &mut Sha256, root: &Path, depth: usize) -> Result<(), BundleError> {
    if depth > MAX_TREE_DEPTH {
        return Err(BundleError::LimitExceeded(format!(
            "resource pack directory nesting is too deep at {}",
            root.display()
        )));
    }
    let entries = fs::read_dir(root)
        .map_err(|error| BundleError::Io(format!("cannot read {}: {error}", root.display())))?;
    let mut names = Vec::new();
    for entry in entries.flatten() {
        if names.len() >= MAX_TREE_ENTRIES {
            return Err(BundleError::LimitExceeded(format!(
                "resource pack directory contains more than {MAX_TREE_ENTRIES} entries"
            )));
        }
        names.push(entry.file_name());
    }
    names.sort();
    for name in names {
        let child = root.join(&name);
        let metadata = fs::symlink_metadata(&child).map_err(|error| {
            BundleError::Io(format!("cannot inspect {}: {error}", child.display()))
        })?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        hasher.update(name.to_string_lossy().as_bytes());
        hasher.update(metadata.len().to_le_bytes());
        if let Ok(modified) = metadata.modified() {
            if let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH) {
                hasher.update(duration.as_nanos().to_le_bytes());
            }
        }
        if metadata.is_dir() {
            hash_tree(hasher, &child, depth + 1)?;
        }
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut output, "{byte:02x}");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsafe_logical_paths_are_rejected() {
        for path in [
            "",
            "/assets/minecraft/models/block/stone.json",
            "assets/../secret",
            "assets//empty",
            "assets\\minecraft",
            "C:/secret",
            "assets/\0bad",
        ] {
            assert!(safe_components(path).is_none(), "{path:?}");
        }
        assert!(safe_components("assets/minecraft/models/block/stone.json").is_some());
    }
}
