//! Tests for the verified region writer (M8.7).
//!
//! Every test exercises the public writer API. The synthetic world has two allocated
//! chunks in one region so the writer must both replace one chunk and preserve the other
//! byte for byte.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use flate2::write::ZlibEncoder;
use flate2::Compression;
use world_domain::BlockState;
use world_io::nbt::{self, NbtFile, NbtValue, TAG_COMPOUND, TAG_STRING};
use world_io::{
    region_file_name, write_region_from_overlay, write_region_from_overlay_with_policy,
    write_verified_region, DerivedDataPolicy, JavaWorld, RegionFile, RegionWriteError,
    VerifiedChunkReplacement, WorldOverlay,
};

const TARGET: i32 = 4189;
const OVERWORLD: &str = "minecraft:overworld";

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "furybuilder-region-write-{label}-{}-{}",
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

fn level_dat_bytes() -> Vec<u8> {
    let file = NbtFile::new(
        "",
        NbtValue::compound([(
            "Data",
            NbtValue::compound([
                ("DataVersion", NbtValue::Int(TARGET)),
                (
                    "Version",
                    NbtValue::compound([
                        ("Id", NbtValue::Int(TARGET)),
                        ("Name", NbtValue::String("1.21.4".to_owned())),
                    ]),
                ),
                (
                    "LevelName",
                    NbtValue::String("Region write test".to_owned()),
                ),
            ]),
        )]),
    );
    nbt::write_gzip(&file).expect("encode level.dat")
}

fn block_palette(name: &str) -> NbtValue {
    NbtValue::compound([(
        "palette",
        NbtValue::List {
            kind: TAG_COMPOUND,
            items: vec![NbtValue::compound([(
                "Name",
                NbtValue::String(name.to_owned()),
            )])],
        },
    )])
}

fn biome_palette(biome: &str) -> NbtValue {
    NbtValue::compound([(
        "palette",
        NbtValue::List {
            kind: TAG_STRING,
            items: vec![NbtValue::String(biome.to_owned())],
        },
    )])
}

fn section(y: i8, block: &str, biome: &str) -> NbtValue {
    NbtValue::compound([
        ("Y", NbtValue::Byte(y)),
        ("block_states", block_palette(block)),
        ("biomes", biome_palette(biome)),
        ("SkyLight", NbtValue::ByteArray(vec![0x11; 2048])),
        ("BlockLight", NbtValue::ByteArray(vec![0x22; 2048])),
        (
            "UnknownSectionTag",
            NbtValue::compound([("value", NbtValue::Long(99))]),
        ),
    ])
}

fn chunk_nbt(x: i32, z: i32, status: &str, blocks: &str, biome: &str) -> NbtFile {
    NbtFile::new(
        "",
        NbtValue::compound([
            ("DataVersion", NbtValue::Int(TARGET)),
            ("xPos", NbtValue::Int(x)),
            ("zPos", NbtValue::Int(z)),
            ("yPos", NbtValue::Int(-4)),
            ("Status", NbtValue::String(status.to_owned())),
            ("isLightOn", NbtValue::Byte(1)),
            (
                "Heightmaps",
                NbtValue::compound([
                    ("MOTION_BLOCKING", NbtValue::LongArray(vec![1, 2, 3])),
                    ("WORLD_SURFACE", NbtValue::LongArray(vec![4])),
                ]),
            ),
            (
                "PostProcessing",
                NbtValue::List {
                    kind: TAG_COMPOUND,
                    items: vec![NbtValue::compound([(
                        "pos",
                        NbtValue::IntArray(vec![0, -60, 0]),
                    )])],
                },
            ),
            (
                "sections",
                NbtValue::List {
                    kind: TAG_COMPOUND,
                    items: vec![section(-4, blocks, biome)],
                },
            ),
            (
                "block_entities",
                NbtValue::List {
                    kind: TAG_COMPOUND,
                    items: vec![NbtValue::compound([
                        ("id", NbtValue::String("minecraft:chest".to_owned())),
                        ("x", NbtValue::Int(x * 16)),
                        ("y", NbtValue::Int(-64)),
                        ("z", NbtValue::Int(z * 16)),
                        ("keepPacked", NbtValue::Byte(1)),
                        (
                            "Items",
                            NbtValue::List {
                                kind: TAG_COMPOUND,
                                items: vec![NbtValue::compound([
                                    ("Slot", NbtValue::Byte(3)),
                                    ("id", NbtValue::String("minecraft:stone".to_owned())),
                                    ("count", NbtValue::Int(2)),
                                ])],
                            },
                        ),
                        (
                            "example:opaque",
                            NbtValue::compound([
                                ("nan", NbtValue::Float(f32::from_bits(0x7fc0_0042))),
                                ("zero", NbtValue::Double(-0.0)),
                            ]),
                        ),
                    ])],
                },
            ),
            (
                "entities",
                NbtValue::List {
                    kind: TAG_COMPOUND,
                    items: vec![NbtValue::compound([(
                        "id",
                        NbtValue::String("minecraft:item".to_owned()),
                    )])],
                },
            ),
            (
                "UnknownChunkTag",
                NbtValue::compound([("text", NbtValue::String("preserve me".to_owned()))]),
            ),
        ]),
    )
}

