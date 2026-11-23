//! Atomic file replacement and directory publication.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::PipelineError;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Replaces `destination` with `source` in one filesystem operation. The caller must keep
/// both paths on the same volume.
pub fn replace_file(source: &Path, destination: &Path) -> Result<(), PipelineError> {
    if let Some(parent) = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    replace_file_platform(source, destination).map_err(|error| {
        PipelineError::Io(format!("cannot publish {}: {error}", destination.display()))
    })
}

/// Writes `bytes` to a same-directory temporary file, flushes it, and atomically replaces
/// `path`. A crash leaves the previous file intact instead of a half-written one.
pub fn write_file(path: &Path, bytes: &[u8]) -> Result<(), PipelineError> {
    let temporary = temporary_sibling(path)?;
    let result = (|| {
        let mut file = File::create(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        replace_file(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

/// Publishes a fully prepared directory. The destination must not exist, so an interrupted
/// install can never be mistaken for a complete one.
pub fn publish_directory(source: &Path, destination: &Path) -> Result<(), PipelineError> {
    if destination.exists() {
        return Err(PipelineError::Io(format!(
            "{} already exists",
            destination.display()
        )));
    }
    if let Some(parent) = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    fs::rename(source, destination).map_err(|error| {
        PipelineError::Io(format!("cannot publish {}: {error}", destination.display()))
    })
}

fn temporary_sibling(path: &Path) -> Result<PathBuf, PipelineError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    if path.file_name().is_none() {
        return Err(PipelineError::Io(format!(
            "{} has no file name",
            path.display()
        )));
    }
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    Ok(parent.join(format!(".download-{}-{counter}.tmp", std::process::id())))
}

#[cfg(windows)]
fn replace_file_platform(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let source: Vec<u16> = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: both buffers are NUL-terminated UTF-16 paths and stay alive for the call.
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn replace_file_platform(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("furybuilder-atomic-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn write_file_replaces_existing_content_without_leaving_temporaries() {
        let dir = temp_dir("write");
        let path = dir.join("settings.json");
        fs::write(&path, b"old").expect("seed");
        write_file(&path, b"new content").expect("atomic write");
        assert_eq!(fs::read(&path).expect("read"), b"new content");
        let leftovers: Vec<_> = fs::read_dir(&dir)
            .expect("read dir")
            .flatten()
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".download-")
            })
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn publish_directory_refuses_to_overwrite_an_existing_install() {
        let dir = temp_dir("publish");
        let source = dir.join("staging");
        let destination = dir.join("installed");
        fs::create_dir_all(&source).expect("staging");
        fs::write(source.join("file"), b"x").expect("file");
        publish_directory(&source, &destination).expect("publish");
        assert!(destination.join("file").is_file());
        assert!(!source.exists());

        let second = dir.join("staging-2");
        fs::create_dir_all(&second).expect("staging 2");
        let error = publish_directory(&second, &destination).expect_err("must refuse");
        assert!(error.to_string().contains("already exists"));
        assert!(second.exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
