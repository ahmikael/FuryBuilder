//! Byte-preserving passthrough copy for supported classic Java worlds.
//!
//! M8.4 deliberately does not re-encode anything. Every source file, including unknown
//! directories and metadata, is copied byte for byte; region files are copied as complete
//! containers rather than being rebuilt from decoded chunks. This is the safe baseline for
//! an overlay: unchanged data never passes through a lossy parser.
//!
//! The copy is validated before publication: the source must be a supported classic
//! world, every allocated chunk is decoded, and the copied tree is opened and validated
//! again. Files are written into a temporary sibling directory, hashed, re-read, and only
//! then renamed to the requested destination. The destination must not already exist.
//!
//! `session.lock` is intentionally not copied: it is a transient lock created by the game.
//! All other files are preserved, including unknown ones. Symlinks, junctions and special
//! files are refused instead of being followed.

use std::fs::{self, File};
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::anvil::{AnvilError, JavaWorld};
use crate::world_selection::classify_world_selection;

pub const MAX_PASSTHROUGH_FILES: usize = 100_000;
pub const MAX_PASSTHROUGH_FILE_BYTES: u64 = 8 * 1024 * 1024 * 1024;
pub const MAX_PASSTHROUGH_TOTAL_BYTES: u64 = 64 * 1024 * 1024 * 1024;
/// Maximum directory nesting copied from an imported world. A save never needs more;
/// the limit keeps a hostile tree from exhausting the stack in the recursive copy.
pub const MAX_PASSTHROUGH_DEPTH: usize = 64;

const COPY_BUFFER_BYTES: usize = 64 * 1024;

/// Receives byte progress from a passthrough copy and decides whether it should continue.
/// Implementations must be cheap: the callback runs for every copied buffer.
pub trait CopyObserver {
    fn progress(&self, copied: u64, total: u64);
    fn is_cancelled(&self) -> bool;
}

/// Observer for copies that cannot be cancelled and do not need progress.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopCopyObserver;

