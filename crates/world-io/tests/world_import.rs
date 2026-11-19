//! Tests for read-only analysis and editable block import of classic Java worlds (M8.6).

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use flate2::write::ZlibEncoder;
use flate2::Compression;
use world_domain::BlockPos;
use world_io::nbt::{self, NbtFile, NbtValue, TAG_COMPOUND, TAG_STRING};
use world_io::world_import::{
    analyze_java_world, load_java_world, load_java_world_with_options, WorldImportOptions,
};

const TARGET: i32 = 4189;
const OVERWORLD: &str = "minecraft:overworld";

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "furybuilder-import-{label}-{}-{}",
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

fn level_dat_bytes(data_version: i32) -> Vec<u8> {
    let file = NbtFile::new(
        "",
        NbtValue::compound([(
            "Data",
            NbtValue::compound([
                ("DataVersion", NbtValue::Int(data_version)),
                (
                    "Version",
                    NbtValue::compound([
                        ("Id", NbtValue::Int(data_version)),
                        ("Name", NbtValue::String("test".to_owned())),
                    ]),
                ),
                ("LevelName", NbtValue::String("Import test".to_owned())),
            ]),
        )]),
    );
    nbt::write_gzip(&file).expect("encode level.dat")
}

fn block_palette(states: &[(&str, &[(&str, &str)])]) -> NbtValue {
    let items = states
        .iter()
        .map(|(name, properties)| {
            let mut compound = BTreeMap::new();
            compound.insert("Name".to_owned(), NbtValue::String((*name).to_owned()));
            if !properties.is_empty() {
                compound.insert(
                    "Properties".to_owned(),
                    NbtValue::Compound(
                        properties
                            .iter()
                            .map(|(key, value)| {
                                ((*key).to_owned(), NbtValue::String((*value).to_owned()))
                            })
                            .collect(),
                    ),
                );
            }
            NbtValue::Compound(compound)
        })
        .collect();
    NbtValue::compound([(
        "palette",
        NbtValue::List {
            kind: TAG_COMPOUND,
            items,
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

fn section(y: i8, name: &str, properties: &[(&str, &str)]) -> NbtValue {
    NbtValue::compound([
        ("Y", NbtValue::Byte(y)),
        ("block_states", block_palette(&[(name, properties)])),
        ("biomes", biome_palette("minecraft:plains")),
    ])
}

fn chunk_nbt(x: i32, z: i32, sections: Vec<NbtValue>) -> NbtFile {
    NbtFile::new(
        "",
        NbtValue::compound([
            ("DataVersion", NbtValue::Int(TARGET)),
            ("xPos", NbtValue::Int(x)),
            ("zPos", NbtValue::Int(z)),
            ("yPos", NbtValue::Int(-4)),
            ("Status", NbtValue::String("minecraft:full".to_owned())),
            (
                "sections",
                NbtValue::List {
                    kind: TAG_COMPOUND,
                    items: sections,
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
                NbtValue::compound([("text", NbtValue::String("preserve".to_owned()))]),
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
        let timestamp = 1_700_000_000i32.to_be_bytes();
        file[4096 + base..4096 + base + 4].copy_from_slice(&timestamp);
        let start = offset * 4096;
        file[start..start + 4].copy_from_slice(&((payload.len() as u32 + 1).to_be_bytes()));
        file[start + 4] = 2;
        file[start + 5..start + 5 + payload.len()].copy_from_slice(payload);
        offset += sectors;
    }
    file
}

fn write_world(root: &Path, data_version: i32) {
    fs::create_dir_all(root.join("region")).expect("create region");
    fs::write(root.join("level.dat"), level_dat_bytes(data_version)).expect("write level.dat");
    fs::write(root.join("session.lock"), b"1234567890123456").expect("write session lock");

    let first = chunk_nbt(
        0,
        0,
        vec![
            section(-4, "minecraft:unknown_machine", &[("powered", "true")]),
            section(-3, "minecraft:stone", &[]),
        ],
    );
    let second = chunk_nbt(1, 0, vec![section(-4, "minecraft:dirt", &[])]);
    fs::write(
        root.join("region").join("r.0.0.mca"),
        region_bytes(&[(0, 0, first), (1, 0, second)]),
    )
    .expect("write region");
    fs::write(root.join("custom-unknown.dat"), b"keep me").expect("write unknown file");
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut stack = vec![PathBuf::new()];
    while let Some(relative) = stack.pop() {
        let directory = root.join(&relative);
        for entry in fs::read_dir(&directory).expect("read directory") {
            let entry = entry.expect("entry");
            let entry_relative = relative.join(entry.file_name());
            let metadata = fs::symlink_metadata(entry.path()).expect("metadata");
            if metadata.is_dir() {
                stack.push(entry_relative);
            } else {
                files.insert(entry_relative, fs::read(entry.path()).expect("read file"));
            }
        }
    }
    files
}

#[test]
fn analysis_reports_supported_world_and_never_writes() {
    let dir = TempDir::new("analysis");
    let root = dir.path().join("world");
    write_world(&root, TARGET);
    let before = snapshot(&root);

    let analysis = analyze_java_world(&root);
    assert!(analysis.supported, "blockers: {:?}", analysis.blockers);
    assert_eq!(analysis.layout, "classic_anvil");
    assert_eq!(analysis.data_version, Some(TARGET));
    assert_eq!(analysis.version_name.as_deref(), Some("test"));
    assert_eq!(analysis.level_name.as_deref(), Some("Import test"));
    assert_eq!(analysis.regions, 1);
    assert_eq!(analysis.chunks, 2);
    assert_eq!(analysis.block_entities, 2);
    assert_eq!(analysis.entities, 2);
    assert_eq!(analysis.dimensions.len(), 1);
    assert_eq!(analysis.dimensions[0].id, OVERWORLD);
    assert_eq!(analysis.dimensions[0].region_files, 1);
    assert_eq!(analysis.dimensions[0].chunks, 2);
    assert!(analysis
        .warnings
        .iter()
        .any(|warning| warning.contains("read-only")));
    assert!(analysis
        .warnings
        .iter()
        .any(|warning| warning.contains("biome cells become editable")));
    assert_eq!(snapshot(&root), before, "analysis must be read-only");
}

#[test]
fn load_imports_block_states_and_preserves_the_source() {
    let dir = TempDir::new("load");
    let root = dir.path().join("world");
    write_world(&root, TARGET);
    let before = snapshot(&root);

    let (world, report) = load_java_world(&root).expect("load world");
    assert_eq!(report.chunks, 2);
    assert_eq!(report.sections, 3);
    assert_eq!(report.biome_sections, 3);
    assert_eq!(report.non_air_blocks, 12_288);
    assert_eq!(report.block_entities, 2);
    assert_eq!(report.entities, 2);
    assert_eq!(report.dimensions, vec![OVERWORLD.to_owned()]);
    assert_eq!(world.chunk_count(), 2);
    assert_eq!(
        world
            .get_block(OVERWORLD, BlockPos::new(0, -64, 0))
            .expect("unknown block")
            .to_string(),
        "minecraft:unknown_machine[powered=true]"
    );
    assert_eq!(
        world
            .get_block(OVERWORLD, BlockPos::new(1, -46, 3))
            .expect("stone")
            .to_string(),
        "minecraft:stone"
    );
    assert_eq!(
        world
            .get_biome(OVERWORLD, BlockPos::new(1, -46, 3))
            .expect("biome")
            .expect("biome data")
            .as_str(),
        "minecraft:plains",
        "biome cells must enter the editable world"
    );
    assert_eq!(
        world
            .get_block(OVERWORLD, BlockPos::new(16, -64, 0))
            .expect("neighbor block")
            .to_string(),
        "minecraft:dirt"
    );
    assert_eq!(snapshot(&root), before, "import must not write the source");
}

#[test]
fn chunk_limit_is_enforced_before_partial_import_is_returned() {
    let dir = TempDir::new("limit");
    let root = dir.path().join("world");
    write_world(&root, TARGET);

    let error = load_java_world_with_options(
        &root,
        WorldImportOptions {
            max_chunks: 1,
            ..WorldImportOptions::default()
        },
    )
    .expect_err("chunk limit");
    assert_eq!(error.code(), "limit_exceeded");
    assert!(error.to_string().contains("chunks"));
}

#[test]
fn unsupported_data_version_is_blocked_before_import() {
    let dir = TempDir::new("version");
    let root = dir.path().join("world");
    write_world(&root, 4325);

    let analysis = analyze_java_world(&root);
    assert!(!analysis.supported);
    assert_eq!(analysis.data_version, Some(4325));
    assert!(analysis
        .blockers
        .iter()
        .any(|blocker| blocker.contains("DataVersion 4325")));
    let error = load_java_world(&root).expect_err("unsupported version");
    assert_eq!(error.code(), "unsupported_version");
}

#[test]
fn split_layout_is_detected_and_refused() {
    let dir = TempDir::new("split");
    let root = dir.path().join("world");
    fs::create_dir_all(root.join("dimensions").join("minecraft").join("overworld"))
        .expect("split folder");
    fs::write(root.join("level.dat"), level_dat_bytes(5023)).expect("level.dat");

    let analysis = analyze_java_world(&root);
    assert!(!analysis.supported);
    assert_eq!(analysis.layout, "split_26_1_plus");
    assert!(analysis
        .blockers
        .iter()
        .any(|blocker| blocker.contains("26.1+ split")));
}

#[test]
fn missing_folder_is_reported_as_a_blocker() {
    let dir = TempDir::new("missing");
    let analysis = analyze_java_world(dir.path().join("absent"));
    assert!(!analysis.supported);
    assert!(!analysis.blockers.is_empty());
    assert!(analysis.data_version.is_none());
}

#[test]
fn empty_world_still_decodes_to_an_empty_editable_world() {
    let dir = TempDir::new("empty");
    let root = dir.path().join("world");
    fs::create_dir_all(root.join("region")).expect("region");
    fs::write(root.join("level.dat"), level_dat_bytes(TARGET)).expect("level.dat");

    let (world, report) = load_java_world(&root).expect("load empty world");
    assert_eq!(report.chunks, 0);
    assert_eq!(world.chunk_count(), 0);
    assert_eq!(world.block_count(), 0);
}
