use std::fs;
use std::path::{Path, PathBuf};

use minecraft_version::VersionProfile;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::archive::Archive;
use crate::bundle::BundleError;
use crate::discovery::inspect_asset_path_for;
use crate::pack_store::{require_compatible, PackIssue, ResourcePackInput};
use crate::resource_pack::{inspect_resource_pack, is_link};
use crate::settings::AssetSourceSetting;
use crate::MAX_RESOURCE_PACKS;

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
    Zip {
        path: PathBuf,
        archive: Archive,
        sha1: String,
    },
    Folder {
        path: PathBuf,
    },
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
    pub pack_issues: Vec<String>,
    pub fingerprint: String,
    /// Stable fingerprint of the ordered resource-pack stack only. The base JAR is not part
    /// of it, so catalog caches can combine it with the exact version and JAR SHA-1.
    pub pack_stack_hash: String,
}

impl ResourceStack {
    pub fn open(
        profile: &VersionProfile,
        source: Option<&AssetSourceSetting>,
        expected_sha1: Option<&str>,
        packs: &[ResourcePackInput],
    ) -> Result<Self, BundleError> {
        if packs.len() > MAX_RESOURCE_PACKS {
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
            pack_issues: Vec::new(),
            fingerprint: String::new(),
            pack_stack_hash: String::new(),
        };
        let mut source_pack = None;
        if let Some(source) = source {
            match source.kind.as_str() {
                "minecraft_root" | "version_directory" | "client_jar" => {
                    let report = inspect_asset_path_for(&source.path, profile);
                    if !report.valid || report.kind.as_str() != source.kind {
                        return Err(BundleError::UnsupportedFormat(format!(
                            "the selected client source is not usable: {}",
                            report.errors.first().cloned().unwrap_or_else(|| format!(
                                "expected Minecraft {}",
                                profile.minecraft_version
                            ))
                        )));
                    }
                    let version = report.version.as_ref().ok_or_else(|| {
                        BundleError::UnsupportedFormat(
                            "the selected source has no client JAR".to_owned(),
                        )
                    })?;
                    stack.base_jar_path = Some(PathBuf::from(&version.jar_path));
                    stack.base_jar_sha1 = version.actual_sha1.clone();
                    stack.warnings.extend(version.notes.iter().cloned());
                    if !version.checksum_verified {
                        stack.warnings.push(
                            "client JAR SHA-1 was not verified against launcher metadata"
                                .to_owned(),
                        );
                    }
                }
                "resource_pack" => {
                    source_pack = Some(ResourcePackInput::local(&source.path));
                }
                other => {
                    return Err(BundleError::InvalidArgument(format!(
                        "unsupported asset source kind: {other}"
                    )))
                }
            }
        }

        if expected_sha1.is_some() && stack.base_jar_sha1.as_deref() != expected_sha1 {
            return Err(BundleError::UnsupportedFormat(
                "the client JAR changed since the project catalog was resolved".to_owned(),
            ));
        }
        for (priority, input) in source_pack.iter().chain(packs).enumerate() {
            if let Err(issue) = stack.push_pack(input, priority, profile) {
                let message = format!("{}: {issue}", input.name());
                if input.is_pinned() {
                    stack.pack_issues.push(message);
                } else {
                    return Err(BundleError::UnsupportedFormat(message));
                }
            }
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
        stack.pack_stack_hash = stack.compute_pack_stack_hash()?;
        stack.fingerprint = stack.compute_fingerprint()?;
        Ok(stack)
    }

    fn push_pack(
        &mut self,
        input: &ResourcePackInput,
        priority: usize,
        profile: &VersionProfile,
    ) -> Result<(), PackIssue> {
        let path = &input.path;
        let (source, report) = if input.metadata()?.is_file() {
            let (archive, report, sha1) = input.open_zip(profile)?;
            (
                PackSource::Zip {
                    path: path.clone(),
                    archive,
                    sha1,
                },
                report,
            )
        } else {
            let report = inspect_resource_pack(path, profile);
            require_compatible(&report, profile)?;
            (PackSource::Folder { path: path.clone() }, report)
        };
        let name = if input.is_pinned() {
            input.name()
        } else {
            report.name
        };
        for warning in &report.warnings {
            self.warnings.push(format!("{name}: {warning}"));
        }
        self.packs.push(source);
        self.packs_info.push(BundlePack {
            priority,
            path: path.display().to_string(),
            name,
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
                PackSource::Zip { path, archive, .. } => {
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
        hasher.update(b"packs");
        hasher.update(self.pack_stack_hash.as_bytes());
        Ok(hex(&hasher.finalize()))
    }

    fn compute_pack_stack_hash(&self) -> Result<String, BundleError> {
        let mut hasher = Sha256::new();
        hasher.update(b"furybuilder-pack-stack-v2");
        for pack in &self.packs {
            hasher.update(b"pack");
            match pack {
                PackSource::Zip { sha1, .. } => hasher.update(sha1.as_bytes()),
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
        if is_link(&metadata) {
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
        if is_link(&metadata) {
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
    use std::io::Write;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "furybuilder-stack-{label}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|duration| duration.as_nanos())
                    .unwrap_or(0)
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("create temp directory");
            Self { path }
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn write_client_jar(version_dir: &Path, version: &str) -> PathBuf {
        use zip::write::SimpleFileOptions;

        fs::create_dir_all(version_dir).expect("create version directory");
        let jar_path = version_dir.join(format!("{version}.jar"));
        let file = fs::File::create(&jar_path).expect("create client jar");
        let mut zip = zip::ZipWriter::new(file);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        zip.start_file("version.json", options)
            .expect("start version.json");
        zip.write_all(
            format!(
                r#"{{"id":"{version}","world_version":4189,"pack_version":{{"resource":46,"data":61}},"stable":true}}"#
            )
            .as_bytes(),
        )
        .expect("write version.json");
        zip.start_file("assets/minecraft/blockstates/stone.json", options)
            .expect("start blockstate");
        zip.write_all(br#"{"variants":{"":{"model":"minecraft:block/stone"}}}"#)
            .expect("write blockstate");
        zip.finish().expect("finish client jar");
        jar_path
    }

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

    #[test]
    fn a_selected_version_directory_is_a_valid_client_source() {
        let root = TempDir::new("version-directory");
        let version_dir = root.path.join(".minecraft").join("versions").join("1.21.4");
        let jar = write_client_jar(&version_dir, "1.21.4");
        let source = AssetSourceSetting {
            kind: "version_directory".to_owned(),
            path: version_dir.display().to_string(),
        };

        let stack = ResourceStack::open(crate::target_profile(), Some(&source), None, &[])
            .expect("open version directory");
        assert_eq!(stack.base_jar_path(), Some(jar.as_path()));
        assert!(stack.base_jar_sha1().is_some());
        assert!(stack
            .warnings
            .iter()
            .any(|warning| warning.contains("SHA-1 was not verified")));
    }
}
