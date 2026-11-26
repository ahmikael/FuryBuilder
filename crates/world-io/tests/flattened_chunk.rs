//! Chunk layout decoding for Java 1.13–1.17.
//!
//! The fixtures use the pre-1.18 `Level.Sections` layout with direct
//! `Palette`/`BlockStates` fields. 1.13–1.15 store one continuous bit stream;
//! 1.16+ pads every 64-bit word. These releases are not admitted by the
//! region reader yet, so the tests exercise the profile-dispatched decoder
//! directly.

use std::path::PathBuf;

use minecraft_version::{profile_for_version, VersionProfile};
use world_domain::{BiomeSection, BiomeState, DimensionKind};
use world_io::anvil::{
    AnvilError, ChunkCompression, JavaWorldDimension, RegionChunk, RegionChunkSummary,
};
use world_io::nbt::{NbtFile, NbtValue, TAG_COMPOUND};
use world_io::{patch_flattened_chunk_biomes, ColumnBiomePolicy, DecodedChunk};

const BITS_PER_BLOCK: u8 = 5;
const PALETTE_LEN: usize = 17;
const LIGHT_BYTES: usize = 2048;

fn profile(version: &str) -> VersionProfile {
    profile_for_version(&version.parse().expect("version")).expect("built-in profile")
}

fn dimension() -> JavaWorldDimension {
    JavaWorldDimension {
        id: "minecraft:overworld".to_owned(),
        kind: DimensionKind::Overworld,
        min_y: 0,
        height: 256,
        folder: PathBuf::from("region"),
        region_folder: PathBuf::from("region"),
        region_files: 1,
    }
}

fn expected_packing(profile: &VersionProfile) -> bool {
    profile.data_version >= 2566
}

fn block_indices() -> Vec<u16> {
    (0..4096)
        .map(|index| (index % PALETTE_LEN) as u16)
        .collect()
}

fn pack_unpadded(indices: &[u16]) -> Vec<i64> {
    let mut words = vec![0i64; (indices.len() * BITS_PER_BLOCK as usize).div_ceil(64)];
    for (index, value) in indices.iter().enumerate() {
        let bit = index * BITS_PER_BLOCK as usize;
        let word = bit / 64;
        let shift = bit % 64;
        let value = u64::from(*value);
        words[word] |= (value << shift) as i64;
        if shift + BITS_PER_BLOCK as usize > 64 {
            words[word + 1] |= (value >> (64 - shift)) as i64;
        }
    }
    words
}

fn pack_padded(indices: &[u16]) -> Vec<i64> {
    let slots = 64 / BITS_PER_BLOCK as usize;
    let mut words = vec![0i64; indices.len().div_ceil(slots)];
    for (index, value) in indices.iter().enumerate() {
        let shift = (index % slots) * BITS_PER_BLOCK as usize;
        words[index / slots] |= (*value as i64) << shift;
    }
    words
}

fn block_palette() -> Vec<NbtValue> {
    (0..PALETTE_LEN)
        .map(|index| {
            NbtValue::compound([("Name", NbtValue::String(format!("test:block_{index}")))])
        })
        .collect()
}

fn block_section(y: i8, data: Vec<i64>) -> NbtValue {
    NbtValue::compound([
        ("Y", NbtValue::Byte(y)),
        (
            "Palette",
            NbtValue::List {
                kind: TAG_COMPOUND,
                items: block_palette(),
            },
        ),
        ("BlockStates", NbtValue::LongArray(data)),
    ])
}

fn light_only_section(y: i8) -> NbtValue {
    NbtValue::compound([
        ("Y", NbtValue::Byte(y)),
        ("SkyLight", NbtValue::ByteArray(vec![0xFF; LIGHT_BYTES])),
        ("BlockLight", NbtValue::ByteArray(vec![0; LIGHT_BYTES])),
    ])
}

fn compound_list(count: usize) -> NbtValue {
    NbtValue::List {
        kind: TAG_COMPOUND,
        items: (0..count)
            .map(|index| NbtValue::compound([("id", NbtValue::String(format!("test:{index}")))]))
            .collect(),
    }
}