fn zlib(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes).expect("zlib write");
    encoder.finish().expect("zlib finish")
}

fn region_bytes(chunks: &[(i32, i32, NbtFile)]) -> Vec<u8> {
    let mut records = Vec::new();
    for (_, _, file) in chunks {
        let raw = nbt::write_uncompressed(file).expect("encode chunk");
        let payload = zlib(&raw);
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
        file[4096 + base..4096 + base + 4].copy_from_slice(&1_700_000_000i32.to_be_bytes());
        let start = offset * 4096;
        file[start..start + 4].copy_from_slice(&((payload.len() as u32 + 1).to_be_bytes()));
        file[start + 4] = 2;
        file[start + 5..start + 5 + payload.len()].copy_from_slice(payload);
        offset += sectors;
    }
    file
}

fn write_world(root: &Path, status: &str) {
    fs::create_dir_all(root.join("region")).expect("region folder");
    fs::write(root.join("level.dat"), level_dat_bytes()).expect("level.dat");
    fs::write(
        root.join("region").join(region_file_name(0, 0)),
        region_bytes(&[
            (
                0,
                0,
                chunk_nbt(0, 0, status, "minecraft:stone", "minecraft:plains"),
            ),
            (
                1,
                0,
                chunk_nbt(1, 0, "minecraft:full", "minecraft:dirt", "minecraft:forest"),
            ),
        ]),
    )
    .expect("region file");
}

fn read_tree(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut stack = vec![PathBuf::new()];
    while let Some(relative) = stack.pop() {
        for entry in fs::read_dir(root.join(&relative)).expect("read directory") {
            let entry = entry.expect("entry");
            let child = relative.join(entry.file_name());
            let metadata = fs::symlink_metadata(entry.path()).expect("metadata");
            if metadata.is_dir() {
                stack.push(child);
            } else {
                files.insert(child, fs::read(entry.path()).expect("read file"));
            }
        }
    }
    files
}

fn raw_record(path: &Path, x: i32, z: i32) -> Vec<u8> {
    let region = RegionFile::open(path).expect("open region");
    let entry = region
        .entries()
        .iter()
        .find(|entry| entry.x == x && entry.z == z)
        .expect("chunk entry");
    let bytes = fs::read(path).expect("read region");
    let start = entry.offset_sectors as usize * 4096;
    let end = start + entry.sector_count as usize * 4096;
    bytes[start..end].to_vec()
}

fn output_dir(root: &Path) -> PathBuf {
    let directory = root.join("out").join("region");
    fs::create_dir_all(&directory).expect("output region folder");
    directory
}

