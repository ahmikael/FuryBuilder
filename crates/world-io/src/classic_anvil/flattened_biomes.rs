//! The `Level.Biomes` component of Java 1.13–1.17 Anvil NBT.
//!
//! These transformations operate on a single NBT chunk. The reader, import,
//! overlay and exporter use them for admitted 1.13–1.17 profiles.

use minecraft_version::flattened_biomes::FlattenedBiomeRegistry;
use minecraft_version::{BiomeFormat, ChunkFamily, VersionProfile, WorldLayout};
use thiserror::Error;
use world_domain::{BiomeSection, BiomeState, BIOME_CELLS};

use crate::nbt::{NbtFile, NbtValue};

const SECTIONS: usize = 16; // The stable flattened Anvil releases have Y=0..255.
const BLOCK_SIDE: usize = 16;
const CELL_SIDE: usize = 4;
const COLUMNS: usize = BLOCK_SIDE * BLOCK_SIDE;

/// How to project a 4×4 footprint of 1.13–1.14 block columns to one biome cell.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ColumnBiomePolicy {
    /// Refuse to discard differences between the 16 columns in a footprint.
    #[default]
    RequireUniform,
    /// Sample `(4x + 2, 4z + 2)`, as the vanilla 1.15 upgrade does.
    /// On patch, a changed cell may replace all 16 columns in a mixed footprint.
    SampleCenter,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlattenedBiomeProjection {
    /// Section index `0..16`, with each palette's 64 entries indexed in YZX order.
    pub sections: [BiomeSection; SECTIONS],
    /// Number of horizontal 4×4 footprints reduced by center sampling, not
    /// multiplied by the number of vertical sections they occupy.
    pub mixed_column_cells: usize,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum FlattenedBiomeError {
    #[error("profile `{0}` does not describe a stable flattened Anvil biome format")]
    UnsupportedProfile(String),
    #[error("chunk NBT must contain root DataVersion and a Level compound")]
    InvalidChunk,
    #[error("chunk DataVersion {found} does not match biome profile DataVersion {expected}")]
    VersionMismatch { found: i32, expected: i32 },
    #[error("Level.Biomes must be an int array")]
    MissingArray,
    #[error("Level.Biomes has {found} entries; this format requires {expected}")]
    InvalidLength { found: usize, expected: usize },
    #[error("unknown numeric biome ID {id} at Level.Biomes[{index}]; source data was not changed")]
    UnknownId { id: i32, index: usize },
    #[error("biome `{0}` is not in the source release's numeric registry")]
    UnrepresentableBiome(String),
    #[error(
        "column cell ({x}, {z}) contains different biomes; center sampling must be authorized"
    )]
    MixedColumns { x: usize, z: usize },
    #[error("column cell ({x}, {z}) varies by height and cannot be patched into a column array")]
    VerticalVariation { x: usize, z: usize },
}

/// Decodes source-version biome names into the normalized 4×4×4 section grids.
///
/// The raw chunk is not changed. Unknown IDs, including in columns that are not
/// sampled, stop conversion instead of turning into an unrelated vanilla biome.
pub fn decode_flattened_chunk_biomes(
    nbt: &NbtFile,
    profile: &VersionProfile,
    policy: ColumnBiomePolicy,
) -> Result<FlattenedBiomeProjection, FlattenedBiomeError> {
    let (registry, ids) = read_ids(nbt, profile)?;
    if registry.is_volumetric() {
        return Ok(FlattenedBiomeProjection {
            sections: std::array::from_fn(|y| {
                section(&ids[y * BIOME_CELLS..][..BIOME_CELLS], registry)
            }),
            mixed_column_cells: 0,
        });
    }

    let mut plane = [0; CELL_SIDE * CELL_SIDE];
    let mut mixed_column_cells = 0;
    for (index, sample) in plane.iter_mut().enumerate() {
        let (x, z) = (index % CELL_SIDE, index / CELL_SIDE);
        *sample = ids[center_index(x, z)];
        if mixed_footprint(ids, x, z) {
            if policy == ColumnBiomePolicy::RequireUniform {
                return Err(FlattenedBiomeError::MixedColumns { x, z });
            }
            mixed_column_cells += 1;
        }
    }
    let cells: [i32; BIOME_CELLS] = std::array::from_fn(|index| plane[index % plane.len()]);
    let biome_section = section(&cells, registry);
    Ok(FlattenedBiomeProjection {
        sections: std::array::from_fn(|_| biome_section.clone()),
        mixed_column_cells,
    })
}

