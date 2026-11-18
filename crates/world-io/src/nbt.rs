//! Bounded, deterministic NBT reader/writer.
//!
//! Minecraft NBT is big-endian and uses a small tag set. This implementation deliberately
//! owns the codec instead of exposing a general-purpose serde surface: structure adapters
//! need exact tag counts, unknown-tag detection and hard allocation limits so a malicious
//! file cannot trigger unbounded memory use.
//!
//! The reader validates lengths against the remaining bytes, caps nesting depth, rejects
//! unknown tag ids and refuses trailing bytes after the root tag. All strings must be
//! valid UTF-8. Writing uses a canonical tag order (BTreeMap compound order) so exporting
//! the same structure twice is byte-identical after gzip with the same compression level.

use std::collections::BTreeMap;
use std::io::{Read, Write};

use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use thiserror::Error;

pub const MAX_NBT_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_COMPRESSED_BYTES: usize = 256 * 1024 * 1024;
pub const MAX_NBT_DEPTH: usize = 64;
pub const MAX_COLLECTION_LEN: usize = 8_000_000;
pub const MAX_NAME_BYTES: usize = 32_767;

pub const TAG_END: u8 = 0;
pub const TAG_BYTE: u8 = 1;
pub const TAG_SHORT: u8 = 2;
pub const TAG_INT: u8 = 3;
pub const TAG_LONG: u8 = 4;
pub const TAG_FLOAT: u8 = 5;
pub const TAG_DOUBLE: u8 = 6;
pub const TAG_BYTE_ARRAY: u8 = 7;
pub const TAG_STRING: u8 = 8;
pub const TAG_LIST: u8 = 9;
pub const TAG_COMPOUND: u8 = 10;
pub const TAG_INT_ARRAY: u8 = 11;
pub const TAG_LONG_ARRAY: u8 = 12;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NbtError {
    #[error("unexpected end of NBT data")]
    UnexpectedEof,
    #[error("NBT tag id {0} is not supported")]
    InvalidTag(u8),
    #[error("NBT string is not valid UTF-8")]
    InvalidUtf8,
    #[error("NBT nesting exceeds {MAX_NBT_DEPTH} levels")]
    DepthExceeded,
    #[error("NBT allocation limit exceeded: {0}")]
    LimitExceeded(String),
    #[error("NBT list element type does not match its payload")]
    ListTypeMismatch,
    #[error("NBT list of type End may not contain elements")]
    EmptyListMismatch,
    #[error("trailing bytes after the root NBT tag")]
    TrailingBytes,
    #[error("NBT root tag is not a compound")]
    NonCompoundRoot,
    #[error("NBT gzip stream is invalid or truncated")]
    Gzip,
    #[error("NBT I/O failed: {0}")]
    Io(String),
}

impl NbtError {
    pub fn code(&self) -> &'static str {
        match self {
            NbtError::UnexpectedEof
            | NbtError::InvalidUtf8
            | NbtError::ListTypeMismatch
            | NbtError::EmptyListMismatch
            | NbtError::TrailingBytes
            | NbtError::NonCompoundRoot
            | NbtError::Gzip => "unsupported_format",
            NbtError::InvalidTag(_) => "unsupported_format",
            NbtError::DepthExceeded | NbtError::LimitExceeded(_) => "limit_exceeded",
            NbtError::Io(_) => "internal",
        }
    }
}

/// A named root tag plus deterministic compound/list/array payloads.
#[derive(Debug, Clone, PartialEq)]
pub struct NbtFile {
    pub root_name: String,
    pub root: NbtValue,
}

