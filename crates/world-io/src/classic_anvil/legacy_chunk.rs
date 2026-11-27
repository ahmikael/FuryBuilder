//! Read-only decoding for the pre-1.8 Java chunk layout (1.0–1.7.10).
//!
//! Both McRegion (`1.0`–`1.1`, `.mcr`, Y 0–127) and early classic Anvil
//! (`1.2.1`–`1.7.10`, `.mca`, Y 0–255) store one flat array under `Level`
//! instead of sections:
//!
//! - `Level.Blocks` is one block id per position in XZY order;
//! - `Level.Data` is one metadata nibble per position in the same order;
//! - `Level.Biomes` is one biome id per block column.
//!
//! Numeric ids are resolved through the shared 1.12.2 registry, whose existing
//! entries are stable across the pre-1.8 releases. Unknown ids and malformed
//! arrays are rejected instead of being replaced with air. This module has no
//! encoder: pre-1.8 projects are read-only/passthrough, not native editing.

use std::collections::{BTreeMap, HashMap};

use minecraft_version::{LegacyBiomeRegistry, LegacyRegistry, VersionProfile};
use world_domain::{Section, SECTION_SIZE};

use crate::nbt::{NbtFile, NbtValue};

use super::chunk::{
    compound_list_len, invalid, DecodedChunk, DecodedSection, BLOCK_SECTION_VOLUME,
};
use super::format::LegacyFormat;
use super::numeric_chunk::{decode_column_biomes, legacy_state, nibble};
use super::reader::{AnvilError, JavaWorldDimension};

pub(super) fn decode_chunk(
    nbt: &NbtFile,
    x: i32,
    z: i32,
    dimension: &JavaWorldDimension,
    profile: &VersionProfile,
) -> Result<DecodedChunk, AnvilError> {
    let format = LegacyFormat::for_profile(profile)?;
    let section_range = legacy_section_range(dimension, format.height)
        .ok_or_else(|| invalid(x, z, "legacy chunks must cover Y 0..128 or Y 0..256"))?;
    let registry = LegacyRegistry::for_legacy(&profile.minecraft_version).ok_or_else(|| {
        AnvilError::UnsupportedLayout(format!(
            "no legacy numeric block registry for profile `{}`",
            profile.id
        ))
    })?;
    let biome_registry =
        LegacyBiomeRegistry::for_release(&profile.minecraft_version).ok_or_else(|| {
            AnvilError::UnsupportedLayout(format!(
                "no legacy biome registry for profile `{}`",
                profile.id
            ))
        })?;

    let root = nbt
        .root
        .as_compound()
        .ok_or_else(|| invalid(x, z, "root tag is not a compound"))?;
    if root.contains_key("DataVersion") {
        return Err(invalid(
            x,
            z,
            "legacy chunk carries a DataVersion tag; this is not a pre-1.8 chunk",
        ));
    }
    let level = root
        .get("Level")
        .and_then(NbtValue::as_compound)
        .ok_or_else(|| invalid(x, z, "legacy chunk has no Level compound"))?;
    let stored_x = super::chunk::chunk_int(level, "xPos", x, z)?;
    let stored_z = super::chunk::chunk_int(level, "zPos", x, z)?;
    if stored_x != x || stored_z != z {
        return Err(invalid(
            x,
            z,
            format!("xPos/zPos {stored_x}/{stored_z} do not match the chunk coordinates"),
        ));
    }
    let status = legacy_status(level, x, z)?;

    let blocks = byte_array(level, "Blocks", format.blocks_len(), x, z)?;
    let data = byte_array(level, "Data", format.data_len(), x, z)?;
    let (biomes, mixed_biome_columns) = decode_column_biomes(level, biome_registry, x, z)?;

    let mut sections = BTreeMap::new();
    for section_y in section_range {
        let mut palette = Vec::new();
        let mut palette_indices: HashMap<world_domain::BlockState, u16> = HashMap::new();
        let mut indices = vec![0u16; BLOCK_SECTION_VOLUME];
        for local_y in 0..SECTION_SIZE {
            let y = section_y * SECTION_SIZE + local_y;
            for local_z in 0..SECTION_SIZE {
                for local_x in 0..SECTION_SIZE {
                    let legacy_index = ((local_x as usize) * 16 + local_z as usize)
                        * format.height as usize
                        + y as usize;
                    let id = u16::from(blocks[legacy_index]);
                    let metadata = nibble(Some(data), legacy_index);
                    let legacy = registry.block(id, metadata).ok_or_else(|| {
                        invalid(
                            x,
                            z,
                            format!(
                                "unknown numeric block id {id}:{metadata} at local ({local_x}, {y}, {local_z})"
                            ),
                        )
                    })?;
                    let state = legacy_state(legacy, x, z)?;
                    let palette_index = match palette_indices.get(&state) {
                        Some(index) => *index,
                        None => {
                            let index = u16::try_from(palette.len())
                                .map_err(|_| invalid(x, z, "legacy palette index overflow"))?;
                            palette.push(state.clone());
                            palette_indices.insert(state, index);
                            index
                        }
                    };
                    let canonical =
                        ((local_y as usize * 16 + local_z as usize) * 16) + local_x as usize;
                    indices[canonical] = palette_index;
                }
            }
        }
        let section = Section::from_palette_indices(palette, &indices)
            .ok_or_else(|| invalid(x, z, "legacy block ids do not form a valid section"))?;
        sections.insert(
            section_y,
            DecodedSection {
                y: section_y,
                blocks: section,
                biomes: biomes.clone(),
            },
        );
    }

    Ok(DecodedChunk {
        x,
        z,
        data_version: profile.data_version,
        status,
        y_pos: 0,
        sections,
        missing_sections: Vec::new(),
        block_entities: compound_list_len(level, "TileEntities", x, z)?,
        entities: compound_list_len(level, "Entities", x, z)?,
        mixed_biome_columns,
    })
}

fn legacy_section_range(
    dimension: &JavaWorldDimension,
    height: i32,
) -> Option<std::ops::Range<i32>> {
    if dimension.min_y != 0 || dimension.height != height || height % SECTION_SIZE != 0 {
        return None;
    }
    Some(0..height / SECTION_SIZE)
}

pub(super) fn legacy_status(
    level: &BTreeMap<String, NbtValue>,
    x: i32,
    z: i32,
) -> Result<Option<String>, AnvilError> {
    match level.get("TerrainPopulated") {
        Some(NbtValue::Byte(value)) => Ok(Some(if *value != 0 {
            "full".to_owned()
        } else {
            "partial".to_owned()
        })),
        Some(_) => Err(invalid(x, z, "Level.TerrainPopulated is not a byte")),
        None => Ok(None),
    }
}

pub(super) fn byte_array<'a>(
    level: &'a BTreeMap<String, NbtValue>,
    key: &str,
    expected: usize,
    x: i32,
    z: i32,
) -> Result<&'a [u8], AnvilError> {
    match level.get(key) {
        Some(NbtValue::ByteArray(bytes)) if bytes.len() == expected => Ok(bytes),
        Some(NbtValue::ByteArray(bytes)) => Err(invalid(
            x,
            z,
            format!("Level.{key} has {} bytes, expected {expected}", bytes.len()),
        )),
        Some(_) => Err(invalid(x, z, format!("Level.{key} is not a byte array"))),
        None => Err(invalid(
            x,
            z,
            format!("legacy chunk has no Level.{key} array"),
        )),
    }
}