fn chunk(
    profile: &VersionProfile,
    x: i32,
    z: i32,
    sections: Vec<NbtValue>,
    mut biomes: Vec<i32>,
) -> NbtFile {
    if profile.biome_format == minecraft_version::BiomeFormat::Section {
        biomes.resize(1024, 1);
        biomes[64..128].fill(2); // section 1: minecraft:desert
    } else {
        biomes.resize(256, 1);
    }
    let level = NbtValue::compound([
        ("xPos", NbtValue::Int(x)),
        ("zPos", NbtValue::Int(z)),
        ("Status", NbtValue::String("full".to_owned())),
        ("Biomes", NbtValue::IntArray(biomes)),
        (
            "Sections",
            NbtValue::List {
                kind: TAG_COMPOUND,
                items: sections,
            },
        ),
        ("TileEntities", compound_list(2)),
        ("Entities", compound_list(1)),
    ]);
    NbtFile::new(
        "",
        NbtValue::compound([
            ("DataVersion", NbtValue::Int(profile.data_version)),
            ("Level", level),
        ]),
    )
}

fn decode(
    profile: &VersionProfile,
    nbt: NbtFile,
    x: i32,
    z: i32,
) -> Result<DecodedChunk, AnvilError> {
    let summary = RegionChunkSummary {
        x,
        z,
        data_version: profile.data_version,
        status: Some("full".to_owned()),
        y_pos: None,
        sections: 0,
        block_entities: 0,
        entities: 0,
    };
    RegionChunk {
        x,
        z,
        timestamp: 0,
        compression: ChunkCompression::Zlib,
        compressed_bytes: 0,
        nbt_bytes: 0,
        summary,
        nbt,
    }
    .decode(&dimension(), profile)
}

#[test]
fn flattened_empty_proto_chunk_has_no_editor_sections() {
    let profile = profile("1.13.2");
    let mut nbt = chunk(&profile, 0, 0, Vec::new(), Vec::new());
    let level = nbt
        .root
        .as_compound_mut()
        .unwrap()
        .get_mut("Level")
        .unwrap()
        .as_compound_mut()
        .unwrap();
    level.insert("Status".to_owned(), NbtValue::String("empty".to_owned()));
    level.insert("Biomes".to_owned(), NbtValue::IntArray(Vec::new()));
    let decoded = decode(&profile, nbt.clone(), 0, 0).expect("empty proto-chunk");
    assert!(decoded.sections.is_empty());
    assert_eq!(decoded.missing_sections.len(), 16);
    assert_eq!(decoded.status.as_deref(), Some("empty"));

    let level = nbt
        .root
        .as_compound_mut()
        .unwrap()
        .get_mut("Level")
        .unwrap()
        .as_compound_mut()
        .unwrap();
    level.insert(
        "Status".to_owned(),
        NbtValue::String("postprocessed".to_owned()),
    );
    assert!(decode(&profile, nbt, 0, 0).is_err());
}

#[test]
fn flattened_sections_decode_across_the_packing_boundary() {
    for version in ["1.13.2", "1.15.2", "1.16.5", "1.17.1"] {
        let profile = profile(version);
        let padded = expected_packing(&profile);
        let indices = block_indices();
        let data = if padded {
            pack_padded(&indices)
        } else {
            pack_unpadded(&indices)
        };
        assert_eq!(
            data.len(),
            if padded { 342 } else { 320 },
            "{version} block array length"
        );
        let nbt = chunk(
            &profile,
            0,
            0,
            vec![
                light_only_section(-1),
                block_section(0, data.clone()),
                block_section(1, data),
                light_only_section(16),
            ],
            Vec::new(),
        );
        assert!(
            world_io::supports_data_version(profile.data_version),
            "{version} is admitted to the region reader"
        );

        let decoded =
            decode(&profile, nbt, 0, 0).unwrap_or_else(|error| panic!("{version}: {error}"));
        assert_eq!(decoded.sections.len(), 2, "{version}");
        assert_eq!(decoded.status.as_deref(), Some("full"), "{version}");
        assert_eq!(decoded.y_pos, 0, "{version}");
        assert_eq!(decoded.block_entities, 2, "{version}");
        assert_eq!(decoded.entities, 1, "{version}");
        assert_eq!(decoded.missing_sections.len(), 14, "{version}");
        assert_eq!(decoded.mixed_biome_columns, 0, "{version}");

        let local_y = 4i32;
        let expected = format!(
            "test:block_{}",
            ((local_y as usize * 16 + 5) * 16 + 3) % PALETTE_LEN
        );
        assert_eq!(
            decoded.block_at(3, local_y, 5).unwrap().name(),
            expected,
            "{version}"
        );
        assert_eq!(
            decoded.biome_at(0, 0, 0),
            Some("minecraft:plains"),
            "{version}"
        );
        let section_one = if profile.biome_format == minecraft_version::BiomeFormat::Section {
            "minecraft:desert"
        } else {
            "minecraft:plains"
        };
        assert_eq!(decoded.biome_at(0, 20, 0), Some(section_one), "{version}");
    }
}

