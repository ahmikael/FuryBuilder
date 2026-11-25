//! Tests for Minecraft 1.21.4 section palettes, packed indices and biomes.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use flate2::write::ZlibEncoder;
use flate2::Compression;
use minecraft_version::{builtin_profiles, default_profile, profile_for_version};
use world_domain::{BlockState, DimensionKind};
use world_io::anvil::{
    region_file_name, AnvilError, ChunkCompression, JavaWorldDimension, RegionChunk,
    RegionChunkSummary, RegionFile,
};
use world_io::nbt::{self, NbtFile, NbtValue, TAG_COMPOUND, TAG_STRING};

const TARGET: i32 = 4189;

fn simple(name: &str) -> NbtValue {
    NbtValue::compound([("Name", NbtValue::String(name.to_owned()))])
}

fn with_properties(name: &str, properties: &[(&str, &str)]) -> NbtValue {
    let properties = properties
        .iter()
        .map(|(key, value)| ((*key).to_owned(), NbtValue::String((*value).to_owned())))
        .collect::<BTreeMap<_, _>>();
    NbtValue::compound([
        ("Name", NbtValue::String(name.to_owned())),
        ("Properties", NbtValue::Compound(properties)),
    ])
}

fn block_states(palette: Vec<NbtValue>, kind: u8, data: Option<Vec<i64>>) -> NbtValue {
    let mut map = BTreeMap::new();
    map.insert(
        "palette".to_owned(),
        NbtValue::List {
            kind,
            items: palette,
        },
    );
    if let Some(data) = data {
        map.insert("data".to_owned(), NbtValue::LongArray(data));
    }
    NbtValue::Compound(map)
}

fn biomes(palette: &[&str], data: Option<Vec<i64>>) -> NbtValue {
    let items = palette
        .iter()
        .map(|name| NbtValue::String((*name).to_owned()))
        .collect();
    let mut map = BTreeMap::new();
    map.insert(
        "palette".to_owned(),
        NbtValue::List {
            kind: TAG_STRING,
            items,
        },
    );
    if let Some(data) = data {
        map.insert("data".to_owned(), NbtValue::LongArray(data));
    }
    NbtValue::Compound(map)
}

fn section(y: i8, blocks: NbtValue, biomes: NbtValue) -> NbtValue {
    NbtValue::compound([
        ("Y", NbtValue::Byte(y)),
        ("block_states", blocks),
        ("biomes", biomes),
    ])
}

fn section_without_blocks(y: i8, biomes: NbtValue) -> NbtValue {
    NbtValue::compound([("Y", NbtValue::Byte(y)), ("biomes", biomes)])
}

fn section_without_biomes(y: i8, blocks: NbtValue) -> NbtValue {
    NbtValue::compound([("Y", NbtValue::Byte(y)), ("block_states", blocks)])
}

fn chunk_root(x: i32, z: i32, y_pos: i32, sections: Option<Vec<NbtValue>>) -> NbtFile {
    let mut root = BTreeMap::new();
    root.insert("DataVersion".to_owned(), NbtValue::Int(TARGET));
    root.insert("xPos".to_owned(), NbtValue::Int(x));
    root.insert("zPos".to_owned(), NbtValue::Int(z));
    root.insert("yPos".to_owned(), NbtValue::Int(y_pos));
    root.insert(
        "Status".to_owned(),
        NbtValue::String("minecraft:full".to_owned()),
    );
    if let Some(sections) = sections {
        root.insert(
            "sections".to_owned(),
            NbtValue::List {
                kind: TAG_COMPOUND,
                items: sections,
            },
        );
    }
    NbtFile::new("", NbtValue::Compound(root))
}

fn overworld() -> JavaWorldDimension {
    JavaWorldDimension {
        id: "minecraft:overworld".to_owned(),
        kind: DimensionKind::Overworld,
        min_y: -64,
        height: 384,
        folder: PathBuf::new(),
        region_folder: PathBuf::new(),
        region_files: 0,
    }
}

