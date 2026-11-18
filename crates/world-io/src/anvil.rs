//! Read-only Minecraft Java Anvil world adapter for DataVersion 4189.
//!
//! This module deliberately has no write path. It reads `level.dat`, discovers the
//! classic (pre-26.1) dimension folders and decodes region files one chunk at a time.
//! Region chunks are returned as bounded [`NbtFile`] values so callers can preserve
//! unknown fields instead of re-encoding a partial chunk.
//!
//! Region format notes (Minecraft Java 1.21.4):
//! - the file starts with two 4 KiB tables: 1024 locations and 1024 timestamps;
//! - a location is a 24-bit sector offset plus an 8-bit sector count;
//! - a chunk record is `length: u32`, `compression: u8`, then `length - 1` bytes;
//! - chunks larger than 255 sectors are stored externally in `c.<x>.<z>.mcc`.
//!
//! External chunks, LZ4 and custom compression are refused with explicit errors. The
//! classic layout is validated before a single chunk is decoded; the 26.1+ split layout
//! is refused instead of being misread as an empty Overworld.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{ErrorKind, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use flate2::read::{GzDecoder, ZlibDecoder};
use thiserror::Error;
use world_domain::{DimensionKind, TARGET_DATA_VERSION};

use crate::nbt::{self, NbtError, NbtFile, NbtValue, MAX_NBT_BYTES, TAG_COMPOUND};

pub const ANVIL_REGION_EXTENSION: &str = "mca";
pub const ANVIL_SECTOR_BYTES: u64 = 4096;
pub const ANVIL_HEADER_SECTORS: u64 = 2;
pub const ANVIL_HEADER_BYTES: usize = (ANVIL_HEADER_SECTORS * ANVIL_SECTOR_BYTES) as usize;
pub const ANVIL_LOCATION_ENTRIES: usize = 1024;
pub const ANVIL_REGION_AXIS_CHUNKS: i32 = 32;
pub const MAX_LEVEL_DAT_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_REGION_FILE_BYTES: u64 = 1024 * 1024 * 1024;
pub const MAX_EXTERNAL_CHUNK_BYTES: u64 = 256 * 1024 * 1024;

const MAX_REGION_SECTORS: u64 = MAX_REGION_FILE_BYTES / ANVIL_SECTOR_BYTES;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AnvilError {
    #[error(transparent)]
    Nbt(#[from] NbtError),
    #[error("world path is not a directory: {0}")]
    NotDirectory(String),
    #[error("refusing to follow symlink: {0}")]
    Symlink(String),
    #[error("required world file is missing: {0}")]
    Missing(String),
    #[error("world I/O failed: {0}")]
    Io(String),
    #[error("level.dat is invalid: {0}")]
    InvalidLevel(String),
    #[error("world DataVersion {found} is not supported; supported version is {supported}")]
    UnsupportedDataVersion { found: i32, supported: i32 },
    #[error("world layout is not supported: {0}")]
    UnsupportedLayout(String),
    #[error("region file is invalid: {0}")]
    InvalidRegion(String),
    #[error("region filename is not r.<x>.<z>.mca: {0}")]
    InvalidRegionName(String),
    #[error("region chunk compression id {0} is not supported")]
    UnsupportedCompression(u8),
    #[error(
        "region chunk ({x}, {z}) is stored externally in a .mcc file, which is not supported yet"
    )]
    ExternalChunk { x: i32, z: i32 },
    #[error("chunk ({x}, {z}) does not belong to region ({region_x}, {region_z})")]
    ChunkOutsideRegion {
        x: i32,
        z: i32,
        region_x: i32,
        region_z: i32,
    },
    #[error("region chunk ({x}, {z}) is not allocated")]
    ChunkNotPresent { x: i32, z: i32 },
    #[error("chunk ({x}, {z}) is invalid: {message}")]
    InvalidChunk { x: i32, z: i32, message: String },
    #[error("unknown dimension: {0}")]
    UnknownDimension(String),
    #[error("Anvil limit exceeded: {0}")]
    LimitExceeded(String),
}

