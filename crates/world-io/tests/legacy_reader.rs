//! McRegion and pre-1.8 Anvil reader tests (1.0–1.7.10).

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use flate2::write::{GzEncoder, ZlibEncoder};
use flate2::Compression;
use minecraft_version::{builtin_profiles, profile_for_version, VersionProfile, WorldLayout};
use world_domain::SECTION_SIZE;
use world_io::anvil::{AnvilError, LegacyLevelDat, LegacyWorld};
use world_io::nbt::{self, NbtFile, NbtValue, TAG_COMPOUND};

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "furybuilder-legacy-{label}-{}-{}",
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

fn profile_of(version: &str) -> VersionProfile {
    profile_for_version(&version.parse().expect("version")).expect(version)
}

fn extension(profile: &VersionProfile) -> &'static str {
    match profile.world_layout {
        WorldLayout::McRegion => "mcr",
        WorldLayout::ClassicAnvil => "mca",
        _ => panic!("not a legacy profile"),
    }
}

fn level_format(profile: &VersionProfile) -> i32 {
    match profile.world_layout {
        WorldLayout::McRegion => 19132,
        WorldLayout::ClassicAnvil => 19133,
        _ => panic!("not a legacy profile"),
    }
}

fn level_dat_bytes(format_version: i32) -> Vec<u8> {
    let file = NbtFile::new(
        "",
        NbtValue::compound([(
            "Data",
            NbtValue::compound([
                ("Version", NbtValue::Int(format_version)),
                ("LevelName", NbtValue::String("Legacy test".to_owned())),
                ("LastPlayed", NbtValue::Long(1_700_000_000_000)),
                ("RandomSeed", NbtValue::Long(42)),
            ]),
        )]),
    );
    nbt::write_gzip(&file).expect("encode level.dat")
}

fn set_nibble(bytes: &mut [u8], index: usize, value: u8) {
    let byte = &mut bytes[index >> 1];
    if index & 1 == 0 {
        *byte = (*byte & 0xf0) | (value & 0x0f);
    } else {
        *byte = (*byte & 0x0f) | ((value & 0x0f) << 4);
    }
}

fn height(profile: &VersionProfile) -> usize {
    profile.dimension_profiles[0].height as usize
}

fn chunk_nbt(
    profile: &VersionProfile,
    x: i32,
    z: i32,
    blocks: &[(u8, u8, u8, u16, u8)],
    biomes: &[u8; 256],
) -> NbtFile {
    let height = height(profile);
    let blocks_len = 16 * 16 * height;
    let mut block_ids = vec![0u8; blocks_len];
    let mut data = vec![0u8; blocks_len / 2];
    for (local_x, y, local_z, id, metadata) in blocks {
        assert!((*y as usize) < height);
        let index = (*local_x as usize * 16 + *local_z as usize) * height + *y as usize;
        block_ids[index] = (*id & 0xff) as u8;
        set_nibble(&mut data, index, *metadata);
    }
    let mut level = BTreeMap::new();
    level.insert("xPos".to_owned(), NbtValue::Int(x));
    level.insert("zPos".to_owned(), NbtValue::Int(z));
    level.insert("TerrainPopulated".to_owned(), NbtValue::Byte(1));
    level.insert("LastUpdate".to_owned(), NbtValue::Long(1234));
    level.insert("Blocks".to_owned(), NbtValue::ByteArray(block_ids));
    level.insert("Data".to_owned(), NbtValue::ByteArray(data));
    level.insert(
        "SkyLight".to_owned(),
        NbtValue::ByteArray(vec![0xff; blocks_len / 2]),
    );
    level.insert(
        "BlockLight".to_owned(),
        NbtValue::ByteArray(vec![0; blocks_len / 2]),
    );
    level.insert("HeightMap".to_owned(), NbtValue::ByteArray(vec![0; 256]));
    level.insert("Biomes".to_owned(), NbtValue::ByteArray(biomes.to_vec()));
    level.insert(
        "TileEntities".to_owned(),
        NbtValue::List {
            kind: TAG_COMPOUND,
            items: vec![NbtValue::compound([(
                "id",
                NbtValue::String("minecraft:chest".to_owned()),
            )])],
        },
    );
    level.insert(
        "Entities".to_owned(),
        NbtValue::List {
            kind: TAG_COMPOUND,
            items: vec![NbtValue::compound([(
                "id",
                NbtValue::String("minecraft:item".to_owned()),
            )])],
        },
    );
    level.insert(
        "UnknownLevelTag".to_owned(),
        NbtValue::String("preserve".to_owned()),
    );
    NbtFile::new(
        "",
        NbtValue::compound([
            ("Level", NbtValue::Compound(level)),
            (
                "UnknownRootTag",
                NbtValue::compound([("text", NbtValue::String("preserve".to_owned()))]),
            ),
        ]),
    )
}

fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes).expect("gzip write");
    encoder.finish().expect("gzip finish")
}

fn zlib(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes).expect("zlib write");
    encoder.finish().expect("zlib finish")
}

fn region_bytes(chunks: &[(i32, i32, NbtFile)], compression: &str) -> Vec<u8> {
    let mut records = Vec::new();
    for (_, _, file) in chunks {
        let raw = nbt::write_uncompressed(file).expect("encode chunk");
        let payload = if compression == "gzip" {
            gzip(&raw)
        } else {
            zlib(&raw)
        };
        let record_len = 5 + payload.len();
        records.push((payload, record_len.div_ceil(4096)));
    }
    let total_sectors = 2 + records.iter().map(|(_, sectors)| *sectors).sum::<usize>();
    let mut file = vec![0u8; total_sectors * 4096];
    let mut offset = 2usize;
    for ((x, z, _), (payload, sectors)) in chunks.iter().zip(records.iter()) {
        let index = x.rem_euclid(32) + z.rem_euclid(32) * 32;
        let base = index as usize * 4;
        file[base] = ((offset >> 16) & 0xff) as u8;
        file[base + 1] = ((offset >> 8) & 0xff) as u8;
        file[base + 2] = (offset & 0xff) as u8;
        file[base + 3] = *sectors as u8;
        let timestamp = 1_700_000_000i32.to_be_bytes();
        file[4096 + base..4096 + base + 4].copy_from_slice(&timestamp);
        let start = offset * 4096;
        file[start..start + 4].copy_from_slice(&((payload.len() as u32 + 1).to_be_bytes()));
        file[start + 4] = if compression == "gzip" { 1 } else { 2 };
        file[start + 5..start + 5 + payload.len()].copy_from_slice(payload);
        offset += sectors;
    }
    file
}

fn write_world(root: &Path, profile: &VersionProfile, chunks: &[(i32, i32, NbtFile)]) {
    fs::create_dir_all(root.join("region")).expect("create region");
    fs::write(
        root.join("level.dat"),
        level_dat_bytes(level_format(profile)),
    )
    .expect("write level.dat");
    let extension = extension(profile);
    let compression = if extension == "mcr" { "gzip" } else { "zlib" };
    fs::write(
        root.join("region").join(format!("r.0.0.{extension}")),
        region_bytes(chunks, compression),
    )
    .expect("write region");
}

fn clay_biomes() -> [u8; 256] {
    let mut biomes = [1u8; 256];
    biomes[2 * 16 + 2] = 4;
    biomes
}

#[test]
fn mcregion_world_decodes_xzy_blocks_metadata_and_biomes() {
    let dir = TempDir::new("mcregion");
    let profile = profile_of("1.1");
    let chunk = chunk_nbt(
        &profile,
        0,
        0,
        &[
            (0, 0, 0, 1, 0),
            (15, 127, 15, 7, 0),
            (3, 70, 5, 35, 1),
            (7, 64, 2, 17, 1),
            (1, 63, 1, 8, 0),
        ],
        &clay_biomes(),
    );
    write_world(dir.path(), &profile, &[(0, 0, chunk)]);

    let world = LegacyWorld::open(dir.path(), &profile).expect("open McRegion world");
    assert_eq!(world.level.format_version, 19132);
    assert_eq!(world.level.level_name.as_deref(), Some("Legacy test"));
    assert_eq!(world.level.layout(), WorldLayout::McRegion);
    let dimension = world
        .dimension("minecraft:overworld")
        .expect("overworld dimension");
    assert_eq!((dimension.min_y, dimension.height), (0, 128));
    assert_eq!(
        world.region_coordinates("minecraft:overworld").unwrap(),
        vec![(0, 0)]
    );

    let region = world
        .read_region("minecraft:overworld", 0, 0)
        .expect("read region");
    assert_eq!(region.entries().len(), 1);
    let summary = region.read_chunk(0, 0).expect("read chunk").summary;
    assert_eq!(summary.data_version, -1);
    assert_eq!(summary.status.as_deref(), Some("full"));
    assert_eq!(summary.sections, 8);
    assert_eq!(summary.block_entities, 1);
    assert_eq!(summary.entities, 1);

    let chunk = region.read_chunk(0, 0).expect("read chunk");
    let decoded = world
        .decode_chunk("minecraft:overworld", &chunk)
        .expect("decode chunk");
    assert_eq!(decoded.sections.len(), 8);
    assert_eq!(decoded.data_version, -1);
    assert_eq!(decoded.mixed_biome_columns, 1);
    assert_eq!(
        decoded.block_at(0, 0, 0).map(|state| state.name()),
        Some("minecraft:stone")
    );
    assert_eq!(
        decoded.block_at(15, 127, 15).map(|state| state.name()),
        Some("minecraft:bedrock")
    );
    assert_eq!(
        decoded.block_at(3, 70, 5).map(|state| state.to_string()),
        Some("minecraft:orange_wool".to_owned())
    );
    let log = decoded.block_at(7, 64, 2).expect("spruce log");
    assert_eq!(log.name(), "minecraft:spruce_log");
    assert_eq!(log.properties().get("axis").map(String::as_str), Some("y"));
    let water = decoded.block_at(1, 63, 1).expect("water");
    assert_eq!(water.name(), "minecraft:water");
    assert_eq!(decoded.biome_at(0, 0, 0), Some("minecraft:forest"));
    assert_eq!(decoded.biome_at(4, 0, 4), Some("minecraft:plains"));

    let root = chunk.nbt.root.as_compound().expect("root");
    assert!(root.contains_key("UnknownRootTag"));
    let level = root
        .get("Level")
        .and_then(NbtValue::as_compound)
        .expect("Level");
    assert_eq!(
        level.get("UnknownLevelTag"),
        Some(&NbtValue::String("preserve".to_owned()))
    );
}