fn dimension(id: &str, kind: DimensionKind, min_y: i32, height: i32) -> JavaWorldDimension {
    JavaWorldDimension {
        id: id.to_owned(),
        kind,
        min_y,
        height,
        folder: PathBuf::new(),
        region_folder: PathBuf::new(),
        region_files: 0,
    }
}

fn region_chunk(nbt: NbtFile, x: i32, z: i32, y_pos: i32, section_count: usize) -> RegionChunk {
    let data_version = nbt.root.as_compound().unwrap()["DataVersion"]
        .as_int()
        .unwrap();
    RegionChunk {
        x,
        z,
        timestamp: 0,
        compression: ChunkCompression::Zlib,
        compressed_bytes: 0,
        nbt_bytes: 0,
        summary: RegionChunkSummary {
            x,
            z,
            data_version,
            status: Some("minecraft:full".to_owned()),
            y_pos: Some(y_pos),
            sections: section_count,
            block_entities: 0,
            entities: 0,
        },
        nbt,
    }
}

fn decode(
    nbt: NbtFile,
    x: i32,
    z: i32,
    y_pos: i32,
    section_count: usize,
    dimension: &JavaWorldDimension,
) -> Result<world_io::java_chunk::DecodedChunk, AnvilError> {
    region_chunk(nbt, x, z, y_pos, section_count).decode(dimension, &default_profile())
}

fn decode_error(nbt: NbtFile) -> AnvilError {
    decode(nbt, 0, 0, -4, 1, &overworld()).expect_err("fixture must be rejected")
}

fn pack_indices(indices: &[u16], bits: u8) -> Vec<i64> {
    assert!(bits > 0 && bits <= 12);
    let values_per_long = 64 / bits as usize;
    let mut data = vec![0i64; indices.len().div_ceil(values_per_long)];
    for (index, value) in indices.iter().enumerate() {
        let word = index / values_per_long;
        let shift = (index % values_per_long) * bits as usize;
        data[word] |= (*value as i64) << shift;
    }
    data
}

fn palette_strings(count: usize, prefix: &str) -> Vec<String> {
    (0..count)
        .map(|index| format!("minecraft:{prefix}_{index}"))
        .collect()
}

fn zlib(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes).expect("zlib write");
    encoder.finish().expect("zlib finish")
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "furybuilder-chunk-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create temp directory");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn write_region(dir: &Path, nbt: &NbtFile, x: i32, z: i32) -> PathBuf {
    let raw = nbt::write_uncompressed(nbt).expect("encode chunk");
    let compressed = zlib(&raw);
    let index = (x.rem_euclid(32) + z.rem_euclid(32) * 32) as usize;
    let sectors = (compressed.len() + 5).div_ceil(4096);
    assert!(sectors < 128);
    let mut file = vec![0u8; 8192];
    let base = index * 4;
    file[base] = 0;
    file[base + 1] = 0;
    file[base + 2] = 2;
    file[base + 3] = sectors as u8;
    file.extend_from_slice(&((compressed.len() as u32 + 1).to_be_bytes()));
    file.push(2);
    file.extend_from_slice(&compressed);
    file.resize(8192 + sectors * 4096, 0);
    let path = dir.join(region_file_name(x.div_euclid(32), z.div_euclid(32)));
    fs::write(&path, file).expect("write region");
    path
}