impl NbtFile {
    pub fn new(name: impl Into<String>, root: NbtValue) -> Self {
        Self {
            root_name: name.into(),
            root,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum NbtValue {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<u8>),
    String(String),
    /// `kind` is the element tag id. Empty lists still need an element type.
    List {
        kind: u8,
        items: Vec<NbtValue>,
    },
    Compound(BTreeMap<String, NbtValue>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

impl NbtValue {
    pub fn tag_id(&self) -> u8 {
        match self {
            NbtValue::Byte(_) => TAG_BYTE,
            NbtValue::Short(_) => TAG_SHORT,
            NbtValue::Int(_) => TAG_INT,
            NbtValue::Long(_) => TAG_LONG,
            NbtValue::Float(_) => TAG_FLOAT,
            NbtValue::Double(_) => TAG_DOUBLE,
            NbtValue::ByteArray(_) => TAG_BYTE_ARRAY,
            NbtValue::String(_) => TAG_STRING,
            NbtValue::List { .. } => TAG_LIST,
            NbtValue::Compound(_) => TAG_COMPOUND,
            NbtValue::IntArray(_) => TAG_INT_ARRAY,
            NbtValue::LongArray(_) => TAG_LONG_ARRAY,
        }
    }

    pub fn compound(entries: impl IntoIterator<Item = (impl Into<String>, NbtValue)>) -> Self {
        NbtValue::Compound(
            entries
                .into_iter()
                .map(|(key, value)| (key.into(), value))
                .collect(),
        )
    }

    pub fn int_list(values: impl IntoIterator<Item = i32>) -> Self {
        NbtValue::List {
            kind: TAG_INT,
            items: values.into_iter().map(NbtValue::Int).collect(),
        }
    }

    pub fn empty_list(kind: u8) -> Self {
        NbtValue::List {
            kind,
            items: Vec::new(),
        }
    }

    pub fn as_compound(&self) -> Option<&BTreeMap<String, NbtValue>> {
        match self {
            NbtValue::Compound(map) => Some(map),
            _ => None,
        }
    }

    pub fn as_list(&self) -> Option<(u8, &[NbtValue])> {
        match self {
            NbtValue::List { kind, items } => Some((*kind, items)),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<i32> {
        match self {
            NbtValue::Int(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_string(&self) -> Option<&str> {
        match self {
            NbtValue::String(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_int_array(&self) -> Option<&[i32]> {
        match self {
            NbtValue::IntArray(values) => Some(values),
            _ => None,
        }
    }
}

/// Reads a gzip-compressed NBT document, capping both compressed and decompressed size.
pub fn read_gzip(bytes: &[u8]) -> Result<NbtFile, NbtError> {
    if bytes.len() > MAX_COMPRESSED_BYTES {
        return Err(NbtError::LimitExceeded(format!(
            "compressed NBT is {} bytes, limit is {MAX_COMPRESSED_BYTES}",
            bytes.len()
        )));
    }
    let mut decoder = GzDecoder::new(bytes);
    let mut output = Vec::new();
    decoder
        .by_ref()
        .take(MAX_NBT_BYTES as u64 + 1)
        .read_to_end(&mut output)
        .map_err(|_| NbtError::Gzip)?;
    if output.len() > MAX_NBT_BYTES {
        return Err(NbtError::LimitExceeded(format!(
            "decompressed NBT exceeds {MAX_NBT_BYTES} bytes"
        )));
    }
    read_uncompressed(&output)
}

/// Writes a gzip-compressed NBT document with the default compression level.
pub fn write_gzip(file: &NbtFile) -> Result<Vec<u8>, NbtError> {
    let raw = write_uncompressed(file)?;
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(&raw)
        .map_err(|error| NbtError::Io(error.to_string()))?;
    encoder
        .finish()
        .map_err(|error| NbtError::Io(error.to_string()))
}

pub fn read_uncompressed(bytes: &[u8]) -> Result<NbtFile, NbtError> {
    if bytes.len() > MAX_NBT_BYTES {
        return Err(NbtError::LimitExceeded(format!(
            "NBT is {} bytes, limit is {MAX_NBT_BYTES}",
            bytes.len()
        )));
    }
    let mut reader = Reader { bytes, position: 0 };
    let tag = reader.u8()?;
    if tag != TAG_COMPOUND {
        return Err(NbtError::NonCompoundRoot);
    }
    let name = reader.string()?;
    let root = reader.payload(TAG_COMPOUND, 0)?;
    if reader.position != bytes.len() {
        return Err(NbtError::TrailingBytes);
    }
    Ok(NbtFile {
        root_name: name,
        root,
    })
}

pub fn write_uncompressed(file: &NbtFile) -> Result<Vec<u8>, NbtError> {
    if file.root.tag_id() != TAG_COMPOUND {
        return Err(NbtError::NonCompoundRoot);
    }
    let mut writer = Writer { bytes: Vec::new() };
    writer.u8(TAG_COMPOUND);
    writer.string(&file.root_name)?;
    writer.payload(&file.root, 0)?;
    Ok(writer.bytes)
}

struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.position)
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], NbtError> {
        if self.remaining() < count {
            return Err(NbtError::UnexpectedEof);
        }
        let slice = &self.bytes[self.position..self.position + count];
        self.position += count;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, NbtError> {
        Ok(self.take(1)?[0])
    }

    fn i16(&mut self) -> Result<i16, NbtError> {
        let bytes: [u8; 2] = self.take(2)?.try_into().expect("length checked");
        Ok(i16::from_be_bytes(bytes))
    }

    fn i32(&mut self) -> Result<i32, NbtError> {
        let bytes: [u8; 4] = self.take(4)?.try_into().expect("length checked");
        Ok(i32::from_be_bytes(bytes))
    }

    fn i64(&mut self) -> Result<i64, NbtError> {
        let bytes: [u8; 8] = self.take(8)?.try_into().expect("length checked");
        Ok(i64::from_be_bytes(bytes))
    }

    fn f32(&mut self) -> Result<f32, NbtError> {
        Ok(f32::from_bits(self.i32()? as u32))
    }

    fn f64(&mut self) -> Result<f64, NbtError> {
        Ok(f64::from_bits(self.i64()? as u64))
    }

    fn string(&mut self) -> Result<String, NbtError> {
        let length = self.i16()?;
        if length < 0 {
            return Err(NbtError::LimitExceeded("negative string length".to_owned()));
        }
        let length = length as usize;
        if length > MAX_NAME_BYTES {
            return Err(NbtError::LimitExceeded(format!(
                "NBT string is {length} bytes"
            )));
        }
        let bytes = self.take(length)?;
        std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|_| NbtError::InvalidUtf8)
    }

    fn length(&mut self, element_size: usize) -> Result<usize, NbtError> {
        let length = self.i32()?;
        if length < 0 {
            return Err(NbtError::LimitExceeded(
                "negative NBT collection length".to_owned(),
            ));
        }
        let length = length as usize;
        if length > MAX_COLLECTION_LEN {
            return Err(NbtError::LimitExceeded(format!(
                "NBT collection has {length} elements"
            )));
        }
        let required = length
            .checked_mul(element_size)
            .ok_or_else(|| NbtError::LimitExceeded("NBT collection size overflow".to_owned()))?;
        if self.remaining() < required {
            return Err(NbtError::UnexpectedEof);
        }
        Ok(length)
    }

    fn payload(&mut self, tag: u8, depth: usize) -> Result<NbtValue, NbtError> {
        if depth > MAX_NBT_DEPTH {
            return Err(NbtError::DepthExceeded);
        }
        Ok(match tag {
            TAG_BYTE => NbtValue::Byte(self.take(1)?[0] as i8),
            TAG_SHORT => NbtValue::Short(self.i16()?),
            TAG_INT => NbtValue::Int(self.i32()?),
            TAG_LONG => NbtValue::Long(self.i64()?),
            TAG_FLOAT => NbtValue::Float(self.f32()?),
            TAG_DOUBLE => NbtValue::Double(self.f64()?),
            TAG_BYTE_ARRAY => {
                let length = self.length(1)?;
                NbtValue::ByteArray(self.take(length)?.to_vec())
            }
            TAG_STRING => NbtValue::String(self.string()?),
            TAG_LIST => {
                let kind = self.u8()?;
                if kind > TAG_LONG_ARRAY {
                    return Err(NbtError::InvalidTag(kind));
                }
                let length = self.length(1)?;
                if kind == TAG_END && length > 0 {
                    return Err(NbtError::EmptyListMismatch);
                }
                if kind == TAG_END {
                    return Ok(NbtValue::empty_list(TAG_END));
                }
                let mut items = Vec::with_capacity(length.min(4096));
                for _ in 0..length {
                    let value = self.payload(kind, depth + 1)?;
                    if value.tag_id() != kind {
                        return Err(NbtError::ListTypeMismatch);
                    }
                    items.push(value);
                }
                NbtValue::List { kind, items }
            }
            TAG_COMPOUND => {
                let mut entries = BTreeMap::new();
                loop {
                    let child = self.u8()?;
                    if child == TAG_END {
                        break;
                    }
                    if child > TAG_LONG_ARRAY {
                        return Err(NbtError::InvalidTag(child));
                    }
                    let name = self.string()?;
                    let value = self.payload(child, depth + 1)?;
                    entries.insert(name, value);
                }
                NbtValue::Compound(entries)
            }
            TAG_INT_ARRAY => {
                let length = self.length(4)?;
                let mut values = Vec::with_capacity(length.min(4096));
                for _ in 0..length {
                    values.push(self.i32()?);
                }
                NbtValue::IntArray(values)
            }
            TAG_LONG_ARRAY => {
                let length = self.length(8)?;
                let mut values = Vec::with_capacity(length.min(4096));
                for _ in 0..length {
                    values.push(self.i64()?);
                }
                NbtValue::LongArray(values)
            }
            TAG_END => return Err(NbtError::InvalidTag(TAG_END)),
            other => return Err(NbtError::InvalidTag(other)),
        })
    }
}

struct Writer {
    bytes: Vec<u8>,
}

impl Writer {
    fn u8(&mut self, value: u8) {
        self.bytes.push(value);
    }

    fn i16(&mut self, value: i16) {
        self.bytes.extend_from_slice(&value.to_be_bytes());
    }

    fn i32(&mut self, value: i32) {
        self.bytes.extend_from_slice(&value.to_be_bytes());
    }

    fn i64(&mut self, value: i64) {
        self.bytes.extend_from_slice(&value.to_be_bytes());
    }

    fn string(&mut self, value: &str) -> Result<(), NbtError> {
        if value.len() > MAX_NAME_BYTES {
            return Err(NbtError::LimitExceeded(format!(
                "NBT string is {} bytes",
                value.len()
            )));
        }
        self.i16(value.len() as i16);
        self.bytes.extend_from_slice(value.as_bytes());
        Ok(())
    }

    fn payload(&mut self, value: &NbtValue, depth: usize) -> Result<(), NbtError> {
        if depth > MAX_NBT_DEPTH {
            return Err(NbtError::DepthExceeded);
        }
        match value {
            NbtValue::Byte(value) => self.u8(*value as u8),
            NbtValue::Short(value) => self.i16(*value),
            NbtValue::Int(value) => self.i32(*value),
            NbtValue::Long(value) => self.i64(*value),
            NbtValue::Float(value) => self.i32(value.to_bits() as i32),
            NbtValue::Double(value) => self.i64(value.to_bits() as i64),
            NbtValue::ByteArray(values) => {
                self.i32(values.len() as i32);
                self.bytes.extend_from_slice(values);
            }
            NbtValue::String(value) => self.string(value)?,
            NbtValue::List { kind, items } => {
                self.u8(*kind);
                if *kind > TAG_LONG_ARRAY {
                    return Err(NbtError::InvalidTag(*kind));
                }
                if *kind == TAG_END && !items.is_empty() {
                    return Err(NbtError::EmptyListMismatch);
                }
                if items.len() > MAX_COLLECTION_LEN {
                    return Err(NbtError::LimitExceeded(
                        "list is too large to write".to_owned(),
                    ));
                }
                self.i32(items.len() as i32);
                for item in items {
                    if item.tag_id() != *kind {
                        return Err(NbtError::ListTypeMismatch);
                    }
                    self.payload(item, depth + 1)?;
                }
            }
            NbtValue::Compound(entries) => {
                for (name, child) in entries {
                    let tag = child.tag_id();
                    if tag == TAG_END {
                        return Err(NbtError::InvalidTag(TAG_END));
                    }
                    self.u8(tag);
                    self.string(name)?;
                    self.payload(child, depth + 1)?;
                }
                self.u8(TAG_END);
            }
            NbtValue::IntArray(values) => {
                self.i32(values.len() as i32);
                for value in values {
                    self.i32(*value);
                }
            }
            NbtValue::LongArray(values) => {
                self.i32(values.len() as i32);
                for value in values {
                    self.i64(*value);
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_file() -> NbtFile {
        let nested = NbtValue::compound([
            ("name", NbtValue::String("Железо".to_owned())),
            ("level", NbtValue::Short(-2)),
            ("empty", NbtValue::empty_list(TAG_COMPOUND)),
            (
                "ints",
                NbtValue::List {
                    kind: TAG_INT,
                    items: vec![NbtValue::Int(1), NbtValue::Int(-2), NbtValue::Int(3)],
                },
            ),
        ]);
        NbtFile::new(
            "root",
            NbtValue::compound([
                ("byte", NbtValue::Byte(-7)),
                ("short", NbtValue::Short(300)),
                ("int", NbtValue::Int(-40)),
                ("long", NbtValue::Long(9_007_199_254_740_991)),
                ("float", NbtValue::Float(1.5)),
                ("double", NbtValue::Double(-2.25)),
                ("bytes", NbtValue::ByteArray(vec![0, 1, 127, 128, 255])),
                ("string", NbtValue::String("minecraft:stone".to_owned())),
                ("nested", nested),
                ("int_array", NbtValue::IntArray(vec![3, -1, 0])),
                ("long_array", NbtValue::LongArray(vec![1, -1, i64::MAX])),
            ]),
        )
    }

    #[test]
    fn round_trips_every_tag_in_canonical_order() {
        let file = sample_file();
        let raw = write_uncompressed(&file).unwrap();
        let decoded = read_uncompressed(&raw).unwrap();
        assert_eq!(decoded, file);
        assert_eq!(write_uncompressed(&decoded).unwrap(), raw);
    }

    #[test]
    fn gzip_round_trip_and_magic_bytes() {
        let file = sample_file();
        let packed = write_gzip(&file).unwrap();
        assert_eq!(&packed[..2], &[0x1f, 0x8b]);
        assert_eq!(read_gzip(&packed).unwrap(), file);
    }

    #[test]
    fn rejects_truncated_trailing_and_unknown_tags() {
        let file = sample_file();
        let raw = write_uncompressed(&file).unwrap();
        assert_eq!(
            read_uncompressed(&raw[..raw.len() - 1]),
            Err(NbtError::UnexpectedEof)
        );
        let mut trailing = raw.clone();
        trailing.push(0);
        assert_eq!(read_uncompressed(&trailing), Err(NbtError::TrailingBytes));

        // Root compound with an unknown child tag id and an empty name.
        let mut unknown = vec![TAG_COMPOUND, 0, 0, 99, 0, 0];
        unknown.push(TAG_END);
        assert_eq!(read_uncompressed(&unknown), Err(NbtError::InvalidTag(99)));

        let mut tag = raw;
        tag[0] = 9;
        assert_eq!(read_uncompressed(&tag), Err(NbtError::NonCompoundRoot));
    }

    #[test]
    fn rejects_depth_bombs() {
        let mut value = NbtValue::Compound(BTreeMap::new());
        for _ in 0..(MAX_NBT_DEPTH + 2) {
            value = NbtValue::compound([("nested", value)]);
        }
        let file = NbtFile::new("deep", value);
        // The writer refuses to produce a stream beyond the shared depth limit.
        assert_eq!(write_uncompressed(&file), Err(NbtError::DepthExceeded));

        // A hand-built stream beyond the limit is refused by the reader as well.
        let mut raw = vec![TAG_COMPOUND, 0, 4, b'd', b'e', b'e', b'p'];
        for _ in 0..(MAX_NBT_DEPTH + 2) {
            raw.push(TAG_COMPOUND);
            raw.extend_from_slice(&1u16.to_be_bytes());
            raw.push(b'n');
        }
        for _ in 0..(MAX_NBT_DEPTH + 2) {
            raw.push(TAG_END);
        }
        raw.push(TAG_END);
        assert_eq!(read_uncompressed(&raw), Err(NbtError::DepthExceeded));
    }

    #[test]
    fn rejects_list_element_type_mismatch_on_write() {
        let file = NbtFile::new(
            "bad",
            NbtValue::compound([(
                "list",
                NbtValue::List {
                    kind: TAG_INT,
                    items: vec![NbtValue::String("not an int".to_owned())],
                },
            )]),
        );
        assert_eq!(write_uncompressed(&file), Err(NbtError::ListTypeMismatch));
    }
}