#[test]
fn early_anvil_world_uses_256_height_and_mca() {
    let dir = TempDir::new("early-anvil");
    let profile = profile_of("1.7.10");
    let chunk = chunk_nbt(
        &profile,
        0,
        0,
        &[(0, 0, 0, 1, 0), (2, 128, 3, 3, 0), (15, 255, 15, 7, 0)],
        &[1u8; 256],
    );
    write_world(dir.path(), &profile, &[(0, 0, chunk)]);

    let world = LegacyWorld::open(dir.path(), &profile).expect("open early Anvil world");
    assert_eq!(world.level.format_version, 19133);
    assert_eq!(world.level.layout(), WorldLayout::ClassicAnvil);
    let dimension = world
        .dimension("minecraft:overworld")
        .expect("overworld dimension");
    assert_eq!((dimension.min_y, dimension.height), (0, 256));
    let region = world
        .read_region("minecraft:overworld", 0, 0)
        .expect("read region");
    let summary = region.read_chunk(0, 0).expect("read chunk").summary;
    assert_eq!(summary.sections, 16);
    let chunk = region.read_chunk(0, 0).expect("read chunk");
    let decoded = world
        .decode_chunk("minecraft:overworld", &chunk)
        .expect("decode chunk");
    assert_eq!(decoded.sections.len(), 16);
    assert_eq!(
        decoded.block_at(15, 255, 15).map(|state| state.name()),
        Some("minecraft:bedrock")
    );
    assert_eq!(
        decoded.block_at(2, 128, 3).map(|state| state.name()),
        Some("minecraft:dirt")
    );
}

#[test]
fn reads_every_legacy_layout_boundary() {
    for (version, expected_extension, expected_height) in [
        ("1.0", "mcr", 128),
        ("1.1", "mcr", 128),
        ("1.2.1", "mca", 256),
        ("1.6.4", "mca", 256),
        ("1.7.10", "mca", 256),
    ] {
        let dir = TempDir::new(version);
        let profile = profile_of(version);
        assert_eq!(extension(&profile), expected_extension, "{version}");
        let chunk = chunk_nbt(&profile, 0, 0, &[(0, 0, 0, 1, 0)], &[1u8; 256]);
        write_world(dir.path(), &profile, &[(0, 0, chunk)]);
        let world = LegacyWorld::open(dir.path(), &profile)
            .unwrap_or_else(|error| panic!("{version}: {error}"));
        let dimension = world
            .dimension("minecraft:overworld")
            .unwrap_or_else(|| panic!("{version}: overworld"));
        assert_eq!(dimension.height, expected_height, "{version}");
        let region = world
            .read_region("minecraft:overworld", 0, 0)
            .unwrap_or_else(|error| panic!("{version}: {error}"));
        assert_eq!(
            region
                .read_chunk(0, 0)
                .expect("read chunk")
                .summary
                .sections,
            (expected_height / SECTION_SIZE) as usize,
            "{version}"
        );
    }
}

