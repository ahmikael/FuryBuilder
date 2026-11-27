//! Numeric classic Anvil chunk tests (1.8.9–1.12.2).

use std::path::PathBuf;

use minecraft_version::{profile_for_version, VersionProfile};
use world_domain::DimensionKind;
use world_io::anvil::{
    AnvilError, ChunkCompression, JavaWorldDimension, RegionChunk, RegionChunkSummary,
};
use world_io::nbt::{NbtFile, NbtValue, TAG_COMPOUND};

const TARGET: i32 = 1343;

fn profile() -> VersionProfile {
    profile_for_version(&"1.12.2".parse().expect("version")).expect("1.12.2 profile")
}

fn profile_of(version: &str) -> VersionProfile {
    profile_for_version(&version.parse().expect("version")).expect(version)
}

fn overworld() -> JavaWorldDimension {
    JavaWorldDimension {
        id: "minecraft:overworld".to_owned(),
        kind: DimensionKind::Overworld,
        min_y: 0,
        height: 256,
        folder: PathBuf::new(),
        region_folder: PathBuf::new(),
        region_files: 0,
    }
}

fn set_nibble(bytes: &mut [u8], index: usize, value: u8) {
    let byte = &mut bytes[index >> 1];
    if index & 1 == 0 {
        *byte = (*byte & 0xf0) | (value & 0x0f);
    } else {
        *byte = (*byte & 0x0f) | ((value & 0x0f) << 4);
    }
}

fn section(y: i8, blocks: &[(usize, u16, u8)]) -> NbtValue {
    let mut block_ids = vec![0u8; 4096];
    let mut data = vec![0u8; 2048];
    let mut add = vec![0u8; 2048];
    let mut has_add = false;
    for (index, id, metadata) in blocks {
        block_ids[*index] = (*id & 0xff) as u8;
        if *id > 0xff {
            has_add = true;
            set_nibble(&mut add, *index, (*id >> 8) as u8);
        }
        set_nibble(&mut data, *index, *metadata);
    }
    let mut fields = vec![
        ("Y".to_owned(), NbtValue::Byte(y)),
        ("Blocks".to_owned(), NbtValue::ByteArray(block_ids)),
        ("Data".to_owned(), NbtValue::ByteArray(data)),
        ("SkyLight".to_owned(), NbtValue::ByteArray(vec![0xff; 2048])),
        ("BlockLight".to_owned(), NbtValue::ByteArray(vec![0; 2048])),
        (
            "UnknownSectionTag".to_owned(),
            NbtValue::String("preserve".to_owned()),
        ),
    ];
    if has_add {
        fields.push(("Add".to_owned(), NbtValue::ByteArray(add)));
    }
    NbtValue::Compound(fields.into_iter().collect())
}

fn chunk_nbt(x: i32, z: i32, sections: Vec<NbtValue>, biomes: Vec<u8>) -> NbtFile {
    NbtFile::new(
        "",
        NbtValue::compound([
            ("DataVersion", NbtValue::Int(TARGET)),
            (
                "Level",
                NbtValue::compound([
                    ("xPos", NbtValue::Int(x)),
                    ("zPos", NbtValue::Int(z)),
                    ("TerrainPopulated", NbtValue::Byte(1)),
                    ("LastUpdate", NbtValue::Long(1234)),
                    ("Biomes", NbtValue::ByteArray(biomes)),
                    ("HeightMap", NbtValue::IntArray(vec![0; 256])),
                    (
                        "Sections",
                        NbtValue::List {
                            kind: TAG_COMPOUND,
                            items: sections,
                        },
                    ),
                    (
                        "TileEntities",
                        NbtValue::List {
                            kind: TAG_COMPOUND,
                            items: vec![NbtValue::compound([(
                                "id",
                                NbtValue::String("minecraft:chest".to_owned()),
                            )])],
                        },
                    ),
                    (
                        "Entities",
                        NbtValue::List {
                            kind: TAG_COMPOUND,
                            items: vec![NbtValue::compound([(
                                "id",
                                NbtValue::String("minecraft:item".to_owned()),
                            )])],
                        },
                    ),
                    ("UnknownLevelTag", NbtValue::String("preserve".to_owned())),
                ]),
            ),
            (
                "UnknownRootTag",
                NbtValue::compound([("text", NbtValue::String("preserve".to_owned()))]),
            ),
        ]),
    )
}

