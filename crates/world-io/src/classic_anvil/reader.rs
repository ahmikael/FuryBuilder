//! Read-only Minecraft Java world adapter for classic Anvil and McRegion saves.
//!
//! This module deliberately has no write path. It reads `level.dat`, discovers the
//! classic (pre-26.1) dimension folders and decodes region files one chunk at a time.
//! Region chunks are returned as bounded [`NbtFile`] values so callers can preserve
//! unknown fields instead of re-encoding a partial chunk.
//!
//! Region format notes (Anvil `.mca` and McRegion `.mcr`):
//! - the file starts with two 4 KiB tables: 1024 locations and 1024 timestamps;
//! - a location is a 24-bit sector offset plus an 8-bit sector count;
//! - a chunk record is `length: u32`, `compression: u8`, then `length - 1` bytes;
//! - chunks larger than 255 sectors are stored externally in `c.<x>.<z>.mcc`.
//!
//! External chunks, LZ4 and custom compression are refused with explicit errors. The
//! classic layout is validated before a single chunk is decoded; the 26.1+ split layout
//! is refused instead of being misread as an empty Overworld. Pre-1.8 saves have no
//! `DataVersion`; [`LegacyLevelDat`] validates their `level.dat` format version and
//! [`LegacyWorld`] binds an exact legacy profile to the region extension and height.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{ErrorKind, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use flate2::read::{GzDecoder, ZlibDecoder};
use minecraft_version::VersionProfile;
use thiserror::Error;
use world_domain::DimensionKind;

use crate::nbt::{self, NbtError, NbtFile, NbtValue, MAX_NBT_BYTES, TAG_COMPOUND};
use crate::version_support;

use super::format::{ClassicChunkFormat, LegacyFormat, DATA_VERSION};

