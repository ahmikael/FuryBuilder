//! Decoding of Minecraft Java 1.21.4 chunk sections.
//!
//! Region containers are decoded in [`crate::anvil`]. This module turns the NBT inside a
//! chunk into real [`Section`] values and bounded biome data. It rejects any layout it
//! cannot prove instead of treating a malformed palette as air.
//!
//! 1.21.4 paletted containers use values that do not span `long` boundaries:
//! - block states use `max(4, ceil(log2(palette_len)))` bits and 4096 indices;
//! - biomes use `ceil(log2(palette_len))` bits and 64 cells of 4×4×4 blocks.
//!
//! The decoded block indices are re-packed by `Section::from_palette_indices` into the
//! canonical internal layout. Unknown NBT remains on the original [`crate::anvil::RegionChunk`].

use std::collections::{BTreeMap, BTreeSet};

use world_domain::{BlockState, Chunk, ChunkPos, Section, SECTION_SIZE};

use crate::anvil::{AnvilError, JavaWorld, JavaWorldDimension, RegionChunk};
use crate::nbt::{NbtFile, NbtValue, TAG_COMPOUND, TAG_STRING};

pub const BLOCK_SECTION_VOLUME: usize = 4096;
pub const BIOME_SECTION_VOLUME: usize = 64;
pub const MAX_BLOCK_PALETTE: usize = 4096;
pub const MAX_BIOME_PALETTE: usize = 64;

/// Biome palette plus 64 cells in YZX order, each cell covering 4×4×4 blocks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BiomeSection {
    palette: Vec<String>,
    cells: Vec<u16>,
}

impl BiomeSection {
    pub fn from_palette(palette: Vec<String>, cells: Vec<u16>) -> Option<Self> {
        if palette.is_empty()
            || palette.len() > MAX_BIOME_PALETTE
            || cells.len() != BIOME_SECTION_VOLUME
        {
            return None;
        }
        if cells.iter().any(|index| *index as usize >= palette.len()) {
            return None;
        }
        Some(Self { palette, cells })
    }

    pub fn palette(&self) -> &[String] {
        &self.palette
    }

    pub fn cells(&self) -> &[u16] {
        &self.cells
    }

    /// Cell coordinates are 0..=3 in each axis.
    pub fn get(&self, x: u8, y: u8, z: u8) -> Option<&str> {
        if x >= 4 || y >= 4 || z >= 4 {
            return None;
        }
        let index = biome_cell_index(x, y, z);
        self.palette
            .get(*self.cells.get(index)? as usize)
            .map(String::as_str)
    }

    /// Converts the Java palette into the editable biome section. Decoding already
    /// validated every id, so a failure here means the caller bypassed the strict reader.
    pub fn to_domain(&self) -> Option<world_domain::BiomeSection> {
        let palette = self
            .palette
            .iter()
            .map(|biome| biome.parse::<world_domain::BiomeState>().ok())
            .collect::<Option<Vec<_>>>()?;
        world_domain::BiomeSection::from_palette(palette, self.cells.clone())
    }

    /// Converts an editable biome section back into the Java palette representation.
    pub fn from_domain(section: &world_domain::BiomeSection) -> Option<Self> {
        let palette = section
            .palette()
            .iter()
            .map(|biome| biome.as_str().to_owned())
            .collect::<Vec<_>>();
        Self::from_palette(palette, section.cells().to_vec())
    }

    /// Block coordinates are 0..=15 in each axis; the result is the covering biome cell.
    pub fn biome_at_block(&self, local_x: u8, local_y: u8, local_z: u8) -> Option<&str> {
        if local_x >= 16 || local_y >= 16 || local_z >= 16 {
            return None;
        }
        self.get(local_x / 4, local_y / 4, local_z / 4)
    }
}

fn biome_cell_index(x: u8, y: u8, z: u8) -> usize {
    ((y as usize * 4 + z as usize) * 4) + x as usize
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedSection {
    pub y: i32,
    pub blocks: Section,
    pub biomes: BiomeSection,
}

/// A decoded chunk. Raw NBT is still available through the originating `RegionChunk`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedChunk {
    pub x: i32,
    pub z: i32,
    pub data_version: i32,
    pub status: Option<String>,
    pub y_pos: i32,
    pub sections: BTreeMap<i32, DecodedSection>,
    pub missing_sections: Vec<i32>,
    pub block_entities: usize,
    pub entities: usize,
}