#[test]
fn decodes_block_palette_properties_biomes_and_missing_sections() {
    let mut block_palette = vec![
        simple("minecraft:air"),
        with_properties(
            "minecraft:oak_stairs",
            &[
                ("facing", "east"),
                ("half", "bottom"),
                ("waterlogged", "false"),
            ],
        ),
    ];
    for index in 0..18 {
        block_palette.push(simple(&format!("minecraft:test_block_{index}")));
    }
    let block_indices = (0..4096)
        .map(|index| (index % block_palette.len()) as u16)
        .collect::<Vec<_>>();
    let block_data = pack_indices(&block_indices, 5);
    let biome_cells = (0..64).map(|index| (index % 3) as u16).collect::<Vec<_>>();
    let biome_data = pack_indices(&biome_cells, 2);
    let nbt = chunk_root(
        0,
        0,
        -4,
        Some(vec![section(
            -4,
            block_states(block_palette, TAG_COMPOUND, Some(block_data)),
            biomes(
                &["minecraft:plains", "minecraft:forest", "minecraft:desert"],
                Some(biome_data),
            ),
        )]),
    );

    let decoded = decode(nbt, 0, 0, -4, 1, &overworld()).expect("decode chunk");
    assert_eq!(decoded.data_version, TARGET);
    assert_eq!(decoded.y_pos, -4);
    assert_eq!(decoded.status.as_deref(), Some("minecraft:full"));
    assert_eq!(decoded.missing_sections.len(), 23);
    assert!(decoded.missing_sections.contains(&-3));
    assert_eq!(decoded.sections.len(), 1);

    let section = decoded.section(-4).expect("section -4");
    assert_eq!(section.blocks.palette_len(), 20);
    assert_eq!(section.blocks.non_air_count(), 3891);
    let stairs: BlockState = "minecraft:oak_stairs[facing=east,half=bottom,waterlogged=false]"
        .parse()
        .expect("stairs state");
    assert_eq!(section.blocks.get(1, 2, 3), Some(&stairs));
    assert_eq!(decoded.block_at(1, -62, 3), Some(&stairs));
    assert_eq!(section.biomes.palette().len(), 3);
    assert_eq!(section.biomes.get(0, 0, 0), Some("minecraft:plains"));
    assert_eq!(section.biomes.get(1, 0, 0), Some("minecraft:forest"));
    assert_eq!(decoded.biome_at(4, -64, 0), Some("minecraft:forest"));
    assert_eq!(
        section.biomes.biome_at_block(9, 1, 1),
        Some("minecraft:desert")
    );
    assert_eq!(decoded.biome_at(9, -63, 1), Some("minecraft:desert"));

    let chunk = decoded.to_chunk();
    assert_eq!(chunk.get_local(1, 3, -62), stairs);
    assert_eq!(chunk.sections().len(), 1);
}

#[test]
fn decodes_single_state_and_treats_absent_sections_as_missing() {
    let nbt = chunk_root(
        0,
        0,
        -4,
        Some(vec![
            section(
                -4,
                block_states(vec![simple("minecraft:stone")], TAG_COMPOUND, None),
                biomes(&["minecraft:plains"], None),
            ),
            section_without_blocks(0, biomes(&["minecraft:the_void"], None)),
        ]),
    );
    let decoded = decode(nbt, 0, 0, -4, 2, &overworld()).expect("decode chunk");
    assert_eq!(
        decoded
            .section(-4)
            .expect("solid section")
            .blocks
            .non_air_count(),
        4096
    );
    assert!(decoded.section(0).expect("empty section").blocks.is_empty());
    assert!(decoded.block_at(0, 0, 0).expect("air block").is_air());
    assert_eq!(decoded.biome_at(0, 0, 0), Some("minecraft:the_void"));
    assert_eq!(decoded.block_at(0, 200, 0), None);
    assert_eq!(decoded.missing_sections.len(), 22);
    let chunk = decoded.to_chunk();
    assert_eq!(
        chunk.sections().len(),
        2,
        "a section with biome data but no blocks is kept"
    );
    assert_eq!(
        chunk.get_biome(0, 0, 0).map(|biome| biome.as_str()),
        Some("minecraft:the_void")
    );
    assert_eq!(
        chunk.get_local(15, 15, -64),
        "minecraft:stone".parse::<BlockState>().unwrap()
    );
}

