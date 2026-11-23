//! Disk-space preflight before a download or extraction starts.

use std::path::{Path, PathBuf};

use crate::error::PipelineError;

/// Extra room reserved for filesystem metadata, temporary files and safe replacement.
pub const DISK_MARGIN_BYTES: u64 = 64 * 1024 * 1024;

/// Free bytes available to the current user on the volume that contains `path`. Missing
/// parents are walked up to the nearest existing directory.
pub fn available_space(path: &Path) -> Result<u64, PipelineError> {
    let probe = existing_ancestor(path);
    platform_available_space(&probe)
}

/// Refuses a write when the destination volume cannot hold `needed` bytes plus the safety
/// margin. Returns the available byte count so callers can report it.
pub fn ensure_space(path: &Path, needed: u64) -> Result<u64, PipelineError> {
    let margin = DISK_MARGIN_BYTES.max(needed / 10);
    let required = needed.saturating_add(margin);
    let available = available_space(path)?;
    if available < required {
        return Err(PipelineError::InsufficientSpace {
            path: path.display().to_string(),
            required,
            available,
        });
    }
    Ok(available)
}

fn existing_ancestor(path: &Path) -> PathBuf {
    let mut current = if path.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        path.to_path_buf()
    };
    loop {
        if current.exists() {
            return current;
        }
        match current.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => {
                current = parent.to_path_buf();
            }
            _ => return PathBuf::from("."),
        }
    }
}

#[cfg(windows)]
fn platform_available_space(path: &Path) -> Result<u64, PipelineError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    let probe = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let wide: Vec<u16> = probe
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut available = 0u64;
    // SAFETY: `wide` is a NUL-terminated UTF-16 path and every out-pointer is valid for the
    // duration of the call. The remaining two out-parameters are intentionally null.
    let result = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut available,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if result == 0 {
        return Err(PipelineError::Io(format!(
            "cannot read free disk space for {}: {}",
            probe.display(),
            std::io::Error::last_os_error()
        )));
    }
    Ok(available)
}

#[cfg(unix)]
fn platform_available_space(path: &Path) -> Result<u64, PipelineError> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let probe = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let path = CString::new(probe.as_os_str().as_bytes()).map_err(|_| {
        PipelineError::Io(format!(
            "{} contains a NUL byte and cannot be inspected",
            probe.display()
        ))
    })?;
    let mut stats: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `path` is a valid NUL-terminated path and `stats` is a writable `statvfs`.
    if unsafe { libc::statvfs(path.as_ptr(), &mut stats) } != 0 {
        return Err(PipelineError::Io(format!(
            "cannot read free disk space for {}: {}",
            probe.display(),
            std::io::Error::last_os_error()
        )));
    }
    Ok((stats.f_bavail as u64).saturating_mul(stats.f_frsize as u64))
}

#[cfg(not(any(windows, unix)))]
fn platform_available_space(_path: &Path) -> Result<u64, PipelineError> {
    // The desktop targets Windows, macOS and Linux. On any other platform, skip the preflight
    // rather than blocking a legitimate download.
    Ok(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("furybuilder-disk-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn reports_space_and_walks_up_to_an_existing_parent() {
        let dir = temp_dir("space");
        let available = available_space(&dir).expect("available space");
        assert!(available > 0);
        let missing = dir.join("a").join("b").join("c");
        assert_eq!(existing_ancestor(&missing), dir);
        assert!(ensure_space(&missing, 1024).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn refuses_an_impossible_request_with_a_clear_error() {
        let dir = temp_dir("impossible");
        let error = ensure_space(&dir, u64::MAX).expect_err("must not fit");
        assert!(matches!(
            error,
            PipelineError::InsufficientSpace {
                required,
                available,
                ..
            } if required == u64::MAX && available < required
        ));
        assert_eq!(error.code(), "disk_space");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