impl AnvilError {
    pub fn code(&self) -> &'static str {
        match self {
            AnvilError::Nbt(error) => error.code(),
            AnvilError::NotDirectory(_)
            | AnvilError::Missing(_)
            | AnvilError::ChunkNotPresent { .. } => "not_found",
            AnvilError::Symlink(_) => "unsafe_path",
            AnvilError::Io(_) => "io_error",
            AnvilError::InvalidLevel(_)
            | AnvilError::InvalidRegion(_)
            | AnvilError::InvalidRegionName(_)
            | AnvilError::InvalidChunk { .. } => "unsupported_format",
            AnvilError::UnsupportedDataVersion { .. }
            | AnvilError::UnsupportedLayout(_)
            | AnvilError::UnsupportedCompression(_) => "unsupported_version",
            AnvilError::ExternalChunk { .. } => "unsupported_format",
            AnvilError::ChunkOutsideRegion { .. } | AnvilError::UnknownDimension(_) => {
                "invalid_argument"
            }
            AnvilError::LimitExceeded(_) => "limit_exceeded",
        }
    }
}

/// Version metadata read from `level.dat`.
///
/// `DataVersion` and `Version.Id` are both stored by Minecraft and must agree. The
/// adapter keeps the complete NBT root so later import stages can inspect unknown
/// fields without guessing their values.
#[derive(Debug, Clone, PartialEq)]
pub struct LevelDat {
    pub data_version: i32,
    pub version_id: i32,
    pub version_name: Option<String>,
    pub level_name: Option<String>,
    pub raw: NbtFile,
}

impl LevelDat {
    pub fn read(path: impl AsRef<Path>) -> Result<Self, AnvilError> {
        let path = path.as_ref();
        let bytes = read_file_bounded(path, MAX_LEVEL_DAT_BYTES)?;
        Self::from_gzip(&bytes)
    }

    pub fn from_gzip(bytes: &[u8]) -> Result<Self, AnvilError> {
        let file = nbt::read_gzip(bytes)?;
        let root = file
            .root
            .as_compound()
            .ok_or_else(|| AnvilError::InvalidLevel("root tag is not a compound".to_owned()))?;
        let data = compound_field(root, "Data", "level.dat")?;
        let data_version = int_field(data, "DataVersion", "level.dat")?;
        let version = compound_field(data, "Version", "level.dat")?;
        let version_id = int_field(version, "Id", "level.dat Version")?;
        if version_id != data_version {
            return Err(AnvilError::InvalidLevel(format!(
                "DataVersion {data_version} does not match Version.Id {version_id}"
            )));
        }
        let version_name = optional_string(version, "Name", "level.dat Version")?;
        let level_name = optional_string(data, "LevelName", "level.dat")?;
        Ok(Self {
            data_version,
            version_id,
            version_name,
            level_name,
            raw: file,
        })
    }

    /// Reject a world whose version is not the one this adapter actually implements.
    pub fn ensure_supported(&self) -> Result<(), AnvilError> {
        if self.data_version != TARGET_DATA_VERSION || self.version_id != TARGET_DATA_VERSION {
            return Err(AnvilError::UnsupportedDataVersion {
                found: self.data_version,
                supported: TARGET_DATA_VERSION,
            });
        }
        Ok(())
    }

    /// Dimension IDs declared by `Data.WorldGenSettings.dimensions`.
    ///
    /// This is metadata, not proof that a dimension folder exists. An empty list means the
    /// tag is absent (for example in a heavily modified world) and is not silently filled
    /// with the standard dimensions.
    pub fn declared_dimensions(&self) -> Result<Vec<String>, AnvilError> {
        let root = self
            .raw
            .root
            .as_compound()
            .ok_or_else(|| AnvilError::InvalidLevel("root tag is not a compound".to_owned()))?;
        let data = compound_field(root, "Data", "level.dat")?;
        let Some(world_gen) = data.get("WorldGenSettings") else {
            return Ok(Vec::new());
        };
        let world_gen = world_gen.as_compound().ok_or_else(|| {
            AnvilError::InvalidLevel("level.dat WorldGenSettings is not a compound".to_owned())
        })?;
        let Some(dimensions) = world_gen.get("dimensions") else {
            return Ok(Vec::new());
        };
        let dimensions = dimensions.as_compound().ok_or_else(|| {
            AnvilError::InvalidLevel(
                "level.dat WorldGenSettings.dimensions is not a compound".to_owned(),
            )
        })?;
        Ok(dimensions.keys().cloned().collect())
    }
}

