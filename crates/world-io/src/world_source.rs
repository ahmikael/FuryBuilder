//! Import source preparation: world folders and safe `.zip` world archives.
//!
//! A Java world is normally a folder, but users often receive one as a `.zip`
//! archive. This module extracts such an archive into a caller-owned staging
//! directory and finds the actual world root, then hands that folder to the
//! regular read-only analysis and import path.
//!
//! Extraction is deliberately conservative: entries must stay inside the
//! staging directory, symlinks and special files are refused, duplicate names
//! are refused, and entry count / uncompressed size limits bound the work. The
//! original archive is never modified.

use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use zip::ZipArchive;

use crate::support_matrix::support_matrix;
use crate::world_import::{analyze_java_world, JavaWorldAnalysis};

/// Maximum number of file entries accepted from one world archive.
pub const MAX_ARCHIVE_ENTRIES: usize = 100_000;
/// Maximum uncompressed bytes accepted from one world archive.
pub const MAX_ARCHIVE_BYTES: u64 = 8 * 1024 * 1024 * 1024;
/// Maximum uncompressed size of one archive entry.
pub const MAX_ARCHIVE_ENTRY_BYTES: u64 = 4 * 1024 * 1024 * 1024;
/// How many directory levels below the archive root are searched for `level.dat`.
pub const MAX_WORLD_ROOT_DEPTH: usize = 2;

/// The kind of path selected by the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorldSourceKind {
    Folder,
    Archive,
}

impl WorldSourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            WorldSourceKind::Folder => "folder",
            WorldSourceKind::Archive => "archive",
        }
    }
}

/// The import policy selected in the wizard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ImportStrategy {
    ImportAsIs,
    Convert,
}

impl ImportStrategy {
    pub fn as_str(self) -> &'static str {
        match self {
            ImportStrategy::ImportAsIs => "importAsIs",
            ImportStrategy::Convert => "convert",
        }
    }
}

/// Bounded facts about the extracted archive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveReport {
    /// Number of extracted file entries.
    pub entries: usize,
    /// Total uncompressed bytes written to staging.
    pub bytes: u64,
}

/// A prepared source folder plus the facts needed by the wizard and importer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedWorldSource {
    kind: WorldSourceKind,
    source: PathBuf,
    root: PathBuf,
    archive: Option<ArchiveReport>,
}

impl PreparedWorldSource {
    pub fn kind(&self) -> WorldSourceKind {
        self.kind
    }