impl DecodedChunk {
    pub fn section(&self, y: i32) -> Option<&DecodedSection> {
        self.sections.get(&y)
    }

    /// Reads a block by local chunk coordinates. Missing sections read as air.
    pub fn block_at(&self, local_x: u8, y: i32, local_z: u8) -> Option<&BlockState> {
        let section_y = y.div_euclid(SECTION_SIZE);
        let local_y = y.rem_euclid(SECTION_SIZE) as u8;
        self.sections
            .get(&section_y)
            .and_then(|section| section.blocks.get(local_x, local_y, local_z))
    }

    /// Reads the biome covering a block by local chunk coordinates.
    pub fn biome_at(&self, local_x: u8, y: i32, local_z: u8) -> Option<&str> {
        let section_y = y.div_euclid(SECTION_SIZE);
        let local_y = y.rem_euclid(SECTION_SIZE) as u8;
        self.sections
            .get(&section_y)
            .and_then(|section| section.biomes.biome_at_block(local_x, local_y, local_z))
    }

    /// Converts the decoded sections into the editor's sparse chunk model, including the
    /// 4x4x4 biome cells that belong to each section.
    pub fn to_chunk(&self) -> Chunk {
        let mut chunk = Chunk::new(ChunkPos::new(self.x, self.z));
        for (y, section) in &self.sections {
            let mut block_section = section.blocks.clone();
            if let Some(biomes) = section.biomes.to_domain() {
                block_section.set_biomes(Some(biomes));
            }
            chunk.install_section(*y, Some(block_section));
        }
        chunk
    }
}

impl RegionChunk {
    /// Decodes block states and biomes for the given dimension.
    pub fn decode(&self, dimension: &JavaWorldDimension) -> Result<DecodedChunk, AnvilError> {
        decode_chunk(&self.nbt, self.x, self.z, dimension)
    }
}

impl JavaWorld {
    pub fn decode_chunk(
        &self,
        dimension_id: &str,
        chunk: &RegionChunk,
    ) -> Result<DecodedChunk, AnvilError> {
        let dimension = self
            .dimension(dimension_id)
            .ok_or_else(|| AnvilError::UnknownDimension(dimension_id.to_owned()))?;
        chunk.decode(dimension)
    }

    /// Decodes a chunk NBT that is not backed by a region file.
    ///
    /// This is used to verify a re-encoded chunk before it is published, so the same
    /// validation runs on freshly written data as on imported data.
    pub fn decode_chunk_nbt(
        &self,
        dimension_id: &str,
        x: i32,
        z: i32,
        nbt: &NbtFile,
    ) -> Result<DecodedChunk, AnvilError> {
        let dimension = self
            .dimension(dimension_id)
            .ok_or_else(|| AnvilError::UnknownDimension(dimension_id.to_owned()))?;
        decode_chunk(nbt, x, z, dimension)
    }
}

