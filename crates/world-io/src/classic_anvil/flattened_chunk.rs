//! Chunk decoder for the flattened classic Anvil releases (1.13–1.17).
//!
//! These chunks keep the pre-1.18 `Level` wrapper and store block palettes
//! directly on each section (`Palette`/`BlockStates`). Block indices are one
//! continuous bit stream through 1.15 and become padded per 64-bit word in
//! 1.16. Biomes live in the chunk-wide `Level.Biomes` array and are mapped to
//! the normalized 4×4×4 section model by the existing flattened converter.
//!
//! Decoding is intentionally separate from save admission. The reader,
//! project creator and exporter still refuse these releases until the full
//! P2 adapter verifies them.

use std::collections::{BTreeMap, BTreeSet};

use minecraft_version::{BiomeFormat, ChunkFamily, VersionFamily, VersionProfile, WorldLayout};
use world_domain::{Section, SECTION_SIZE};

use crate::nbt::{NbtValue, TAG_COMPOUND};

use super::chunk::{
    compound_list_len, decode_block_section, invalid, BiomeSection, DecodedChunk, DecodedSection,
    BLOCK_SECTION_VOLUME,
};
use super::flattened_biomes::{
    decode_flattened_chunk_biomes, ColumnBiomePolicy, FlattenedBiomeError,
};
use super::format::LongPacking;
use super::reader::{AnvilError, JavaWorldDimension};

/// 1.16 changed `BlockStates` from a continuous bit stream to padded words.
/// The exact release boundary is DataVersion 2566 (1.16).
const PADDED_BLOCK_STATES_SINCE_DATA_VERSION: i32 = 2566;
const BIOME_SECTIONS: usize = 16;
const LIGHT_BYTES: usize = 2048;

