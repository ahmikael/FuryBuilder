//! Decoding and encoding for the flattened classic Anvil releases (1.13–1.17).
//!
//! These chunks keep the pre-1.18 `Level` wrapper and store block palettes
//! directly on each section (`Palette`/`BlockStates`). Block indices are one
//! continuous bit stream through 1.15 and become padded per 64-bit word in
//! 1.16. Biomes live in the chunk-wide `Level.Biomes` array and are mapped to
//! the normalized 4×4×4 section model by the existing flattened converter.
//!
//! The reader, import, overlay, region writer and same-version export share
//! this module for every admitted 1.13–1.17 profile.

use std::collections::{BTreeMap, BTreeSet};

use minecraft_version::{BiomeFormat, VersionProfile};
use world_domain::{BiomeSection as DomainBiomeSection, BiomeState, Section, SECTION_SIZE};

use crate::nbt::{NbtFile, NbtValue, TAG_COMPOUND};

use super::chunk::{
    compound_list_len, decode_block_section, invalid, BiomeSection, DecodedChunk, DecodedSection,
    BLOCK_SECTION_VOLUME,
};
use super::flattened_biomes::{
    decode_flattened_chunk_biomes, patch_flattened_chunk_biomes, ColumnBiomePolicy,
    FlattenedBiomeError,
};
use super::format::{FlattenedFormat, LongPacking, PackedLongs};
use super::overlay::{block_palette_entries, packed_block_indices, OverlayError};
use super::reader::{AnvilError, JavaWorldDimension};

/// 1.16 changed `BlockStates` from a continuous bit stream to padded words.
/// The exact release boundary is DataVersion 2566 (1.16).
const BIOME_SECTIONS: usize = 16;
const LIGHT_BYTES: usize = 2048;

pub(super) fn decode_chunk(
    nbt: &crate::nbt::NbtFile,
    x: i32,
    z: i32,
    dimension: &JavaWorldDimension,
    profile: &VersionProfile,
) -> Result<DecodedChunk, AnvilError> {
    let format = FlattenedFormat::for_profile(profile)?;
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

    // Vanilla can save an empty proto-chunk before it assigns biomes or blocks.
    let empty_proto_chunk = status.as_deref() == Some("empty")
        && items.is_empty()
        && match level.get("Biomes") {
            None => true,
            Some(NbtValue::IntArray(ids)) => ids.is_empty(),
            Some(_) => false,
        };
    let (mut biome_sections, mixed_biome_columns) = if empty_proto_chunk {
        (BTreeMap::new(), 0)
    } else {
        let projection =
            decode_flattened_chunk_biomes(nbt, profile, ColumnBiomePolicy::SampleCenter)
                .map_err(|error| biome_error(x, z, error))?;
        let sections = projection
            .sections
            .into_iter()
            .enumerate()
            .map(|(y, section)| (y as i32, section))
            .collect();
        (sections, projection.mixed_column_cells)
    };

    let packing = format.packing;
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
                true,
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
        mixed_biome_columns,
    })
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

/// Encodes overlay edits into a flattened chunk while preserving every field
/// outside `Level.Sections[].Palette`/`BlockStates` and `Level.Biomes`.
pub(super) fn encode_chunk(
    source: &NbtFile,
    dimension: &JavaWorldDimension,
    profile: &VersionProfile,
    source_decoded: &DecodedChunk,
    sections: &BTreeMap<i32, Section>,
    biomes: &BTreeMap<i32, DomainBiomeSection>,
) -> Result<NbtFile, OverlayError> {
    let (x, z) = (source_decoded.x, source_decoded.z);
    let format = FlattenedFormat::for_profile(profile).map_err(OverlayError::Anvil)?;
    let mut patched = source.clone();
    patch_block_sections(&mut patched, x, z, format.packing, sections)?;
    let expected_biomes = if biomes.is_empty() {
        None
    } else {
        Some(patch_biomes(&mut patched, profile, biomes)?)
    };

    verify_flattened_preserved_fields(source, &patched)?;
    verify_flattened_edit_scope(
        source,
        &patched,
        sections.keys().copied().collect(),
        !biomes.is_empty(),
    )?;

    let decoded = decode_chunk(&patched, x, z, dimension, profile).map_err(OverlayError::Anvil)?;
    for (section_y, section) in sections {
        let decoded_section = decoded.sections.get(section_y).ok_or_else(|| {
            OverlayError::VerificationFailed(format!(
                "re-decoded flattened chunk lost section {section_y}"
            ))
        })?;
        if &decoded_section.blocks != section {
            return Err(OverlayError::VerificationFailed(format!(
                "section {section_y} did not round-trip through flattened NBT"
            )));
        }
    }
    if let Some(expected_biomes) = expected_biomes {
        for (section_y, decoded_section) in &decoded.sections {
            let section = expected_biomes.get(*section_y as usize).ok_or_else(|| {
                OverlayError::VerificationFailed(format!(
                    "re-decoded flattened biome section {section_y} is outside 0..{BIOME_SECTIONS}"
                ))
            })?;
            let decoded_biomes = decoded_section.biomes.to_domain().ok_or_else(|| {
                OverlayError::VerificationFailed(format!(
                    "re-decoded flattened biome section {section_y} cannot be represented"
                ))
            })?;
            let same_cells = (0..4u8).all(|y| {
                (0..4u8)
                    .all(|z| (0..4u8).all(|x| decoded_biomes.get(x, y, z) == section.get(x, y, z)))
            });
            if !same_cells {
                let mut detail = "unknown cell".to_owned();
                'outer: for y in 0..4u8 {
                    for z in 0..4u8 {
                        for x in 0..4u8 {
                            if decoded_biomes.get(x, y, z) != section.get(x, y, z) {
                                detail = format!(
                                    "cell ({x}, {y}, {z}) decoded {:?} expected {:?}",
                                    decoded_biomes.get(x, y, z).map(BiomeState::as_str),
                                    section.get(x, y, z).map(BiomeState::as_str)
                                );
                                break 'outer;
                            }
                        }
                    }
                }
                return Err(OverlayError::VerificationFailed(format!(
                    "biome section {section_y} did not round-trip through flattened NBT: {detail}"
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
                "re-decoded flattened chunk lost untouched section {section_y}"
            ))
        })?;
        if decoded_section.blocks != source_section.blocks {
            return Err(OverlayError::VerificationFailed(format!(
                "untouched section {section_y} changed while editing the flattened chunk"
            )));
        }
    }
    Ok(patched)
}