fn decode_chunk(
    nbt: &NbtFile,
    x: i32,
    z: i32,
    dimension: &JavaWorldDimension,
) -> Result<DecodedChunk, AnvilError> {
    let root = nbt
        .root
        .as_compound()
        .ok_or_else(|| invalid(x, z, "root tag is not a compound"))?;
    let data_version = chunk_int(root, "DataVersion", x, z)?;
    if data_version != world_domain::TARGET_DATA_VERSION {
        return Err(AnvilError::UnsupportedDataVersion {
            found: data_version,
            supported: world_domain::TARGET_DATA_VERSION,
        });
    }
    let stored_x = chunk_int(root, "xPos", x, z)?;
    let stored_z = chunk_int(root, "zPos", x, z)?;
    if stored_x != x || stored_z != z {
        return Err(invalid(
            x,
            z,
            format!("xPos/zPos {stored_x}/{stored_z} do not match the chunk coordinates"),
        ));
    }
    let y_pos = chunk_int(root, "yPos", x, z)?;
    let expected_y_pos = dimension.min_y.div_euclid(SECTION_SIZE);
    if y_pos != expected_y_pos {
        return Err(invalid(
            x,
            z,
            format!(
                "yPos {y_pos} does not match dimension `{}` starting section {expected_y_pos}",
                dimension.id
            ),
        ));
    }
    if dimension.height <= 0 || dimension.height % SECTION_SIZE != 0 {
        return Err(invalid(
            x,
            z,
            format!("dimension `{}` has an invalid height", dimension.id),
        ));
    }
    let section_count = dimension.height / SECTION_SIZE;
    let section_end = y_pos
        .checked_add(section_count)
        .ok_or_else(|| invalid(x, z, "section range overflow"))?;

    let status = match root.get("Status") {
        Some(NbtValue::String(value)) => Some(value.clone()),
        Some(_) => return Err(invalid(x, z, "Status is not a string")),
        None => None,
    };

    let sections_value = root
        .get("sections")
        .ok_or_else(|| invalid(x, z, "chunk has no sections list"))?;
    let (kind, items) = sections_value
        .as_list()
        .ok_or_else(|| invalid(x, z, "sections is not a list"))?;
    if !items.is_empty() && kind != TAG_COMPOUND {
        return Err(invalid(x, z, "sections is not a list of compounds"));
    }
    if items.len() > section_count as usize {
        return Err(invalid(
            x,
            z,
            format!(
                "chunk has {} sections, dimension `{}` has room for {section_count}",
                items.len(),
                dimension.id
            ),
        ));
    }

    let mut sections = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for item in items {
        let section = item
            .as_compound()
            .ok_or_else(|| invalid(x, z, "section is not a compound"))?;
        let y = match section.get("Y") {
            Some(NbtValue::Byte(value)) => *value as i32,
            Some(_) => return Err(invalid(x, z, "section Y is not a byte")),
            None => return Err(invalid(x, z, "section has no Y")),
        };
        if y < y_pos || y >= section_end {
            return Err(invalid(
                x,
                z,
                format!(
                    "section Y {y} is outside the chunk range {y_pos}..{}",
                    section_end - 1
                ),
            ));
        }
        if !seen.insert(y) {
            return Err(invalid(x, z, format!("duplicate section Y {y}")));
        }
        let blocks = decode_block_states(section, x, z)?;
        let biomes = decode_biomes(section, x, z)?;
        sections.insert(y, DecodedSection { y, blocks, biomes });
    }

    let missing_sections = (y_pos..section_end)
        .filter(|y| !sections.contains_key(y))
        .collect();

    Ok(DecodedChunk {
        x,
        z,
        data_version,
        status,
        y_pos,
        sections,
        missing_sections,
        block_entities: compound_list_len(root, "block_entities", x, z)?,
        entities: compound_list_len(root, "entities", x, z)?,
    })
}

fn compound_list_len(
    root: &BTreeMap<String, NbtValue>,
    key: &str,
    x: i32,
    z: i32,
) -> Result<usize, AnvilError> {
    match root.get(key) {
        None => Ok(0),
        Some(NbtValue::List { kind, items }) => {
            if *kind != TAG_COMPOUND && !items.is_empty() {
                return Err(invalid(x, z, format!("{key} is not a list of compounds")));
            }
            Ok(items.len())
        }
        Some(_) => Err(invalid(x, z, format!("{key} is not a list"))),
    }
}