pub(super) fn decode_chunk(
    nbt: &crate::nbt::NbtFile,
    x: i32,
    z: i32,
    dimension: &JavaWorldDimension,
    profile: &VersionProfile,
) -> Result<DecodedChunk, AnvilError> {
    validate_profile(profile)?;
    let section_range = flattened_section_range(dimension)
        .ok_or_else(|| invalid(x, z, "flattened chunk dimensions must cover Y 0..255"))?;
    let light_range = section_range.start - 1..section_range.end + 1;

    let root = nbt
        .root
        .as_compound()
        .ok_or_else(|| invalid(x, z, "root tag is not a compound"))?;
    let data_version = super::chunk::chunk_int(root, "DataVersion", x, z)?;
    if data_version != profile.data_version {
        return Err(AnvilError::UnsupportedDataVersion {
            found: data_version,
            supported: vec![profile.data_version],
        });
    }
    let level = root
        .get("Level")
        .and_then(NbtValue::as_compound)
        .ok_or_else(|| invalid(x, z, "flattened chunk has no Level compound"))?;
    let stored_x = super::chunk::chunk_int(level, "xPos", x, z)?;
    let stored_z = super::chunk::chunk_int(level, "zPos", x, z)?;
    if stored_x != x || stored_z != z {
        return Err(invalid(
            x,
            z,
            format!("xPos/zPos {stored_x}/{stored_z} do not match the chunk coordinates"),
        ));
    }
    let status = match level.get("Status") {
        Some(NbtValue::String(value)) => Some(value.clone()),
        Some(_) => return Err(invalid(x, z, "Level.Status is not a string")),
        None => None,
    };

    let projection = decode_flattened_chunk_biomes(nbt, profile, ColumnBiomePolicy::RequireUniform)
        .map_err(|error| biome_error(x, z, error))?;
    debug_assert_eq!(
        projection.mixed_column_cells, 0,
        "RequireUniform cannot project mixed columns"
    );
    let mut biome_sections = projection
        .sections
        .into_iter()
        .enumerate()
        .map(|(y, section)| (y as i32, section))
        .collect::<BTreeMap<_, _>>();

    let sections_value = level
        .get("Sections")
        .ok_or_else(|| invalid(x, z, "flattened chunk has no Sections list"))?;
    let (kind, items) = sections_value
        .as_list()
        .ok_or_else(|| invalid(x, z, "Sections is not a list"))?;
    if !items.is_empty() && kind != TAG_COMPOUND {
        return Err(invalid(x, z, "Sections is not a list of compounds"));
    }
    if items.len() > light_range.len() {
        return Err(invalid(
            x,
            z,
            format!(
                "chunk has {} sections, a 256-block dimension has room for {} including light-only sections",
                items.len(),
                light_range.len()
            ),
        ));
    }

    let packing = if data_version >= PADDED_BLOCK_STATES_SINCE_DATA_VERSION {
        LongPacking::Padded
    } else {
        LongPacking::Unpadded
    };
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
        if !seen.insert(y) {
            return Err(invalid(x, z, format!("duplicate section Y {y}")));
        }
        if !section_range.contains(&y) {
            if light_range.contains(&y)
                && !section.contains_key("Palette")
                && !section.contains_key("BlockStates")
            {
                for key in ["SkyLight", "BlockLight"] {
                    if let Some(value) = section.get(key) {
                        if !matches!(value, NbtValue::ByteArray(bytes) if bytes.len() == LIGHT_BYTES)
                        {
                            return Err(invalid(
                                x,
                                z,
                                format!("light-only section {y} {key} is not a {LIGHT_BYTES}-byte array"),
                            ));
                        }
                    }
                }
                continue;
            }
            return Err(invalid(
                x,
                z,
                format!(
                    "section Y {y} is outside the chunk range {}..{}",
                    section_range.start,
                    section_range.end - 1
                ),
            ));
        }

        let blocks = if section.contains_key("Palette") || section.contains_key("BlockStates") {
            decode_block_section(
                section,
                "Palette",
                "BlockStates",
                BLOCK_SECTION_VOLUME,
                4,
                packing,
                x,
                z,
            )?
        } else {
            Section::empty()
        };
        let biomes = biome_sections
            .remove(&y)
            .ok_or_else(|| invalid(x, z, format!("section Y {y} has no biome data")))?;
        let biomes = BiomeSection::from_domain(&biomes).ok_or_else(|| {
            invalid(
                x,
                z,
                format!("section Y {y} biome data is not representable"),
            )
        })?;
        sections.insert(y, DecodedSection { y, blocks, biomes });
    }

    let missing_sections = section_range
        .filter(|y| !sections.contains_key(y))
        .collect();

    Ok(DecodedChunk {
        x,
        z,
        data_version,
        status,
        y_pos: 0,
        sections,
        missing_sections,
        block_entities: compound_list_len(level, "TileEntities", x, z)?,
        entities: compound_list_len(level, "Entities", x, z)?,
    })
}

fn validate_profile(profile: &VersionProfile) -> Result<(), AnvilError> {
    if profile.family != VersionFamily::Flattened
        || profile.world_layout != WorldLayout::ClassicAnvil
        || profile.chunk_family != ChunkFamily::SectionBiomes
        || !matches!(
            profile.biome_format,
            BiomeFormat::Column | BiomeFormat::Section
        )
    {
        return Err(AnvilError::UnsupportedLayout(format!(
            "profile `{}` is not a flattened classic Anvil chunk profile",
            profile.id
        )));
    }
    Ok(())
}

fn biome_error(x: i32, z: i32, error: FlattenedBiomeError) -> AnvilError {
    invalid(x, z, format!("flattened biomes: {error}"))
}

fn flattened_section_range(dimension: &JavaWorldDimension) -> Option<std::ops::Range<i32>> {
    if dimension.min_y != 0 || dimension.height != BIOME_SECTIONS as i32 * SECTION_SIZE {
        return None;
    }
    Some(0..BIOME_SECTIONS as i32)
}