#[test]
fn padded_and_unpadded_arrays_are_not_interchangeable() {
    let unpadded = profile("1.13.2");
    let padded = profile("1.16.5");
    let indices = block_indices();

    let error = decode(
        &unpadded,
        chunk(
            &unpadded,
            0,
            0,
            vec![block_section(0, pack_padded(&indices))],
            Vec::new(),
        ),
        0,
        0,
    )
    .expect_err("padded array under 1.13");
    assert!(format!("{error}").contains("expected 320"), "{error}");

    let error = decode(
        &padded,
        chunk(
            &padded,
            0,
            0,
            vec![block_section(0, pack_unpadded(&indices))],
            Vec::new(),
        ),
        0,
        0,
    )
    .expect_err("unpadded array under 1.16");
    assert!(format!("{error}").contains("expected 342"), "{error}");
}

#[test]
fn mixed_column_biomes_use_center_sampling_and_are_counted() {
    let profile = profile("1.14.4");
    let indices = block_indices();
    let data = pack_unpadded(&indices);
    let mut biomes = vec![1; 256];
    biomes[0] = 2; // Footprint (0, 0) mixes desert and plains; its center is plains.
    let decoded = decode(
        &profile,
        chunk(
            &profile,
            0,
            0,
            vec![block_section(0, data.clone()), block_section(1, data)],
            biomes,
        ),
        0,
        0,
    )
    .expect("center sampling");
    assert_eq!(decoded.mixed_biome_columns, 1);
    assert_eq!(decoded.biome_at(0, 0, 0), Some("minecraft:plains"));
    assert_eq!(decoded.biome_at(0, 20, 0), Some("minecraft:plains"));
    let expected = format!("test:block_{}", ((4usize * 16 + 5) * 16 + 3) % PALETTE_LEN);
    assert_eq!(decoded.block_at(3, 4, 5).unwrap().name(), expected.as_str());
}

#[test]
fn mixed_columns_patch_only_the_changed_footprint_and_keep_blocks() {
    let profile = profile("1.14.4");
    let indices = block_indices();
    let source = chunk(
        &profile,
        0,
        0,
        vec![
            block_section(0, pack_unpadded(&indices)),
            block_section(1, pack_unpadded(&indices)),
        ],
        {
            let mut biomes = vec![1; 256];
            biomes[0] = 2;
            biomes
        },
    );
    let decoded = decode(&profile, source.clone(), 0, 0).expect("decode source");
    let plains: BiomeState = "minecraft:plains".parse().expect("biome");
    let desert: BiomeState = "minecraft:desert".parse().expect("biome");
    let mut edited: [BiomeSection; 16] =
        std::array::from_fn(|_| BiomeSection::uniform(plains.clone()));
    for section in &mut edited {
        for y in 0..4 {
            assert_eq!(section.set(0, y, 0, &desert), Some(true));
        }
    }

    let patched =
        patch_flattened_chunk_biomes(&source, &profile, &edited, ColumnBiomePolicy::SampleCenter)
            .expect("patch sampled footprint");
    let patched_decoded = decode(&profile, patched, 0, 0).expect("decode patched chunk");
    assert_eq!(patched_decoded.mixed_biome_columns, 0);
    assert_eq!(patched_decoded.biome_at(0, 0, 0), Some("minecraft:desert"));
    assert_eq!(patched_decoded.biome_at(0, 20, 0), Some("minecraft:desert"));
    assert_eq!(
        patched_decoded.block_at(3, 4, 5).unwrap().name(),
        decoded.block_at(3, 4, 5).unwrap().name()
    );
    assert_eq!(
        patched_decoded.block_at(15, 31, 15).unwrap().name(),
        decoded.block_at(15, 31, 15).unwrap().name()
    );
}