fn decoded(nbt: NbtFile) -> RegionChunk {
    RegionChunk {
        x: 0,
        z: 0,
        timestamp: 0,
        compression: ChunkCompression::Zlib,
        compressed_bytes: 0,
        nbt_bytes: 0,
        summary: RegionChunkSummary {
            x: 0,
            z: 0,
            data_version: TARGET,
            status: None,
            y_pos: None,
            sections: 0,
            block_entities: 0,
            entities: 0,
        },
        nbt,
    }
}

#[test]
fn decodes_numeric_ids_metadata_and_column_biomes() {
    let mut biomes = vec![1u8; 256];
    biomes[2 * 16 + 2] = 2;
    let nbt = chunk_nbt(
        0,
        0,
        vec![
            section(
                0,
                &[
                    (0, 1, 0),
                    (1, 1, 1),
                    (16, 35, 14),
                    (256, 53, 0),
                    (257, 8, 0),
                ],
            ),
            section(1, &[(0, 3, 0)]),
        ],
        biomes,
    );
    let chunk = decoded(nbt);
    let decoded = chunk
        .decode(&overworld(), &profile())
        .expect("decode numeric chunk");

    assert_eq!(decoded.data_version, TARGET);
    assert_eq!(decoded.status.as_deref(), Some("full"));
    assert_eq!(decoded.y_pos, 0);
    assert_eq!(decoded.sections.len(), 2);
    assert_eq!(decoded.missing_sections.len(), 14);
    assert_eq!(decoded.block_entities, 1);
    assert_eq!(decoded.entities, 1);
    assert_eq!(decoded.mixed_biome_columns, 1);

    assert_eq!(
        decoded
            .block_at(0, 0, 0)
            .map(ToString::to_string)
            .as_deref(),
        Some("minecraft:stone")
    );
    assert_eq!(
        decoded
            .block_at(1, 0, 0)
            .map(ToString::to_string)
            .as_deref(),
        Some("minecraft:granite")
    );
    assert_eq!(
        decoded
            .block_at(0, 0, 1)
            .map(ToString::to_string)
            .as_deref(),
        Some("minecraft:red_wool")
    );
    assert_eq!(
        decoded
            .block_at(0, 1, 0)
            .map(ToString::to_string)
            .as_deref(),
        Some("minecraft:oak_stairs[facing=east,half=bottom,shape=outer_right]")
    );
    assert_eq!(
        decoded
            .block_at(1, 1, 0)
            .map(ToString::to_string)
            .as_deref(),
        Some("minecraft:water[level=0]")
    );
    assert_eq!(
        decoded
            .block_at(0, 16, 0)
            .map(ToString::to_string)
            .as_deref(),
        Some("minecraft:dirt")
    );

    assert_eq!(decoded.biome_at(0, 0, 0), Some("minecraft:desert"));
    assert_eq!(decoded.biome_at(0, 0, 15), Some("minecraft:plains"));
    assert_eq!(decoded.biome_at(0, 16, 0), Some("minecraft:desert"));
}

#[test]
fn one_eight_chunks_without_a_data_version_use_the_level_dat_profile() {
    let profile = profile_of("1.8.9");
    let mut nbt = chunk_nbt(0, 0, vec![section(0, &[(0, 1, 0)])], vec![1; 256]);
    if let NbtValue::Compound(root) = &mut nbt.root {
        root.remove("DataVersion");
    }
    let decoded = decoded(nbt)
        .decode(&overworld(), &profile)
        .expect("1.8.9 chunk without DataVersion");
    assert_eq!(decoded.data_version, 47);
    assert_eq!(
        decoded
            .block_at(0, 0, 0)
            .map(ToString::to_string)
            .as_deref(),
        Some("minecraft:stone")
    );
}