#[test]
fn writes_only_the_edited_chunk_and_preserves_everything_else() {
    let dir = TempDir::new("roundtrip");
    let root = dir.path().join("world");
    write_world(&root, "minecraft:full");
    let before = read_tree(&root);

    let mut overlay = WorldOverlay::open(&root).expect("open overlay");
    overlay
        .set_block(
            OVERWORLD,
            0,
            -64,
            0,
            &"minecraft:gold_block".parse::<BlockState>().unwrap(),
        )
        .expect("set block");
    let destination = output_dir(dir.path()).join(region_file_name(0, 0));
    let report =
        write_region_from_overlay(&overlay, OVERWORLD, 0, 0, &destination).expect("write region");

    assert_eq!(report.chunks, 2);
    assert_eq!(report.replaced, 1);
    assert_eq!(report.preserved, 1);
    assert!(report.untouched_records_byte_identical);
    assert!(report.output_redecoded);
    assert!(report.source_unchanged);
    assert_eq!(report.source_region_sha256.len(), 64);
    assert_eq!(
        report.support.fields,
        ["sections[].block_states", "sections[].biomes"]
    );
    assert_eq!(report.support.data_version, TARGET);
    assert!(!report.support.recomputes_derived_metadata);
    assert_eq!(read_tree(&root), before, "the source world must not change");

    let source_region = root.join("region").join(region_file_name(0, 0));
    assert_eq!(
        raw_record(&destination, 1, 0),
        raw_record(&source_region, 1, 0),
        "untouched chunk records must stay byte-identical"
    );

    let world = JavaWorld::open(&root).expect("reopen source world read-only");
    let written = RegionFile::open(&destination).expect("open written region");
    let edited = written.read_chunk(0, 0).expect("edited chunk");
    assert_eq!(edited.summary.block_entities, 1);
    assert_eq!(edited.summary.entities, 1);
    let decoded = world
        .decode_chunk(OVERWORLD, &edited)
        .expect("decode edited chunk");
    assert_eq!(
        decoded
            .block_at(0, -64, 0)
            .expect("block at 0,-64,0")
            .to_string(),
        "minecraft:gold_block"
    );
    assert_eq!(
        decoded
            .block_at(15, -64, 15)
            .expect("block at 15,-64,15")
            .to_string(),
        "minecraft:stone"
    );
    assert_eq!(
        decoded.biome_at(0, -64, 0).expect("biome"),
        "minecraft:plains"
    );

    let untouched = written.read_chunk(1, 0).expect("untouched chunk");
    assert_eq!(untouched.summary.status.as_deref(), Some("minecraft:full"));
}

#[test]
fn refuses_to_overwrite_an_existing_destination() {
    let dir = TempDir::new("exists");
    let root = dir.path().join("world");
    write_world(&root, "minecraft:full");
    let destination = output_dir(dir.path()).join(region_file_name(0, 0));
    fs::write(&destination, b"do not replace").expect("existing file");
    let before = read_tree(&root);

    let overlay = WorldOverlay::open(&root).expect("open overlay");
    let error = write_region_from_overlay(&overlay, OVERWORLD, 0, 0, &destination)
        .expect_err("destination exists");
    assert!(matches!(error, RegionWriteError::AlreadyExists(_)));
    assert_eq!(error.code(), "conflict");
    assert_eq!(fs::read(&destination).unwrap(), b"do not replace");
    assert_eq!(read_tree(&root), before);
}

#[test]
fn refuses_the_source_file_as_destination() {
    let dir = TempDir::new("source-path");
    let root = dir.path().join("world");
    write_world(&root, "minecraft:full");
    let source = root.join("region").join(region_file_name(0, 0));
    let before = read_tree(&root);
    let overlay = WorldOverlay::open(&root).expect("open overlay");

    let error = write_region_from_overlay(&overlay, OVERWORLD, 0, 0, &source)
        .expect_err("source destination");
    assert_eq!(error.code(), "unsafe_path");
    assert_eq!(read_tree(&root), before);
}

#[test]
fn refuses_a_destination_with_the_wrong_region_name() {
    let dir = TempDir::new("name");
    let root = dir.path().join("world");
    write_world(&root, "minecraft:full");
    let destination = output_dir(dir.path()).join(region_file_name(0, 1));
    let overlay = WorldOverlay::open(&root).expect("open overlay");

    let error =
        write_region_from_overlay(&overlay, OVERWORLD, 0, 0, &destination).expect_err("wrong name");
    assert_eq!(error.code(), "invalid_argument");
    assert!(!destination.exists());
}