#[test]
fn unknown_and_malformed_biomes_are_not_substituted() {
    let v116 = profile("1.16.5");
    let indices = block_indices();
    let mut unknown = vec![1; 1024];
    unknown[0] = 255;
    let error = decode(
        &v116,
        chunk(
            &v116,
            0,
            0,
            vec![block_section(0, pack_padded(&indices))],
            unknown,
        ),
        0,
        0,
    )
    .expect_err("unknown biome id");
    assert!(format!("{error}").contains("255"), "{error}");

    let v113 = profile("1.13.2");
    let mut short = chunk(
        &v113,
        0,
        0,
        vec![block_section(0, pack_unpadded(&indices))],
        Vec::new(),
    );
    let NbtValue::Compound(root) = &mut short.root else {
        unreachable!("fixture root")
    };
    let NbtValue::Compound(level) = root.get_mut("Level").expect("Level") else {
        unreachable!("fixture Level")
    };
    level.insert("Biomes".to_owned(), NbtValue::IntArray(vec![1; 255]));
    let error = decode(&v113, short, 0, 0).expect_err("short column array");
    assert!(format!("{error}").contains("256"), "{error}");
}

#[test]
fn malformed_sections_are_rejected() {
    let profile = profile("1.16.5");
    let indices = block_indices();
    let data = pack_padded(&indices);

    let duplicate = chunk(
        &profile,
        0,
        0,
        vec![
            block_section(0, data.clone()),
            block_section(0, data.clone()),
        ],
        Vec::new(),
    );
    let error = decode(&profile, duplicate, 0, 0).expect_err("duplicate section");
    assert!(
        format!("{error}").contains("duplicate section Y 0"),
        "{error}"
    );

    let outside = chunk(
        &profile,
        0,
        0,
        vec![block_section(17, data.clone())],
        Vec::new(),
    );
    let error = decode(&profile, outside, 0, 0).expect_err("outside section");
    assert!(
        format!("{error}").contains("outside the chunk range"),
        "{error}"
    );

    let mut palette_overflow = block_indices();
    palette_overflow[0] = PALETTE_LEN as u16;
    let overflow = chunk(
        &profile,
        0,
        0,
        vec![block_section(0, pack_padded(&palette_overflow))],
        Vec::new(),
    );
    let error = decode(&profile, overflow, 0, 0).expect_err("palette overflow");
    assert!(format!("{error}").contains("outside a palette"), "{error}");

    let mismatch = chunk(&profile, 0, 0, vec![block_section(0, data)], Vec::new());
    let error = decode(&profile, mismatch, 1, 0).expect_err("coordinate mismatch");
    assert!(format!("{error}").contains("xPos/zPos"), "{error}");
}

#[test]
fn light_only_sections_must_carry_complete_light_arrays() {
    let profile = profile("1.16.5");
    let indices = block_indices();
    let mut light = light_only_section(-1);
    let NbtValue::Compound(map) = &mut light else {
        unreachable!("fixture section")
    };
    map.insert(
        "SkyLight".to_owned(),
        NbtValue::ByteArray(vec![0; LIGHT_BYTES - 1]),
    );
    let nbt = chunk(
        &profile,
        0,
        0,
        vec![light, block_section(0, pack_padded(&indices))],
        Vec::new(),
    );
    let error = decode(&profile, nbt, 0, 0).expect_err("short light array");
    assert!(
        format!("{error}").contains("light-only section -1"),
        "{error}"
    );
}
