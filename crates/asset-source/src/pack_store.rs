//! Local resource-pack snapshots. Project manifests hold only logical paths and content pins.

use std::fs::{self, File, Metadata};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use component_model::{AssetsPack, RESOURCE_PACK_CACHE_DIR};
use minecraft_version::VersionProfile;
use serde::Serialize;
use tempfile::NamedTempFile;
use zip::{write::SimpleFileOptions, ZipWriter};

use crate::archive::Archive;
use crate::jar::reader_sha1;
use crate::resource_pack::{folder_files, inspect_open_zip, is_link, MAX_PACK_TOTAL_BYTES};
use crate::{ResourcePackReport, ResourcePackStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PackState {
    Ready,
    Missing,
    Changed,
    Incompatible,
    Invalid,
}

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct PackIssue {
    pub state: PackState,
    pub message: String,
}

impl PackIssue {
    fn new(state: PackState, message: impl Into<String>) -> Self {
        Self {
            state,
            message: message.into(),
        }
    }

    fn invalid(error: impl ToString) -> Self {
        Self::new(PackState::Invalid, error.to_string())
    }
}

impl From<io::Error> for PackIssue {
    fn from(error: io::Error) -> Self {
        let state = if error.kind() == io::ErrorKind::NotFound {
            PackState::Missing
        } else {
            PackState::Invalid
        };
        Self::new(state, error.to_string())
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectPackStatus {
    #[serde(flatten)]
    pub pin: AssetsPack,
    pub name: String,
    pub state: PackState,
    pub message: Option<String>,
    pub pack_format: Option<i32>,
    pub supported_formats: Option<String>,
}

/// One pack to open, either a local source for standalone asset tooling or a project pin.
#[derive(Clone)]
pub struct ResourcePackInput {
    pub(crate) path: PathBuf,
    pinned: Option<(PathBuf, AssetsPack)>,
}

impl ResourcePackInput {
    pub fn local(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            pinned: None,
        }
    }

    pub(crate) fn is_pinned(&self) -> bool {
        self.pinned.is_some()
    }

    pub(crate) fn metadata(&self) -> Result<Metadata, PackIssue> {
        if let Some((root, pin)) = &self.pinned {
            let Some((directory, _)) = cache_parts(pin) else {
                return Err(PackIssue::invalid("invalid resource pack cache pin"));
            };
            check_directory(root)?;
            check_directory(&root.join(directory))?;
        }
        let metadata = fs::symlink_metadata(&self.path)?;
        if is_link(&metadata) || !(metadata.is_file() || (!self.is_pinned() && metadata.is_dir())) {
            return Err(PackIssue::invalid(
                "resource pack must be a regular file or folder, not a link",
            ));
        }
        Ok(metadata)
    }

    pub(crate) fn name(&self) -> String {
        if let Some((_, pin)) = &self.pinned {
            if let Some((_, name)) = cache_parts(pin) {
                return name
                    .strip_prefix("pack-")
                    .and_then(|name| name.strip_suffix(".zip"))
                    .unwrap_or(name)
                    .to_owned();
            }
            return pin.path.clone();
        }
        self.path.display().to_string()
    }

    pub(crate) fn open_zip(
        &self,
        profile: &VersionProfile,
    ) -> Result<(Archive, ResourcePackReport, String), PackIssue> {
        let expected = self.pinned.as_ref().map(|(_, pin)| pin.sha1.as_str());
        verify_zip(&self.path, File::open(&self.path)?, expected, profile)
    }
}

pub struct ResourcePackStore {
    root: PathBuf,
}

impl ResourcePackStore {
    /// `asset_cache` is the same cache root that holds managed client JARs.
    pub fn new(asset_cache: impl AsRef<Path>) -> Self {
        Self {
            root: asset_cache.as_ref().join(RESOURCE_PACK_CACHE_DIR),
        }
    }

    pub fn inputs(&self, pins: &[AssetsPack]) -> Vec<ResourcePackInput> {
        pins.iter()
            .map(|pin| ResourcePackInput {
                path: cache_parts(pin)
                    .map(|(dir, name)| self.root.join(dir).join(name))
                    .unwrap_or_default(),
                pinned: Some((self.root.clone(), pin.clone())),
            })
            .collect()
    }

    pub fn status(&self, pins: &[AssetsPack], profile: &VersionProfile) -> Vec<ProjectPackStatus> {
        pins.iter()
            .zip(self.inputs(pins))
            .map(|(pin, input)| {
                let (state, message, pack_format, supported_formats) =
                    match input.metadata().and_then(|_| input.open_zip(profile)) {
                        Ok((_, report, _)) => (
                            PackState::Ready,
                            None,
                            report.pack_format,
                            report.supported_formats,
                        ),
                        Err(issue) => (issue.state, Some(issue.message), None, None),
                    };
                ProjectPackStatus {
                    pin: pin.clone(),
                    name: input.name(),
                    state,
                    message,
                    pack_format,
                    supported_formats,
                }
            })
            .collect()
    }

    /// Installs a private copy, not a mutable link to the selected file. Folder imports are
    /// normalized into an ordered, uncompressed ZIP so they can be pinned and reopened alike.
    pub fn import(&self, path: &Path, profile: &VersionProfile) -> Result<AssetsPack, PackIssue> {
        let metadata = ResourcePackInput::local(path).metadata()?;
        let files = if metadata.is_dir() {
            Some(folder_files(path)?.files)
        } else {
            None
        };
        fs::create_dir_all(&self.root)?;
        check_directory(&self.root)?;
        let mut temp = NamedTempFile::new_in(&self.root)?;
        if let Some(files) = files {
            let mut zip = ZipWriter::new(temp.as_file_mut());
            let options =
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
            let mut remaining = MAX_PACK_TOTAL_BYTES;
            for (name, path) in files {
                zip.start_file(name, options).map_err(PackIssue::invalid)?;
                remaining -= copy_bounded(&path, &mut zip, remaining)?;
            }
            zip.finish().map_err(PackIssue::invalid)?;
        } else {
            copy_bounded(path, temp.as_file_mut(), MAX_PACK_TOTAL_BYTES)?;
        }
        temp.as_file().sync_all()?;
        let (_, _, sha1) = verify_zip(path, temp.reopen()?, None, profile)?;
        let raw_name = if metadata.is_dir() {
            path.file_name()
        } else {
            path.file_stem()
        };
        let slug: String = raw_name
            .and_then(|name| name.to_str())
            .unwrap_or("resource-pack")
            .chars()
            .take(80)
            .map(|c| {
                if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_') {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let slug = slug.trim();
        let filename = format!(
            "pack-{}.zip",
            if slug.is_empty() {
                "resource-pack"
            } else {
                slug
            }
        );
        let dir = self.root.join(&sha1);
        match fs::create_dir(&dir) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
        check_directory(&dir)?;
        temp.persist(dir.join(&filename))
            .map_err(|error| PackIssue::from(error.error))?;
        Ok(AssetsPack {
            path: format!("{RESOURCE_PACK_CACHE_DIR}/{sha1}/{filename}"),
            sha1,
        })
    }
}

fn cache_parts(pin: &AssetsPack) -> Option<(&str, &str)> {
    let mut parts = pin.path.split('/');
    let (root, sha1, file) = (parts.next()?, parts.next()?, parts.next()?);
    let name = file.strip_prefix("pack-")?.strip_suffix(".zip")?;
    (parts.next().is_none()
        && root == RESOURCE_PACK_CACHE_DIR
        && sha1 == pin.sha1
        && sha1.len() == 40
        && sha1
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        && !name.is_empty()
        && name.chars().count() <= 80
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_')))
    .then_some((sha1, file))
}

fn check_directory(path: &Path) -> Result<(), PackIssue> {
    let metadata = fs::symlink_metadata(path)?;
    if is_link(&metadata) || !metadata.is_dir() {
        return Err(PackIssue::invalid(
            "resource pack cache contains a linked or invalid directory",
        ));
    }
    Ok(())
}

fn copy_bounded(path: &Path, out: &mut impl io::Write, limit: u64) -> Result<u64, PackIssue> {
    let metadata = fs::symlink_metadata(path)?;
    if is_link(&metadata) || !metadata.is_file() {
        return Err(PackIssue::invalid(
            "resource pack contains a linked or invalid file",
        ));
    }
    if metadata.len() > limit {
        return Err(PackIssue::invalid(
            "resource pack exceeds the total size limit",
        ));
    }
    let bytes = io::copy(&mut File::open(path)?.take(limit + 1), out)?;
    if bytes > limit {
        return Err(PackIssue::invalid(
            "resource pack exceeds the total size limit",
        ));
    }
    Ok(bytes)
}

fn verify_zip(
    path: &Path,
    mut file: File,
    expected: Option<&str>,
    profile: &VersionProfile,
) -> Result<(Archive, ResourcePackReport, String), PackIssue> {
    let bytes = file.metadata()?.len();
    if bytes > MAX_PACK_TOTAL_BYTES {
        return Err(PackIssue::new(
            if expected.is_some() {
                PackState::Changed
            } else {
                PackState::Invalid
            },
            "resource pack exceeds the total size limit",
        ));
    }
    file.seek(SeekFrom::Start(0))?;
    let sha1 = reader_sha1((&mut file).take(MAX_PACK_TOTAL_BYTES + 1))?;
    if expected.is_some_and(|expected| expected != sha1) {
        return Err(PackIssue::new(
            PackState::Changed,
            "cached pack checksum no longer matches the project pin; add the original pack again or remove it",
        ));
    }
    let mut archive = Archive::from_file(file).map_err(PackIssue::invalid)?;
    let report = inspect_open_zip(path, bytes, &mut archive, profile);
    require_compatible(&report, profile)?;
    Ok((archive, report, sha1))
}

pub(crate) fn require_compatible(
    report: &ResourcePackReport,
    profile: &VersionProfile,
) -> Result<(), PackIssue> {
    match report.status {
        ResourcePackStatus::Compatible if report.errors.is_empty() => Ok(()),
        ResourcePackStatus::IncompatibleFormat => {
            let expected = profile.resource_pack_format.map_or_else(
                || "unknown".to_owned(),
                |major| {
                    profile.pack_format_minors.map_or_else(
                        || major.to_string(),
                        |minor| format!("{major}.{}", minor.resource),
                    )
                },
            );
            Err(PackIssue::new(
                PackState::Incompatible,
                format!(
                "resource pack format {} is incompatible with Minecraft {} (requires {expected})",
                report.supported_formats.clone()
                    .or_else(|| report.pack_format.map(|value| value.to_string()))
                    .unwrap_or_else(|| "unknown".to_owned()),
                profile.minecraft_version
            ),
            ))
        }
        _ => Err(PackIssue::invalid(
            report
                .errors
                .first()
                .cloned()
                .unwrap_or_else(|| format!("{:?}", report.status)),
        )),
    }
}