#[test]
fn rejects_region_extension_and_level_dat_layout_mismatch() {
    let dir = TempDir::new("layout-mismatch");
    let mcregion = profile_of("1.1");
    fs::create_dir_all(dir.path().join("region")).expect("region");
    fs::write(dir.path().join("level.dat"), level_dat_bytes(19133)).expect("level.dat");
    fs::write(
        dir.path().join("region").join("r.0.0.mcr"),
        region_bytes(&[], "gzip"),
    )
    .expect("mcr");
    let error = LegacyWorld::open(dir.path(), &mcregion).expect_err("layout mismatch");
    assert!(matches!(error, AnvilError::UnsupportedLayout(_)), "{error}");

    let dir = TempDir::new("wrong-extension");
    let early_anvil = profile_of("1.7.10");
    fs::create_dir_all(dir.path().join("region")).expect("region");
    fs::write(dir.path().join("level.dat"), level_dat_bytes(19133)).expect("level.dat");
    fs::write(
        dir.path().join("region").join("r.0.0.mcr"),
        region_bytes(&[], "gzip"),
    )
    .expect("mcr");
    let error = LegacyWorld::open(dir.path(), &early_anvil).expect_err("wrong extension");
    assert!(matches!(error, AnvilError::UnsupportedLayout(_)), "{error}");

    let dir = TempDir::new("mixed-extension");
    let profile = profile_of("1.1");
    fs::create_dir_all(dir.path().join("region")).expect("region");
    fs::write(dir.path().join("level.dat"), level_dat_bytes(19132)).expect("level.dat");
    fs::write(
        dir.path().join("region").join("r.0.0.mcr"),
        region_bytes(&[], "gzip"),
    )
    .expect("mcr");
    fs::write(
        dir.path().join("region").join("r.0.0.mca"),
        region_bytes(&[], "zlib"),
    )
    .expect("mca");
    let error = LegacyWorld::open(dir.path(), &profile).expect_err("mixed extensions");
    assert!(matches!(error, AnvilError::UnsupportedLayout(_)), "{error}");
}

#[test]
fn rejects_unknown_block_and_biome_ids() {
    let dir = TempDir::new("unknown-block");
    let profile = profile_of("1.1");
    let chunk = chunk_nbt(&profile, 0, 0, &[(0, 0, 0, 253, 0)], &[1u8; 256]);
    write_world(dir.path(), &profile, &[(0, 0, chunk)]);
    let world = LegacyWorld::open(dir.path(), &profile).expect("open");
    let region = world
        .read_region("minecraft:overworld", 0, 0)
        .expect("read");
    let chunk = region.read_chunk(0, 0).expect("read chunk");
    let error = world
        .decode_chunk("minecraft:overworld", &chunk)
        .expect_err("unknown block");
    assert!(error.to_string().contains("unknown numeric block id 253"));

    let dir = TempDir::new("unknown-biome");
    let mut biomes = [1u8; 256];
    biomes[0] = 200;
    let chunk = chunk_nbt(&profile, 0, 0, &[(0, 0, 0, 1, 0)], &biomes);
    write_world(dir.path(), &profile, &[(0, 0, chunk)]);
    let world = LegacyWorld::open(dir.path(), &profile).expect("open");
    let region = world
        .read_region("minecraft:overworld", 0, 0)
        .expect("read");
    let chunk = region.read_chunk(0, 0).expect("read chunk");
    let error = world
        .decode_chunk("minecraft:overworld", &chunk)
        .expect_err("unknown biome");
    assert!(error.to_string().contains("unknown numeric biome ID 200"));
}