fn patch_block_sections(
    patched: &mut NbtFile,
    x: i32,
    z: i32,
    packing: LongPacking,
    sections: &BTreeMap<i32, Section>,
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
        compound.insert(
            "Palette".to_owned(),
            NbtValue::List {
                kind: TAG_COMPOUND,
                items: block_palette_entries(section),
            },
        );
        let packed = PackedLongs::new(section.palette_len() - 1, 4, packing);
        let word_count = packed.word_count(BLOCK_SECTION_VOLUME);
        let words = packed_block_indices(section, packed).unwrap_or_else(|| vec![0; word_count]);
        compound.insert("BlockStates".to_owned(), NbtValue::LongArray(words));
    }
    Ok(())
}

fn patch_biomes(
    patched: &mut NbtFile,
    profile: &VersionProfile,
    edits: &BTreeMap<i32, DomainBiomeSection>,
) -> Result<[DomainBiomeSection; BIOME_SECTIONS], OverlayError> {
    let projection =
        decode_flattened_chunk_biomes(patched, profile, ColumnBiomePolicy::SampleCenter).map_err(
            |error| OverlayError::VerificationFailed(format!("flattened biomes: {error}")),
        )?;
    let original = projection.sections;
    let mut sections = original.clone();
    if profile.biome_format == BiomeFormat::Column {
        // A column world stores one biome per horizontal cell. Collect the cells
        // that actually changed in the edited sections, require each column to
        // change to a single value, then apply it to all 16 vertical sections.
        let mut changes: BTreeMap<(u8, u8), BiomeState> = BTreeMap::new();
        for (section_y, edit) in edits {
            let base = original.get(*section_y as usize).ok_or_else(|| {
                OverlayError::VerificationFailed(format!(
                    "flattened biome section {section_y} is outside 0..{BIOME_SECTIONS}"
                ))
            })?;
            for z in 0..4u8 {
                for x in 0..4u8 {
                    let mut changed = None;
                    for y in 0..4u8 {
                        let value = edit.get(x, y, z).ok_or_else(|| {
                            OverlayError::VerificationFailed(format!(
                                "edited biome section {section_y} is missing cell ({x}, {y}, {z})"
                            ))
                        })?;
                        if base.get(x, y, z) == Some(value) {
                            continue;
                        }
                        match &changed {
                            None => changed = Some(value.clone()),
                            Some(previous) if previous != value => {
                                return Err(OverlayError::VerificationFailed(format!(
                                    "column biome cell ({x}, {z}) in section {section_y} changes to different values by height"
                                )));
                            }
                            Some(_) => {}
                        }
                    }
                    let Some(value) = changed else { continue };
                    if let Some(previous) = changes.insert((x, z), value.clone()) {
                        if previous != value {
                            return Err(OverlayError::VerificationFailed(format!(
                                "column biome cell ({x}, {z}) changes to different values in different sections"
                            )));
                        }
                    }
                }
            }
        }
        for ((x, z), value) in changes {
            if (0..4u8).all(|y| sections[0].get(x, y, z) == Some(&value)) {
                continue;
            }
            for target in &mut sections {
                for y in 0..4u8 {
                    target.set(x, y, z, &value).ok_or_else(|| {
                        OverlayError::VerificationFailed(
                            "flattened biome palette overflow".to_owned(),
                        )
                    })?;
                }
            }
        }
    } else {
        for (section_y, edit) in edits {
            let index = usize::try_from(*section_y).map_err(|_| {
                OverlayError::VerificationFailed(format!(
                    "flattened biome section {section_y} is negative"
                ))
            })?;
            let target = sections.get_mut(index).ok_or_else(|| {
                OverlayError::VerificationFailed(format!(
                    "flattened biome section {section_y} is outside 0..{BIOME_SECTIONS}"
                ))
            })?;
            *target = edit.clone();
        }
    }
    *patched =
        patch_flattened_chunk_biomes(patched, profile, &sections, ColumnBiomePolicy::SampleCenter)
            .map_err(|error| {
                OverlayError::VerificationFailed(format!("flattened biomes: {error}"))
            })?;
    Ok(sections)
}