impl CopyObserver for NoopCopyObserver {
    fn progress(&self, _copied: u64, _total: u64) {}
    fn is_cancelled(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PassthroughOptions {
    /// Decode every allocated chunk while validating the source and the copy.
    pub validate_chunks: bool,
    pub max_files: usize,
    pub max_file_bytes: u64,
    pub max_total_bytes: u64,
}

impl Default for PassthroughOptions {
    fn default() -> Self {
        Self {
            validate_chunks: true,
            max_files: MAX_PASSTHROUGH_FILES,
            max_file_bytes: MAX_PASSTHROUGH_FILE_BYTES,
            max_total_bytes: MAX_PASSTHROUGH_TOTAL_BYTES,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopiedFile {
    pub relative_path: PathBuf,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedEntry {
    pub relative_path: PathBuf,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PassthroughReport {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub files: Vec<CopiedFile>,
    pub skipped: Vec<SkippedEntry>,
    pub regions: usize,
    pub chunks: usize,
    pub chunk_validation: bool,
}

impl PassthroughReport {
    pub fn bytes(&self) -> u64 {
        self.files.iter().map(|file| file.bytes).sum()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PassthroughError {
    #[error(transparent)]
    Anvil(#[from] AnvilError),
    #[error("destination already exists: {0}")]
    DestinationExists(String),
    #[error("destination must not be inside the source world: {0}")]
    DestinationInsideSource(String),
    #[error("destination must include a final directory name: {0}")]
    InvalidDestination(String),
    #[error("selection is not a complete world save: {0}")]
    NotAWorldSave(String),
    #[error("refusing to follow symlink, junction or reparse point: {0}")]
    Symlink(String),
    #[error("unsupported file type in world: {0}")]
    NotRegularFile(String),
    #[error("passthrough limit exceeded: {0}")]
    LimitExceeded(String),
    #[error("source changed while it was being copied: {0}")]
    SourceChanged(String),
    #[error("world copy was cancelled before publication")]
    Cancelled,
    #[error("copy verification failed for {path}: expected sha256 {expected}, got {actual}")]
    VerificationFailed {
        path: String,
        expected: String,
        actual: String,
    },
    #[error("world copy I/O failed: {0}")]
    Io(String),
}

impl PassthroughError {
    pub fn code(&self) -> &'static str {
        match self {
            PassthroughError::Anvil(error) => error.code(),
            PassthroughError::DestinationExists(_) => "already_exists",
            PassthroughError::DestinationInsideSource(_)
            | PassthroughError::InvalidDestination(_) => "invalid_argument",
            PassthroughError::NotAWorldSave(_) => "unsupported_format",
            PassthroughError::Symlink(_) => "unsafe_path",
            PassthroughError::NotRegularFile(_) => "unsupported_format",
            PassthroughError::LimitExceeded(_) => "limit_exceeded",
            PassthroughError::SourceChanged(_) => "source_changed",
            PassthroughError::Cancelled => "cancelled",
            PassthroughError::VerificationFailed { .. } => "verification_failed",
            PassthroughError::Io(_) => "io_error",
        }
    }
}

/// Copies a supported classic world to a new directory without changing any file bytes.
pub fn passthrough_java_world(
    source: impl AsRef<Path>,
    destination: impl AsRef<Path>,
) -> Result<PassthroughReport, PassthroughError> {
    passthrough_java_world_observed(
        source,
        destination,
        PassthroughOptions::default(),
        0,
        &NoopCopyObserver,
    )
}

pub fn passthrough_java_world_with_options(
    source: impl AsRef<Path>,
    destination: impl AsRef<Path>,
    options: PassthroughOptions,
) -> Result<PassthroughReport, PassthroughError> {
    passthrough_java_world_observed(source, destination, options, 0, &NoopCopyObserver)
}

/// Copies a supported classic world while reporting byte progress and honouring a
/// cooperative cancel request. Cancellation before publication removes the temporary copy;
/// the source and any existing destination are never touched.
pub fn passthrough_java_world_observed(
    source: impl AsRef<Path>,
    destination: impl AsRef<Path>,
    options: PassthroughOptions,
    total_bytes: u64,
    observer: &dyn CopyObserver,
) -> Result<PassthroughReport, PassthroughError> {
    if observer.is_cancelled() {
        return Err(PassthroughError::Cancelled);
    }
    let source_path = source.as_ref();
    let source_metadata =
        fs::symlink_metadata(source_path).map_err(|error| io_error(source_path, &error))?;
    if is_link_like(&source_metadata) {
        return Err(PassthroughError::Symlink(source_path.display().to_string()));
    }
    let selection = classify_world_selection(source_path);
    if !selection.complete_save {
        return Err(PassthroughError::NotAWorldSave(
            if selection.blockers.is_empty() {
                source_path.display().to_string()
            } else {
                selection.blockers.join("; ")
            },
        ));
    }
    if !source_metadata.is_dir() {
        return Err(PassthroughError::Anvil(AnvilError::NotDirectory(
            source_path.display().to_string(),
        )));
    }
    let source = fs::canonicalize(source_path).map_err(|error| io_error(source_path, &error))?;
    let world = JavaWorld::open(&source)?;
    let (regions, chunks) = validate_world(&world, options.validate_chunks)?;

    let destination = resolve_destination(&source, destination.as_ref())?;
    let parent = destination
        .parent()
        .ok_or_else(|| PassthroughError::InvalidDestination(destination.display().to_string()))?;
    let guard = TempCopy::create(parent)?;
    let source_root = source.clone();

    let mut report = PassthroughReport {
        source,
        destination: destination.clone(),
        files: Vec::new(),
        skipped: Vec::new(),
        regions,
        chunks,
        chunk_validation: options.validate_chunks,
    };
    let mut declared_bytes = 0u64;
    let mut copied_bytes = 0u64;
    copy_dir(
        &source_root,
        guard.path(),
        Path::new(""),
        0,
        &options,
        observer,
        total_bytes,
        &mut copied_bytes,
        &mut report,
        &mut declared_bytes,
    )?;
    if observer.is_cancelled() {
        return Err(PassthroughError::Cancelled);
    }

    let copied = JavaWorld::open(guard.path())?;
    let (copied_regions, copied_chunks) = validate_world(&copied, options.validate_chunks)?;
    if copied_regions != regions || copied_chunks != chunks {
        return Err(PassthroughError::VerificationFailed {
            path: guard.path().display().to_string(),
            expected: format!("{regions} regions / {chunks} chunks"),
            actual: format!("{copied_regions} regions / {copied_chunks} chunks"),
        });
    }

    if observer.is_cancelled() {
        return Err(PassthroughError::Cancelled);
    }
    guard.publish(&destination)?;
    Ok(report)
}

fn validate_world(
    world: &JavaWorld,
    validate_chunks: bool,
) -> Result<(usize, usize), PassthroughError> {
    let mut regions = 0usize;
    let mut chunks = 0usize;
    for dimension in &world.dimensions {
        for (region_x, region_z) in world.region_coordinates(&dimension.id)? {
            let region = world.read_region(&dimension.id, region_x, region_z)?;
            regions += 1;
            chunks += region.entries().len();
            if validate_chunks {
                let summaries = region.summarize_all()?;
                if summaries.len() != region.entries().len() {
                    return Err(PassthroughError::VerificationFailed {
                        path: region.path().display().to_string(),
                        expected: format!("{} chunks", region.entries().len()),
                        actual: format!("{} chunks", summaries.len()),
                    });
                }
            }
        }
    }
    Ok((regions, chunks))
}

fn resolve_destination(source: &Path, destination: &Path) -> Result<PathBuf, PassthroughError> {
    let name = destination
        .file_name()
        .ok_or_else(|| PassthroughError::InvalidDestination(destination.display().to_string()))?;
    let parent = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent = fs::canonicalize(parent).map_err(|error| io_error(parent, &error))?;
    let parent_meta = fs::symlink_metadata(&parent).map_err(|error| io_error(&parent, &error))?;
    if !parent_meta.is_dir() {
        return Err(PassthroughError::InvalidDestination(
            destination.display().to_string(),
        ));
    }
    let resolved = parent.join(name);
    match fs::symlink_metadata(&resolved) {
        Ok(_) => {
            return Err(PassthroughError::DestinationExists(
                resolved.display().to_string(),
            ))
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(io_error(&resolved, &error)),
    }
    if resolved == source || resolved.starts_with(source) {
        return Err(PassthroughError::DestinationInsideSource(
            resolved.display().to_string(),
        ));
    }
    Ok(resolved)
}

#[allow(clippy::too_many_arguments)]
fn copy_dir(
    source_root: &Path,
    temp_root: &Path,
    relative: &Path,
    depth: usize,
    options: &PassthroughOptions,
    observer: &dyn CopyObserver,
    total_bytes: u64,
    copied_bytes: &mut u64,
    report: &mut PassthroughReport,
    declared_bytes: &mut u64,
) -> Result<(), PassthroughError> {
    if observer.is_cancelled() {
        return Err(PassthroughError::Cancelled);
    }
    if depth > MAX_PASSTHROUGH_DEPTH {
        return Err(PassthroughError::LimitExceeded(format!(
            "directory nesting exceeds {MAX_PASSTHROUGH_DEPTH} levels at {}",
            relative.display()
        )));
    }
    let source_dir = source_root.join(relative);
    let mut entries = fs::read_dir(&source_dir)
        .map_err(|error| io_error(&source_dir, &error))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| io_error(&source_dir, &error))?;
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let entry_relative = relative.join(entry.file_name());
        let source_path = entry.path();
        let metadata =
            fs::symlink_metadata(&source_path).map_err(|error| io_error(&source_path, &error))?;
        if is_link_like(&metadata) {
            return Err(PassthroughError::Symlink(source_path.display().to_string()));
        }
        if metadata.is_dir() {
            let target = temp_root.join(&entry_relative);
            fs::create_dir_all(&target).map_err(|error| io_error(&target, &error))?;
            copy_dir(
                source_root,
                temp_root,
                &entry_relative,
                depth + 1,
                options,
                observer,
                total_bytes,
                copied_bytes,
                report,
                declared_bytes,
            )?;
            continue;
        }
        if !metadata.is_file() {
            return Err(PassthroughError::NotRegularFile(
                source_path.display().to_string(),
            ));
        }
        if entry_relative == Path::new("session.lock") {
            report.skipped.push(SkippedEntry {
                relative_path: entry_relative,
                reason: "transient game session lock".to_owned(),
            });
            continue;
        }
        if report.files.len() >= options.max_files {
            return Err(PassthroughError::LimitExceeded(format!(
                "{} files, limit is {}",
                report.files.len(),
                options.max_files
            )));
        }
        let bytes = metadata.len();
        if bytes > options.max_file_bytes {
            return Err(PassthroughError::LimitExceeded(format!(
                "{} is {bytes} bytes, per-file limit is {}",
                source_path.display(),
                options.max_file_bytes
            )));
        }
        *declared_bytes = declared_bytes.checked_add(bytes).ok_or_else(|| {
            PassthroughError::LimitExceeded("total byte count overflow".to_owned())
        })?;
        if *declared_bytes > options.max_total_bytes {
            return Err(PassthroughError::LimitExceeded(format!(
                "{declared_bytes} total bytes, limit is {}",
                options.max_total_bytes
            )));
        }

        let target = temp_root.join(&entry_relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|error| io_error(parent, &error))?;
        }
        let sha256 = copy_file(
            &source_path,
            &target,
            &metadata,
            observer,
            total_bytes,
            copied_bytes,
        )?;
        report.files.push(CopiedFile {
            relative_path: entry_relative,
            bytes,
            sha256,
        });
    }
    Ok(())
}

fn copy_file(
    source: &Path,
    destination: &Path,
    metadata: &fs::Metadata,
    observer: &dyn CopyObserver,
    total_bytes: u64,
    copied_bytes: &mut u64,
) -> Result<String, PassthroughError> {
    let expected_bytes = metadata.len();
    let source_modified = metadata.modified().ok();
    let mut reader = File::open(source).map_err(|error| io_error(source, &error))?;
    let mut writer =
        File::create_new(destination).map_err(|error| io_error(destination, &error))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; COPY_BUFFER_BYTES];
    let mut copied = 0u64;
    loop {
        if observer.is_cancelled() {
            return Err(PassthroughError::Cancelled);
        }
        let read = reader
            .read(&mut buffer)
            .map_err(|error| io_error(source, &error))?;
        if read == 0 {
            break;
        }
        copied = copied
            .checked_add(read as u64)
            .ok_or_else(|| PassthroughError::LimitExceeded("file size overflow".to_owned()))?;
        if copied > expected_bytes {
            return Err(PassthroughError::SourceChanged(
                source.display().to_string(),
            ));
        }
        hasher.update(&buffer[..read]);
        writer
            .write_all(&buffer[..read])
            .map_err(|error| io_error(destination, &error))?;
        *copied_bytes = copied_bytes.saturating_add(read as u64);
        observer.progress(*copied_bytes, total_bytes);
    }
    if copied != expected_bytes {
        return Err(PassthroughError::SourceChanged(
            source.display().to_string(),
        ));
    }
    writer
        .flush()
        .map_err(|error| io_error(destination, &error))?;
    writer
        .sync_all()
        .map_err(|error| io_error(destination, &error))?;
    drop(writer);

    let after = fs::symlink_metadata(source).map_err(|error| io_error(source, &error))?;
    if after.len() != expected_bytes
        || (source_modified.is_some() && after.modified().ok() != source_modified)
    {
        return Err(PassthroughError::SourceChanged(
            source.display().to_string(),
        ));
    }

    let expected = hex(&hasher.finalize());
    let actual = hash_file(destination, observer)?;
    if expected != actual {
        return Err(PassthroughError::VerificationFailed {
            path: destination.display().to_string(),
            expected,
            actual,
        });
    }
    Ok(expected)
}

fn hash_file(path: &Path, observer: &dyn CopyObserver) -> Result<String, PassthroughError> {
    let mut file = File::open(path).map_err(|error| io_error(path, &error))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; COPY_BUFFER_BYTES];
    loop {
        if observer.is_cancelled() {
            return Err(PassthroughError::Cancelled);
        }
        let read = file
            .read(&mut buffer)
            .map_err(|error| io_error(path, &error))?;
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

pub(crate) fn is_link_like(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
        metadata.file_type().is_symlink()
            || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

fn io_error(path: &Path, error: &std::io::Error) -> PassthroughError {
    if error.kind() == ErrorKind::NotFound {
        PassthroughError::Anvil(AnvilError::Missing(path.display().to_string()))
    } else {
        PassthroughError::Io(format!("{}: {error}", path.display()))
    }
}

struct TempCopy {
    path: PathBuf,
    published: bool,
}

impl TempCopy {
    fn create(parent: &Path) -> Result<Self, PassthroughError> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        for attempt in 0..64 {
            let path = parent.join(format!(
                ".furybuilder-passthrough-{}-{stamp}-{attempt}",
                std::process::id()
            ));
            match fs::create_dir(&path) {
                Ok(()) => {
                    return Ok(Self {
                        path,
                        published: false,
                    })
                }
                Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(io_error(&path, &error)),
            }
        }
        Err(PassthroughError::Io(format!(
            "could not allocate a temporary directory in {}",
            parent.display()
        )))
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn publish(mut self, destination: &Path) -> Result<(), PassthroughError> {
        match fs::rename(&self.path, destination) {
            Ok(()) => {
                self.published = true;
                Ok(())
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => Err(
                PassthroughError::DestinationExists(destination.display().to_string()),
            ),
            Err(error) => Err(io_error(destination, &error)),
        }
    }
}

impl Drop for TempCopy {
    fn drop(&mut self) {
        if !self.published {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directory_copy_depth_is_bounded_before_traversal() {
        let root = std::env::temp_dir().join(format!(
            "furybuilder-passthrough-depth-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("temp dir");
        let mut report = PassthroughReport {
            source: root.clone(),
            destination: root.clone(),
            files: Vec::new(),
            skipped: Vec::new(),
            regions: 0,
            chunks: 0,
            chunk_validation: false,
        };
        let mut total = 0u64;
        let mut copied = 0u64;
        let error = copy_dir(
            &root,
            &root,
            Path::new(""),
            MAX_PASSTHROUGH_DEPTH + 1,
            &PassthroughOptions::default(),
            &NoopCopyObserver,
            0,
            &mut copied,
            &mut report,
            &mut total,
        )
        .expect_err("depth guard");
        assert!(matches!(error, PassthroughError::LimitExceeded(_)));
        assert!(report.files.is_empty());
        let _ = fs::remove_dir_all(&root);
    }
}