/// A standard dimension folder in a classic Anvil world.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavaWorldDimension {
    pub id: String,
    pub kind: DimensionKind,
    pub min_y: i32,
    pub height: i32,
    pub folder: PathBuf,
    pub region_folder: PathBuf,
    pub region_files: usize,
}

/// A classic-layout Java world that passed the fixed-version check.
#[derive(Debug, Clone, PartialEq)]
pub struct JavaWorld {
    pub root: PathBuf,
    pub level: LevelDat,
    pub declared_dimensions: Vec<String>,
    pub dimensions: Vec<JavaWorldDimension>,
}

impl JavaWorld {
    /// Opens a classic 1.21.4 world in read-only mode.
    ///
    /// The 26.1+ split layout is detected explicitly and refused: its Overworld does
    /// not live in `region/`, so treating it as a classic world would silently show an
    /// empty map.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, AnvilError> {
        let root = root.as_ref();
        let metadata = fs::symlink_metadata(root).map_err(|error| io_error(root, &error))?;
        if metadata.file_type().is_symlink() {
            return Err(AnvilError::Symlink(root.display().to_string()));
        }
        if !metadata.is_dir() {
            return Err(AnvilError::NotDirectory(root.display().to_string()));
        }

        let level = LevelDat::read(root.join("level.dat"))?;
        level.ensure_supported()?;
        let declared_dimensions = level.declared_dimensions()?;

        let modern_overworld = root.join("dimensions").join("minecraft").join("overworld");
        let split_data = root
            .join("data")
            .join("minecraft")
            .join("world_gen_settings.dat");
        let modern_players = root.join("players");
        let modern_markers = is_real_dir(&modern_overworld)
            || (is_real_dir(&modern_players) && is_real_file(&split_data));
        if modern_markers {
            if is_real_dir(&root.join("region")) {
                return Err(AnvilError::UnsupportedLayout(
                    "mixed classic region/ and 26.1+ split markers".to_owned(),
                ));
            }
            return Err(AnvilError::UnsupportedLayout(
                "26.1+ split world layout is not supported".to_owned(),
            ));
        }
        if !require_directory(&root.join("region"))? {
            return Err(AnvilError::UnsupportedLayout(
                "classic Overworld region/ folder is missing".to_owned(),
            ));
        }

        let custom_dimensions = classic_custom_dimensions(root)?;
        if !custom_dimensions.is_empty() {
            return Err(AnvilError::UnsupportedLayout(format!(
                "classic worlds with custom dimensions are not supported yet: {}",
                custom_dimensions.join(", ")
            )));
        }

        let mut dimensions = vec![discover_dimension(
            root,
            "minecraft:overworld",
            Path::new("."),
            DimensionKind::Overworld,
            -64,
            384,
        )?];
        for (folder, id, kind, min_y, height) in [
            (
                "DIM-1",
                "minecraft:the_nether",
                DimensionKind::Nether,
                0,
                256,
            ),
            ("DIM1", "minecraft:the_end", DimensionKind::End, 0, 256),
        ] {
            if require_directory(&root.join(folder))? {
                dimensions.push(discover_dimension(
                    root,
                    id,
                    Path::new(folder),
                    kind,
                    min_y,
                    height,
                )?);
            }
        }

        Ok(Self {
            root: root.to_path_buf(),
            level,
            declared_dimensions,
            dimensions,
        })
    }

    pub fn dimension(&self, id: &str) -> Option<&JavaWorldDimension> {
        self.dimensions.iter().find(|dimension| dimension.id == id)
    }

    /// Opens one region file of a discovered dimension.
    pub fn read_region(
        &self,
        dimension_id: &str,
        region_x: i32,
        region_z: i32,
    ) -> Result<RegionFile, AnvilError> {
        let dimension = self
            .dimension(dimension_id)
            .ok_or_else(|| AnvilError::UnknownDimension(dimension_id.to_owned()))?;
        RegionFile::open(
            dimension
                .region_folder
                .join(region_file_name(region_x, region_z)),
        )
    }

    /// Lists the `r.<x>.<z>.mca` coordinates present in a dimension folder.
    pub fn region_coordinates(&self, dimension_id: &str) -> Result<Vec<(i32, i32)>, AnvilError> {
        let dimension = self
            .dimension(dimension_id)
            .ok_or_else(|| AnvilError::UnknownDimension(dimension_id.to_owned()))?;
        list_region_coordinates(&dimension.region_folder)
    }
}