#[test]
fn accepts_biome_changes_and_refuses_other_fields() {
    let dir = TempDir::new("fields");
    let root = dir.path().join("world");
    write_world(&root, "minecraft:full");
    let world = JavaWorld::open(&root).expect("open world");
    let source = world
        .read_region(OVERWORLD, 0, 0)
        .expect("region")
        .read_chunk(0, 0)
        .expect("chunk");

    let mut changed_biome = source.nbt.clone();
    if let NbtValue::Compound(root) = &mut changed_biome.root {
        if let Some(NbtValue::List { items, .. }) = root.get_mut("sections") {
            if let NbtValue::Compound(section) = &mut items[0] {
                section.insert("biomes".to_owned(), biome_palette("minecraft:desert"));
            }
        }
    }
    let biome_destination = output_dir(dir.path()).join(region_file_name(0, 0));
    let report = write_verified_region(
        &world,
        OVERWORLD,
        0,
        0,
        &[VerifiedChunkReplacement::new(0, 0, changed_biome)],
        &biome_destination,
    )
    .expect("biome change is a verified write field");
    assert_eq!(report.replaced, 1);
    let written = RegionFile::open(&biome_destination).expect("open written region");
    let decoded = world
        .decode_chunk(OVERWORLD, &written.read_chunk(0, 0).expect("written chunk"))
        .expect("decode written chunk");
    assert_eq!(decoded.biome_at(0, -64, 0), Some("minecraft:desert"));
    assert_eq!(
        decoded.block_at(0, -64, 0).map(ToString::to_string),
        Some("minecraft:stone".to_owned())
    );
    fs::remove_file(&biome_destination).expect("remove verified biome output");

    let mut changed_status = source.nbt.clone();
    if let NbtValue::Compound(root) = &mut changed_status.root {
        root.insert(
            "Status".to_owned(),
            NbtValue::String("minecraft:features".to_owned()),
        );
    }
    assert_eq!(
        write_verified_region(
            &world,
            OVERWORLD,
            0,
            0,
            &[VerifiedChunkReplacement::new(0, 0, changed_status)],
            output_dir(dir.path()).join(region_file_name(0, 0)),
        )
        .expect_err("status change")
        .code(),
        "unsupported_format"
    );

    let mut changed_version = source.nbt.clone();
    if let NbtValue::Compound(root) = &mut changed_version.root {
        root.insert("DataVersion".to_owned(), NbtValue::Int(4325));
    }
    assert_eq!(
        write_verified_region(
            &world,
            OVERWORLD,
            0,
            0,
            &[VerifiedChunkReplacement::new(0, 0, changed_version)],
            output_dir(dir.path()).join(region_file_name(0, 0)),
        )
        .expect_err("version change")
        .code(),
        "unsupported_version"
    );

    let mut changed_entity = source.nbt.clone();
    if let NbtValue::Compound(root) = &mut changed_entity.root {
        root.insert(
            "entities".to_owned(),
            NbtValue::List {
                kind: TAG_COMPOUND,
                items: vec![],
            },
        );
    }
    assert_eq!(
        write_verified_region(
            &world,
            OVERWORLD,
            0,
            0,
            &[VerifiedChunkReplacement::new(0, 0, changed_entity)],
            output_dir(dir.path()).join(region_file_name(0, 0)),
        )
        .expect_err("entity change")
        .code(),
        "unsupported_format"
    );
}

