//! Decoding and encoding for the pre-flattening numeric classic Anvil releases
//! (1.8.9–1.12.2).
//!
//! These chunks keep the pre-1.18 `Level` wrapper, but each section stores raw
//! block ids and metadata instead of a palette:
//!
//! - `Level.Sections[].Blocks` is 4096 bytes, one block id per position;
//! - `Level.Sections[].Data` is 2048 bytes of metadata nibbles;
//! - an optional `Add` nibble array supplies the high four id bits;
//! - `Level.Biomes` is one byte per block column, not a 4×4×4 palette.
//!
//! The shared numeric registry resolves id+metadata to flattened names. The
//! reader, import, overlay, region writer and same-version export all use this
//! module for admitted numeric profiles.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use minecraft_version::{LegacyBiomeRegistry, LegacyBlockState, LegacyRegistry, VersionProfile};
use world_domain::{
    BiomeSection as DomainBiomeSection, BiomeState, BlockState, Section, SECTION_SIZE,
};

use crate::nbt::{NbtFile, NbtValue, TAG_COMPOUND};

use super::chunk::{
    compound_list_len, invalid, BiomeSection, DecodedChunk, DecodedSection, BLOCK_SECTION_VOLUME,
};
use super::format::NumericFormat;
use super::overlay::OverlayError;
use super::reader::{AnvilError, JavaWorldDimension};

const SECTIONS: usize = 16;
const BLOCK_BYTES: usize = BLOCK_SECTION_VOLUME;
const DATA_BYTES: usize = BLOCK_SECTION_VOLUME / 2;
const BIOME_COLUMNS: usize = 16 * 16;