    pub fn source(&self) -> &Path {
        &self.source
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn archive(&self) -> Option<&ArchiveReport> {
        self.archive.as_ref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum WorldSourceError {
    #[error("world source I/O failed: {0}")]
    Io(String),
    #[error("the selected path does not exist: {0}")]
    NotFound(String),
    #[error("the archive is not a valid ZIP file: {0}")]
    InvalidArchive(String),
    #[error("the archive contains an unsafe entry: {0}")]
    UnsafeEntry(String),
    #[error("archive limit exceeded: {0}")]
    LimitExceeded(String),
    #[error("the archive does not contain a Java world folder with level.dat")]
    NoWorldFolder,
    #[error("the archive contains more than one Java world folder: {0}")]
    AmbiguousWorldRoot(String),
    #[error("{0}")]
    UnsupportedSource(String),
}

impl WorldSourceError {
    pub fn code(&self) -> &'static str {
        match self {
            WorldSourceError::Io(_) => "io_error",
            WorldSourceError::NotFound(_) => "not_found",
            WorldSourceError::InvalidArchive(_) => "invalid_archive",
            WorldSourceError::UnsafeEntry(_) => "unsafe_archive_entry",
            WorldSourceError::LimitExceeded(_) => "limit_exceeded",
            WorldSourceError::NoWorldFolder | WorldSourceError::AmbiguousWorldRoot(_) => {
                "unsupported_format"
            }
            WorldSourceError::UnsupportedSource(_) => "unsupported_format",
        }
    }
}

/// Prepares a world folder or extracts a world archive into `staging`.
///
/// The caller owns `staging` and is responsible for removing it. A folder
/// source never touches `staging`.
pub fn prepare_world_source(
    source: impl AsRef<Path>,
    staging: impl AsRef<Path>,
) -> Result<PreparedWorldSource, WorldSourceError> {
    let source = source.as_ref();
    let metadata = fs::symlink_metadata(source).map_err(|error| map_io_error(error, source))?;
    if metadata.file_type().is_symlink() {
        return Err(WorldSourceError::UnsafeEntry(format!(
            "{} is a symlink, junction or reparse point; select the real world folder or archive",
            source.display()
        )));
    }
    if metadata.is_dir() {
        return Ok(PreparedWorldSource {
            kind: WorldSourceKind::Folder,
            source: source.to_path_buf(),
            root: source.to_path_buf(),
            archive: None,
        });
    }
    if metadata.is_file() && is_archive(source) {
        let staging = staging.as_ref();
        let extracted = extract_world_archive(source, staging).and_then(|report| {
            let root = locate_world_root(staging)?;
            Ok((root, report))
        });
        return match extracted {
            Ok((root, archive)) => Ok(PreparedWorldSource {
                kind: WorldSourceKind::Archive,
                source: source.to_path_buf(),
                root,
                archive: Some(archive),
            }),
            Err(error) => {
                let _ = fs::remove_dir_all(staging);
                Err(error)
            }
        };
    }
    Err(WorldSourceError::UnsupportedSource(format!(
        "{} is not a Java world folder or .zip archive",
        source.display()
    )))
}

/// Analyzes a folder or archive without modifying the source.
///
/// Archive extraction failures become explicit blockers in the returned
/// report instead of command errors, so the wizard can explain what happened.
pub fn analyze_world_source(
    source: impl AsRef<Path>,
    staging: impl AsRef<Path>,
) -> JavaWorldAnalysis {
    let source = source.as_ref();
    let staging = staging.as_ref();
    let kind = source_kind_hint(source);
    match prepare_world_source(source, staging) {
        Ok(prepared) => {
            let mut analysis = analyze_java_world(prepared.root());
            if prepared.kind == WorldSourceKind::Archive {
                let _ = fs::remove_dir_all(staging);
                analysis.selection_kind = "world_archive".to_owned();
                analysis.warnings.push(
                    "the .zip archive is extracted to a temporary folder for read-only analysis; the archive itself is never modified"
                        .to_owned(),
                );
            }
            analysis.root = source.display().to_string();
            analysis.source_path = analysis.root.clone();
            analysis.source_kind = prepared.kind;
            analysis.archive = prepared.archive.clone();
            analysis
        }
        Err(error) => match kind {
            WorldSourceKind::Archive => blocked_archive_analysis(source, error),
            WorldSourceKind::Folder => {
                let mut analysis = analyze_java_world(source);
                analysis.source_path = analysis.root.clone();
                analysis.source_kind = WorldSourceKind::Folder;
                analysis.archive = None;
                if analysis.blockers.is_empty() {
                    analysis.blockers.push(error.to_string());
                }
                analysis
            }
        },
    }
}

fn blocked_archive_analysis(source: &Path, error: WorldSourceError) -> JavaWorldAnalysis {
    JavaWorldAnalysis {
        root: source.display().to_string(),
        layout: "archive".to_owned(),
        selection_kind: "world_archive".to_owned(),
        complete_save: false,
        supported: false,
        data_version: None,
        version_id: None,
        version_name: None,
        level_name: None,
        declared_dimensions: Vec::new(),
        dimensions: Vec::new(),
        regions: 0,
        chunks: 0,
        block_entities: 0,
        entities: 0,
        source_path: source.display().to_string(),
        source_kind: WorldSourceKind::Archive,
        archive: None,
        warnings: Vec::new(),
        blockers: vec![error.to_string()],
        support_matrix: support_matrix(),
    }
}

fn source_kind_hint(source: &Path) -> WorldSourceKind {
    if is_archive(source) {
        WorldSourceKind::Archive
    } else {
        WorldSourceKind::Folder
    }
}

fn is_archive(source: &Path) -> bool {
    source
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("zip"))
}

fn map_io_error(error: io::Error, path: &Path) -> WorldSourceError {
    if error.kind() == io::ErrorKind::NotFound {
        WorldSourceError::NotFound(path.display().to_string())
    } else {
        WorldSourceError::Io(format!("{}: {error}", path.display()))
    }
}

fn map_zip_error(error: zip::result::ZipError) -> WorldSourceError {
    match error {
        zip::result::ZipError::Io(error) => WorldSourceError::Io(error.to_string()),
        zip::result::ZipError::InvalidArchive(message) => {
            WorldSourceError::InvalidArchive(message.to_string())
        }
        zip::result::ZipError::UnsupportedArchive(message) => {
            WorldSourceError::InvalidArchive(message.to_string())
        }
        zip::result::ZipError::FileNotFound => {
            WorldSourceError::InvalidArchive("an entry is missing from the archive".to_owned())
        }
        other => WorldSourceError::InvalidArchive(other.to_string()),
    }
}

fn extract_world_archive(
    archive: &Path,
    staging: &Path,
) -> Result<ArchiveReport, WorldSourceError> {
    fs::create_dir_all(staging).map_err(|error| map_io_error(error, staging))?;
    let mut existing = fs::read_dir(staging).map_err(|error| map_io_error(error, staging))?;
    if existing.next().is_some() {
        return Err(WorldSourceError::Io(format!(
            "the archive staging directory is not empty: {}",
            staging.display()
        )));
    }

    let file = File::open(archive).map_err(|error| map_io_error(error, archive))?;
    let mut zip = ZipArchive::new(file).map_err(map_zip_error)?;
    if zip.len() > MAX_ARCHIVE_ENTRIES {
        return Err(WorldSourceError::LimitExceeded(format!(
            "{} entries; the limit is {MAX_ARCHIVE_ENTRIES}",
            zip.len()
        )));
    }

    let mut seen = HashSet::new();
    let mut entries = 0usize;
    let mut bytes = 0u64;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(map_zip_error)?;
        let name = entry.name().map_err(map_zip_error)?.into_owned();
        validate_raw_entry_name(&name)?;
        let relative = entry
            .enclosed_name()
            .ok_or_else(|| WorldSourceError::UnsafeEntry(name.clone()))?;
        validate_entry_path(&relative)?;
        if !seen.insert(relative.clone()) {
            return Err(WorldSourceError::UnsafeEntry(format!(
                "duplicate archive entry {}",
                relative.display()
            )));
        }
        if let Some(mode) = entry.unix_mode() {
            let kind = mode & 0o170000;
            if kind == 0o120000 {
                return Err(WorldSourceError::UnsafeEntry(format!(
                    "{} is a symlink",
                    relative.display()
                )));
            }
            if kind != 0 && kind != 0o100000 && kind != 0o040000 {
                return Err(WorldSourceError::UnsafeEntry(format!(
                    "{} is not a regular file or directory",
                    relative.display()
                )));
            }
        }

        let destination = staging.join(&relative);
        if entry.is_dir() {
            fs::create_dir_all(&destination).map_err(|error| map_io_error(error, &destination))?;
            continue;
        }

        let declared = entry.size();
        if declared > MAX_ARCHIVE_ENTRY_BYTES {
            return Err(WorldSourceError::LimitExceeded(format!(
                "{} is {declared} bytes; one entry may be at most {MAX_ARCHIVE_ENTRY_BYTES} bytes",
                relative.display()
            )));
        }
        bytes = bytes
            .checked_add(declared)
            .ok_or_else(|| WorldSourceError::LimitExceeded("archive size overflow".to_owned()))?;
        if bytes > MAX_ARCHIVE_BYTES {
            return Err(WorldSourceError::LimitExceeded(format!(
                "the archive expands past {MAX_ARCHIVE_BYTES} bytes"
            )));
        }

        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|error| map_io_error(error, parent))?;
        }
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)
            .map_err(|error| map_io_error(error, &destination))?;
        let mut limited = (&mut entry).take(declared.saturating_add(1));
        let written = io::copy(&mut limited, &mut output)
            .map_err(|error| map_io_error(error, &destination))?;
        if written != declared {
            return Err(WorldSourceError::InvalidArchive(format!(
                "{} declares {declared} bytes but yielded {written}",
                relative.display()
            )));
        }
        entries += 1;
    }

    Ok(ArchiveReport { entries, bytes })
}