/// One allocated chunk record in a region header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegionEntry {
    pub x: i32,
    pub z: i32,
    pub local_index: u16,
    pub offset_sectors: u32,
    pub sector_count: u8,
    pub timestamp: i32,
}

/// Compression scheme recorded in a region chunk header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkCompression {
    Gzip,
    Zlib,
    Uncompressed,
}

impl ChunkCompression {
    pub fn id(self) -> u8 {
        match self {
            ChunkCompression::Gzip => 1,
            ChunkCompression::Zlib => 2,
            ChunkCompression::Uncompressed => 3,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ChunkCompression::Gzip => "gzip",
            ChunkCompression::Zlib => "zlib",
            ChunkCompression::Uncompressed => "none",
        }
    }

    pub(crate) fn from_id(id: u8) -> Result<Self, AnvilError> {
        match id {
            1 => Ok(ChunkCompression::Gzip),
            2 => Ok(ChunkCompression::Zlib),
            3 => Ok(ChunkCompression::Uncompressed),
            other => Err(AnvilError::UnsupportedCompression(other)),
        }
    }
}

/// Bounded structural summary of a decoded chunk. Palettes and blocks are decoded by
/// the next M8 stage; this stage proves the container and the NBT root are trustworthy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegionChunkSummary {
    pub x: i32,
    pub z: i32,
    pub data_version: i32,
    pub status: Option<String>,
    pub y_pos: Option<i32>,
    pub sections: usize,
    pub block_entities: usize,
    pub entities: usize,
}

/// A decoded region chunk. The complete NBT root is retained for passthrough work.
#[derive(Debug, Clone, PartialEq)]
pub struct RegionChunk {
    pub x: i32,
    pub z: i32,
    pub timestamp: i32,
    pub compression: ChunkCompression,
    pub compressed_bytes: usize,
    pub nbt_bytes: usize,
    pub summary: RegionChunkSummary,
    pub nbt: NbtFile,
}

/// A validated region header. Chunks are decoded lazily, one at a time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegionFile {
    path: PathBuf,
    region_x: i32,
    region_z: i32,
    file_len: u64,
    entries: Vec<RegionEntry>,
}

impl RegionFile {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, AnvilError> {
        let path = path.as_ref();
        let (region_x, region_z) = parse_region_file_name(path)?;
        let file_len = file_len_checked(path, MAX_REGION_FILE_BYTES)?;
        if file_len < ANVIL_HEADER_BYTES as u64 || file_len % ANVIL_SECTOR_BYTES != 0 {
            return Err(AnvilError::InvalidRegion(format!(
                "{} is {} bytes; a region file must be 4 KiB aligned and at least 8 KiB",
                path.display(),
                file_len
            )));
        }
        let total_sectors = file_len / ANVIL_SECTOR_BYTES;
        if total_sectors > MAX_REGION_SECTORS {
            return Err(AnvilError::LimitExceeded(format!(
                "{} has {total_sectors} sectors, limit is {MAX_REGION_SECTORS}",
                path.display()
            )));
        }

        let mut header = vec![0u8; ANVIL_HEADER_BYTES];
        let mut file = File::open(path).map_err(|error| io_error(path, &error))?;
        file.read_exact(&mut header)
            .map_err(|error| io_error(path, &error))?;