pub(super) fn decode_chunk(
    nbt: &NbtFile,
    x: i32,
    z: i32,
    dimension: &JavaWorldDimension,
    profile: &VersionProfile,
) -> Result<DecodedChunk, AnvilError> {
    NumericFormat::for_profile(profile)?;
    let registry = LegacyRegistry::for_release(&profile.minecraft_version).ok_or_else(|| {
        AnvilError::UnsupportedLayout(format!(
            "no numeric block registry for profile `{}`",
            profile.id
        ))
    })?;
    let biome_registry =
        LegacyBiomeRegistry::for_release(&profile.minecraft_version).ok_or_else(|| {
            AnvilError::UnsupportedLayout(format!(
                "no numeric biome registry for profile `{}`",
                profile.id
            ))
        })?;
    let section_range = numeric_section_range(dimension)
        .ok_or_else(|| invalid(x, z, "numeric chunk dimensions must cover Y 0..255"))?;

    let root = nbt
        .root
        .as_compound()
        .ok_or_else(|| invalid(x, z, "root tag is not a compound"))?;
    // Chunks saved before 1.9 have no root DataVersion. The level.dat profile
    // is the exact release; a present tag must still agree with it.
    let data_version = match root.get("DataVersion") {
        Some(_) => {
            let found = super::chunk::chunk_int(root, "DataVersion", x, z)?;
            if found != profile.data_version {
                return Err(AnvilError::UnsupportedDataVersion {
                    found,
                    supported: vec![profile.data_version],
                });
            }
            found
        }
        None => profile.data_version,
    };
    let level = root
        .get("Level")
        .and_then(NbtValue::as_compound)
        .ok_or_else(|| invalid(x, z, "numeric chunk has no Level compound"))?;
    let stored_x = super::chunk::chunk_int(level, "xPos", x, z)?;
    let stored_z = super::chunk::chunk_int(level, "zPos", x, z)?;
    if stored_x != x || stored_z != z {
        return Err(invalid(
            x,
            z,
            format!("xPos/zPos {stored_x}/{stored_z} do not match the chunk coordinates"),
        ));
    }
    let status = match level.get("TerrainPopulated") {
        Some(NbtValue::Byte(value)) => Some(if *value != 0 {
            "full".to_owned()
        } else {
            "partial".to_owned()
        }),
        Some(_) => return Err(invalid(x, z, "Level.TerrainPopulated is not a byte")),
        None => None,
    };

    let (biomes, mixed_biome_columns) = decode_column_biomes(level, biome_registry, x, z)?;

    let sections_value = level
        .get("Sections")
        .ok_or_else(|| invalid(x, z, "numeric chunk has no Sections list"))?;
    let (kind, items) = sections_value
        .as_list()
        .ok_or_else(|| invalid(x, z, "Sections is not a list"))?;
    if !items.is_empty() && kind != TAG_COMPOUND {
        return Err(invalid(x, z, "Sections is not a list of compounds"));
    }
    if items.len() > SECTIONS {
        return Err(invalid(
            x,
            z,
            format!(
                "chunk has {} sections, a 256-block dimension has room for {SECTIONS}",
                items.len()
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
        if !seen.insert(y) {
            return Err(invalid(x, z, format!("duplicate section Y {y}")));
        }
        if !section_range.contains(&y) {
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
        let blocks = decode_block_section(section, registry, x, z)?;
        sections.insert(
            y,
            DecodedSection {
                y,
                blocks,
                biomes: biomes.clone(),
            },
        );
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
        mixed_biome_columns,
    })
}

fn numeric_section_range(dimension: &JavaWorldDimension) -> Option<std::ops::Range<i32>> {
    if dimension.min_y != 0 || dimension.height != SECTIONS as i32 * SECTION_SIZE {
        return None;
    }
    Some(0..SECTIONS as i32)
}

fn decode_block_section(
    section: &BTreeMap<String, NbtValue>,
    registry: &LegacyRegistry,
    x: i32,
    z: i32,
) -> Result<Section, AnvilError> {
    let Some(blocks_value) = section.get("Blocks") else {
        if section.contains_key("Data") || section.contains_key("Add") {
            return Err(invalid(
                x,
                z,
                "section has block metadata without a Blocks array",
            ));
        }
        return Ok(Section::empty());
    };
    let blocks = match blocks_value {
        NbtValue::ByteArray(bytes) if bytes.len() == BLOCK_BYTES => bytes,
        NbtValue::ByteArray(bytes) => {
            return Err(invalid(
                x,
                z,
                format!(
                    "section Blocks has {} bytes, expected {BLOCK_BYTES}",
                    bytes.len()
                ),
            ))
        }
        _ => return Err(invalid(x, z, "section Blocks is not a byte array")),
    };
    let data = match section.get("Data") {
        Some(NbtValue::ByteArray(bytes)) if bytes.len() == DATA_BYTES => bytes,
        Some(NbtValue::ByteArray(bytes)) => {
            return Err(invalid(
                x,
                z,
                format!(
                    "section Data has {} bytes, expected {DATA_BYTES}",
                    bytes.len()
                ),
            ))
        }
        Some(_) => return Err(invalid(x, z, "section Data is not a byte array")),
        None => return Err(invalid(x, z, "section has no Data array")),
    };
    let add = match section.get("Add") {
        None => None,
        Some(NbtValue::ByteArray(bytes)) if bytes.len() == DATA_BYTES => Some(bytes),
        Some(NbtValue::ByteArray(bytes)) => {
            return Err(invalid(
                x,
                z,
                format!(
                    "section Add has {} bytes, expected {DATA_BYTES}",
                    bytes.len()
                ),
            ))
        }
        Some(_) => return Err(invalid(x, z, "section Add is not a byte array")),
    };

    let mut palette = Vec::new();
    let mut palette_indices: HashMap<BlockState, u16> = HashMap::new();
    let mut indices = Vec::with_capacity(BLOCK_SECTION_VOLUME);
    for (index, id_byte) in blocks.iter().enumerate() {
        let id = u16::from(*id_byte) | (nibble(add.map(Vec::as_slice), index) << 8);
        let metadata = nibble(Some(data.as_slice()), index);
        let legacy = registry.block(id, metadata).ok_or_else(|| {
            let local_x = index % 16;
            let local_z = (index / 16) % 16;
            let local_y = index / 256;
            invalid(
                x,
                z,
                format!(
                    "unknown numeric block id {id}:{metadata} at section Y {} local ({local_x}, {local_y}, {local_z})",
                    section.get("Y").and_then(|value| match value {
                        NbtValue::Byte(value) => Some(*value as i32),
                        _ => None,
                    }).unwrap_or_default()
                ),
            )
        })?;
        let state = legacy_state(legacy, x, z)?;
        let palette_index = match palette_indices.get(&state) {
            Some(index) => *index,
            None => {
                let index = u16::try_from(palette.len())
                    .map_err(|_| invalid(x, z, "numeric palette index overflow"))?;
                palette.push(state.clone());
                palette_indices.insert(state, index);
                index
            }
        };
        indices.push(palette_index);
    }
    Section::from_palette_indices(palette, &indices)
        .ok_or_else(|| invalid(x, z, "numeric block ids do not form a valid section"))
}

fn decode_column_biomes(
    level: &BTreeMap<String, NbtValue>,
    registry: LegacyBiomeRegistry,
    x: i32,
    z: i32,
) -> Result<(BiomeSection, usize), AnvilError> {
    let bytes = match level.get("Biomes") {
        Some(NbtValue::ByteArray(bytes)) if bytes.len() == BIOME_COLUMNS => bytes,
        Some(NbtValue::ByteArray(bytes)) => {
            return Err(invalid(
                x,
                z,
                format!(
                    "Level.Biomes has {} bytes, expected {BIOME_COLUMNS}",
                    bytes.len()
                ),
            ))
        }
        Some(_) => return Err(invalid(x, z, "Level.Biomes is not a byte array")),
        None => return Err(invalid(x, z, "numeric chunk has no Level.Biomes array")),
    };
    let mut names = Vec::with_capacity(BIOME_COLUMNS);
    for (index, byte) in bytes.iter().enumerate() {
        let id = i32::from(*byte);
        let name = registry.name(id).ok_or_else(|| {
            invalid(
                x,
                z,
                format!("unknown numeric biome ID {id} at Level.Biomes[{index}]"),
            )
        })?;
        names.push(name);
    }
    let (palette, cells, mixed) = sample_columns(&names);
    let biomes = BiomeSection::from_palette(palette, cells)
        .ok_or_else(|| invalid(x, z, "numeric biome columns are not representable"))?;
    Ok((biomes, mixed))
}

fn sample_columns(names: &[&'static str]) -> (Vec<String>, Vec<u16>, usize) {
    let mut palette = Vec::new();
    let mut cells = vec![0u16; 64];
    let mut mixed = 0usize;
    for cell_z in 0..4usize {
        for cell_x in 0..4usize {
            let center = names[(cell_z * 4 + 2) * 16 + (cell_x * 4 + 2)];
            let mut uniform = true;
            'footprint: for column_z in cell_z * 4..cell_z * 4 + 4 {
                for column_x in cell_x * 4..cell_x * 4 + 4 {
                    if names[column_z * 16 + column_x] != center {
                        uniform = false;
                        break 'footprint;
                    }
                }
            }
            if !uniform {
                mixed += 1;
            }
            let palette_index = match palette.iter().position(|entry| entry == center) {
                Some(index) => index,
                None => {
                    palette.push(center.to_owned());
                    palette.len() - 1
                }
            };
            for cell_y in 0..4usize {
                cells[(cell_y * 4 + cell_z) * 4 + cell_x] = palette_index as u16;
            }
        }
    }
    (palette, cells, mixed)
}

fn legacy_state(legacy: &LegacyBlockState, x: i32, z: i32) -> Result<BlockState, AnvilError> {
    BlockState::new(legacy.name.clone(), legacy.properties.clone()).map_err(|error| {
        invalid(
            x,
            z,
            format!(
                "numeric block {}:{} maps to invalid state `{}`: {error}",
                legacy.id,
                legacy.metadata,
                legacy.state_string()
            ),
        )
    })
}

/// Encodes overlay edits into a numeric chunk while preserving every field
/// outside `Level.Sections[].Blocks/Data/Add` and `Level.Biomes`.
pub(super) fn encode_chunk(
    source: &NbtFile,
    dimension: &JavaWorldDimension,
    profile: &VersionProfile,
    source_decoded: &DecodedChunk,
    sections: &BTreeMap<i32, Section>,
    biomes: &BTreeMap<i32, DomainBiomeSection>,
) -> Result<NbtFile, OverlayError> {
    let (x, z) = (source_decoded.x, source_decoded.z);
    NumericFormat::for_profile(profile).map_err(OverlayError::Anvil)?;
    let registry = LegacyRegistry::for_release(&profile.minecraft_version).ok_or_else(|| {
        OverlayError::Anvil(AnvilError::UnsupportedLayout(format!(
            "no numeric block registry for profile `{}`",
            profile.id
        )))
    })?;
    let biome_registry =
        LegacyBiomeRegistry::for_release(&profile.minecraft_version).ok_or_else(|| {
            OverlayError::Anvil(AnvilError::UnsupportedLayout(format!(
                "no numeric biome registry for profile `{}`",
                profile.id
            )))
        })?;
    let mut patched = source.clone();
    patch_block_sections(&mut patched, x, z, source_decoded, sections, registry)?;
    let expected_biomes = if biomes.is_empty() {
        None
    } else {
        Some(patch_biomes(
            &mut patched,
            biome_registry,
            &source_decoded.sections,
            biomes,
        )?)
    };

    verify_numeric_preserved_fields(source, &patched)?;

    let decoded = decode_chunk(&patched, x, z, dimension, profile).map_err(OverlayError::Anvil)?;
    for (section_y, section) in sections {
        let decoded_section = decoded.sections.get(section_y).ok_or_else(|| {
            OverlayError::VerificationFailed(format!(
                "re-decoded numeric chunk lost section {section_y}"
            ))
        })?;
        for index in 0..BLOCK_SECTION_VOLUME {
            let local_x = (index % 16) as u8;
            let local_z = ((index / 16) % 16) as u8;
            let local_y = (index / 256) as u8;
            let expected = section.get(local_x, local_y, local_z).ok_or_else(|| {
                OverlayError::VerificationFailed(format!(
                    "edited section {section_y} is missing local block ({local_x}, {local_y}, {local_z})"
                ))
            })?;
            let actual = decoded_section
                .blocks
                .get(local_x, local_y, local_z)
                .ok_or_else(|| {
                    OverlayError::VerificationFailed(format!(
                        "re-decoded section {section_y} is missing local block ({local_x}, {local_y}, {local_z})"
                    ))
                })?;
            if !same_legacy_state(expected, actual) {
                return Err(OverlayError::VerificationFailed(format!(
                    "section {section_y} block ({local_x}, {local_y}, {local_z}) did not round-trip: expected `{expected}`, decoded `{actual}`"
                )));
            }
        }
    }
    for (section_y, source_section) in &source_decoded.sections {
        if sections.contains_key(section_y) {
            continue;
        }
        let decoded_section = decoded.sections.get(section_y).ok_or_else(|| {
            OverlayError::VerificationFailed(format!(
                "re-decoded numeric chunk lost untouched section {section_y}"
            ))
        })?;
        if decoded_section.blocks != source_section.blocks {
            return Err(OverlayError::VerificationFailed(format!(
                "untouched numeric section {section_y} changed while editing the chunk"
            )));
        }
    }
    if let Some(expected) = expected_biomes {
        for (section_y, decoded_section) in &decoded.sections {
            let decoded_biomes = decoded_section.biomes.to_domain().ok_or_else(|| {
                OverlayError::VerificationFailed(format!(
                    "re-decoded numeric biome section {section_y} cannot be represented"
                ))
            })?;
            if !same_biome_cells(&decoded_biomes, &expected) {
                return Err(OverlayError::VerificationFailed(format!(
                    "biome section {section_y} did not round-trip through numeric Level.Biomes"
                )));
            }
        }
    } else {
        for (section_y, source_section) in &source_decoded.sections {
            let decoded_section = decoded.sections.get(section_y).ok_or_else(|| {
                OverlayError::VerificationFailed(format!(
                    "re-decoded numeric chunk lost biome section {section_y}"
                ))
            })?;
            if decoded_section.biomes != source_section.biomes {
                return Err(OverlayError::VerificationFailed(format!(
                    "numeric biome section {section_y} changed without a biome edit"
                )));
            }
        }
    }
    Ok(patched)
}

fn patch_block_sections(
    patched: &mut NbtFile,
    x: i32,
    z: i32,
    source_decoded: &DecodedChunk,
    sections: &BTreeMap<i32, Section>,
    registry: &LegacyRegistry,
) -> Result<(), OverlayError> {
    if sections.is_empty() {
        return Ok(());
    }
    let root = match &mut patched.root {
        NbtValue::Compound(root) => root,
        _ => {
            return Err(OverlayError::VerificationFailed(
                "patched chunk root is not a compound".to_owned(),
            ))
        }
    };
    let level = root
        .get_mut("Level")
        .and_then(NbtValue::as_compound_mut)
        .ok_or_else(|| {
            OverlayError::VerificationFailed("patched chunk has no Level compound".to_owned())
        })?;
    let items = match level.get_mut("Sections") {
        Some(NbtValue::List { kind, items }) if *kind == TAG_COMPOUND => items,
        Some(_) => {
            return Err(OverlayError::VerificationFailed(
                "patched chunk Sections is not a compound list".to_owned(),
            ))
        }
        None => {
            return Err(OverlayError::VerificationFailed(
                "patched chunk has no Sections list".to_owned(),
            ))
        }
    };
    for (section_y, section) in sections {
        let entry = items
            .iter_mut()
            .find(|item| section_y_of(item) == Some(*section_y))
            .ok_or_else(|| {
                OverlayError::VerificationFailed(format!(
                    "source chunk ({x}, {z}) has no section {section_y}"
                ))
            })?;
        let compound = match entry {
            NbtValue::Compound(compound) => compound,
            _ => {
                return Err(OverlayError::VerificationFailed(
                    "section is not a compound".to_owned(),
                ))
            }
        };
        let source_section = source_decoded.sections.get(section_y).ok_or_else(|| {
            OverlayError::VerificationFailed(format!(
                "source chunk ({x}, {z}) did not decode section {section_y}"
            ))
        })?;
        let source_blocks = byte_array(compound.get("Blocks"));
        let source_data = byte_array(compound.get("Data"));
        let source_add = byte_array(compound.get("Add"));
        let mut blocks = vec![0u8; BLOCK_BYTES];
        let mut data = vec![0u8; DATA_BYTES];
        let mut add = source_add.map(|_| vec![0u8; DATA_BYTES]);
        for index in 0..BLOCK_SECTION_VOLUME {
            let local_x = (index % 16) as u8;
            let local_z = ((index / 16) % 16) as u8;
            let local_y = (index / 256) as u8;
            let state = section.get(local_x, local_y, local_z).ok_or_else(|| {
                OverlayError::VerificationFailed(format!(
                    "edited section {section_y} has no block at local ({local_x}, {local_y}, {local_z})"
                ))
            })?;
            let source_state = source_section.blocks.get(local_x, local_y, local_z);
            if source_state == Some(state) {
                if let (Some(source_blocks), Some(source_data)) = (source_blocks, source_data) {
                    blocks[index] = source_blocks[index];
                    set_nibble(&mut data, index, nibble(Some(source_data), index));
                    if let (Some(target), Some(source)) = (add.as_mut(), source_add) {
                        set_nibble(target, index, nibble(Some(source), index));
                    }
                    continue;
                }
            }
            let (id, metadata) = legacy_id(registry, state).ok_or_else(|| {
                OverlayError::VerificationFailed(format!(
                    "block state `{state}` has no 1.8–1.12 numeric id"
                ))
            })?;
            blocks[index] = (id & 0xff) as u8;
            set_nibble(&mut data, index, metadata);
            if id > 0xff {
                add.get_or_insert_with(|| vec![0u8; DATA_BYTES]);
                if let Some(target) = add.as_mut() {
                    set_nibble(target, index, id >> 8);
                }
            }
        }
        compound.insert("Blocks".to_owned(), NbtValue::ByteArray(blocks));
        compound.insert("Data".to_owned(), NbtValue::ByteArray(data));
        if let Some(add) = add {
            compound.insert("Add".to_owned(), NbtValue::ByteArray(add));
        }
    }
    Ok(())
}

fn patch_biomes(
    patched: &mut NbtFile,
    registry: LegacyBiomeRegistry,
    source_sections: &BTreeMap<i32, DecodedSection>,
    edits: &BTreeMap<i32, DomainBiomeSection>,
) -> Result<DomainBiomeSection, OverlayError> {
    let root = match &mut patched.root {
        NbtValue::Compound(root) => root,
        _ => {
            return Err(OverlayError::VerificationFailed(
                "patched chunk root is not a compound".to_owned(),
            ))
        }
    };
    let level = root
        .get_mut("Level")
        .and_then(NbtValue::as_compound_mut)
        .ok_or_else(|| {
            OverlayError::VerificationFailed("patched chunk has no Level compound".to_owned())
        })?;
    let columns = match level.get("Biomes") {
        Some(NbtValue::ByteArray(bytes)) if bytes.len() == BIOME_COLUMNS => bytes.clone(),
        Some(_) => {
            return Err(OverlayError::VerificationFailed(
                "patched chunk Level.Biomes is not a 256-byte array".to_owned(),
            ))
        }
        None => {
            return Err(OverlayError::VerificationFailed(
                "patched chunk has no Level.Biomes array".to_owned(),
            ))
        }
    };
    let baseline_section = source_sections
        .values()
        .next()
        .ok_or_else(|| {
            OverlayError::VerificationFailed(
                "numeric chunk has no decoded section for its biome data".to_owned(),
            )
        })?
        .biomes
        .to_domain()
        .ok_or_else(|| {
            OverlayError::VerificationFailed(
                "source numeric biome data cannot be represented".to_owned(),
            )
        })?;
    let mut changes: BTreeMap<(u8, u8), BiomeState> = BTreeMap::new();
    for (section_y, edit) in edits {
        let base = source_sections
            .get(section_y)
            .ok_or_else(|| {
                OverlayError::VerificationFailed(format!(
                    "source numeric chunk has no section {section_y}"
                ))
            })?
            .biomes
            .to_domain()
            .ok_or_else(|| {
                OverlayError::VerificationFailed(format!(
                    "source numeric biome section {section_y} cannot be represented"
                ))
            })?;
        for cell_z in 0..4u8 {
            for cell_x in 0..4u8 {
                for cell_y in 0..4u8 {
                    let value = edit.get(cell_x, cell_y, cell_z).ok_or_else(|| {
                        OverlayError::VerificationFailed(format!(
                            "edited numeric biome section {section_y} is missing cell ({cell_x}, {cell_y}, {cell_z})"
                        ))
                    })?;
                    if base.get(cell_x, cell_y, cell_z) == Some(value) {
                        continue;
                    }
                    match changes.insert((cell_x, cell_z), value.clone()) {
                        Some(previous) if &previous != value => {
                            return Err(OverlayError::VerificationFailed(format!(
                                "numeric column biome cell ({cell_x}, {cell_z}) changes to different values"
                            )))
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    let mut updated = columns;
    for ((cell_x, cell_z), biome) in &changes {
        let name = biome.as_str();
        let id = registry.id(name).ok_or_else(|| {
            OverlayError::VerificationFailed(format!("biome `{name}` has no 1.8–1.12 numeric id"))
        })?;
        for column_z in cell_z * 4..cell_z * 4 + 4 {
            for column_x in cell_x * 4..cell_x * 4 + 4 {
                updated[(column_z as usize) * 16 + column_x as usize] = id as u8;
            }
        }
    }
    let mut expected = baseline_section;
    for ((cell_x, cell_z), biome) in &changes {
        for cell_y in 0..4u8 {
            expected
                .set(*cell_x, cell_y, *cell_z, biome)
                .ok_or_else(|| {
                    OverlayError::VerificationFailed("numeric biome palette overflow".to_owned())
                })?;
        }
    }
    level.insert("Biomes".to_owned(), NbtValue::ByteArray(updated));
    Ok(expected)
}

/// Broad preservation check used by the overlay and region writer: only the
/// section block arrays and `Level.Biomes` may differ.
pub(super) fn verify_numeric_preserved_fields(
    source: &NbtFile,
    patched: &NbtFile,
) -> Result<(), OverlayError> {
    if source.root_name != patched.root_name {
        return Err(OverlayError::VerificationFailed(
            "root name changed while editing blocks".to_owned(),
        ));
    }
    let source_root = source.root.as_compound().ok_or_else(|| {
        OverlayError::VerificationFailed("source root is not a compound".to_owned())
    })?;
    let patched_root = patched.root.as_compound().ok_or_else(|| {
        OverlayError::VerificationFailed("patched root is not a compound".to_owned())
    })?;
    if source_root.keys().collect::<Vec<_>>() != patched_root.keys().collect::<Vec<_>>() {
        return Err(OverlayError::VerificationFailed(
            "chunk root tags changed while editing blocks".to_owned(),
        ));
    }
    for (key, value) in source_root {
        if key == "Level" {
            continue;
        }
        if patched_root.get(key) != Some(value) {
            return Err(OverlayError::VerificationFailed(format!(
                "chunk root tag `{key}` changed while editing blocks"
            )));
        }
    }
    let source_level = source_root
        .get("Level")
        .and_then(NbtValue::as_compound)
        .ok_or_else(|| {
            OverlayError::VerificationFailed("source Level is not a compound".to_owned())
        })?;
    let patched_level = patched_root
        .get("Level")
        .and_then(NbtValue::as_compound)
        .ok_or_else(|| {
            OverlayError::VerificationFailed("patched Level is not a compound".to_owned())
        })?;
    if source_level.keys().collect::<Vec<_>>() != patched_level.keys().collect::<Vec<_>>() {
        return Err(OverlayError::VerificationFailed(
            "Level tags changed while editing blocks".to_owned(),
        ));
    }
    for (key, value) in source_level {
        if key == "Sections" || key == "Biomes" {
            continue;
        }
        if patched_level.get(key) != Some(value) {
            return Err(OverlayError::VerificationFailed(format!(
                "Level tag `{key}` changed while editing blocks"
            )));
        }
    }
    let source_sections = numeric_sections(source_level)?;
    let patched_sections = numeric_sections(patched_level)?;
    if source_sections.len() != patched_sections.len() {
        return Err(OverlayError::VerificationFailed(
            "section list changed while editing blocks".to_owned(),
        ));
    }
    for (source_section, patched_section) in source_sections.iter().zip(&patched_sections) {
        let source_keys = persistent_section_keys(source_section);
        let patched_keys = persistent_section_keys(patched_section);
        if source_keys != patched_keys {
            return Err(OverlayError::VerificationFailed(
                "section tags changed while editing blocks".to_owned(),
            ));
        }
        for (key, value) in *source_section {
            if matches!(key.as_str(), "Blocks" | "Data" | "Add") {
                continue;
            }
            if patched_section.get(key) != Some(value) {
                return Err(OverlayError::VerificationFailed(format!(
                    "section tag `{key}` changed while editing blocks"
                )));
            }
        }
    }
    Ok(())
}

/// Section keys outside the editable block arrays. `Add` may be introduced by an
/// edit that needs ids above 255 or dropped when it becomes all zero.
fn persistent_section_keys(section: &BTreeMap<String, NbtValue>) -> Vec<&str> {
    section
        .keys()
        .map(String::as_str)
        .filter(|key| !matches!(*key, "Blocks" | "Data" | "Add"))
        .collect()
}

fn numeric_sections(
    level: &BTreeMap<String, NbtValue>,
) -> Result<Vec<&BTreeMap<String, NbtValue>>, OverlayError> {
    let value = level
        .get("Sections")
        .ok_or_else(|| OverlayError::VerificationFailed("chunk has no Sections list".to_owned()))?;
    match value {
        NbtValue::List { kind, items } if *kind == TAG_COMPOUND || items.is_empty() => items
            .iter()
            .map(|item| {
                item.as_compound().ok_or_else(|| {
                    OverlayError::VerificationFailed("chunk has a non-compound section".to_owned())
                })
            })
            .collect(),
        _ => Err(OverlayError::VerificationFailed(
            "chunk Sections is not a compound list".to_owned(),
        )),
    }
}

fn section_y_of(value: &NbtValue) -> Option<i32> {
    match value.as_compound()?.get("Y")? {
        NbtValue::Byte(y) => Some(i32::from(*y)),
        _ => None,
    }
}

fn byte_array(value: Option<&NbtValue>) -> Option<&[u8]> {
    match value {
        Some(NbtValue::ByteArray(bytes)) => Some(bytes),
        _ => None,
    }
}

fn nibble(bytes: Option<&[u8]>, index: usize) -> u16 {
    let Some(bytes) = bytes else {
        return 0;
    };
    let byte = bytes[index >> 1];
    if index & 1 == 0 {
        u16::from(byte & 0x0f)
    } else {
        u16::from(byte >> 4)
    }
}

fn set_nibble(bytes: &mut [u8], index: usize, value: u16) {
    let value = (value & 0x0f) as u8;
    let byte = &mut bytes[index >> 1];
    if index & 1 == 0 {
        *byte = (*byte & 0xf0) | value;
    } else {
        *byte = (*byte & 0x0f) | (value << 4);
    }
}

fn legacy_id(registry: &LegacyRegistry, state: &BlockState) -> Option<(u16, u16)> {
    if state.properties().get("waterlogged").map(String::as_str) == Some("true") {
        return None;
    }
    let derived = derived_keys(state.name());
    let wanted = normalized_properties(state.properties(), derived);
    registry
        .blocks_named(state.name())
        .filter(|candidate| normalized_properties(&candidate.properties, derived) == wanted)
        .min_by(|left, right| preference(left, right, state))
        .map(|state| (state.id, state.metadata))
}

fn preference(
    left: &LegacyBlockState,
    right: &LegacyBlockState,
    state: &BlockState,
) -> std::cmp::Ordering {
    let name = state.name();
    if matches!(name, "minecraft:water" | "minecraft:lava") {
        let level_zero = state.properties().get("level").map(String::as_str) == Some("0");
        let left_stationary = left.id % 2 == 1;
        let right_stationary = right.id % 2 == 1;
        let left_rank = if level_zero {
            left_stationary
        } else {
            !left_stationary
        };
        let right_rank = if level_zero {
            right_stationary
        } else {
            !right_stationary
        };
        return right_rank.cmp(&left_rank);
    }
    (left.id, left.metadata).cmp(&(right.id, right.metadata))
}

fn derived_keys(name: &str) -> &'static [&'static str] {
    if name.ends_with("_fence_gate") {
        &["in_wall"]
    } else if name.ends_with("_stairs") {
        &["shape", "waterlogged"]
    } else if name.ends_with("_fence")
        || name.ends_with("_pane")
        || name.ends_with("_wall")
        || name == "minecraft:iron_bars"
    {
        &["east", "south", "north", "west", "up", "waterlogged"]
    } else if name == "minecraft:redstone_wire" {
        &["east", "south", "north", "west"]
    } else {
        &[]
    }
}

fn normalized_properties<'a>(
    properties: &'a BTreeMap<String, String>,
    derived: &[&str],
) -> BTreeMap<&'a str, &'a str> {
    properties
        .iter()
        .filter(|(key, _)| !derived.contains(&key.as_str()))
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect()
}

fn same_legacy_state(left: &BlockState, right: &BlockState) -> bool {
    left.name() == right.name()
        && normalized_properties(left.properties(), derived_keys(left.name()))
            == normalized_properties(right.properties(), derived_keys(right.name()))
}

fn same_biome_cells(left: &DomainBiomeSection, right: &DomainBiomeSection) -> bool {
    (0..4u8).all(|y| (0..4u8).all(|z| (0..4u8).all(|x| left.get(x, y, z) == right.get(x, y, z))))
}