#[test]
fn preserves_opaque_block_entities_and_palette_extensions_and_refuses_tampering() {
    let dir = TempDir::new("opaque-fields");
    let root = dir.path().join("world");
    write_world(&root, "minecraft:full");
    let source_path = root.join("region").join(region_file_name(0, 0));
    let mut original = chunk_nbt(
        0,
        0,
        "minecraft:full",
        "minecraft:chest",
        "minecraft:plains",
    );
    let section = first_section(&mut original);
    for key in ["block_states", "biomes"] {
        let NbtValue::Compound(container) = section.get_mut(key).unwrap() else {
            panic!("fixture container");
        };
        container.insert("example:extra".into(), NbtValue::IntArray(vec![1, 2, 3]));
    }
    fs::write(&source_path, region_bytes(&[(0, 0, original.clone())])).unwrap();
    let before = read_tree(&root);

    let mut overlay = WorldOverlay::open(&root).unwrap();
    overlay
        .set_block(
            OVERWORLD,
            1,
            -64,
            0,
            &"minecraft:gold_block".parse().unwrap(),
        )
        .unwrap();
    overlay
        .set_biome(OVERWORLD, 0, -64, 0, &"minecraft:desert".parse().unwrap())
        .unwrap();
    let edited = overlay
        .materialize_chunk_nbt(OVERWORLD, 0, 0)
        .unwrap()
        .unwrap();
    for (label, policy) in [
        ("preserve", DerivedDataPolicy::Preserve),
        ("rebuild", DerivedDataPolicy::VanillaRebuild),
    ] {
        let folder = dir.path().join(label);
        fs::create_dir(&folder).unwrap();
        let destination = folder.join(region_file_name(0, 0));
        write_region_from_overlay_with_policy(&overlay, OVERWORLD, 0, 0, &destination, policy)
            .unwrap();
        let written = RegionFile::open(&destination)
            .unwrap()
            .read_chunk(0, 0)
            .unwrap();
        assert_eq!(
            written.nbt.root.as_compound().unwrap()["block_entities"],
            original.root.as_compound().unwrap()["block_entities"],
        );
        let mut nbt = written.nbt;
        let section = first_section(&mut nbt);
        for key in ["block_states", "biomes"] {
            let container = section[key].as_compound().unwrap();
            assert_eq!(
                container["example:extra"],
                NbtValue::IntArray(vec![1, 2, 3])
            );
            assert!(matches!(container["data"], NbtValue::LongArray(_)));
        }
    }

    let mut wrong_entity = edited.clone();
    let NbtValue::Compound(root_tag) = &mut wrong_entity.root else {
        panic!("fixture root");
    };
    let Some(NbtValue::List { items, .. }) = root_tag.get_mut("block_entities") else {
        panic!("fixture entity list");
    };
    let NbtValue::Compound(entity) = &mut items[0] else {
        panic!("fixture entity");
    };
    let Some(NbtValue::Compound(opaque)) = entity.get_mut("example:opaque") else {
        panic!("fixture opaque payload");
    };
    opaque.insert("zero".into(), NbtValue::Double(0.0));

    let mut wrong_palette = edited;
    let Some(NbtValue::Compound(blocks)) =
        first_section(&mut wrong_palette).get_mut("block_states")
    else {
        panic!("fixture block states");
    };
    blocks.remove("example:extra");

    let destination = output_dir(dir.path()).join(region_file_name(0, 0));
    for (nbt, field) in [
        (wrong_entity, "block_entities"),
        (wrong_palette, "block_states"),
    ] {
        let error = write_verified_region(
            overlay.world(),
            OVERWORLD,
            0,
            0,
            &[VerifiedChunkReplacement::new(0, 0, nbt)],
            &destination,
        )
        .unwrap_err();
        assert!(error.to_string().contains(field), "{error}");
        assert!(!destination.exists());
    }
    assert_eq!(read_tree(&root), before, "source bytes must stay identical");
}

fn first_section(nbt: &mut NbtFile) -> &mut BTreeMap<String, NbtValue> {
    let NbtValue::Compound(root) = &mut nbt.root else {
        panic!("fixture root");
    };
    let Some(NbtValue::List { items, .. }) = root.get_mut("sections") else {
        panic!("fixture sections");
    };
    let NbtValue::Compound(section) = &mut items[0] else {
        panic!("fixture section");
    };
    section
}

#[test]
fn refuses_to_edit_a_chunk_that_is_not_fully_generated() {
    let dir = TempDir::new("non-full");
    let root = dir.path().join("world");
    write_world(&root, "minecraft:features");
    let mut overlay = WorldOverlay::open(&root).expect("open overlay");
    overlay
        .set_block(
            OVERWORLD,
            0,
            -64,
            0,
            &"minecraft:gold_block".parse::<BlockState>().unwrap(),
        )
        .expect("set block");
    let destination = output_dir(dir.path()).join(region_file_name(0, 0));

    let error = write_region_from_overlay(&overlay, OVERWORLD, 0, 0, &destination)
        .expect_err("non-full chunk");
    assert_eq!(error.code(), "unsupported_format");
    assert!(!destination.exists());
}

#[test]
fn refuses_an_identical_replacement() {
    let dir = TempDir::new("identical");
    let root = dir.path().join("world");
    write_world(&root, "minecraft:full");
    let world = JavaWorld::open(&root).expect("open world");
    let source = world
        .read_region(OVERWORLD, 0, 0)
        .expect("region")
        .read_chunk(0, 0)
        .expect("chunk");
    let error = write_verified_region(
        &world,
        OVERWORLD,
        0,
        0,
        &[VerifiedChunkReplacement::new(0, 0, source.nbt.clone())],
        output_dir(dir.path()).join(region_file_name(0, 0)),
    )
    .expect_err("identical replacement");
    assert_eq!(error.code(), "invalid_argument");
}