fn decode_block_states(
    section: &BTreeMap<String, NbtValue>,
    x: i32,
    z: i32,
) -> Result<Section, AnvilError> {
    let Some(value) = section.get("block_states") else {
        return Ok(Section::empty());
    };
    let block_states = value
        .as_compound()
        .ok_or_else(|| invalid(x, z, "block_states is not a compound"))?;
    let palette_value = block_states
        .get("palette")
        .ok_or_else(|| invalid(x, z, "block_states has no palette"))?;
    let (kind, items) = palette_value
        .as_list()
        .ok_or_else(|| invalid(x, z, "block palette is not a list"))?;
    if items.is_empty() {
        return Err(invalid(x, z, "block palette is empty"));
    }
    if items.len() > MAX_BLOCK_PALETTE {
        return Err(invalid(
            x,
            z,
            format!(
                "block palette has {} entries, limit is {MAX_BLOCK_PALETTE}",
                items.len()
            ),
        ));
    }
    if kind != TAG_COMPOUND && kind != TAG_STRING {
        return Err(invalid(
            x,
            z,
            "block palette is not a list of compounds or strings",
        ));
    }
    let mut palette = Vec::with_capacity(items.len());
    for item in items {
        palette.push(decode_block_state(item, x, z)?);
    }

    let indices = if palette.len() == 1 {
        match block_states.get("data") {
            None => vec![0u16; BLOCK_SECTION_VOLUME],
            Some(NbtValue::LongArray(data)) if data.is_empty() => {
                vec![0u16; BLOCK_SECTION_VOLUME]
            }
            Some(NbtValue::LongArray(_)) => {
                return Err(invalid(
                    x,
                    z,
                    "single-entry block palette has a non-empty data array",
                ))
            }
            Some(_) => return Err(invalid(x, z, "block data is not a long array")),
        }
    } else {
        let data = match block_states.get("data") {
            Some(NbtValue::LongArray(data)) => data,
            Some(_) => return Err(invalid(x, z, "block data is not a long array")),
            None => return Err(invalid(x, z, "block data is missing")),
        };
        let bits = bits_for_palette(palette.len(), 4);
        let values_per_long = 64 / bits as usize;
        let expected_words = BLOCK_SECTION_VOLUME.div_ceil(values_per_long);
        if data.len() != expected_words {
            return Err(invalid(
                x,
                z,
                format!(
                    "block data has {} longs, expected {expected_words} for {bits} bits",
                    data.len()
                ),
            ));
        }
        unpack_indices(
            data,
            bits,
            values_per_long,
            BLOCK_SECTION_VOLUME,
            palette.len(),
            x,
            z,
        )?
    };

    Section::from_palette_indices(palette, &indices)
        .ok_or_else(|| invalid(x, z, "block palette indices do not form a valid section"))
}

fn decode_block_state(value: &NbtValue, x: i32, z: i32) -> Result<BlockState, AnvilError> {
    match value {
        NbtValue::String(name) => BlockState::simple(name.clone())
            .map_err(|error| invalid(x, z, format!("invalid block state `{name}`: {error}"))),
        NbtValue::Compound(map) => {
            let name = map
                .get("Name")
                .or_else(|| map.get("id"))
                .and_then(NbtValue::as_string)
                .ok_or_else(|| invalid(x, z, "block palette entry has no Name"))?;
            let properties_value = map.get("Properties").or_else(|| map.get("properties"));
            let mut properties = BTreeMap::new();
            if let Some(value) = properties_value {
                let properties_map = value
                    .as_compound()
                    .ok_or_else(|| invalid(x, z, "block Properties is not a compound"))?;
                for (key, value) in properties_map {
                    let value = value.as_string().ok_or_else(|| {
                        invalid(x, z, format!("block property `{key}` is not a string"))
                    })?;
                    properties.insert(key.clone(), value.to_owned());
                }
            }
            BlockState::new(name.to_owned(), properties)
                .map_err(|error| invalid(x, z, format!("invalid block state `{name}`: {error}")))
        }
        _ => Err(invalid(
            x,
            z,
            "block palette entry is not a compound or string",
        )),
    }
}