        let mut entries = Vec::new();
        let mut ranges = Vec::new();
        for index in 0..ANVIL_LOCATION_ENTRIES {
            let base = index * 4;
            let offset = u32::from_be_bytes([0, header[base], header[base + 1], header[base + 2]]);
            let sector_count = header[base + 3];
            if offset == 0 && sector_count == 0 {
                continue;
            }
            if offset == 0 || sector_count == 0 {
                return Err(AnvilError::InvalidRegion(format!(
                    "chunk table entry {index} is partially allocated"
                )));
            }
            if offset < ANVIL_HEADER_SECTORS as u32 {
                return Err(AnvilError::InvalidRegion(format!(
                    "chunk table entry {index} points into the {ANVIL_HEADER_SECTORS}-sector header"
                )));
            }
            let end = u64::from(offset) + u64::from(sector_count);
            if end > total_sectors {
                return Err(AnvilError::InvalidRegion(format!(
                    "chunk table entry {index} ends at sector {end}, past the {total_sectors}-sector file"
                )));
            }
            let local_x = index as i32 % ANVIL_REGION_AXIS_CHUNKS;
            let local_z = index as i32 / ANVIL_REGION_AXIS_CHUNKS;
            let x = region_x
                .checked_mul(ANVIL_REGION_AXIS_CHUNKS)
                .and_then(|value| value.checked_add(local_x))
                .ok_or_else(|| AnvilError::InvalidRegion("chunk coordinate overflow".to_owned()))?;
            let z = region_z
                .checked_mul(ANVIL_REGION_AXIS_CHUNKS)
                .and_then(|value| value.checked_add(local_z))
                .ok_or_else(|| AnvilError::InvalidRegion("chunk coordinate overflow".to_owned()))?;
            let timestamp_offset = ANVIL_SECTOR_BYTES as usize + base;
            let timestamp = i32::from_be_bytes([
                header[timestamp_offset],
                header[timestamp_offset + 1],
                header[timestamp_offset + 2],
                header[timestamp_offset + 3],
            ]);
            entries.push(RegionEntry {
                x,
                z,
                local_index: index as u16,
                offset_sectors: offset,
                sector_count,
                timestamp,
            });
            ranges.push((u64::from(offset), end, index));
        }

        ranges.sort_unstable();
        for pair in ranges.windows(2) {
            if pair[0].1 > pair[1].0 {
                return Err(AnvilError::InvalidRegion(format!(
                    "chunk table entries {} and {} overlap",
                    pair[0].2, pair[1].2
                )));
            }
        }