#[test]
fn vanilla_rebuild_policy_invalidates_only_derived_fields() {
    let dir = TempDir::new("vanilla-rebuild");
    let root = dir.path().join("world");
    write_world(&root, "minecraft:full");
    let before = read_tree(&root);

    let mut overlay = WorldOverlay::open(&root).expect("open overlay");
    overlay
        .set_block(
            OVERWORLD,
            0,
            -64,
            0,
            &"minecraft:gold_block".parse::<BlockState>().unwrap(),
        )
        .expect("set block");
    let destination = output_dir(dir.path()).join(region_file_name(0, 0));
    let report = write_region_from_overlay_with_policy(
        &overlay,
        OVERWORLD,
        0,
        0,
        &destination,
        DerivedDataPolicy::VanillaRebuild,
    )
    .expect("write region with vanilla rebuild policy");

    assert_eq!(
        report.derived_data_policy,
        DerivedDataPolicy::VanillaRebuild
    );
    assert_eq!(
        report.derived_data_action,
        "invalidated_for_vanilla_rebuild"
    );
    assert_eq!(
        report.invalidated_fields,
        [
            "Heightmaps",
            "isLightOn",
            "PostProcessing",
            "sections[].SkyLight",
            "sections[].BlockLight"
        ]
    );
    assert_eq!(report.replaced, 1);
    assert_eq!(report.preserved, 1);
    assert!(report.untouched_records_byte_identical);
    assert_eq!(read_tree(&root), before, "the source world must not change");

    let source_region = root.join("region").join(region_file_name(0, 0));
    let source = RegionFile::open(&source_region)
        .expect("open source region")
        .read_chunk(0, 0)
        .expect("source chunk");
    let source_root = source.nbt.root.as_compound().expect("source root");
    assert!(source_root.contains_key("Heightmaps"));
    assert_eq!(source_root.get("isLightOn"), Some(&NbtValue::Byte(1)));

    let written = RegionFile::open(&destination).expect("open written region");
    let edited = written.read_chunk(0, 0).expect("edited chunk");
    let edited_root = edited.nbt.root.as_compound().expect("edited root");
    assert!(!edited_root.contains_key("Heightmaps"));
    assert!(!edited_root.contains_key("PostProcessing"));
    assert_eq!(edited_root.get("isLightOn"), Some(&NbtValue::Byte(0)));
    assert_eq!(
        edited_root.get("block_entities"),
        source_root.get("block_entities"),
        "block entities are outside the invalidation policy"
    );
    assert_eq!(
        edited_root.get("entities"),
        source_root.get("entities"),
        "entities are outside the invalidation policy"
    );
    assert_eq!(
        edited_root.get("UnknownChunkTag"),
        source_root.get("UnknownChunkTag"),
        "unknown tags must stay untouched"
    );
    let edited_sections = match edited_root.get("sections") {
        Some(NbtValue::List { items, .. }) => items,
        other => panic!("unexpected sections: {other:?}"),
    };
    let edited_section = edited_sections[0].as_compound().expect("section");
    assert!(!edited_section.contains_key("SkyLight"));
    assert!(!edited_section.contains_key("BlockLight"));
    assert_eq!(
        edited_section.get("UnknownSectionTag"),
        Some(&NbtValue::compound([("value", NbtValue::Long(99))])),
        "unknown section tags must stay untouched"
    );

    let world = JavaWorld::open(&root).expect("reopen source world read-only");
    let decoded = world
        .decode_chunk(OVERWORLD, &edited)
        .expect("decode edited chunk");
    assert_eq!(
        decoded
            .block_at(0, -64, 0)
            .expect("block at 0,-64,0")
            .to_string(),
        "minecraft:gold_block"
    );
    assert_eq!(
        decoded.biome_at(0, -64, 0).expect("biome"),
        "minecraft:plains"
    );

    let untouched = written.read_chunk(1, 0).expect("untouched chunk");
    let untouched_root = untouched.nbt.root.as_compound().expect("untouched root");
    assert!(
        untouched_root.contains_key("Heightmaps"),
        "untouched chunks must not be invalidated"
    );
    assert_eq!(untouched_root.get("isLightOn"), Some(&NbtValue::Byte(1)));
}