pub const ANVIL_REGION_EXTENSION: &str = "mca";
pub const ANVIL_SECTOR_BYTES: u64 = 4096;
pub const ANVIL_HEADER_SECTORS: u64 = 2;
pub const ANVIL_HEADER_BYTES: usize = (ANVIL_HEADER_SECTORS * ANVIL_SECTOR_BYTES) as usize;
pub const ANVIL_LOCATION_ENTRIES: usize = 1024;
pub const ANVIL_REGION_AXIS_CHUNKS: i32 = 32;
pub const MAX_LEVEL_DAT_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_REGION_FILE_BYTES: u64 = 1024 * 1024 * 1024;
pub const MAX_EXTERNAL_CHUNK_BYTES: u64 = 256 * 1024 * 1024;
/// Maximum number of region files listed for one dimension. A supported import is capped
/// at 20,000 chunks (about 20 fully populated region files); the much larger directory
/// limit only prevents a hostile folder from making enumeration itself unbounded.
pub const MAX_REGION_FILES: usize = 4096;

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
    #[error("world DataVersion {found} is not supported; supported versions: {supported:?}")]
    UnsupportedDataVersion { found: i32, supported: Vec<i32> },
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

    /// Returns the built-in profile that owns this world's `DataVersion`.
    pub fn profile(&self) -> Result<VersionProfile, AnvilError> {
        version_support::profile_for(self.data_version).ok_or_else(|| {
            AnvilError::UnsupportedDataVersion {
                found: self.data_version,
                supported: version_support::supported_data_versions(),
            }
        })
    }

    /// Reject a world whose version is not backed by an implemented codec.
    pub fn ensure_supported(&self) -> Result<(), AnvilError> {
        self.profile().map(|_| ())
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

/// Version metadata read from a pre-1.8 `level.dat`.
///
/// These saves predate `DataVersion`; `Data.Version` is an integer level-format
/// version (`19132` for McRegion, `19133` for classic Anvil). It identifies the
/// container/layout but not one exact release, so an exact built-in profile must
/// be supplied by the caller.
#[derive(Debug, Clone, PartialEq)]
pub struct LegacyLevelDat {
    pub format_version: i32,
    pub level_name: Option<String>,
    pub raw: NbtFile,
}

impl LegacyLevelDat {
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
        if data.contains_key("DataVersion") {
            return Err(AnvilError::InvalidLevel(
                "level.dat carries a DataVersion tag; this is not a pre-1.8 save".to_owned(),
            ));
        }
        let format_version = int_field(data, "Version", "level.dat")?;
        if !matches!(format_version, 19132 | 19133) {
            return Err(AnvilError::InvalidLevel(format!(
                "level.dat Version {format_version} is not a pre-1.8 level format"
            )));
        }
        let level_name = optional_string(data, "LevelName", "level.dat")?;
        Ok(Self {
            format_version,
            level_name,
            raw: file,
        })
    }

    /// Returns the container/layout declared by `level.dat`.
    pub fn layout(&self) -> minecraft_version::WorldLayout {
        match self.format_version {
            19132 => minecraft_version::WorldLayout::McRegion,
            19133 => minecraft_version::WorldLayout::ClassicAnvil,
            _ => minecraft_version::WorldLayout::Unknown,
        }
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
    /// The verified profile for this world's exact `DataVersion`.
    pub profile: VersionProfile,
    pub declared_dimensions: Vec<String>,
    pub dimensions: Vec<JavaWorldDimension>,
}

impl JavaWorld {
    /// Opens a verified classic world in read-only mode.
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
        let profile = level.profile()?;
        let declared_dimensions = level.declared_dimensions()?;

        if crate::split_anvil::has_markers(root) {
            if crate::world_selection::has_classic_dimension_markers(root) {
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
        let region_dir = root.join("region");
        if !has_file_with_extension(&region_dir, "mca")
            && has_file_with_extension(&region_dir, "mcr")
        {
            return Err(AnvilError::UnsupportedLayout(
                "legacy McRegion (.mcr) region files are not supported; only Anvil .mca files are readable"
                    .to_owned(),
            ));
        }

        let custom_dimensions = classic_custom_dimensions(root)?;
        if !custom_dimensions.is_empty() {
            return Err(AnvilError::UnsupportedLayout(format!(
                "classic worlds with custom dimensions are not supported yet: {}",
                custom_dimensions.join(", ")
            )));
        }

        let mut dimensions = Vec::new();
        for spec in &profile.dimension_profiles {
            let dimension_root = Path::new(&spec.folder)
                .parent()
                .unwrap_or_else(|| Path::new(""));
            let is_overworld = spec.kind == minecraft_version::DimensionKind::Overworld;
            if !is_overworld && !require_directory(&root.join(dimension_root))? {
                continue;
            }
            dimensions.push(discover_dimension(root, spec)?);
        }

        Ok(Self {
            root: root.to_path_buf(),
            level,
            profile,
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
        RegionFile::open_for_profile(
            dimension
                .region_folder
                .join(region_file_name(region_x, region_z)),
            Some(self.profile.clone()),
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

/// A pre-1.8 Java world bound to an exact built-in legacy profile.
///
/// `level.dat` only distinguishes McRegion from classic Anvil, so the caller
/// chooses the exact release profile. The reader validates that the profile's
/// layout, region extension and dimension heights match the save before any
/// chunk is decoded.
#[derive(Debug, Clone, PartialEq)]
pub struct LegacyWorld {
    pub root: PathBuf,
    pub level: LegacyLevelDat,
    pub profile: VersionProfile,
    pub dimensions: Vec<JavaWorldDimension>,
}

impl LegacyWorld {
    pub fn open(root: impl AsRef<Path>, profile: &VersionProfile) -> Result<Self, AnvilError> {
        let root = root.as_ref();
        let metadata = fs::symlink_metadata(root).map_err(|error| io_error(root, &error))?;
        if metadata.file_type().is_symlink() {
            return Err(AnvilError::Symlink(root.display().to_string()));
        }
        if !metadata.is_dir() {
            return Err(AnvilError::NotDirectory(root.display().to_string()));
        }

        let format = LegacyFormat::for_profile(profile)?;
        let level = LegacyLevelDat::read(root.join("level.dat"))?;
        if level.format_version != format.level_format_version {
            return Err(AnvilError::UnsupportedLayout(format!(
                "level.dat format {} is {}, but profile `{}` expects {}",
                level.format_version,
                if level.format_version == 19132 {
                    "McRegion"
                } else {
                    "classic Anvil"
                },
                profile.id,
                if format.level_format_version == 19132 {
                    "McRegion"
                } else {
                    "classic Anvil"
                },
            )));
        }

        if crate::split_anvil::has_markers(root) {
            return Err(AnvilError::UnsupportedLayout(
                "pre-1.8 world contains 26.1+ split markers".to_owned(),
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
                "pre-1.8 worlds with custom dimensions are not supported: {}",
                custom_dimensions.join(", ")
            )));
        }

        let mut dimensions = Vec::new();
        for spec in &profile.dimension_profiles {
            let dimension_root = Path::new(&spec.folder)
                .parent()
                .unwrap_or_else(|| Path::new(""));
            let is_overworld = spec.kind == minecraft_version::DimensionKind::Overworld;
            if !is_overworld && !require_directory(&root.join(dimension_root))? {
                continue;
            }
            reject_mixed_region_extensions(&root.join(&spec.folder), format.region_extension)?;
            dimensions.push(discover_dimension_with_extension(
                root,
                spec,
                format.region_extension,
            )?);
        }

        Ok(Self {
            root: root.to_path_buf(),
            level,
            profile: profile.clone(),
            dimensions,
        })
    }

    pub fn dimension(&self, id: &str) -> Option<&JavaWorldDimension> {
        self.dimensions.iter().find(|dimension| dimension.id == id)
    }

    /// Opens one `.mcr`/`.mca` region file of a discovered dimension.
    pub fn read_region(
        &self,
        dimension_id: &str,
        region_x: i32,
        region_z: i32,
    ) -> Result<RegionFile, AnvilError> {
        let dimension = self
            .dimension(dimension_id)
            .ok_or_else(|| AnvilError::UnknownDimension(dimension_id.to_owned()))?;
        let format = LegacyFormat::for_profile(&self.profile)?;
        RegionFile::open_for_profile_with_extension(
            dimension.region_folder.join(format!(
                "r.{region_x}.{region_z}.{}",
                format.region_extension
            )),
            Some(self.profile.clone()),
            format.region_extension,
        )
    }

    /// Lists the region coordinates for the profile's container extension.
    pub fn region_coordinates(&self, dimension_id: &str) -> Result<Vec<(i32, i32)>, AnvilError> {
        let dimension = self
            .dimension(dimension_id)
            .ok_or_else(|| AnvilError::UnknownDimension(dimension_id.to_owned()))?;
        let format = LegacyFormat::for_profile(&self.profile)?;
        list_region_coordinates_with_extension(&dimension.region_folder, format.region_extension)
    }

    /// Decodes a legacy chunk with the world's exact profile and dimension bounds.
    pub fn decode_chunk(
        &self,
        dimension_id: &str,
        chunk: &RegionChunk,
    ) -> Result<super::chunk::DecodedChunk, AnvilError> {
        let dimension = self
            .dimension(dimension_id)
            .ok_or_else(|| AnvilError::UnknownDimension(dimension_id.to_owned()))?;
        chunk.decode(dimension, &self.profile)
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

struct DecodedRecord {
    entry: RegionEntry,
    compression: ChunkCompression,
    compressed_bytes: usize,
    nbt_bytes: usize,
    nbt: NbtFile,
}

/// A validated region header. Chunks are decoded lazily, one at a time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegionFile {
    path: PathBuf,
    region_x: i32,
    region_z: i32,
    file_len: u64,
    entries: Vec<RegionEntry>,
    /// Exact world profile when the caller has one. Numeric 1.8 chunks have no
    /// root `DataVersion`, so their summaries need the level.dat profile.
    profile: Option<VersionProfile>,
}

impl RegionFile {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, AnvilError> {
        Self::open_with_extension(path, None, ANVIL_REGION_EXTENSION)
    }

    pub(crate) fn open_for_profile(
        path: impl AsRef<Path>,
        profile: Option<VersionProfile>,
    ) -> Result<Self, AnvilError> {
        Self::open_with_extension(path, profile, ANVIL_REGION_EXTENSION)
    }

    pub(crate) fn open_for_profile_with_extension(
        path: impl AsRef<Path>,
        profile: Option<VersionProfile>,
        extension: &str,
    ) -> Result<Self, AnvilError> {
        Self::open_with_extension(path, profile, extension)
    }

    fn open_with_extension(
        path: impl AsRef<Path>,
        profile: Option<VersionProfile>,
        extension: &str,
    ) -> Result<Self, AnvilError> {
        let path = path.as_ref();
        let (region_x, region_z) = parse_region_file_name_with_extension(path, extension)?;
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
            profile,
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
        let record = self.read_record(x, z)?;
        let summary = summarize_chunk(&record.nbt, x, z, self.profile.as_ref())?;
        Ok(RegionChunk {
            x,
            z,
            timestamp: record.entry.timestamp,
            compression: record.compression,
            compressed_bytes: record.compressed_bytes,
            nbt_bytes: record.nbt_bytes,
            summary,
            nbt: record.nbt,
        })
    }

    /// Reads one chunk record without applying the strict chunk-summary schema.
    ///
    /// This is used for sidecar containers (`poi/`, `entities/`) whose chunk roots do
    /// not carry the block-chunk fields (`xPos`, `zPos`, `Status`, `sections`).
    pub fn read_nbt(&self, x: i32, z: i32) -> Result<NbtFile, AnvilError> {
        Ok(self.read_record(x, z)?.nbt)
    }

    fn read_record(&self, x: i32, z: i32) -> Result<DecodedRecord, AnvilError> {
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
        Ok(DecodedRecord {
            entry,
            compression,
            compressed_bytes: payload_len,
            nbt_bytes: nbt_bytes.len(),
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

fn summarize_chunk(
    nbt: &NbtFile,
    x: i32,
    z: i32,
    profile_hint: Option<&VersionProfile>,
) -> Result<RegionChunkSummary, AnvilError> {
    let root = nbt.root.as_compound().ok_or_else(|| {
        AnvilError::InvalidRegion(format!("chunk ({x}, {z}) root is not a compound"))
    })?;
    if let Some(profile) =
        profile_hint.filter(|profile| profile.family == minecraft_version::VersionFamily::Legacy)
    {
        return summarize_legacy_chunk(root, x, z, profile);
    }
    // Chunks saved before 1.9 have no root DataVersion. The level.dat profile
    // supplies it when JavaWorld opened the region; a bare RegionFile cannot.
    let data_version = match root.get(DATA_VERSION) {
        Some(NbtValue::Int(value)) => *value,
        Some(_) => {
            return Err(AnvilError::InvalidRegion(format!(
                "chunk ({x}, {z}) DataVersion is not an int"
            )))
        }
        None => profile_hint
            .filter(|profile| profile.family == minecraft_version::VersionFamily::Numeric)
            .map(|profile| profile.data_version)
            .ok_or_else(|| {
                AnvilError::InvalidRegion(format!("chunk ({x}, {z}) has no DataVersion"))
            })?,
    };
    let profile = profile_hint
        .filter(|profile| profile.data_version == data_version)
        .or_else(|| version_support::read_profile(data_version))
        .ok_or_else(|| AnvilError::UnsupportedDataVersion {
            found: data_version,
            supported: version_support::supported_data_versions(),
        })?;
    match profile.family {
        minecraft_version::VersionFamily::Flattened => {
            return summarize_flattened_chunk(root, x, z, data_version);
        }
        minecraft_version::VersionFamily::Numeric => {
            return summarize_numeric_chunk(root, x, z, data_version);
        }
        _ => {}
    }
    let format = ClassicChunkFormat::for_profile(profile)?;
    let stored_x = int_field(root, format.x_pos, "chunk")?;
    let stored_z = int_field(root, format.z_pos, "chunk")?;
    if stored_x != x || stored_z != z {
        return Err(AnvilError::InvalidRegion(format!(
            "chunk table position ({x}, {z}) does not match NBT xPos/zPos ({stored_x}, {stored_z})"
        )));
    }
    let status = optional_string(root, format.status, "chunk")?;
    let y_pos = match root.get(format.chunk_y) {
        Some(NbtValue::Int(value)) => Some(*value),
        Some(_) => {
            return Err(AnvilError::InvalidRegion(format!(
                "chunk ({x}, {z}) yPos is not an int"
            )))
        }
        None => None,
    };
    let sections = compound_list_len(root, format.sections, "chunk", true)?;
    let block_entities = compound_list_len(root, format.block_entities, "chunk", false)?;
    let entities = compound_list_len(root, format.entities, "chunk", false)?;

    if let Some(NbtValue::List { kind, items }) = root.get(format.sections) {
        if *kind != TAG_COMPOUND && !items.is_empty() {
            return Err(AnvilError::InvalidRegion(format!(
                "chunk ({x}, {z}) sections is not a list of compounds"
            )));
        }
        for section in items {
            let section = section.as_compound().ok_or_else(|| {
                AnvilError::InvalidRegion(format!("chunk ({x}, {z}) has a non-compound section"))
            })?;
            match section.get(format.section_y) {
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

fn summarize_legacy_chunk(
    root: &BTreeMap<String, NbtValue>,
    x: i32,
    z: i32,
    profile: &VersionProfile,
) -> Result<RegionChunkSummary, AnvilError> {
    let format = LegacyFormat::for_profile(profile)?;
    if root.contains_key(DATA_VERSION) {
        return Err(AnvilError::InvalidRegion(format!(
            "legacy chunk ({x}, {z}) carries a DataVersion tag"
        )));
    }
    let level = root
        .get("Level")
        .and_then(NbtValue::as_compound)
        .ok_or_else(|| {
            AnvilError::InvalidRegion(format!("chunk ({x}, {z}) has no Level compound"))
        })?;
    let stored_x = int_field(level, "xPos", "chunk")?;
    let stored_z = int_field(level, "zPos", "chunk")?;
    if stored_x != x || stored_z != z {
        return Err(AnvilError::InvalidRegion(format!(
            "chunk table position ({x}, {z}) does not match NBT xPos/zPos ({stored_x}, {stored_z})"
        )));
    }
    let status = super::legacy_chunk::legacy_status(level, x, z)?;
    super::legacy_chunk::byte_array(level, "Blocks", format.blocks_len(), x, z)?;
    super::legacy_chunk::byte_array(level, "Data", format.data_len(), x, z)?;
    super::legacy_chunk::byte_array(level, "Biomes", 16 * 16, x, z)?;
    Ok(RegionChunkSummary {
        x,
        z,
        data_version: profile.data_version,
        status,
        y_pos: None,
        sections: (format.height / world_domain::SECTION_SIZE) as usize,
        block_entities: super::chunk::compound_list_len(level, "TileEntities", x, z)?,
        entities: super::chunk::compound_list_len(level, "Entities", x, z)?,
    })
}

fn summarize_numeric_chunk(
    root: &BTreeMap<String, NbtValue>,
    x: i32,
    z: i32,
    data_version: i32,
) -> Result<RegionChunkSummary, AnvilError> {
    let level = root
        .get("Level")
        .and_then(NbtValue::as_compound)
        .ok_or_else(|| {
            AnvilError::InvalidRegion(format!("chunk ({x}, {z}) has no Level compound"))
        })?;
    let stored_x = int_field(level, "xPos", "chunk")?;
    let stored_z = int_field(level, "zPos", "chunk")?;
    if stored_x != x || stored_z != z {
        return Err(AnvilError::InvalidRegion(format!(
            "chunk table position ({x}, {z}) does not match NBT xPos/zPos ({stored_x}, {stored_z})"
        )));
    }
    let status = match level.get("TerrainPopulated") {
        Some(NbtValue::Byte(value)) => Some(if *value != 0 {
            "full".to_owned()
        } else {
            "partial".to_owned()
        }),
        Some(_) => {
            return Err(AnvilError::InvalidRegion(format!(
                "chunk ({x}, {z}) TerrainPopulated is not a byte"
            )))
        }
        None => None,
    };
    let sections = compound_list_len(level, "Sections", "chunk", true)?;
    let block_entities = compound_list_len(level, "TileEntities", "chunk", false)?;
    let entities = compound_list_len(level, "Entities", "chunk", false)?;
    if let Some(NbtValue::List { kind, items }) = level.get("Sections") {
        if *kind != TAG_COMPOUND && !items.is_empty() {
            return Err(AnvilError::InvalidRegion(format!(
                "chunk ({x}, {z}) Sections is not a list of compounds"
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
        y_pos: None,
        sections,
        block_entities,
        entities,
    })
}

fn summarize_flattened_chunk(
    root: &BTreeMap<String, NbtValue>,
    x: i32,
    z: i32,
    data_version: i32,
) -> Result<RegionChunkSummary, AnvilError> {
    let level = root
        .get("Level")
        .and_then(NbtValue::as_compound)
        .ok_or_else(|| {
            AnvilError::InvalidRegion(format!("chunk ({x}, {z}) has no Level compound"))
        })?;
    let stored_x = int_field(level, "xPos", "chunk")?;
    let stored_z = int_field(level, "zPos", "chunk")?;
    if stored_x != x || stored_z != z {
        return Err(AnvilError::InvalidRegion(format!(
            "chunk table position ({x}, {z}) does not match NBT xPos/zPos ({stored_x}, {stored_z})"
        )));
    }
    let status = optional_string(level, "Status", "chunk")?;
    let sections = compound_list_len(level, "Sections", "chunk", true)?;
    let block_entities = compound_list_len(level, "TileEntities", "chunk", false)?;
    let entities = compound_list_len(level, "Entities", "chunk", false)?;
    if let Some(NbtValue::List { kind, items }) = level.get("Sections") {
        if *kind != TAG_COMPOUND && !items.is_empty() {
            return Err(AnvilError::InvalidRegion(format!(
                "chunk ({x}, {z}) Sections is not a list of compounds"
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
        y_pos: None,
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
    spec: &minecraft_version::DimensionProfile,
) -> Result<JavaWorldDimension, AnvilError> {
    discover_dimension_with_extension(root, spec, ANVIL_REGION_EXTENSION)
}

fn discover_dimension_with_extension(
    root: &Path,
    spec: &minecraft_version::DimensionProfile,
    extension: &str,
) -> Result<JavaWorldDimension, AnvilError> {
    let region_folder = root.join(&spec.folder);
    let folder = region_folder
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| root.to_path_buf());
    Ok(JavaWorldDimension {
        id: spec.id.clone(),
        kind: match spec.kind {
            minecraft_version::DimensionKind::Overworld => DimensionKind::Overworld,
            minecraft_version::DimensionKind::Nether => DimensionKind::Nether,
            minecraft_version::DimensionKind::End => DimensionKind::End,
            minecraft_version::DimensionKind::Custom => DimensionKind::Custom,
        },
        min_y: spec.min_y,
        height: spec.height,
        folder,
        region_files: count_region_files_with_extension(&region_folder, extension)?,
        region_folder,
    })
}

fn count_region_files_with_extension(folder: &Path, extension: &str) -> Result<usize, AnvilError> {
    Ok(list_region_coordinates_with_extension(folder, extension)?.len())
}

fn list_region_coordinates(folder: &Path) -> Result<Vec<(i32, i32)>, AnvilError> {
    list_region_coordinates_with_extension(folder, ANVIL_REGION_EXTENSION)
}

fn list_region_coordinates_with_extension(
    folder: &Path,
    extension: &str,
) -> Result<Vec<(i32, i32)>, AnvilError> {
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
        let matches_extension = entry
            .path()
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case(extension));
        if file_type.is_symlink() {
            if matches_extension {
                return Err(AnvilError::Symlink(entry.path().display().to_string()));
            }
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        if matches_extension {
            coordinates.push(parse_region_file_name_with_extension(
                &entry.path(),
                extension,
            )?);
            ensure_region_file_limit(coordinates.len(), extension)?;
        }
    }
    coordinates.sort_unstable();
    Ok(coordinates)
}

pub(crate) fn parse_region_file_name(path: &Path) -> Result<(i32, i32), AnvilError> {
    parse_region_file_name_with_extension(path, ANVIL_REGION_EXTENSION)
}

fn parse_region_file_name_with_extension(
    path: &Path,
    extension: &str,
) -> Result<(i32, i32), AnvilError> {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| AnvilError::InvalidRegionName(path.display().to_string()))?;
    let suffix = format!(".{extension}");
    let stem = name
        .strip_suffix(&suffix)
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

fn reject_mixed_region_extensions(region: &Path, expected: &str) -> Result<(), AnvilError> {
    let other = if expected == "mcr" { "mca" } else { "mcr" };
    let has_expected = has_file_with_extension(region, expected);
    let has_other = has_file_with_extension(region, other);
    if has_expected && has_other {
        return Err(AnvilError::UnsupportedLayout(format!(
            "{} mixes .{expected} and .{other} containers",
            region.display()
        )));
    }
    if !has_expected && has_other {
        return Err(AnvilError::UnsupportedLayout(format!(
            "{} contains only .{other} files, but this profile expects .{expected}",
            region.display()
        )));
    }
    Ok(())
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

fn has_file_with_extension(folder: &Path, extension: &str) -> bool {
    let Ok(entries) = fs::read_dir(folder) else {
        return false;
    };
    entries.flatten().any(|entry| {
        fs::symlink_metadata(entry.path())
            .map(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
            .unwrap_or(false)
            && entry
                .path()
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case(extension))
    })
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

pub(crate) fn read_file_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, AnvilError> {
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

fn ensure_region_file_limit(count: usize, extension: &str) -> Result<(), AnvilError> {
    if count > MAX_REGION_FILES {
        return Err(AnvilError::LimitExceeded(format!(
            "region folder contains more than {MAX_REGION_FILES} .{extension} files"
        )));
    }
    Ok(())
}

fn io_error(path: &Path, error: &std::io::Error) -> AnvilError {
    if error.kind() == ErrorKind::NotFound {
        AnvilError::Missing(path.display().to_string())
    } else {
        AnvilError::Io(format!("{}: {error}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn region_file_count_is_bounded() {
        assert!(ensure_region_file_limit(0, "mca").is_ok());
        assert!(ensure_region_file_limit(MAX_REGION_FILES, "mca").is_ok());
        let error = ensure_region_file_limit(MAX_REGION_FILES + 1, "mca").expect_err("limit");
        assert!(matches!(error, AnvilError::LimitExceeded(_)));
    }
}
