//! Tests for the read-only derived-data inventory (M8.8).
//!
//! The synthetic world deliberately contains both a full chunk and a proto chunk, plus
//! non-empty `poi/` and `entities/` sidecars, so the report is tested against the shapes
//! that real 1.21.4 saves use rather than only against empty lists.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use flate2::write::ZlibEncoder;
use flate2::Compression;
use sha2::{Digest, Sha256};
use world_io::nbt::{self, NbtFile, NbtValue, TAG_COMPOUND, TAG_INT, TAG_LIST, TAG_STRING};
use world_io::{
    analyze_java_world_derived, inspect_chunk_derived, region_file_name, DerivedDataError,
    JavaWorld,
};

const TARGET: i32 = 4189;
const OVERWORLD: &str = "minecraft:overworld";

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "furybuilder-derived-{label}-{}-{}",
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
                    NbtValue::String("Derived data test".to_owned()),
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

fn biome_palette() -> NbtValue {
    NbtValue::compound([(
        "palette",
        NbtValue::List {
            kind: TAG_STRING,
            items: vec![NbtValue::String("minecraft:plains".to_owned())],
        },
    )])
}

fn section(y: i8, with_light: bool) -> NbtValue {
    let mut section = BTreeMap::new();
    section.insert("Y".to_owned(), NbtValue::Byte(y));
    section.insert("block_states".to_owned(), block_palette("minecraft:stone"));
    section.insert("biomes".to_owned(), biome_palette());
    if with_light {
        section.insert("SkyLight".to_owned(), NbtValue::ByteArray(vec![0x11; 2048]));
        section.insert(
            "BlockLight".to_owned(),
            NbtValue::ByteArray(vec![0x22; 2048]),
        );
    }
    NbtValue::Compound(section)
}

fn compound_list(items: Vec<NbtValue>) -> NbtValue {
    NbtValue::List {
        kind: TAG_COMPOUND,
        items,
    }
}

fn full_chunk() -> NbtFile {
    NbtFile::new(
        "",
        NbtValue::compound([
            ("DataVersion", NbtValue::Int(TARGET)),
            ("xPos", NbtValue::Int(0)),
            ("zPos", NbtValue::Int(0)),
            ("yPos", NbtValue::Int(-4)),
            ("Status", NbtValue::String("minecraft:full".to_owned())),
            ("LastUpdate", NbtValue::Long(42)),
            ("InhabitedTime", NbtValue::Long(7)),
            (
                "sections",
                NbtValue::List {
                    kind: TAG_COMPOUND,
                    items: vec![section(-4, true), section(-3, false)],
                },
            ),
            (
                "Heightmaps",
                NbtValue::compound([
                    ("MOTION_BLOCKING", NbtValue::LongArray(vec![1, 2, 3])),
                    ("WORLD_SURFACE", NbtValue::LongArray(vec![4])),
                ]),
            ),
            (
                "block_entities",
                compound_list(vec![NbtValue::compound([
                    ("id", NbtValue::String("minecraft:chest".to_owned())),
                    ("x", NbtValue::Int(0)),
                    ("y", NbtValue::Int(-60)),
                    ("z", NbtValue::Int(0)),
                ])]),
            ),
            (
                "block_ticks",
                compound_list(vec![NbtValue::compound([
                    ("i", NbtValue::String("minecraft:redstone_torch".to_owned())),
                    ("x", NbtValue::Int(0)),
                    ("y", NbtValue::Int(-60)),
                    ("z", NbtValue::Int(0)),
                    ("t", NbtValue::Int(5)),
                    ("p", NbtValue::Int(0)),
                ])]),
            ),
            ("fluid_ticks", compound_list(vec![])),
            (
                "PostProcessing",
                NbtValue::List {
                    kind: TAG_LIST,
                    items: vec![NbtValue::List {
                        kind: TAG_INT,
                        items: vec![NbtValue::Int(0), NbtValue::Int(-60), NbtValue::Int(0)],
                    }],
                },
            ),
            ("isLightOn", NbtValue::Byte(1)),
            (
                "structures",
                NbtValue::compound([
                    (
                        "starts",
                        NbtValue::compound([(
                            "minecraft:village",
                            NbtValue::compound([("id", NbtValue::String("village".to_owned()))]),
                        )]),
                    ),
                    ("References", NbtValue::Compound(BTreeMap::new())),
                ]),
            ),
            (
                "blending_data",
                NbtValue::compound([("min_section", NbtValue::Int(-4))]),
            ),
        ]),
    )
}