#[test]
fn decodes_six_bit_biome_palette() {
    let palette = palette_strings(33, "test_biome");
    let palette_refs = palette.iter().map(String::as_str).collect::<Vec<_>>();
    let cells = (0..64)
        .map(|index| ((index * 7) % 33) as u16)
        .collect::<Vec<_>>();
    let data = pack_indices(&cells, 6);
    let nbt = chunk_root(
        0,
        0,
        -4,
        Some(vec![section(
            -4,
            block_states(vec![simple("minecraft:stone")], TAG_COMPOUND, None),
            biomes(&palette_refs, Some(data)),
        )]),
    );
    let decoded = decode(nbt, 0, 0, -4, 1, &overworld()).expect("decode chunk");
    let biomes = &decoded.section(-4).expect("section").biomes;
    assert_eq!(biomes.palette().len(), 33);
    assert_eq!(biomes.get(3, 3, 3), Some(palette[12].as_str()));
    assert_eq!(decoded.biome_at(15, -49, 15), Some(palette[12].as_str()));
}

#[test]
fn rejects_corrupt_block_and_biome_palettes() {
    let two = vec![simple("minecraft:air"), simple("minecraft:stone")];

    let error = decode_error(chunk_root(
        0,
        0,
        -4,
        Some(vec![section(
            -4,
            block_states(two.clone(), TAG_COMPOUND, Some(vec![0; 1])),
            biomes(&["minecraft:plains"], None),
        )]),
    ));
    assert!(format!("{error}").contains("expected 256"), "{error}");

    let mut indices = vec![0u16; 4096];
    indices[0] = 2;
    let error = decode_error(chunk_root(
        0,
        0,
        -4,
        Some(vec![section(
            -4,
            block_states(two.clone(), TAG_COMPOUND, Some(pack_indices(&indices, 4))),
            biomes(&["minecraft:plains"], None),
        )]),
    ));
    assert!(format!("{error}").contains("outside a palette"), "{error}");

    let error = decode_error(chunk_root(
        0,
        0,
        -4,
        Some(vec![section(
            -4,
            block_states(
                vec![simple("minecraft:stone")],
                TAG_COMPOUND,
                Some(vec![0; 1]),
            ),
            biomes(&["minecraft:plains"], None),
        )]),
    ));
    assert!(format!("{error}").contains("non-empty data"), "{error}");

    let error = decode_error(chunk_root(
        0,
        0,
        -4,
        Some(vec![section(
            -4,
            block_states(two.clone(), TAG_COMPOUND, None),
            biomes(&["minecraft:plains"], None),
        )]),
    ));
    assert!(
        format!("{error}").contains("block data is missing"),
        "{error}"
    );

    let error = decode_error(chunk_root(
        0,
        0,
        -4,
        Some(vec![section(
            -4,
            block_states(vec![simple("minecraft:stone")], TAG_COMPOUND, None),
            biomes(
                &["minecraft:plains", "minecraft:forest", "minecraft:desert"],
                Some(vec![0; 1]),
            ),
        )]),
    ));
    assert!(format!("{error}").contains("biome data"), "{error}");

    let error = decode_error(chunk_root(
        0,
        0,
        -4,
        Some(vec![section(
            -4,
            block_states(vec![simple("minecraft:stone")], TAG_COMPOUND, None),
            NbtValue::Compound(BTreeMap::new()),
        )]),
    ));
    assert!(format!("{error}").contains("no palette"), "{error}");

    let error = decode_error(chunk_root(
        0,
        0,
        -4,
        Some(vec![section(
            -4,
            block_states(vec![simple("minecraft:stone")], TAG_COMPOUND, None),
            biomes(&["Bad Biome"], None),
        )]),
    ));
    assert!(format!("{error}").contains("invalid biome id"), "{error}");

    let error = decode_error(chunk_root(
        0,
        0,
        -4,
        Some(vec![section_without_biomes(
            -4,
            block_states(vec![simple("minecraft:stone")], TAG_COMPOUND, None),
        )]),
    ));
    assert!(format!("{error}").contains("no biomes"), "{error}");
}

