use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use flate2::read::DeflateDecoder;
use thiserror::Error;

const EOCD_SIGNATURE: u32 = 0x0605_4b50;
const CENTRAL_SIGNATURE: u32 = 0x0201_4b50;
const LOCAL_SIGNATURE: u32 = 0x0403_4b50;
const EOCD_MIN_BYTES: usize = 22;
const MAX_COMMENT_BYTES: u64 = u16::MAX as u64;
const MAX_ARCHIVE_ENTRIES: usize = 50_000;
const MAX_CENTRAL_DIRECTORY_BYTES: u64 = 16 * 1024 * 1024;
const MAX_ARCHIVE_UNCOMPRESSED_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_SINGLE_ENTRY_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug, Error)]
pub(crate) enum ArchiveError {
    #[error("file is too small to be a zip archive")]
    TooSmall,
    #[error("not a zip archive: end of central directory was not found")]
    NotZip,
    #[error("unsupported zip layout: {0}")]
    Unsupported(&'static str),
    #[error("invalid zip directory: {0}")]
    Invalid(&'static str),
    #[error("unsafe zip entry name: {0}")]
    UnsafeName(String),
    #[error("zip entry was not found: {0}")]
    Missing(String),
    #[error("zip entry is encrypted")]
    Encrypted,
    #[error("zip entry is too large")]
    EntryTooLarge,
    #[error("zip entry uses unsupported compression method {0}")]
    UnsupportedCompression(u16),
    #[error("zip io error: {0}")]
    Io(#[from] std::io::Error),
}

impl ArchiveError {
    pub(crate) fn is_unsafe_name(&self) -> bool {
        matches!(self, ArchiveError::UnsafeName(_))
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ArchiveEntry {
    pub(crate) name: String,
    pub(crate) compression: u16,
    pub(crate) compressed_size: u64,
    pub(crate) uncompressed_size: u64,
    pub(crate) local_offset: u64,
}

/// A zip/JAR archive parsed in memory. Entries are never extracted to disk.
pub(crate) struct Archive {
    file: File,
    file_len: u64,
    entries: Vec<ArchiveEntry>,
}

impl Archive {
    pub(crate) fn open(path: impl AsRef<Path>) -> Result<Self, ArchiveError> {
        let path = path.as_ref();
        let mut file = File::open(path)?;
        let file_len = file.metadata()?.len();
        if file_len < EOCD_MIN_BYTES as u64 {
            return Err(ArchiveError::TooSmall);
        }
        let tail_len = file_len.min(EOCD_MIN_BYTES as u64 + MAX_COMMENT_BYTES);
        file.seek(SeekFrom::Start(file_len - tail_len))?;
        let mut tail = vec![0u8; tail_len as usize];
        file.read_exact(&mut tail)?;
        let eocd = find_eocd(&tail).ok_or(ArchiveError::NotZip)?;
        let eocd_bytes = &tail[eocd..];
        if eocd_bytes.len() < EOCD_MIN_BYTES {
            return Err(ArchiveError::Invalid("truncated end of central directory"));
        }
        let disk = read_u16(eocd_bytes, 4).unwrap_or(0);
        let directory_disk = read_u16(eocd_bytes, 6).unwrap_or(0);
        let entries_on_disk = read_u16(eocd_bytes, 8).unwrap_or(0);
        let total_entries = read_u16(eocd_bytes, 10).unwrap_or(0);
        let directory_size = read_u32(eocd_bytes, 12).unwrap_or(0) as u64;
        let directory_offset = read_u32(eocd_bytes, 16).unwrap_or(0) as u64;
        if disk != 0 || directory_disk != 0 || entries_on_disk != total_entries {
            return Err(ArchiveError::Unsupported("multi-disk archives"));
        }
        if total_entries == u16::MAX
            || directory_size == u32::MAX as u64
            || directory_offset == u32::MAX as u64
        {
            return Err(ArchiveError::Unsupported("zip64 archives"));
        }
        let total_entries = total_entries as usize;
        if total_entries > MAX_ARCHIVE_ENTRIES {
            return Err(ArchiveError::Unsupported("too many archive entries"));
        }
        if directory_size > MAX_CENTRAL_DIRECTORY_BYTES {
            return Err(ArchiveError::Unsupported("central directory is too large"));
        }
        if directory_offset
            .checked_add(directory_size)
            .map_or(true, |end| end > file_len)
        {
            return Err(ArchiveError::Invalid(
                "central directory is outside the file",
            ));
        }
        file.seek(SeekFrom::Start(directory_offset))?;
        let mut directory = vec![0u8; directory_size as usize];
        file.read_exact(&mut directory)?;

        let mut offset = 0usize;
        let mut entries = Vec::with_capacity(total_entries.min(1024));
        let mut total_uncompressed = 0u64;
        for _ in 0..total_entries {
            if offset + 46 > directory.len() {
                return Err(ArchiveError::Invalid("truncated central directory entry"));
            }
            if read_u32(&directory, offset) != Some(CENTRAL_SIGNATURE) {
                return Err(ArchiveError::Invalid("missing central directory signature"));
            }
            let flags = read_u16(&directory, offset + 8).unwrap_or(0);
            let compression = read_u16(&directory, offset + 10).unwrap_or(0);
            let compressed_size = read_u32(&directory, offset + 20).unwrap_or(0) as u64;
            let uncompressed_size = read_u32(&directory, offset + 24).unwrap_or(0) as u64;
            let name_len = read_u16(&directory, offset + 28).unwrap_or(0) as usize;
            let extra_len = read_u16(&directory, offset + 30).unwrap_or(0) as usize;
            let comment_len = read_u16(&directory, offset + 32).unwrap_or(0) as usize;
            let local_offset = read_u32(&directory, offset + 42).unwrap_or(0) as u64;
            if flags & 1 != 0 {
                return Err(ArchiveError::Encrypted);
            }
            if compressed_size == u32::MAX as u64
                || uncompressed_size == u32::MAX as u64
                || local_offset == u32::MAX as u64
            {
                return Err(ArchiveError::Unsupported("zip64 entries"));
            }
            if uncompressed_size > MAX_SINGLE_ENTRY_BYTES {
                return Err(ArchiveError::EntryTooLarge);
            }
            total_uncompressed = total_uncompressed.saturating_add(uncompressed_size);
            if total_uncompressed > MAX_ARCHIVE_UNCOMPRESSED_BYTES {
                return Err(ArchiveError::Unsupported(
                    "archive expands beyond the uncompressed size limit",
                ));
            }
            let name_start = offset + 46;
            let name_end = name_start
                .checked_add(name_len)
                .ok_or(ArchiveError::Invalid("entry name overflow"))?;
            if name_end > directory.len() {
                return Err(ArchiveError::Invalid("truncated entry name"));
            }
            let name = std::str::from_utf8(&directory[name_start..name_end])
                .map_err(|_| ArchiveError::Invalid("entry name is not UTF-8"))?
                .to_owned();
            if !is_safe_entry_name(&name) {
                return Err(ArchiveError::UnsafeName(name));
            }
            entries.push(ArchiveEntry {
                name,
                compression,
                compressed_size,
                uncompressed_size,
                local_offset,
            });
            offset = name_end
                .checked_add(extra_len)
                .and_then(|value| value.checked_add(comment_len))
                .ok_or(ArchiveError::Invalid("central directory overflow"))?;
        }
        Ok(Self {
            file,
            file_len,
            entries,
        })
    }

    pub(crate) fn entries(&self) -> &[ArchiveEntry] {
        &self.entries
    }

    pub(crate) fn total_uncompressed(&self) -> u64 {
        self.entries
            .iter()
            .map(|entry| entry.uncompressed_size)
            .sum()
    }

    /// Reads one entry with an explicit decompressed-size ceiling.
    pub(crate) fn read_entry(
        &mut self,
        name: &str,
        max_bytes: u64,
    ) -> Result<Vec<u8>, ArchiveError> {
        let entry = self
            .entries
            .iter()
            .find(|entry| entry.name == name)
            .cloned()
            .ok_or_else(|| ArchiveError::Missing(name.to_owned()))?;
        if entry.uncompressed_size > max_bytes {
            return Err(ArchiveError::EntryTooLarge);
        }
        if entry.compressed_size > MAX_SINGLE_ENTRY_BYTES {
            return Err(ArchiveError::EntryTooLarge);
        }
        let header_offset = entry.local_offset;
        if header_offset + 30 > self.file_len {
            return Err(ArchiveError::Invalid("local header is outside the file"));
        }
        self.file.seek(SeekFrom::Start(header_offset))?;
        let mut header = [0u8; 30];
        self.file.read_exact(&mut header)?;
        if read_u32(&header, 0) != Some(LOCAL_SIGNATURE) {
            return Err(ArchiveError::Invalid("missing local file header signature"));
        }
        let name_len = read_u16(&header, 26).unwrap_or(0) as u64;
        let extra_len = read_u16(&header, 28).unwrap_or(0) as u64;
        let data_offset = header_offset + 30 + name_len + extra_len;
        if data_offset
            .checked_add(entry.compressed_size)
            .map_or(true, |end| end > self.file_len)
        {
            return Err(ArchiveError::Invalid("entry data is outside the file"));
        }
        self.file.seek(SeekFrom::Start(data_offset))?;
        let mut compressed = vec![0u8; entry.compressed_size as usize];
        self.file.read_exact(&mut compressed)?;
        match entry.compression {
            0 => {
                if entry.compressed_size != entry.uncompressed_size {
                    return Err(ArchiveError::Invalid("stored entry sizes do not match"));
                }
                Ok(compressed)
            }
            8 => {
                let mut decoder = DeflateDecoder::new(compressed.as_slice()).take(max_bytes + 1);
                let mut output = Vec::new();
                decoder.read_to_end(&mut output)?;
                if output.len() as u64 > max_bytes {
                    return Err(ArchiveError::EntryTooLarge);
                }
                Ok(output)
            }
            other => Err(ArchiveError::UnsupportedCompression(other)),
        }
    }
}

fn find_eocd(bytes: &[u8]) -> Option<usize> {
    if bytes.len() < EOCD_MIN_BYTES {
        return None;
    }
    let mut index = bytes.len() - EOCD_MIN_BYTES;
    loop {
        if read_u32(bytes, index) == Some(EOCD_SIGNATURE) {
            let comment_len = read_u16(bytes, index + 20)? as usize;
            if index + EOCD_MIN_BYTES + comment_len <= bytes.len() {
                return Some(index);
            }
        }
        if index == 0 {
            return None;
        }
        index -= 1;
    }
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    bytes
        .get(offset..offset + 2)
        .map(|slice| u16::from_le_bytes([slice[0], slice[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    bytes
        .get(offset..offset + 4)
        .map(|slice| u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

pub(crate) fn is_safe_entry_name(name: &str) -> bool {
    if name.is_empty() || name.contains('\0') || name.contains('\\') || name.starts_with('/') {
        return false;
    }
    let mut parts = name.split('/').peekable();
    while let Some(part) = parts.next() {
        if part == ".." {
            return false;
        }
        // A trailing slash marks a directory; everything else must be non-empty.
        if part.is_empty() && parts.peek().is_some() {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsafe_names_are_rejected() {
        for name in [
            "../evil",
            "assets/../../evil",
            "/absolute",
            "windows\\path",
            "nul\0byte",
            "assets//empty",
            "",
        ] {
            assert!(!is_safe_entry_name(name), "{name:?}");
        }
        for name in [
            "version.json",
            "pack.mcmeta",
            "assets/minecraft/blockstates/stone.json",
            "assets/",
        ] {
            assert!(is_safe_entry_name(name), "{name:?}");
        }
    }
}