#[test]
fn rejects_a_chunk_data_version_that_disagrees_with_the_world_profile() {
    let nbt = chunk_nbt(0, 0, vec![section(0, &[(0, 1, 0)])], vec![1; 256]);
    let error = decoded(nbt)
        .decode(&overworld(), &profile_of("1.11.2"))
        .expect_err("mismatched DataVersion");
    assert!(matches!(error, AnvilError::UnsupportedDataVersion { .. }));
}

#[test]
fn mixed_biome_footprints_sample_the_center_column() {
    let mut biomes = vec![1u8; 256];
    biomes[2 * 16 + 2] = 2;
    let nbt = chunk_nbt(0, 0, vec![section(0, &[(0, 1, 0)])], biomes);
    let decoded = decoded(nbt)
        .decode(&overworld(), &profile())
        .expect("decode numeric chunk");
    assert_eq!(decoded.mixed_biome_columns, 1);
    assert_eq!(decoded.biome_at(0, 0, 0), Some("minecraft:desert"));
}

#[test]
fn rejects_unknown_numeric_block_ids_without_substitution() {
    let nbt = chunk_nbt(0, 0, vec![section(0, &[(0, 4095, 0)])], vec![1; 256]);
    let error = decoded(nbt)
        .decode(&overworld(), &profile())
        .expect_err("unknown numeric id");
    assert!(
        matches!(error, AnvilError::InvalidChunk { .. }),
        "{error:?}"
    );
    assert!(error
        .to_string()
        .contains("unknown numeric block id 4095:0"));
}

#[test]
fn rejects_add_ids_that_are_not_in_the_shared_registry() {
    let nbt = chunk_nbt(0, 0, vec![section(0, &[(0, 256, 0)])], vec![1; 256]);
    let error = decoded(nbt)
        .decode(&overworld(), &profile())
        .expect_err("unknown add id");
    assert!(error.to_string().contains("unknown numeric block id 256:0"));
}

#[test]
fn rejects_unknown_biome_ids() {
    let mut biomes = vec![1u8; 256];
    biomes[0] = 200;
    let nbt = chunk_nbt(0, 0, vec![section(0, &[(0, 1, 0)])], biomes);
    let error = decoded(nbt)
        .decode(&overworld(), &profile())
        .expect_err("unknown biome");
    assert!(error.to_string().contains("unknown numeric biome ID 200"));
}

#[test]
fn rejects_malformed_sections_instead_of_treating_them_as_air() {
    let mut malformed = section(0, &[(0, 1, 0)]);
    if let NbtValue::Compound(fields) = &mut malformed {
        fields.insert("Blocks".to_owned(), NbtValue::ByteArray(vec![0; 4095]));
    }
    let nbt = chunk_nbt(0, 0, vec![malformed], vec![1; 256]);
    let error = decoded(nbt)
        .decode(&overworld(), &profile())
        .expect_err("short blocks");
    assert!(error.to_string().contains("expected 4096"));

    let duplicate = chunk_nbt(
        0,
        0,
        vec![section(0, &[(0, 1, 0)]), section(0, &[(0, 1, 0)])],
        vec![1; 256],
    );
    let error = decoded(duplicate)
        .decode(&overworld(), &profile())
        .expect_err("duplicate section");
    assert!(error.to_string().contains("duplicate section Y 0"));
}

#[test]
fn rejects_a_chunk_whose_stored_position_does_not_match_its_region_slot() {
    let nbt = chunk_nbt(4, 0, vec![section(0, &[(0, 1, 0)])], vec![1; 256]);
    let error = decoded(nbt)
        .decode(&overworld(), &profile())
        .expect_err("position mismatch");
    assert!(error.to_string().contains("do not match"));
}