#[test]
fn rejects_malformed_legacy_arrays_and_data_version() {
    let dir = TempDir::new("malformed");
    let profile = profile_of("1.1");
    let mut chunk = chunk_nbt(&profile, 0, 0, &[], &[1u8; 256]);
    if let NbtValue::Compound(root) = &mut chunk.root {
        let level = root
            .get_mut("Level")
            .and_then(NbtValue::as_compound_mut)
            .expect("Level");
        level.insert("Blocks".to_owned(), NbtValue::ByteArray(vec![0; 1]));
    }
    write_world(dir.path(), &profile, &[(0, 0, chunk)]);
    let world = LegacyWorld::open(dir.path(), &profile).expect("open");
    let region = world
        .read_region("minecraft:overworld", 0, 0)
        .expect("read");
    let error = region.read_chunk(0, 0).expect_err("short Blocks");
    assert!(error.to_string().contains("Level.Blocks has 1 bytes"));

    let dir = TempDir::new("missing-biomes");
    let profile = profile_of("1.1");
    let mut chunk = chunk_nbt(&profile, 0, 0, &[], &[1u8; 256]);
    if let NbtValue::Compound(root) = &mut chunk.root {
        let level = root
            .get_mut("Level")
            .and_then(NbtValue::as_compound_mut)
            .expect("Level");
        level.remove("Biomes");
    }
    write_world(dir.path(), &profile, &[(0, 0, chunk)]);
    let world = LegacyWorld::open(dir.path(), &profile).expect("open");
    let region = world
        .read_region("minecraft:overworld", 0, 0)
        .expect("read");
    let error = region.read_chunk(0, 0).expect_err("missing Biomes");
    assert!(error.to_string().contains("no Level.Biomes array"));

    let dir = TempDir::new("data-version");
    let profile = profile_of("1.1");
    let mut chunk = chunk_nbt(&profile, 0, 0, &[], &[1u8; 256]);
    if let NbtValue::Compound(root) = &mut chunk.root {
        root.insert("DataVersion".to_owned(), NbtValue::Int(47));
    }
    write_world(dir.path(), &profile, &[(0, 0, chunk)]);
    let world = LegacyWorld::open(dir.path(), &profile).expect("open");
    let region = world
        .read_region("minecraft:overworld", 0, 0)
        .expect("read");
    let error = region.read_chunk(0, 0).expect_err("DataVersion");
    assert!(error.to_string().contains("carries a DataVersion tag"));
}

#[test]
fn terrain_populated_zero_maps_to_partial_status() {
    let dir = TempDir::new("partial");
    let profile = profile_of("1.7.10");
    let mut chunk = chunk_nbt(&profile, 0, 0, &[(0, 0, 0, 1, 0)], &[1u8; 256]);
    if let NbtValue::Compound(root) = &mut chunk.root {
        let level = root
            .get_mut("Level")
            .and_then(NbtValue::as_compound_mut)
            .expect("Level");
        level.insert("TerrainPopulated".to_owned(), NbtValue::Byte(0));
    }
    write_world(dir.path(), &profile, &[(0, 0, chunk)]);
    let world = LegacyWorld::open(dir.path(), &profile).expect("open");
    let region = world
        .read_region("minecraft:overworld", 0, 0)
        .expect("read");
    let chunk = region.read_chunk(0, 0).expect("read chunk");
    assert_eq!(chunk.summary.status.as_deref(), Some("partial"));
    let decoded = world
        .decode_chunk("minecraft:overworld", &chunk)
        .expect("decode chunk");
    assert_eq!(decoded.status.as_deref(), Some("partial"));
}

#[test]
fn legacy_level_dat_rejects_modern_and_unknown_formats() {
    let dir = TempDir::new("level-dat");
    let path = dir.path().join("level.dat");
    let modern = NbtFile::new(
        "",
        NbtValue::compound([(
            "Data",
            NbtValue::compound([
                ("DataVersion", NbtValue::Int(4189)),
                ("Version", NbtValue::Int(19133)),
            ]),
        )]),
    );
    fs::write(&path, nbt::write_gzip(&modern).unwrap()).expect("level.dat");
    let error = LegacyLevelDat::read(&path).expect_err("modern level.dat");
    assert!(error.to_string().contains("carries a DataVersion"));

    let unknown = NbtFile::new(
        "",
        NbtValue::compound([(
            "Data",
            NbtValue::compound([("Version", NbtValue::Int(12345))]),
        )]),
    );
    fs::write(&path, nbt::write_gzip(&unknown).unwrap()).expect("level.dat");
    let error = LegacyLevelDat::read(&path).expect_err("unknown format");
    assert!(error.to_string().contains("not a pre-1.8 level format"));
}

#[test]
fn all_builtin_legacy_profiles_use_the_pre_1_8_layout() {
    let legacy = builtin_profiles()
        .iter()
        .filter(|profile| {
            profile.minecraft_version.family() == minecraft_version::VersionFamily::Legacy
        })
        .collect::<Vec<_>>();
    assert_eq!(legacy.len(), 28);
    for profile in legacy {
        let expected = match profile.world_layout {
            WorldLayout::McRegion => ("mcr", 19132, 128),
            WorldLayout::ClassicAnvil => ("mca", 19133, 256),
            other => panic!("{}: unexpected layout {other:?}", profile.id),
        };
        assert_eq!(extension(profile), expected.0, "{}", profile.id);
        assert_eq!(level_format(profile), expected.1, "{}", profile.id);
        assert_eq!(height(profile), expected.2, "{}", profile.id);
    }
}

#[allow(dead_code)]
fn assert_send_sync() {
    fn check<T: Send + Sync>() {}
    check::<LegacyWorld>();
    check::<LegacyLevelDat>();
}