pub(super) fn verify_flattened_preserved_fields(
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
    let source_sections = flattened_section_compounds(source_level)?;
    let patched_sections = flattened_section_compounds(patched_level)?;
    if source_sections.keys().collect::<Vec<_>>() != patched_sections.keys().collect::<Vec<_>>() {
        return Err(OverlayError::VerificationFailed(
            "section list changed while editing blocks".to_owned(),
        ));
    }
    for (y, source_section) in source_sections {
        let patched_section = patched_sections
            .get(&y)
            .ok_or_else(|| OverlayError::VerificationFailed(format!("section {y} disappeared")))?;
        let source_rest = source_section
            .iter()
            .filter(|(key, _)| key.as_str() != "Palette" && key.as_str() != "BlockStates")
            .collect::<Vec<_>>();
        let patched_rest = patched_section
            .iter()
            .filter(|(key, _)| key.as_str() != "Palette" && key.as_str() != "BlockStates")
            .collect::<Vec<_>>();
        if source_rest != patched_rest {
            return Err(OverlayError::VerificationFailed(format!(
                "section {y} non-editable tags changed"
            )));
        }
    }
    Ok(())
}

fn verify_flattened_edit_scope(
    source: &NbtFile,
    patched: &NbtFile,
    block_sections: BTreeSet<i32>,
    biomes_edited: bool,
) -> Result<(), OverlayError> {
    let source_root = source.root.as_compound().ok_or_else(|| {
        OverlayError::VerificationFailed("source root is not a compound".to_owned())
    })?;
    let patched_root = patched.root.as_compound().ok_or_else(|| {
        OverlayError::VerificationFailed("patched root is not a compound".to_owned())
    })?;
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
    let source_sections = flattened_section_compounds(source_level)?;
    let patched_sections = flattened_section_compounds(patched_level)?;
    for (y, source_section) in source_sections {
        let patched_section = patched_sections
            .get(&y)
            .ok_or_else(|| OverlayError::VerificationFailed(format!("section {y} disappeared")))?;
        if !block_sections.contains(&y) {
            for key in ["Palette", "BlockStates"] {
                if source_section.get(key) != patched_section.get(key) {
                    return Err(OverlayError::VerificationFailed(format!(
                        "section {y} {key} changed without a block edit"
                    )));
                }
            }
        }
    }
    if !biomes_edited && source_level.get("Biomes") != patched_level.get("Biomes") {
        return Err(OverlayError::VerificationFailed(
            "Level.Biomes changed without a biome edit".to_owned(),
        ));
    }
    Ok(())
}

fn flattened_section_compounds(
    level: &BTreeMap<String, NbtValue>,
) -> Result<BTreeMap<i32, &BTreeMap<String, NbtValue>>, OverlayError> {
    let value = level
        .get("Sections")
        .ok_or_else(|| OverlayError::VerificationFailed("missing Sections list".to_owned()))?;
    let (kind, items) = value
        .as_list()
        .ok_or_else(|| OverlayError::VerificationFailed("Sections is not a list".to_owned()))?;
    if kind != TAG_COMPOUND && !items.is_empty() {
        return Err(OverlayError::VerificationFailed(
            "Sections is not a compound list".to_owned(),
        ));
    }
    let mut sections = BTreeMap::new();
    for item in items {
        let compound = item.as_compound().ok_or_else(|| {
            OverlayError::VerificationFailed("section is not a compound".to_owned())
        })?;
        let y = match compound.get("Y") {
            Some(NbtValue::Byte(value)) => *value as i32,
            _ => {
                return Err(OverlayError::VerificationFailed(
                    "section has no byte Y".to_owned(),
                ))
            }
        };
        if sections.insert(y, compound).is_some() {
            return Err(OverlayError::VerificationFailed(format!(
                "duplicate section Y {y}"
            )));
        }
    }
    Ok(sections)
}

fn section_y_of(value: &NbtValue) -> Option<i32> {
    let compound = value.as_compound()?;
    match compound.get("Y") {
        Some(NbtValue::Byte(y)) => Some(*y as i32),
        _ => None,
    }
}