fn proto_chunk(x: i32) -> NbtFile {
    NbtFile::new(
        "",
        NbtValue::compound([
            ("DataVersion", NbtValue::Int(TARGET)),
            ("xPos", NbtValue::Int(x)),
            ("zPos", NbtValue::Int(0)),
            ("yPos", NbtValue::Int(-4)),
            ("Status", NbtValue::String("minecraft:features".to_owned())),
            ("LastUpdate", NbtValue::Long(1)),
            (
                "sections",
                NbtValue::List {
                    kind: TAG_COMPOUND,
                    items: vec![],
                },
            ),
            ("block_entities", compound_list(vec![])),
            ("block_ticks", compound_list(vec![])),
            ("fluid_ticks", compound_list(vec![])),
            (
                "PostProcessing",
                NbtValue::List {
                    kind: TAG_LIST,
                    items: vec![],
                },
            ),
            (
                "entities",
                compound_list(vec![NbtValue::compound([(
                    "id",
                    NbtValue::String("minecraft:pig".to_owned()),
                )])]),
            ),
            (
                "structures",
                NbtValue::compound([("starts", NbtValue::Compound(BTreeMap::new()))]),
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
        let raw = nbt::write_uncompressed(file).expect("encode nbt");
        let payload = zlib(&raw);
        let sectors = (5 + payload.len()).div_ceil(4096);
        records.push((payload, sectors));
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
        let start = offset * 4096;
        file[start..start + 4].copy_from_slice(&(payload.len() as u32 + 1).to_be_bytes());
        file[start + 4] = 2;
        file[start + 5..start + 5 + payload.len()].copy_from_slice(payload);
        offset += sectors;
    }
    file
}

fn poi_chunk() -> NbtFile {
    NbtFile::new(
        "",
        NbtValue::compound([
            ("DataVersion", NbtValue::Int(TARGET)),
            (
                "Sections",
                NbtValue::compound([(
                    "-4",
                    NbtValue::compound([
                        (
                            "Records",
                            compound_list(vec![
                                NbtValue::compound([
                                    ("type", NbtValue::String("minecraft:bed".to_owned())),
                                    ("pos", NbtValue::IntArray(vec![0, -60, 0])),
                                    ("free_tickets", NbtValue::Int(0)),
                                ]),
                                NbtValue::compound([
                                    ("type", NbtValue::String("minecraft:barrel".to_owned())),
                                    ("pos", NbtValue::IntArray(vec![6, -60, 6])),
                                    ("free_tickets", NbtValue::Int(1)),
                                ]),
                            ]),
                        ),
                        ("Valid", NbtValue::Byte(1)),
                    ]),
                )]),
            ),
        ]),
    )
}

fn entity_chunk() -> NbtFile {
    NbtFile::new(
        "",
        NbtValue::compound([
            ("DataVersion", NbtValue::Int(TARGET)),
            ("Position", NbtValue::LongArray(vec![0, 0, 0, 0])),
            (
                "Entities",
                compound_list(vec![
                    NbtValue::compound([("id", NbtValue::String("minecraft:pig".to_owned()))]),
                    NbtValue::compound([("id", NbtValue::String("minecraft:cow".to_owned()))]),
                ]),
            ),
        ]),
    )
}

fn write_world(root: &Path, with_sidecars: bool) {
    fs::create_dir_all(root.join("region")).expect("region folder");
    fs::create_dir_all(root.join("DIM-1")).expect("nether folder");
    fs::create_dir_all(root.join("DIM1")).expect("end folder");
    fs::write(root.join("level.dat"), level_dat_bytes()).expect("level.dat");
    fs::write(
        root.join("region").join(region_file_name(0, 0)),
        region_bytes(&[(0, 0, full_chunk()), (1, 0, proto_chunk(1))]),
    )
    .expect("region file");
    if with_sidecars {
        fs::create_dir_all(root.join("poi")).expect("poi folder");
        fs::create_dir_all(root.join("entities")).expect("entities folder");
        fs::write(
            root.join("poi").join(region_file_name(0, 0)),
            region_bytes(&[(0, 0, poi_chunk())]),
        )
        .expect("poi file");
        fs::write(
            root.join("entities").join(region_file_name(0, 0)),
            region_bytes(&[(0, 0, entity_chunk())]),
        )
        .expect("entities file");
        fs::write(root.join("poi").join(region_file_name(0, 1)), []).expect("empty poi file");
    }
}

fn tree_hash(root: &Path) -> String {
    let mut files = Vec::new();
    let mut stack = vec![PathBuf::new()];
    while let Some(relative) = stack.pop() {
        for entry in fs::read_dir(root.join(&relative)).expect("read directory") {
            let entry = entry.expect("entry");
            let child = relative.join(entry.file_name());
            let metadata = fs::symlink_metadata(entry.path()).expect("metadata");
            if metadata.is_dir() {
                stack.push(child);
            } else {
                files.push((child, fs::read(entry.path()).expect("read file")));
            }
        }
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
    let mut hasher = Sha256::new();
    for (path, bytes) in files {
        hasher.update(path.to_string_lossy().as_bytes());
        hasher.update((bytes.len() as u64).to_be_bytes());
        hasher.update(bytes);
    }
    format!("{:x}", hasher.finalize())
}

#[test]
fn inspects_full_chunk_derived_fields() {
    let data = inspect_chunk_derived(&full_chunk(), 0, 0).expect("inspect full chunk");
    assert_eq!(data.data_version, TARGET);
    assert_eq!(data.status.as_deref(), Some("minecraft:full"));
    assert_eq!(data.sections, 2);
    assert_eq!(data.sections_with_sky_light, 1);
    assert_eq!(data.sections_with_block_light, 1);
    assert_eq!(data.heightmaps.len(), 2);
    assert_eq!(data.heightmaps["MOTION_BLOCKING"].words, 3);
    assert_eq!(data.heightmaps["WORLD_SURFACE"].words, 1);
    assert_eq!(data.block_entities, 1);
    assert_eq!(data.block_entity_ids, ["minecraft:chest"]);
    assert_eq!(data.block_ticks, 1);
    assert_eq!(data.fluid_ticks, 0);
    assert_eq!(data.post_processing, 1);
    assert_eq!(data.structures, 1);
    assert_eq!(data.light_on, Some(true));
    assert!(data.blending_data);
    assert_eq!(data.section_field_counts["SkyLight"], 1);
    assert_eq!(data.root_fields["Heightmaps"], "compound");
    assert!(data.fingerprints.heightmaps.is_some());
    assert!(data.fingerprints.sky_light.is_some());
    assert!(data.fingerprints.block_light.is_some());
    assert!(data.fingerprints.block_entities.is_some());
    assert!(data.fingerprints.block_ticks.is_some());
    assert!(data.fingerprints.post_processing.is_some());
    assert!(data.fingerprints.structures.is_some());
}

#[test]
fn rejects_unsupported_versions_and_wrong_known_field_types() {
    let mut wrong_version = full_chunk();
    if let NbtValue::Compound(root) = &mut wrong_version.root {
        root.insert("DataVersion".to_owned(), NbtValue::Int(4786));
    }
    assert_eq!(
        inspect_chunk_derived(&wrong_version, 0, 0)
            .expect_err("unsupported version")
            .code(),
        "unsupported_version"
    );

    for (key, value) in [
        ("Heightmaps", NbtValue::Int(1)),
        ("block_entities", NbtValue::Int(1)),
        ("isLightOn", NbtValue::String("yes".to_owned())),
        ("PostProcessing", NbtValue::Int(1)),
        ("LastUpdate", NbtValue::Int(1)),
    ] {
        let mut broken = full_chunk();
        if let NbtValue::Compound(root) = &mut broken.root {
            root.insert(key.to_owned(), value);
        }
        let error = inspect_chunk_derived(&broken, 0, 0).expect_err(key);
        assert!(matches!(error, DerivedDataError::InvalidChunk(_)), "{key}");
    }

    let mut missing_sections = full_chunk();
    if let NbtValue::Compound(root) = &mut missing_sections.root {
        root.remove("sections");
    }
    assert_eq!(
        inspect_chunk_derived(&missing_sections, 0, 0)
            .expect_err("missing sections")
            .code(),
        "invalid_argument"
    );

    let mut bad_heightmap = full_chunk();
    if let NbtValue::Compound(root) = &mut bad_heightmap.root {
        if let Some(NbtValue::Compound(heightmaps)) = root.get_mut("Heightmaps") {
            heightmaps.insert("MOTION_BLOCKING".to_owned(), NbtValue::Int(1));
        }
    }
    assert!(inspect_chunk_derived(&bad_heightmap, 0, 0).is_err());
}

#[test]
fn world_report_aggregates_chunks_and_sidecars_read_only() {
    let dir = TempDir::new("world");
    let root = dir.path().join("world");
    write_world(&root, true);
    let before = tree_hash(&root);

    let world = JavaWorld::open(&root).expect("open world");
    let report = analyze_java_world_derived(&world).expect("analyze world");
    assert_eq!(report.counters.chunks, 2);
    assert_eq!(report.counters.full_chunks, 1);
    assert_eq!(report.counters.chunks_with_block_entities, 1);
    assert_eq!(report.counters.block_entities, 1);
    assert_eq!(report.counters.chunks_with_block_ticks, 1);
    assert_eq!(report.counters.block_ticks, 1);
    assert_eq!(report.counters.chunks_with_entity_list, 1);
    assert_eq!(report.counters.entities_in_chunks, 1);
    assert_eq!(report.counters.chunks_with_post_processing, 1);
    assert_eq!(report.counters.post_processing, 1);
    assert_eq!(report.counters.chunks_with_any_light, 1);
    assert_eq!(report.counters.sections_with_sky_light, 1);
    assert_eq!(report.counters.chunks_with_structures, 1);
    assert_eq!(report.counters.structures, 1);
    assert_eq!(report.counters.heightmap_chunks["MOTION_BLOCKING"], 1);
    assert_eq!(report.counters.heightmap_words["MOTION_BLOCKING"], 3);
    assert!(report.warnings.iter().any(|warning| warning.contains(
        "minecraft:overworld has 2 heightmaps that do not use the expected 37 longs; raw data is retained"
    )));
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("block entity")));
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("scheduled")));
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("legacy chunk entities")));

    let poi = report
        .sidecars
        .iter()
        .find(|sidecar| sidecar.kind == "poi" && sidecar.dimension == OVERWORLD)
        .expect("poi sidecar");
    assert!(poi.exists);
    assert_eq!(poi.files, 2);
    assert_eq!(poi.empty_files, 1);
    assert_eq!(poi.allocated_records, 1);
    assert_eq!(poi.parsed_records, 1);
    assert_eq!(poi.entries, 2);
    assert_eq!(
        poi.sample_ids,
        ["minecraft:barrel".to_owned(), "minecraft:bed".to_owned()]
    );

    let entities = report
        .sidecars
        .iter()
        .find(|sidecar| sidecar.kind == "entities" && sidecar.dimension == OVERWORLD)
        .expect("entities sidecar");
    assert_eq!(entities.entries, 2);
    assert_eq!(
        entities.sample_ids,
        ["minecraft:cow".to_owned(), "minecraft:pig".to_owned()]
    );

    let nether_poi = report
        .sidecars
        .iter()
        .find(|sidecar| sidecar.kind == "poi" && sidecar.dimension == "minecraft:the_nether")
        .expect("nether poi sidecar");
    assert!(!nether_poi.exists);
    assert!(nether_poi.notes.iter().any(|note| note.contains("missing")));
    assert_eq!(
        tree_hash(&root),
        before,
        "analysis must not modify the world"
    );
}

#[test]
fn world_report_without_sidecar_folders_stays_valid() {
    let dir = TempDir::new("no-sidecars");
    let root = dir.path().join("world");
    write_world(&root, false);
    let world = JavaWorld::open(&root).expect("open world");
    let report = analyze_java_world_derived(&world).expect("analyze world");
    assert_eq!(report.counters.chunks, 2);
    assert!(report
        .sidecars
        .iter()
        .all(|sidecar| !sidecar.exists && sidecar.entries == 0));
    assert!(!report
        .warnings
        .iter()
        .any(|warning| warning.contains("sidecar holds")));
}