fn validate_raw_entry_name(raw: &str) -> Result<(), WorldSourceError> {
    if raw.is_empty() {
        return Err(WorldSourceError::UnsafeEntry(
            "an archive entry has an empty path".to_owned(),
        ));
    }
    let path = Path::new(raw);
    for component in path.components() {
        let Component::Normal(name) = component else {
            return Err(WorldSourceError::UnsafeEntry(format!(
                "{raw} is absolute or escapes the archive root"
            )));
        };
        let name = name
            .to_str()
            .ok_or_else(|| WorldSourceError::UnsafeEntry(format!("{raw} is not valid UTF-8")))?;
        validate_entry_name(name, path)?;
    }
    Ok(())
}

fn validate_entry_path(relative: &Path) -> Result<(), WorldSourceError> {
    if relative.as_os_str().is_empty() {
        return Err(WorldSourceError::UnsafeEntry(
            "an archive entry has an empty path".to_owned(),
        ));
    }
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(WorldSourceError::UnsafeEntry(format!(
                "{} escapes the archive root",
                relative.display()
            )));
        };
        let name = name.to_str().ok_or_else(|| {
            WorldSourceError::UnsafeEntry(format!("{} is not valid UTF-8", relative.display()))
        })?;
        validate_entry_name(name, relative)?;
    }
    Ok(())
}

