//! Streaming size and checksum verification into an atomic destination.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use sha1::Digest as _;

use crate::atomic;
use crate::error::PipelineError;

const PROGRESS_INTERVAL: u64 = 512 * 1024;
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Which announced digest a download must match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Checksum<'a> {
    Sha1(&'a str),
    Sha256(&'a str),
}

impl<'a> Checksum<'a> {
    pub fn expected(self) -> &'a str {
        match self {
            Checksum::Sha1(value) | Checksum::Sha256(value) => value,
        }
    }

    pub fn validate(self) -> Result<(), PipelineError> {
        let (value, length, label) = match self {
            Checksum::Sha1(value) => (value, 40, "SHA-1"),
            Checksum::Sha256(value) => (value, 64, "SHA-256"),
        };
        if value.len() != length || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(PipelineError::Checksum {
                expected: format!("a valid {label}"),
                actual: value.to_owned(),
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadPhase {
    Downloading,
    Verified,
}

impl DownloadPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            DownloadPhase::Downloading => "downloading",
            DownloadPhase::Verified => "verified",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DownloadProgress {
    pub phase: DownloadPhase,
    pub downloaded: u64,
    pub total: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadOutcome {
    pub bytes: u64,
    pub checksum: String,
}

/// Streams `reader` into a same-directory temporary file, enforces the announced size and
/// checksum, and only then atomically publishes the destination. Cancellation and every
/// error remove the temporary file.
pub fn write_verified(
    mut reader: impl Read,
    destination: &Path,
    expected_size: u64,
    checksum: Checksum<'_>,
    max_size: u64,
    cancel: &AtomicBool,
    progress: &dyn Fn(DownloadProgress),
) -> Result<DownloadOutcome, PipelineError> {
    checksum.validate()?;
    if expected_size == 0 {
        return Err(PipelineError::Limit(
            "the announced download size is zero".to_owned(),
        ));
    }
    if expected_size > max_size {
        return Err(PipelineError::Limit(format!(
            "announced size {expected_size} exceeds {max_size} bytes"
        )));
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(PipelineError::Cancelled);
    }

    let mut temporary = TemporaryFile::create(destination)?;
    let mut hasher = Hasher::new(checksum);
    let mut buffer = [0u8; 64 * 1024];
    let mut downloaded = 0u64;
    let mut last_report = 0u64;
    progress(DownloadProgress {
        phase: DownloadPhase::Downloading,
        downloaded: 0,
        total: expected_size,
    });

    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(PipelineError::Cancelled);
        }
        let read = reader
            .read(&mut buffer)
            .map_err(|error| PipelineError::Transport(format!("download interrupted: {error}")))?;
        if read == 0 {
            break;
        }
        downloaded = downloaded.saturating_add(read as u64);
        if downloaded > expected_size || downloaded > max_size {
            return Err(PipelineError::Limit(format!(
                "received at least {downloaded} bytes, expected {expected_size}"
            )));
        }
        hasher.update(&buffer[..read]);
        temporary.write(&buffer[..read])?;
        if downloaded - last_report >= PROGRESS_INTERVAL || downloaded == expected_size {
            last_report = downloaded;
            progress(DownloadProgress {
                phase: DownloadPhase::Downloading,
                downloaded,
                total: expected_size,
            });
        }
    }

    temporary.sync()?;
    if downloaded != expected_size {
        return Err(PipelineError::Size {
            expected: expected_size,
            actual: downloaded,
        });
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(PipelineError::Cancelled);
    }
    let actual = hasher.finish();
    if !actual.eq_ignore_ascii_case(checksum.expected()) {
        return Err(PipelineError::Checksum {
            expected: checksum.expected().to_owned(),
            actual,
        });
    }
    temporary.publish(destination)?;
    progress(DownloadProgress {
        phase: DownloadPhase::Verified,
        downloaded,
        total: expected_size,
    });
    Ok(DownloadOutcome {
        bytes: downloaded,
        checksum: actual,
    })
}

enum Hasher {
    Sha1(sha1::Sha1),
    Sha256(sha2::Sha256),
}

impl Hasher {
    fn new(checksum: Checksum<'_>) -> Self {
        match checksum {
            Checksum::Sha1(_) => Hasher::Sha1(sha1::Sha1::new()),
            Checksum::Sha256(_) => Hasher::Sha256(sha2::Sha256::new()),
        }
    }

    fn update(&mut self, bytes: &[u8]) {
        match self {
            Hasher::Sha1(hasher) => sha1::Digest::update(hasher, bytes),
            Hasher::Sha256(hasher) => sha2::Digest::update(hasher, bytes),
        }
    }

    fn finish(self) -> String {
        match self {
            Hasher::Sha1(hasher) => hex(&sha1::Digest::finalize(hasher)),
            Hasher::Sha256(hasher) => hex(&sha2::Digest::finalize(hasher)),
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(output, "{byte:02x}");
    }
    output
}

struct TemporaryFile {
    path: PathBuf,
    file: Option<File>,
    published: bool,
}

impl TemporaryFile {
    fn create(destination: &Path) -> Result<Self, PipelineError> {
        let parent = destination
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        if destination.file_name().is_none() {
            return Err(PipelineError::Io(format!(
                "{} has no file name",
                destination.display()
            )));
        }
        let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!(".download-{}-{counter}.tmp", std::process::id()));
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| {
                PipelineError::Io(format!("cannot create {}: {error}", path.display()))
            })?;
        Ok(Self {
            path,
            file: Some(file),
            published: false,
        })
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), PipelineError> {
        self.file
            .as_mut()
            .ok_or_else(|| PipelineError::Io("the temporary file is closed".to_owned()))?
            .write_all(bytes)
            .map_err(|error| PipelineError::Io(format!("cannot write download: {error}")))
    }

    fn sync(&mut self) -> Result<(), PipelineError> {
        self.file
            .as_mut()
            .ok_or_else(|| PipelineError::Io("the temporary file is closed".to_owned()))?
            .sync_all()
            .map_err(|error| PipelineError::Io(format!("cannot flush download: {error}")))
    }

    fn publish(mut self, destination: &Path) -> Result<(), PipelineError> {
        // Windows cannot replace a destination while the temporary file is still open.
        self.file.take();
        atomic::replace_file(&self.path, destination)?;
        self.published = true;
        Ok(())
    }
}

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        if self.published {
            return;
        }
        self.file.take();
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("furybuilder-verify-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn sha1(bytes: &[u8]) -> String {
        hex(&sha1::Sha1::digest(bytes))
    }

    #[test]
    fn writes_and_publishes_a_verified_file() {
        let dir = temp_dir("success");
        let destination = dir.join("client.jar");
        let data = b"verified payload";
        let progress = std::cell::RefCell::new(Vec::new());
        let outcome = write_verified(
            Cursor::new(data),
            &destination,
            data.len() as u64,
            Checksum::Sha1(&sha1(data)),
            1024,
            &AtomicBool::new(false),
            &|event| progress.borrow_mut().push(event),
        )
        .expect("verified write");
        assert_eq!(outcome.bytes, data.len() as u64);
        assert_eq!(fs::read(&destination).expect("read"), data);
        assert!(progress
            .borrow()
            .iter()
            .any(|event| event.phase == DownloadPhase::Verified));
        assert!(!dir
            .read_dir()
            .expect("read dir")
            .flatten()
            .any(|entry| entry
                .file_name()
                .to_string_lossy()
                .starts_with(".download-")));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_a_wrong_checksum_without_leaving_bytes() {
        let dir = temp_dir("checksum");
        let destination = dir.join("server.jar");
        let data = b"server payload";
        let error = write_verified(
            Cursor::new(data),
            &destination,
            data.len() as u64,
            Checksum::Sha1(&"b".repeat(40)),
            1024,
            &AtomicBool::new(false),
            &|_| {},
        )
        .expect_err("wrong checksum");
        assert!(matches!(error, PipelineError::Checksum { .. }));
        assert!(!destination.exists());
        assert_eq!(fs::read_dir(&dir).expect("read dir").count(), 0);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_a_size_mismatch_and_cancellation_without_publishing() {
        let dir = temp_dir("size");
        let destination = dir.join("server.jar");
        let data = b"short";
        let error = write_verified(
            Cursor::new(data),
            &destination,
            data.len() as u64 + 1,
            Checksum::Sha1(&sha1(data)),
            1024,
            &AtomicBool::new(false),
            &|_| {},
        )
        .expect_err("wrong size");
        assert!(matches!(error, PipelineError::Size { .. }));
        assert!(!destination.exists());

        let error = write_verified(
            Cursor::new(data),
            &destination,
            data.len() as u64,
            Checksum::Sha1(&sha1(data)),
            1024,
            &AtomicBool::new(true),
            &|_| {},
        )
        .expect_err("cancelled");
        assert!(matches!(error, PipelineError::Cancelled));
        assert!(!destination.exists());
        assert_eq!(fs::read_dir(&dir).expect("read dir").count(), 0);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn refuses_invalid_checksums_and_zero_or_oversized_requests() {
        let dir = temp_dir("invalid");
        let destination = dir.join("file");
        let cancel = AtomicBool::new(false);
        assert!(matches!(
            write_verified(
                Cursor::new(b"x"),
                &destination,
                1,
                Checksum::Sha1("abc"),
                10,
                &cancel,
                &|_| {}
            ),
            Err(PipelineError::Checksum { .. })
        ));
        assert!(matches!(
            write_verified(
                Cursor::new(b""),
                &destination,
                0,
                Checksum::Sha1(&"a".repeat(40)),
                10,
                &cancel,
                &|_| {}
            ),
            Err(PipelineError::Limit(_))
        ));
        assert!(matches!(
            write_verified(
                Cursor::new(b"x"),
                &destination,
                20,
                Checksum::Sha256(&"a".repeat(64)),
                10,
                &cancel,
                &|_| {}
            ),
            Err(PipelineError::Limit(_))
        ));
        let _ = fs::remove_dir_all(&dir);
    }
}