fn decode_biomes(
    section: &BTreeMap<String, NbtValue>,
    x: i32,
    z: i32,
) -> Result<BiomeSection, AnvilError> {
    let value = section
        .get("biomes")
        .ok_or_else(|| invalid(x, z, "section has no biomes compound"))?;
    let biomes = value
        .as_compound()
        .ok_or_else(|| invalid(x, z, "biomes is not a compound"))?;
    let palette_value = biomes
        .get("palette")
        .ok_or_else(|| invalid(x, z, "biomes has no palette"))?;
    let (kind, items) = palette_value
        .as_list()
        .ok_or_else(|| invalid(x, z, "biome palette is not a list"))?;
    if items.is_empty() {
        return Err(invalid(x, z, "biome palette is empty"));
    }
    if items.len() > MAX_BIOME_PALETTE {
        return Err(invalid(
            x,
            z,
            format!(
                "biome palette has {} entries, limit is {MAX_BIOME_PALETTE}",
                items.len()
            ),
        ));
    }
    if kind != TAG_STRING {
        return Err(invalid(x, z, "biome palette is not a list of strings"));
    }
    let mut palette = Vec::with_capacity(items.len());
    for item in items {
        let name = item
            .as_string()
            .ok_or_else(|| invalid(x, z, "biome palette entry is not a string"))?;
        if !valid_resource_location(name) {
            return Err(invalid(x, z, format!("invalid biome id `{name}`")));
        }
        palette.push(name.to_owned());
    }

    let cells = if palette.len() == 1 {
        match biomes.get("data") {
            None => vec![0u16; BIOME_SECTION_VOLUME],
            Some(NbtValue::LongArray(data)) if data.is_empty() => {
                vec![0u16; BIOME_SECTION_VOLUME]
            }
            Some(NbtValue::LongArray(_)) => {
                return Err(invalid(
                    x,
                    z,
                    "single-entry biome palette has a non-empty data array",
                ))
            }
            Some(_) => return Err(invalid(x, z, "biome data is not a long array")),
        }
    } else {
        let data = match biomes.get("data") {
            Some(NbtValue::LongArray(data)) => data,
            Some(_) => return Err(invalid(x, z, "biome data is not a long array")),
            None => return Err(invalid(x, z, "biome data is missing")),
        };
        let bits = bits_for_palette(palette.len(), 0);
        let values_per_long = 64 / bits as usize;
        let expected_words = BIOME_SECTION_VOLUME.div_ceil(values_per_long);
        if data.len() != expected_words {
            return Err(invalid(
                x,
                z,
                format!(
                    "biome data has {} longs, expected {expected_words} for {bits} bits",
                    data.len()
                ),
            ));
        }
        unpack_indices(
            data,
            bits,
            values_per_long,
            BIOME_SECTION_VOLUME,
            palette.len(),
            x,
            z,
        )?
    };

    BiomeSection::from_palette(palette, cells)
        .ok_or_else(|| invalid(x, z, "biome palette cells are invalid"))
}

fn bits_for_palette(palette_len: usize, minimum: u8) -> u8 {
    debug_assert!(palette_len >= 2);
    let bits = (usize::BITS - (palette_len - 1).leading_zeros()) as u8;
    bits.max(minimum)
}

fn unpack_indices(
    data: &[i64],
    bits: u8,
    values_per_long: usize,
    count: usize,
    palette_len: usize,
    x: i32,
    z: i32,
) -> Result<Vec<u16>, AnvilError> {
    let mask = (1u64 << bits) - 1;
    let mut indices = Vec::with_capacity(count);
    for index in 0..count {
        let word = index / values_per_long;
        let shift = ((index % values_per_long) * bits as usize) as u32;
        let value = (((data[word] as u64) >> shift) & mask) as usize;
        if value >= palette_len {
            return Err(invalid(
                x,
                z,
                format!("palette index {value} at {index} is outside a palette of {palette_len}"),
            ));
        }
        indices.push(value as u16);
    }
    Ok(indices)
}

fn chunk_int(
    root: &BTreeMap<String, NbtValue>,
    key: &str,
    x: i32,
    z: i32,
) -> Result<i32, AnvilError> {
    match root.get(key) {
        Some(NbtValue::Int(value)) => Ok(*value),
        Some(_) => Err(invalid(x, z, format!("{key} is not an int"))),
        None => Err(invalid(x, z, format!("{key} is missing"))),
    }
}

fn valid_resource_location(value: &str) -> bool {
    let Some((namespace, path)) = value.split_once(':') else {
        return false;
    };
    [namespace, path].iter().all(|part| {
        !part.is_empty()
            && part.bytes().all(|b| {
                b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'.' || b == b'-'
            })
    })
}

fn invalid(x: i32, z: i32, message: impl Into<String>) -> AnvilError {
    AnvilError::InvalidChunk {
        x,
        z,
        message: message.into(),
    }
}