/// Returns a same-version chunk copy with only `Level.Biomes` changed.
///
/// Column format cannot represent vertical variation. A footprint whose
/// projected biome did not change keeps all of its original columns even if
/// they differ. Changing a mixed footprint requires `SampleCenter`.
pub fn patch_flattened_chunk_biomes(
    source: &NbtFile,
    profile: &VersionProfile,
    sections: &[BiomeSection; SECTIONS],
    policy: ColumnBiomePolicy,
) -> Result<NbtFile, FlattenedBiomeError> {
    let (registry, source_ids) = read_ids(source, profile)?;
    let ids = if registry.is_volumetric() {
        sections
            .iter()
            .flat_map(|section| {
                section
                    .cells()
                    .iter()
                    .map(|&index| &section.palette()[index as usize])
            })
            .map(|biome| numeric_id(registry, biome))
            .collect::<Result<_, _>>()?
    } else {
        let mut ids = source_ids.to_vec();
        for z in 0..CELL_SIDE {
            for x in 0..CELL_SIDE {
                let name = sections[0].get(x as u8, 0, z as u8).expect("bounded cell");
                if sections.iter().any(|section| {
                    (0..CELL_SIDE as u8).any(|y| section.get(x as u8, y, z as u8) != Some(name))
                }) {
                    return Err(FlattenedBiomeError::VerticalVariation { x, z });
                }
                let replacement = numeric_id(registry, name)?;
                if source_ids[center_index(x, z)] == replacement {
                    continue;
                }
                if policy == ColumnBiomePolicy::RequireUniform && mixed_footprint(source_ids, x, z)
                {
                    return Err(FlattenedBiomeError::MixedColumns { x, z });
                }
                for row in footprint_rows(x, z) {
                    ids[row..row + CELL_SIDE].fill(replacement);
                }
            }
        }
        ids
    };

    let mut patched = source.clone();
    let NbtValue::Compound(root) = &mut patched.root else {
        unreachable!("root checked by read_ids");
    };
    let Some(NbtValue::Compound(level)) = root.get_mut("Level") else {
        unreachable!("Level checked by read_ids");
    };
    level.insert("Biomes".to_owned(), NbtValue::IntArray(ids));
    Ok(patched)
}

fn read_ids<'a>(
    nbt: &'a NbtFile,
    profile: &VersionProfile,
) -> Result<(FlattenedBiomeRegistry, &'a [i32]), FlattenedBiomeError> {
    let unsupported = || FlattenedBiomeError::UnsupportedProfile(profile.id.clone());
    let registry =
        FlattenedBiomeRegistry::for_release(&profile.minecraft_version).ok_or_else(unsupported)?;
    let (format, expected) = if registry.is_volumetric() {
        (BiomeFormat::Section, SECTIONS * BIOME_CELLS)
    } else {
        (BiomeFormat::Column, COLUMNS)
    };
    if profile.world_layout != WorldLayout::ClassicAnvil
        || profile.chunk_family != ChunkFamily::SectionBiomes
        || profile.biome_format != format
    {
        return Err(unsupported());
    }
    let root = nbt
        .root
        .as_compound()
        .ok_or(FlattenedBiomeError::InvalidChunk)?;
    let Some(NbtValue::Int(found)) = root.get("DataVersion") else {
        return Err(FlattenedBiomeError::InvalidChunk);
    };
    if *found != profile.data_version {
        return Err(FlattenedBiomeError::VersionMismatch {
            found: *found,
            expected: profile.data_version,
        });
    }
    let level = root
        .get("Level")
        .and_then(NbtValue::as_compound)
        .ok_or(FlattenedBiomeError::InvalidChunk)?;
    let Some(NbtValue::IntArray(ids)) = level.get("Biomes") else {
        return Err(FlattenedBiomeError::MissingArray);
    };
    if ids.len() != expected {
        return Err(FlattenedBiomeError::InvalidLength {
            found: ids.len(),
            expected,
        });
    }
    if let Some((index, &id)) = ids
        .iter()
        .enumerate()
        .find(|(_, id)| registry.name(**id).is_none())
    {
        return Err(FlattenedBiomeError::UnknownId { id, index });
    }
    Ok((registry, ids))
}

fn center_index(x: usize, z: usize) -> usize {
    (z * CELL_SIDE + CELL_SIDE / 2) * BLOCK_SIDE + x * CELL_SIDE + CELL_SIDE / 2
}

fn footprint_rows(x: usize, z: usize) -> impl Iterator<Item = usize> {
    (0..CELL_SIDE).map(move |dz| (z * CELL_SIDE + dz) * BLOCK_SIDE + x * CELL_SIDE)
}

fn mixed_footprint(ids: &[i32], x: usize, z: usize) -> bool {
    let center = ids[center_index(x, z)];
    footprint_rows(x, z).any(|row| ids[row..row + CELL_SIDE].iter().any(|&id| id != center))
}

fn numeric_id(
    registry: FlattenedBiomeRegistry,
    biome: &BiomeState,
) -> Result<i32, FlattenedBiomeError> {
    registry
        .id(biome.as_str())
        .ok_or_else(|| FlattenedBiomeError::UnrepresentableBiome(biome.to_string()))
}

fn section(ids: &[i32], registry: FlattenedBiomeRegistry) -> BiomeSection {
    let mut ids_in_palette = Vec::new();
    let mut palette = Vec::new();
    let mut cells = Vec::with_capacity(BIOME_CELLS);
    for id in ids {
        let index = ids_in_palette
            .iter()
            .position(|known| known == id)
            .unwrap_or_else(|| {
                let index = ids_in_palette.len();
                ids_in_palette.push(*id);
                palette.push(
                    registry
                        .name(*id)
                        .expect("ID checked by read_ids")
                        .parse()
                        .expect("valid vanilla biome name"),
                );
                index
            });
        cells.push(index as u16);
    }
    BiomeSection::from_palette(palette, cells).expect("bounded section biome cells")
}