#[test]
fn validates_section_heights_and_dimension_bounds() {
    let stone = section(
        -4,
        block_states(vec![simple("minecraft:stone")], TAG_COMPOUND, None),
        biomes(&["minecraft:plains"], None),
    );

    let error = decode_error(chunk_root(
        0,
        0,
        -4,
        Some(vec![section(
            -5,
            block_states(vec![simple("minecraft:stone")], TAG_COMPOUND, None),
            biomes(&["minecraft:plains"], None),
        )]),
    ));
    assert!(
        format!("{error}").contains("outside the chunk range"),
        "{error}"
    );

    let error = decode_error(chunk_root(0, 0, -4, Some(vec![stone.clone(), stone])));
    assert!(format!("{error}").contains("duplicate section"), "{error}");

    let error = decode_error(chunk_root(0, 0, -3, Some(Vec::new())));
    assert!(format!("{error}").contains("yPos"), "{error}");

    let error = decode_error(chunk_root(0, 0, -4, None));
    assert!(format!("{error}").contains("no sections list"), "{error}");

    let too_many = (-4..23)
        .map(|y| {
            section(
                y as i8,
                block_states(vec![simple("minecraft:stone")], TAG_COMPOUND, None),
                biomes(&["minecraft:plains"], None),
            )
        })
        .collect::<Vec<_>>();
    let error = decode_error(chunk_root(0, 0, -4, Some(too_many)));
    assert!(format!("{error}").contains("room for 26"), "{error}");

    let small = dimension("custom:small", DimensionKind::Custom, 0, 32);
    let nbt = chunk_root(
        0,
        0,
        0,
        Some(vec![section(
            0,
            block_states(vec![simple("minecraft:stone")], TAG_COMPOUND, None),
            biomes(&["minecraft:plains"], None),
        )]),
    );
    let decoded = decode(nbt, 0, 0, 0, 1, &small).expect("small dimension");
    assert_eq!(decoded.missing_sections, vec![1]);

    let nbt = chunk_root(
        0,
        0,
        0,
        Some(vec![section(
            2,
            block_states(vec![simple("minecraft:stone")], TAG_COMPOUND, None),
            biomes(&["minecraft:plains"], None),
        )]),
    );
    let error = decode(nbt, 0, 0, 0, 1, &small).expect_err("section 2 is outside height 32");
    assert!(
        format!("{error}").contains("outside the chunk range 0..1"),
        "{error}"
    );
}

#[test]
fn rejects_invalid_light_only_section_records() {
    let outside_light_range = NbtValue::compound([
        ("Y", NbtValue::Byte(-6)),
        ("SkyLight", NbtValue::ByteArray(vec![0x11; 2048])),
    ]);
    let malformed_light = NbtValue::compound([
        ("Y", NbtValue::Byte(-5)),
        ("SkyLight", NbtValue::ByteArray(vec![0x11])),
    ]);
    let halo = NbtValue::compound([
        ("Y", NbtValue::Byte(-5)),
        ("SkyLight", NbtValue::ByteArray(vec![0x11; 2048])),
    ]);
    for (sections, expected) in [
        (vec![outside_light_range], "outside the chunk range"),
        (vec![malformed_light], "not a 2048-byte array"),
        (vec![halo.clone(), halo], "duplicate section Y -5"),
    ] {
        let error = decode_error(chunk_root(0, 0, -4, Some(sections)));
        assert!(error.to_string().contains(expected), "{error}");
    }
}