        Ok(Self {
            path: path.to_path_buf(),
            region_x,
            region_z,
            file_len,
            entries,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn region_coordinates(&self) -> (i32, i32) {
        (self.region_x, self.region_z)
    }

    pub fn file_len(&self) -> u64 {
        self.file_len
    }

    pub fn entries(&self) -> &[RegionEntry] {
        &self.entries
    }

    pub fn read_chunk(&self, x: i32, z: i32) -> Result<RegionChunk, AnvilError> {
        if x.div_euclid(ANVIL_REGION_AXIS_CHUNKS) != self.region_x
            || z.div_euclid(ANVIL_REGION_AXIS_CHUNKS) != self.region_z
        {
            return Err(AnvilError::ChunkOutsideRegion {
                x,
                z,
                region_x: self.region_x,
                region_z: self.region_z,
            });
        }
        let local_x = x.rem_euclid(ANVIL_REGION_AXIS_CHUNKS) as u16;
        let local_z = z.rem_euclid(ANVIL_REGION_AXIS_CHUNKS) as u16;
        let index = local_x + local_z * ANVIL_REGION_AXIS_CHUNKS as u16;
        let entry = self
            .entries
            .iter()
            .find(|entry| entry.local_index == index)
            .copied()
            .ok_or(AnvilError::ChunkNotPresent { x, z })?;

        let path = &self.path;
        let mut file = File::open(path).map_err(|error| io_error(path, &error))?;
        let start = u64::from(entry.offset_sectors) * ANVIL_SECTOR_BYTES;
        file.seek(SeekFrom::Start(start))
            .map_err(|error| io_error(path, &error))?;

        let mut record_header = [0u8; 5];
        file.read_exact(&mut record_header)
            .map_err(|error| io_error(path, &error))?;
        let length = u32::from_be_bytes([
            record_header[0],
            record_header[1],
            record_header[2],
            record_header[3],
        ]);
        if length == 0 {
            return Err(AnvilError::InvalidRegion(format!(
                "chunk ({x}, {z}) has a zero length"
            )));
        }
        let available = u64::from(entry.sector_count) * ANVIL_SECTOR_BYTES - 4;
        if u64::from(length) > available {
            return Err(AnvilError::InvalidRegion(format!(
                "chunk ({x}, {z}) length {length} exceeds its {} allocated sector(s)",
                entry.sector_count
            )));
        }
        let compression_id = record_header[4];
        if compression_id & 0x80 != 0 {
            return Err(AnvilError::ExternalChunk { x, z });
        }
        let compression = ChunkCompression::from_id(compression_id)?;
        let payload_len = (length - 1) as usize;
        let mut payload = vec![0u8; payload_len];
        file.read_exact(&mut payload)
            .map_err(|error| io_error(path, &error))?;
        let nbt_bytes = decompress_chunk(&payload, compression, x, z)?;
        let nbt = nbt::read_uncompressed(&nbt_bytes)?;
        let summary = summarize_chunk(&nbt, x, z)?;
        Ok(RegionChunk {
            x,
            z,
            timestamp: entry.timestamp,
            compression,
            compressed_bytes: payload_len,
            nbt_bytes: nbt_bytes.len(),
            summary,
            nbt,
        })
    }

    /// Decodes every allocated chunk and discards its NBT after summarising it.
    ///
    /// This keeps peak memory at one chunk while proving that the whole region table and
    /// every chunk record are readable. It returns an error on the first bad chunk rather
    /// than reporting a partial region as valid.
    pub fn summarize_all(&self) -> Result<Vec<RegionChunkSummary>, AnvilError> {
        let mut summaries = Vec::with_capacity(self.entries.len());
        for entry in &self.entries {
            summaries.push(self.read_chunk(entry.x, entry.z)?.summary);
        }
        Ok(summaries)
    }
}

pub fn region_file_name(region_x: i32, region_z: i32) -> String {
    format!("r.{region_x}.{region_z}.{ANVIL_REGION_EXTENSION}")
}

fn decompress_chunk(
    payload: &[u8],
    compression: ChunkCompression,
    x: i32,
    z: i32,
) -> Result<Vec<u8>, AnvilError> {
    let read = |reader: &mut dyn Read| -> Result<Vec<u8>, AnvilError> {
        let mut output = Vec::new();
        reader
            .take(MAX_NBT_BYTES as u64 + 1)
            .read_to_end(&mut output)
            .map_err(|_| {
                AnvilError::InvalidRegion(format!(
                    "chunk ({x}, {z}) has an invalid {} stream",
                    compression.name()
                ))
            })?;
        if output.len() > MAX_NBT_BYTES {
            return Err(AnvilError::LimitExceeded(format!(
                "chunk ({x}, {z}) decompresses beyond {MAX_NBT_BYTES} bytes"
            )));
        }
        Ok(output)
    };

    match compression {
        ChunkCompression::Gzip => read(&mut GzDecoder::new(payload)),
        ChunkCompression::Zlib => read(&mut ZlibDecoder::new(payload)),
        ChunkCompression::Uncompressed => {
            if payload.len() > MAX_NBT_BYTES {
                return Err(AnvilError::LimitExceeded(format!(
                    "chunk ({x}, {z}) is larger than {MAX_NBT_BYTES} bytes"
                )));
            }
            Ok(payload.to_vec())
        }
    }
}

fn summarize_chunk(nbt: &NbtFile, x: i32, z: i32) -> Result<RegionChunkSummary, AnvilError> {
    let root = nbt.root.as_compound().ok_or_else(|| {
        AnvilError::InvalidRegion(format!("chunk ({x}, {z}) root is not a compound"))
    })?;
    let data_version = int_field(root, "DataVersion", "chunk")?;
    if data_version != TARGET_DATA_VERSION {
        return Err(AnvilError::UnsupportedDataVersion {
            found: data_version,
            supported: TARGET_DATA_VERSION,
        });
    }
    let stored_x = int_field(root, "xPos", "chunk")?;
    let stored_z = int_field(root, "zPos", "chunk")?;
    if stored_x != x || stored_z != z {
        return Err(AnvilError::InvalidRegion(format!(
            "chunk table position ({x}, {z}) does not match NBT xPos/zPos ({stored_x}, {stored_z})"
        )));
    }
    let status = optional_string(root, "Status", "chunk")?;
    let y_pos = match root.get("yPos") {
        Some(NbtValue::Int(value)) => Some(*value),
        Some(_) => {
            return Err(AnvilError::InvalidRegion(format!(
                "chunk ({x}, {z}) yPos is not an int"
            )))
        }
        None => None,
    };
    let sections = compound_list_len(root, "sections", "chunk", true)?;
    let block_entities = compound_list_len(root, "block_entities", "chunk", false)?;
    let entities = compound_list_len(root, "entities", "chunk", false)?;

    if let Some(NbtValue::List { kind, items }) = root.get("sections") {
        if *kind != TAG_COMPOUND && !items.is_empty() {
            return Err(AnvilError::InvalidRegion(format!(
                "chunk ({x}, {z}) sections is not a list of compounds"
            )));
        }
        for section in items {
            let section = section.as_compound().ok_or_else(|| {
                AnvilError::InvalidRegion(format!("chunk ({x}, {z}) has a non-compound section"))
            })?;
            match section.get("Y") {
                Some(NbtValue::Byte(_)) => {}
                Some(_) => {
                    return Err(AnvilError::InvalidRegion(format!(
                        "chunk ({x}, {z}) section Y is not a byte"
                    )))
                }
                None => {
                    return Err(AnvilError::InvalidRegion(format!(
                        "chunk ({x}, {z}) section has no Y"
                    )))
                }
            }
        }
    }

    Ok(RegionChunkSummary {
        x,
        z,
        data_version,
        status,
        y_pos,
        sections,
        block_entities,
        entities,
    })
}

fn compound_list_len(
    root: &BTreeMap<String, NbtValue>,
    key: &str,
    context: &str,
    required: bool,
) -> Result<usize, AnvilError> {
    match root.get(key) {
        None if required => Err(AnvilError::InvalidRegion(format!(
            "{context} has no {key} list"
        ))),
        None => Ok(0),
        Some(NbtValue::List { kind, items }) => {
            if *kind != TAG_COMPOUND && !items.is_empty() {
                return Err(AnvilError::InvalidRegion(format!(
                    "{context} {key} is not a list of compounds"
                )));
            }
            Ok(items.len())
        }
        Some(_) => Err(AnvilError::InvalidRegion(format!(
            "{context} {key} is not a list"
        ))),
    }
}

fn compound_field<'a>(
    root: &'a BTreeMap<String, NbtValue>,
    key: &str,
    context: &str,
) -> Result<&'a BTreeMap<String, NbtValue>, AnvilError> {
    match root.get(key) {
        Some(NbtValue::Compound(values)) => Ok(values),
        Some(_) => Err(AnvilError::InvalidLevel(format!(
            "{context} {key} is not a compound"
        ))),
        None => Err(AnvilError::InvalidLevel(format!(
            "{context} has no {key} compound"
        ))),
    }
}

fn int_field(
    root: &BTreeMap<String, NbtValue>,
    key: &str,
    context: &str,
) -> Result<i32, AnvilError> {
    match root.get(key) {
        Some(NbtValue::Int(value)) => Ok(*value),
        Some(_) => Err(AnvilError::InvalidLevel(format!(
            "{context} {key} is not an int"
        ))),
        None => Err(AnvilError::InvalidLevel(format!("{context} has no {key}"))),
    }
}

fn optional_string(
    root: &BTreeMap<String, NbtValue>,
    key: &str,
    context: &str,
) -> Result<Option<String>, AnvilError> {
    match root.get(key) {
        Some(NbtValue::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(AnvilError::InvalidLevel(format!(
            "{context} {key} is not a string"
        ))),
        None => Ok(None),
    }
}

fn discover_dimension(
    root: &Path,
    id: &str,
    relative_folder: &Path,
    kind: DimensionKind,
    min_y: i32,
    height: i32,
) -> Result<JavaWorldDimension, AnvilError> {
    let folder = if relative_folder == Path::new(".") {
        root.to_path_buf()
    } else {
        root.join(relative_folder)
    };
    let region_folder = folder.join("region");
    Ok(JavaWorldDimension {
        id: id.to_owned(),
        kind,
        min_y,
        height,
        folder,
        region_files: count_region_files(&region_folder)?,
        region_folder,
    })
}

fn count_region_files(folder: &Path) -> Result<usize, AnvilError> {
    Ok(list_region_coordinates(folder)?.len())
}

fn list_region_coordinates(folder: &Path) -> Result<Vec<(i32, i32)>, AnvilError> {
    let metadata = match fs::symlink_metadata(folder) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(io_error(folder, &error)),
    };
    if metadata.file_type().is_symlink() {
        return Err(AnvilError::Symlink(folder.display().to_string()));
    }
    if !metadata.is_dir() {
        return Err(AnvilError::InvalidRegion(format!(
            "{} is not a directory",
            folder.display()
        )));
    }
    let mut coordinates = Vec::new();
    for entry in fs::read_dir(folder).map_err(|error| io_error(folder, &error))? {
        let entry = entry.map_err(|error| io_error(folder, &error))?;
        let file_type = entry
            .file_type()
            .map_err(|error| io_error(&entry.path(), &error))?;
        if file_type.is_symlink() {
            if entry.file_name().to_string_lossy().ends_with(".mca") {
                return Err(AnvilError::Symlink(entry.path().display().to_string()));
            }
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        if entry.file_name().to_string_lossy().ends_with(".mca") {
            coordinates.push(parse_region_file_name(&entry.path())?);
        }
    }
    coordinates.sort_unstable();
    Ok(coordinates)
}

fn parse_region_file_name(path: &Path) -> Result<(i32, i32), AnvilError> {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| AnvilError::InvalidRegionName(path.display().to_string()))?;
    let stem = name
        .strip_suffix(".mca")
        .ok_or_else(|| AnvilError::InvalidRegionName(name.to_owned()))?;
    let rest = stem
        .strip_prefix("r.")
        .ok_or_else(|| AnvilError::InvalidRegionName(name.to_owned()))?;
    let mut parts = rest.split('.');
    let x = parts
        .next()
        .ok_or_else(|| AnvilError::InvalidRegionName(name.to_owned()))?;
    let z = parts
        .next()
        .ok_or_else(|| AnvilError::InvalidRegionName(name.to_owned()))?;
    if parts.next().is_some() || x.is_empty() || z.is_empty() {
        return Err(AnvilError::InvalidRegionName(name.to_owned()));
    }
    let x = x
        .parse::<i32>()
        .map_err(|_| AnvilError::InvalidRegionName(name.to_owned()))?;
    let z = z
        .parse::<i32>()
        .map_err(|_| AnvilError::InvalidRegionName(name.to_owned()))?;
    Ok((x, z))
}

fn is_real_dir(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
        .unwrap_or(false)
}

fn is_real_file(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
        .unwrap_or(false)
}

/// Returns `true` for a real directory, `false` when the path is absent.
/// Symlinks and non-directories are refused instead of being treated as absent.
fn require_directory(path: &Path) -> Result<bool, AnvilError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(AnvilError::Symlink(path.display().to_string()));
            }
            if !metadata.is_dir() {
                return Err(AnvilError::UnsupportedLayout(format!(
                    "{} is not a directory",
                    path.display()
                )));
            }
            Ok(true)
        }
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(io_error(path, &error)),
    }
}

