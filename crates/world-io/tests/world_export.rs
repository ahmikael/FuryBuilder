//! Tests for complete Java world-folder export (M8.10).
//!
//! The synthetic world deliberately contains all three classic dimensions, sidecar
//! containers, unknown files and `session.lock`, so the export must copy the whole save
//! folder and rebuild only the dirty region file.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use flate2::write::ZlibEncoder;
use flate2::Compression;
use sha2::{Digest, Sha256};
use world_domain::{BiomeState, BlockPos, BlockState};
use world_io::nbt::{self, NbtFile, NbtValue, TAG_COMPOUND, TAG_STRING};
use world_io::{
    analyze_java_world, export_java_world, export_java_world_with_options, load_java_world,
    region_file_name, DerivedDataPolicy, JavaWorld, WorldExportError, WorldExportOptions,
    WorldOverlay, ANVIL_SECTOR_BYTES,
};

const TARGET: i32 = 4189;
const OVERWORLD: &str = "minecraft:overworld";

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "furybuilder-world-export-{label}-{}-{}",
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
                    NbtValue::String("World export test".to_owned()),
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

fn chunk_nbt(x: i32, z: i32, section_y: i8, status: &str, blocks: &str, biome: &str) -> NbtFile {
    NbtFile::new(
        "",
        NbtValue::compound([
            ("DataVersion", NbtValue::Int(TARGET)),
            ("xPos", NbtValue::Int(x)),
            ("zPos", NbtValue::Int(z)),
            ("yPos", NbtValue::Int(i32::from(section_y))),
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
                        NbtValue::IntArray(vec![0, i32::from(section_y) * 16 + 4, 0]),
                    )])],
                },
            ),
            (
                "sections",
                NbtValue::List {
                    kind: TAG_COMPOUND,
                    items: vec![section(section_y, blocks, biome)],
                },
            ),
            (
                "block_entities",
                NbtValue::List {
                    kind: TAG_COMPOUND,
                    items: vec![NbtValue::compound([(
                        "id",
                        NbtValue::String("minecraft:chest".to_owned()),
                    )])],
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

fn write_world(root: &Path, overworld_status: &str) {
    fs::create_dir_all(root.join("region")).expect("overworld region");
    fs::create_dir_all(root.join("DIM-1").join("region")).expect("nether region");
    fs::create_dir_all(root.join("DIM1").join("region")).expect("end region");
    fs::create_dir_all(root.join("poi")).expect("poi folder");
    fs::create_dir_all(root.join("entities")).expect("entities folder");
    fs::create_dir_all(root.join("data")).expect("data folder");
    fs::create_dir_all(root.join("unknown-folder")).expect("unknown folder");
    fs::write(root.join("level.dat"), level_dat_bytes()).expect("level.dat");
    fs::write(
        root.join("region").join(region_file_name(0, 0)),
        region_bytes(&[
            (
                0,
                0,
                chunk_nbt(
                    0,
                    0,
                    -4,
                    overworld_status,
                    "minecraft:stone",
                    "minecraft:plains",
                ),
            ),
            (
                1,
                0,
                chunk_nbt(
                    1,
                    0,
                    -4,
                    "minecraft:full",
                    "minecraft:dirt",
                    "minecraft:forest",
                ),
            ),
        ]),
    )
    .expect("overworld region file");
    fs::write(
        root.join("DIM-1")
            .join("region")
            .join(region_file_name(0, 0)),
        region_bytes(&[(
            0,
            0,
            chunk_nbt(
                0,
                0,
                0,
                "minecraft:full",
                "minecraft:netherrack",
                "minecraft:nether_wastes",
            ),
        )]),
    )
    .expect("nether region file");
    fs::write(
        root.join("DIM1")
            .join("region")
            .join(region_file_name(0, 0)),
        region_bytes(&[(
            0,
            0,
            chunk_nbt(
                0,
                0,
                0,
                "minecraft:full",
                "minecraft:end_stone",
                "minecraft:the_end",
            ),
        )]),
    )
    .expect("end region file");
    fs::write(root.join("poi").join(region_file_name(0, 0)), []).expect("poi placeholder");
    fs::write(root.join("entities").join(region_file_name(0, 0)), [])
        .expect("entities placeholder");
    fs::write(root.join("data").join("unknown.dat"), b"unknown data").expect("unknown data");
    fs::write(
        root.join("unknown-folder").join("note.txt"),
        b"keep this file",
    )
    .expect("unknown note");
    fs::write(root.join("session.lock"), b"lock bytes").expect("session lock");
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

fn output_path(root: &Path) -> PathBuf {
    let out = root.join("out");
    fs::create_dir_all(&out).expect("out folder");
    out.join("exported-world")
}

fn staging_directories(parent: &Path) -> Vec<PathBuf> {
    fs::read_dir(parent)
        .expect("read output parent")
        .filter_map(|entry| {
            let path = entry.expect("entry").path();
            let name = path.file_name()?.to_string_lossy();
            name.starts_with(".furybuilder-world-export-")
                .then_some(path)
        })
        .collect()
}

fn classic_anvil_export_manifest(policy: DerivedDataPolicy) -> String {
    let dir = TempDir::new("classic-anvil-golden");
    let root = dir.path().join("world");
    write_world(&root, "minecraft:full");
    let before = read_tree(&root);
    let mut overlay = WorldOverlay::open(&root).expect("open fixture");
    overlay
        .set_block(
            OVERWORLD,
            0,
            -64,
            0,
            &"minecraft:gold_block".parse().unwrap(),
        )
        .expect("edit block");
    overlay
        .set_biome(OVERWORLD, 0, -64, 0, &"minecraft:desert".parse().unwrap())
        .expect("edit biome");
    let destination = output_path(dir.path());
    let report = export_java_world_with_options(
        &overlay,
        &destination,
        WorldExportOptions {
            derived_data_policy: policy,
            ..WorldExportOptions::default()
        },
    )
    .expect("export fixture");
    assert_eq!((report.replaced_regions, report.replaced_chunks), (1, 1));
    assert!(report.output_revalidated);
    assert_eq!(read_tree(&root), before, "source bytes changed");

    let mut files = BTreeMap::new();
    for (relative, mut bytes) in read_tree(&destination) {
        if relative == Path::new("region/r.0.0.mca") {
            // The writer timestamps the edited chunk (0, 0). Hash every other
            // byte, including the original timestamp of the untouched chunk.
            let offset = ANVIL_SECTOR_BYTES as usize;
            let timestamp = &mut bytes[offset..offset + 4];
            assert!(i32::from_be_bytes(timestamp.try_into().unwrap()) > 0);
            timestamp.fill(0);
        }
        let name = relative
            .iter()
            .map(|part| part.to_str().expect("fixture path is UTF-8"))
            .collect::<Vec<_>>()
            .join("/");
        files.insert(name, format!("{:x}", Sha256::digest(&bytes)));
    }
    files
        .iter()
        .map(|(name, hash)| format!("{hash}  {name}\n"))
        .collect()
}

#[test]
fn golden_classic_anvil_preserve() {
    let actual = classic_anvil_export_manifest(DerivedDataPolicy::Preserve);
    assert_eq!(
        actual,
        include_str!("goldens/classic-anvil-1.21.4-preserve.sha256").replace("\r\n", "\n"),
        "classic Anvil preserve export differs from the baseline:\n{actual}"
    );
}

#[test]
fn golden_classic_anvil_vanilla_rebuild() {
    let actual = classic_anvil_export_manifest(DerivedDataPolicy::VanillaRebuild);
    assert_eq!(
        actual,
        include_str!("goldens/classic-anvil-1.21.4-rebuild.sha256").replace("\r\n", "\n"),
        "classic Anvil rebuild export differs from the baseline:\n{actual}"
    );
}

#[test]
fn exports_complete_world_and_preserves_source() {
    let dir = TempDir::new("complete");
    let root = dir.path().join("world");
    write_world(&root, "minecraft:full");
    let before = read_tree(&root);
    let destination = output_path(dir.path());

    let inspection = analyze_java_world(&root);
    assert!(inspection.supported, "{:?}", inspection.blockers);
    assert_eq!(inspection.data_version, Some(TARGET));
    assert_eq!(inspection.chunks, 4);

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
    overlay
        .set_biome(
            OVERWORLD,
            0,
            -64,
            0,
            &"minecraft:desert".parse::<BiomeState>().unwrap(),
        )
        .expect("set biome");
    let report = export_java_world(&overlay, &destination).expect("export world");

    assert_eq!(report.regions, 3);
    assert_eq!(report.chunks, 4);
    assert_eq!(report.replaced_regions, 1);
    assert_eq!(report.replaced_chunks, 1);
    assert_eq!(report.files, before.len() - 1, "session.lock is skipped");
    assert_eq!(report.skipped_files, ["session.lock"]);
    assert!(report.source_tree_verified);
    assert!(report.source_unchanged);
    assert!(report.copied_files_match_source);
    assert!(report.output_revalidated);
    assert_eq!(report.derived_data_policy, DerivedDataPolicy::Preserve);
    assert!(report.invalidated_fields.is_empty());
    assert!(report.source_tree_sha256_before.is_some());
    assert_eq!(
        report.source_tree_sha256_before,
        report.source_tree_sha256_after
    );
    assert_eq!(read_tree(&root), before, "the source world must not change");
    assert!(staging_directories(dir.path()).is_empty());

    let exported = read_tree(&destination);
    assert!(!exported.contains_key(Path::new("session.lock")));
    for relative in [
        Path::new("level.dat"),
        Path::new("data/unknown.dat"),
        Path::new("unknown-folder/note.txt"),
        Path::new("poi/r.0.0.mca"),
        Path::new("entities/r.0.0.mca"),
    ] {
        assert_eq!(
            exported.get(relative),
            before.get(relative),
            "{relative:?} must be copied byte for byte"
        );
    }
    assert_eq!(
        exported.get(Path::new("DIM-1/region/r.0.0.mca")),
        before.get(Path::new("DIM-1/region/r.0.0.mca")),
        "untouched nether region must be byte-identical"
    );

    let source_world = JavaWorld::open(&root).expect("reopen source world");
    let source_dimension = source_world.dimension(OVERWORLD).expect("source overworld");
    let source_region = source_world
        .read_region(OVERWORLD, 0, 0)
        .expect("source region");
    let source_chunk = source_region.read_chunk(0, 0).expect("source chunk");
    let source_decoded = source_chunk
        .decode(source_dimension)
        .expect("decode source");
    assert_eq!(
        source_decoded
            .block_at(0, -64, 0)
            .expect("source block")
            .to_string(),
        "minecraft:stone"
    );
    assert_eq!(
        source_decoded.biome_at(0, -64, 0),
        Some("minecraft:plains"),
        "the source biome must stay untouched"
    );

    let exported_world = JavaWorld::open(&destination).expect("reopen exported world");
    let exported_dimension = exported_world
        .dimension(OVERWORLD)
        .expect("exported overworld");
    let exported_region = exported_world
        .read_region(OVERWORLD, 0, 0)
        .expect("exported region");
    let edited = exported_region.read_chunk(0, 0).expect("edited chunk");
    let decoded = edited.decode(exported_dimension).expect("decode edited");
    assert_eq!(
        decoded
            .block_at(0, -64, 0)
            .expect("edited block")
            .to_string(),
        "minecraft:gold_block"
    );
    assert_eq!(
        decoded.biome_at(0, -64, 0),
        Some("minecraft:desert"),
        "the exported biome must match the edited cell"
    );
    assert_eq!(
        decoded.biome_at(8, -64, 8),
        Some("minecraft:plains"),
        "neighbouring biome cells must stay untouched"
    );
    assert_eq!(edited.summary.block_entities, 1);
    assert_eq!(edited.summary.entities, 1);
    assert_eq!(
        exported_region
            .read_chunk(1, 0)
            .expect("untouched chunk")
            .summary
            .status
            .as_deref(),
        Some("minecraft:full")
    );

    let (reopened, load) = load_java_world(&destination).expect("import the exported world");
    assert_eq!(load.data_version, TARGET);
    assert_eq!(load.chunks, inspection.chunks);
    let origin = BlockPos::new(0, -64, 0);
    assert_eq!(
        reopened.get_block(OVERWORLD, origin).unwrap().to_string(),
        "minecraft:gold_block"
    );
    assert_eq!(
        reopened
            .get_biome(OVERWORLD, origin)
            .unwrap()
            .unwrap()
            .as_str(),
        "minecraft:desert"
    );
}

#[test]
fn export_without_edits_is_byte_identical_copy() {
    let dir = TempDir::new("copy");
    let root = dir.path().join("world");
    write_world(&root, "minecraft:full");
    let before = read_tree(&root);
    let destination = output_path(dir.path());
    let overlay = WorldOverlay::open(&root).expect("open overlay");

    let report = export_java_world(&overlay, &destination).expect("export copy");
    assert_eq!(report.replaced_regions, 0);
    assert_eq!(report.replaced_chunks, 0);
    assert!(report.exported_regions.is_empty());
    assert_eq!(read_tree(&root), before);
    let exported = read_tree(&destination);
    assert_eq!(exported.len(), before.len() - 1);
    for (relative, bytes) in &before {
        if relative == Path::new("session.lock") {
            continue;
        }
        assert_eq!(exported.get(relative), Some(bytes), "{relative:?}");
    }
}

#[test]
fn vanilla_rebuild_policy_only_invalidates_the_replaced_chunk() {
    let dir = TempDir::new("rebuild");
    let root = dir.path().join("world");
    write_world(&root, "minecraft:full");
    let before = read_tree(&root);
    let destination = output_path(dir.path());

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
    let options = WorldExportOptions {
        derived_data_policy: DerivedDataPolicy::VanillaRebuild,
        ..WorldExportOptions::default()
    };
    let report = export_java_world_with_options(&overlay, &destination, options)
        .expect("export with rebuild policy");
    assert_eq!(
        report.derived_data_policy,
        DerivedDataPolicy::VanillaRebuild
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
    assert_eq!(read_tree(&root), before);

    let exported_world = JavaWorld::open(&destination).expect("reopen exported world");
    let region = exported_world.read_region(OVERWORLD, 0, 0).expect("region");
    let edited = region.read_chunk(0, 0).expect("edited chunk");
    let root_tag = edited.nbt.root.as_compound().expect("edited root");
    assert!(!root_tag.contains_key("Heightmaps"));
    assert!(!root_tag.contains_key("PostProcessing"));
    assert_eq!(root_tag.get("isLightOn"), Some(&NbtValue::Byte(0)));
    let sections = match root_tag.get("sections") {
        Some(NbtValue::List { items, .. }) => items,
        other => panic!("unexpected sections: {other:?}"),
    };
    let section = sections[0].as_compound().expect("section");
    assert!(!section.contains_key("SkyLight"));
    assert!(!section.contains_key("BlockLight"));
    assert_eq!(
        section.get("UnknownSectionTag"),
        Some(&NbtValue::compound([("value", NbtValue::Long(99))]))
    );

    let untouched = region.read_chunk(1, 0).expect("untouched chunk");
    let untouched_root = untouched.nbt.root.as_compound().expect("untouched root");
    assert!(untouched_root.contains_key("Heightmaps"));
    assert_eq!(untouched_root.get("isLightOn"), Some(&NbtValue::Byte(1)));
}

#[test]
fn refuses_an_existing_destination_and_a_destination_inside_the_source() {
    let dir = TempDir::new("destinations");
    let root = dir.path().join("world");
    write_world(&root, "minecraft:full");
    let overlay = WorldOverlay::open(&root).expect("open overlay");
    let before = read_tree(&root);

    let existing = output_path(dir.path());
    fs::write(&existing, b"do not replace").expect("existing destination");
    let error = export_java_world(&overlay, &existing).expect_err("existing destination");
    assert!(matches!(error, WorldExportError::AlreadyExists(_)));
    assert_eq!(error.code(), "conflict");
    assert_eq!(fs::read(&existing).unwrap(), b"do not replace");
    assert_eq!(read_tree(&root), before);

    let inside = root.join("exported");
    let error = export_java_world(&overlay, &inside).expect_err("inside source");
    assert!(matches!(error, WorldExportError::UnsafePath(_)));
    assert_eq!(error.code(), "unsafe_path");
    assert!(!inside.exists());
    assert_eq!(read_tree(&root), before);
}

#[test]
fn fails_closed_on_a_non_full_chunk_without_publishing() {
    let dir = TempDir::new("non-full");
    let root = dir.path().join("world");
    write_world(&root, "minecraft:features");
    let destination = output_path(dir.path());
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

    let error = export_java_world(&overlay, &destination).expect_err("non-full chunk");
    assert_eq!(error.code(), "unsupported_format");
    assert!(!destination.exists());
    assert!(staging_directories(dir.path()).is_empty());
}