#[test]
fn decodes_negative_chunk_and_region_coordinates() {
    let nbt = chunk_root(
        -1,
        -1,
        -4,
        Some(vec![section(
            -4,
            block_states(vec![simple("minecraft:stone")], TAG_COMPOUND, None),
            biomes(&["minecraft:plains"], None),
        )]),
    );
    let decoded = decode(nbt.clone(), -1, -1, -4, 1, &overworld()).expect("direct chunk");
    assert_eq!((decoded.x, decoded.z), (-1, -1));
    let stone = "minecraft:stone".parse::<BlockState>().unwrap();
    assert_eq!(decoded.block_at(15, -64, 15), Some(&stone));
    assert_eq!(decoded.biome_at(15, -64, 15), Some("minecraft:plains"));

    let dir = TempDir::new("negative-region");
    let path = write_region(dir.path(), &nbt, -1, -1);
    let chunk = RegionFile::open(&path)
        .expect("open negative region")
        .read_chunk(-1, -1)
        .expect("read negative region chunk");
    let decoded = chunk
        .decode(&overworld(), &default_profile())
        .expect("decode negative region chunk");
    assert_eq!((decoded.x, decoded.z), (-1, -1));
    assert_eq!(decoded.block_at(0, -64, 0), Some(&stone));
}

#[test]
fn decodes_section_palettes_with_each_exact_modern_profile_without_admitting_world_io() {
    let mut blocks = vec![0; 4096];
    blocks[12] = 16; // First entry of the second padded long at 5 bits.
    let palette = (0..17)
        .map(|i| with_properties("example:block", &[("variant", &i.to_string())]))
        .collect();
    let mut biome_cells = vec![0; 64];
    biome_cells[12] = 2;
    let mut fixture = chunk_root(
        0,
        0,
        -4,
        Some(vec![section(
            -4,
            block_states(palette, TAG_COMPOUND, Some(pack_indices(&blocks, 5))),
            biomes(
                &["minecraft:plains", "minecraft:forest", "example:biome"],
                Some(pack_indices(&biome_cells, 2)),
            ),
        )]),
    );
    let NbtValue::Compound(root) = &mut fixture.root else {
        unreachable!("fixture root");
    };
    root.insert(
        "block_entities".to_owned(),
        NbtValue::List {
            kind: TAG_COMPOUND,
            items: vec![NbtValue::compound([
                ("id", NbtValue::String("example:machine".to_owned())),
                ("opaque", NbtValue::IntArray(vec![7, 8, 9])),
            ])],
        },
    );
    let wrong_profile = default_profile();
    let dim = overworld();
    let mut tested = 0;
    for profile in builtin_profiles() {
        let mut root = fixture.root.as_compound().unwrap().clone();
        root.insert(
            "DataVersion".to_owned(),
            NbtValue::Int(profile.data_version),
        );
        let nbt = NbtFile::new("", NbtValue::Compound(root));
        let region_chunk = region_chunk(nbt, 0, 0, -4, 1);
        let decoded = region_chunk.decode(&dim, profile).unwrap();
        assert_eq!(decoded.data_version, profile.data_version);
        assert_eq!(decoded.block_entities, 1);
        assert_eq!(
            decoded
                .block_at(12, -64, 0)
                .map(ToString::to_string)
                .as_deref(),
            Some("example:block[variant=16]"),
        );
        assert_eq!(decoded.biome_at(0, -64, 12), Some("example:biome"));
        if !profile.capabilities.read {
            assert!(!world_io::supports_data_version(profile.data_version));
            assert_eq!(
                region_chunk.decode(&dim, &wrong_profile).unwrap_err(),
                AnvilError::UnsupportedDataVersion {
                    found: profile.data_version,
                    supported: vec![wrong_profile.data_version],
                }
            );
        }
        tested += 1;
    }
    assert_eq!(tested, 27);

    let profile = profile_for_version(&"1.18.2".parse().unwrap()).unwrap();
    let NbtValue::Compound(root) = &mut fixture.root else {
        unreachable!("fixture root");
    };
    root.insert("DataVersion".into(), NbtValue::Int(profile.data_version));
    let dir = TempDir::new("unverified-region");
    let path = write_region(dir.path(), &fixture, 0, 0);
    let region = RegionFile::open(path).unwrap();
    assert!(matches!(
        region.read_chunk(0, 0),
        Err(AnvilError::UnsupportedDataVersion { found, .. }) if found == profile.data_version
    ));
}