fn classic_custom_dimensions(root: &Path) -> Result<Vec<String>, AnvilError> {
    let folder = root.join("dimensions");
    if !require_directory(&folder)? {
        return Ok(Vec::new());
    }
    let mut names = Vec::new();
    for entry in fs::read_dir(&folder).map_err(|error| io_error(&folder, &error))? {
        let entry = entry.map_err(|error| io_error(&folder, &error))?;
        names.push(entry.file_name().to_string_lossy().into_owned());
    }
    names.sort();
    Ok(names)
}

fn file_len_checked(path: &Path, limit: u64) -> Result<u64, AnvilError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| match error.kind() {
        ErrorKind::NotFound => AnvilError::Missing(path.display().to_string()),
        _ => io_error(path, &error),
    })?;
    if metadata.file_type().is_symlink() {
        return Err(AnvilError::Symlink(path.display().to_string()));
    }
    if !metadata.is_file() {
        return Err(AnvilError::InvalidRegion(format!(
            "{} is not a regular file",
            path.display()
        )));
    }
    if metadata.len() > limit {
        return Err(AnvilError::LimitExceeded(format!(
            "{} is {} bytes, limit is {limit}",
            path.display(),
            metadata.len()
        )));
    }
    Ok(metadata.len())
}

fn read_file_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, AnvilError> {
    let len = file_len_checked(path, limit)?;
    let file = File::open(path).map_err(|error| io_error(path, &error))?;
    let mut bytes = Vec::with_capacity(len.min(1 << 20) as usize);
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| io_error(path, &error))?;
    if bytes.len() as u64 > limit {
        return Err(AnvilError::LimitExceeded(format!(
            "{} grew beyond the {limit}-byte limit while reading",
            path.display()
        )));
    }
    Ok(bytes)
}

fn io_error(path: &Path, error: &std::io::Error) -> AnvilError {
    if error.kind() == ErrorKind::NotFound {
        AnvilError::Missing(path.display().to_string())
    } else {
        AnvilError::Io(format!("{}: {error}", path.display()))
    }
}