fn validate_entry_name(name: &str, relative: &Path) -> Result<(), WorldSourceError> {
    if name.is_empty() || name == "." || name == ".." {
        return Err(WorldSourceError::UnsafeEntry(format!(
            "{} has an invalid path component",
            relative.display()
        )));
    }
    if name.contains(':') || name.contains('\\') || name.chars().any(char::is_control) {
        return Err(WorldSourceError::UnsafeEntry(format!(
            "{} contains characters that are unsafe on this platform",
            relative.display()
        )));
    }
    if name.ends_with('.') || name.ends_with(' ') {
        return Err(WorldSourceError::UnsafeEntry(format!(
            "{} ends with a dot or space",
            relative.display()
        )));
    }
    let stem = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
    if matches!(
        stem.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    ) {
        return Err(WorldSourceError::UnsafeEntry(format!(
            "{} uses a reserved device name",
            relative.display()
        )));
    }
    Ok(())
}

fn locate_world_root(staging: &Path) -> Result<PathBuf, WorldSourceError> {
    let mut candidates = Vec::new();
    collect_world_roots(staging, 0, &mut candidates)?;
    candidates.sort();
    match candidates.len() {
        0 => Err(WorldSourceError::NoWorldFolder),
        1 => Ok(candidates.remove(0)),
        _ => Err(WorldSourceError::AmbiguousWorldRoot(
            candidates
                .iter()
                .map(|candidate| {
                    candidate
                        .strip_prefix(staging)
                        .unwrap_or(candidate)
                        .display()
                        .to_string()
                })
                .collect::<Vec<_>>()
                .join(", "),
        )),
    }
}

fn collect_world_roots(
    directory: &Path,
    depth: usize,
    candidates: &mut Vec<PathBuf>,
) -> Result<(), WorldSourceError> {
    if is_regular_file(&directory.join("level.dat")) {
        candidates.push(directory.to_path_buf());
        return Ok(());
    }
    if depth >= MAX_WORLD_ROOT_DEPTH {
        return Ok(());
    }
    let entries = fs::read_dir(directory).map_err(|error| map_io_error(error, directory))?;
    for entry in entries {
        let entry = entry.map_err(|error| map_io_error(error, directory))?;
        let metadata = fs::symlink_metadata(entry.path())
            .map_err(|error| map_io_error(error, &entry.path()))?;
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            collect_world_roots(&entry.path(), depth + 1, candidates)?;
        }
    }
    Ok(())
}

fn is_regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_strategy_serializes_with_camel_case_values() {
        assert_eq!(
            serde_json::to_string(&ImportStrategy::ImportAsIs).unwrap(),
            "\"importAsIs\""
        );
        assert_eq!(
            serde_json::to_string(&ImportStrategy::Convert).unwrap(),
            "\"convert\""
        );
    }

    #[test]
    fn unsafe_entry_names_are_rejected_before_writing() {
        let relative = Path::new("world/CON.txt");
        assert!(matches!(
            validate_entry_path(relative),
            Err(WorldSourceError::UnsafeEntry(_))
        ));
        let relative = Path::new("world/name:stream");
        assert!(matches!(
            validate_entry_path(relative),
            Err(WorldSourceError::UnsafeEntry(_))
        ));
        let relative = Path::new("world/trailing.");
        assert!(matches!(
            validate_entry_path(relative),
            Err(WorldSourceError::UnsafeEntry(_))
        ));
    }
}
