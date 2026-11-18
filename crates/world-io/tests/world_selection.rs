//! Tests for selection classification and layout boundaries (M8.11).
//!
//! A single `.mca` file and a `region/` directory are parts of a save, not saves. The
//! classifier must say so before any `level.dat` path is constructed, and the passthrough
//! copy must refuse every unsupported or incomplete selection.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use flate2::write::ZlibEncoder;
use flate2::Compression;
use world_io::nbt::{self, NbtFile, NbtValue, TAG_COMPOUND, TAG_STRING};
use world_io::{
    analyze_java_world, classify_world_selection, passthrough_java_world, region_file_name,
    WorldSelectionKind,
};

const TARGET: i32 = 4189;

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "furybuilder-selection-{label}-{}-{}",
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
                        ("Name", NbtValue::String("1.21.4".to_owned())),
                    ]),
                ),
                ("LevelName", NbtValue::String("Selection test".to_owned())),
            ]),
        )]),
    );
    nbt::write_gzip(&file).expect("encode level.dat")
}

fn section(y: i8) -> NbtValue {
    NbtValue::compound([
        ("Y", NbtValue::Byte(y)),
        (
            "block_states",
            NbtValue::compound([(
                "palette",
                NbtValue::List {
                    kind: TAG_COMPOUND,
                    items: vec![NbtValue::compound([(
                        "Name",
                        NbtValue::String("minecraft:stone".to_owned()),
                    )])],
                },
            )]),
        ),
        (
            "biomes",
            NbtValue::compound([(
                "palette",
                NbtValue::List {
                    kind: TAG_STRING,
                    items: vec![NbtValue::String("minecraft:plains".to_owned())],
                },
            )]),
        ),
    ])
}

fn chunk_nbt(x: i32, z: i32) -> NbtFile {
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
                    items: vec![section(-4)],
                },
            ),
        ]),
    )
}

fn zlib(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes).expect("zlib write");
    encoder.finish().expect("zlib finish")
}

fn region_bytes(chunks: &[(i32, i32)]) -> Vec<u8> {
    let mut records = Vec::new();
    for (x, z) in chunks {
        let raw = nbt::write_uncompressed(&chunk_nbt(*x, *z)).expect("encode chunk");
        let payload = zlib(&raw);
        let record_len = 5 + payload.len();
        records.push((payload, record_len.div_ceil(4096)));
    }
    let total_sectors = 2 + records.iter().map(|(_, sectors)| *sectors).sum::<usize>();
    let mut file = vec![0u8; total_sectors * 4096];
    let mut offset = 2usize;
    for ((x, z), (payload, sectors)) in chunks.iter().zip(records.iter()) {
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

fn write_classic_world(root: &Path) {
    fs::create_dir_all(root.join("region")).expect("region");
    fs::create_dir_all(root.join("poi")).expect("poi");
    fs::create_dir_all(root.join("entities")).expect("entities");
    fs::write(root.join("level.dat"), level_dat_bytes(TARGET)).expect("level.dat");
    fs::write(
        root.join("region").join(region_file_name(0, 0)),
        region_bytes(&[(0, 0), (1, 0)]),
    )
    .expect("region file");
    fs::write(root.join("poi").join(region_file_name(0, 0)), []).expect("poi");
    fs::write(root.join("entities").join(region_file_name(0, 0)), []).expect("entities");
    fs::write(root.join("session.lock"), b"lock").expect("session.lock");
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
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

#[test]
fn a_single_region_file_is_not_a_world_save() {
    let dir = TempDir::new("region-file");
    let region = dir.path().join(region_file_name(0, 0));
    fs::write(&region, region_bytes(&[(0, 0), (1, 0)])).expect("region file");

    let selection = classify_world_selection(&region);
    assert_eq!(selection.kind, WorldSelectionKind::RegionFile);
    assert!(!selection.complete_save);
    assert!(!selection.supported);
    assert_eq!(selection.layout, "region_file");
    assert_eq!(selection.region_files, 1);
    assert_eq!(selection.chunks, 2);
    let file = selection.region_file.expect("region metadata");
    assert_eq!((file.region_x, file.region_z), (0, 0));
    assert_eq!(file.chunks, 2);
    assert!(
        selection
            .blockers
            .iter()
            .any(|blocker| blocker.contains("not a complete world save")),
        "{:?}",
        selection.blockers
    );

    let analysis = analyze_java_world(&region);
    assert_eq!(analysis.selection_kind, "region_file");
    assert!(!analysis.complete_save);
    assert!(!analysis.supported);
    assert!(
        analysis
            .blockers
            .iter()
            .any(|blocker| blocker.contains("not a complete world save")),
        "{:?}",
        analysis.blockers
    );
}

#[test]
fn a_region_directory_is_not_a_world_save() {
    let dir = TempDir::new("region-dir");
    let world = dir.path().join("world");
    write_classic_world(&world);
    let region_dir = world.join("region");

    let selection = classify_world_selection(&region_dir);
    assert_eq!(selection.kind, WorldSelectionKind::RegionDirectory);
    assert!(!selection.complete_save);
    assert!(!selection.supported);
    assert_eq!(selection.layout, "region_directory");
    assert_eq!(selection.region_files, 1);
    assert!(
        selection
            .blockers
            .iter()
            .any(|blocker| blocker.contains("no level.dat")),
        "{:?}",
        selection.blockers
    );

    let analysis = analyze_java_world(&region_dir);
    assert_eq!(analysis.selection_kind, "region_directory");
    assert!(!analysis.supported);
    assert!(analysis
        .blockers
        .iter()
        .any(|blocker| blocker.contains("no level.dat")));
}

#[test]
fn a_complete_classic_world_is_reported_with_its_dimensions() {
    let dir = TempDir::new("classic");
    let world = dir.path().join("world");
    write_classic_world(&world);

    let selection = classify_world_selection(&world);
    assert_eq!(selection.kind, WorldSelectionKind::WorldFolder);
    assert!(selection.complete_save);
    assert!(selection.supported, "{:?}", selection.blockers);
    assert_eq!(selection.layout, "classic_anvil");
    assert!(selection.has_level_dat);
    assert_eq!(selection.data_version, Some(TARGET));
    assert_eq!(selection.version_name.as_deref(), Some("1.21.4"));
    assert_eq!(selection.related_folders, ["region", "poi", "entities"]);
    assert_eq!(selection.dimensions.len(), 1);
    assert_eq!(selection.dimensions[0].id, "minecraft:overworld");
    assert_eq!(selection.dimensions[0].folder, ".");
    assert_eq!(selection.dimensions[0].region_files, 1);
    assert_eq!(selection.dimensions[0].chunks, 2);
    assert_eq!(selection.region_files, 1);
    assert_eq!(selection.chunks, 2);

    let analysis = analyze_java_world(&world);
    assert_eq!(analysis.selection_kind, "world_folder");
    assert!(analysis.complete_save);
    assert!(analysis.supported, "{:?}", analysis.blockers);
}

#[test]
fn layouts_are_distinguished_before_import() {
    let dir = TempDir::new("layouts");

    let split = dir.path().join("split");
    fs::create_dir_all(split.join("dimensions").join("minecraft").join("overworld"))
        .expect("split dimensions");
    fs::write(split.join("level.dat"), level_dat_bytes(5023)).expect("split level.dat");
    fs::write(
        split
            .join("dimensions")
            .join("minecraft")
            .join("overworld")
            .join(region_file_name(0, 0)),
        region_bytes(&[(0, 0)]),
    )
    .expect("split region");
    let selection = classify_world_selection(&split);
    assert_eq!(selection.kind, WorldSelectionKind::WorldFolder);
    assert!(selection.complete_save);
    assert!(!selection.supported);
    assert_eq!(selection.layout, "split_26_1_plus");
    assert!(selection
        .blockers
        .iter()
        .any(|blocker| blocker.contains("26.1+")));

    let mixed = dir.path().join("mixed");
    write_classic_world(&mixed);
    fs::create_dir_all(mixed.join("dimensions").join("minecraft").join("overworld"))
        .expect("mixed split marker");
    let selection = classify_world_selection(&mixed);
    assert_eq!(selection.layout, "mixed");
    assert!(!selection.supported);

    let legacy = dir.path().join("legacy");
    fs::create_dir_all(legacy.join("region")).expect("legacy region");
    fs::write(legacy.join("level.dat"), level_dat_bytes(TARGET)).expect("legacy level.dat");
    fs::write(legacy.join("region").join("r.0.0.mcr"), b"legacy").expect("mcr");
    let selection = classify_world_selection(&legacy);
    assert_eq!(selection.kind, WorldSelectionKind::WorldFolder);
    assert!(selection.complete_save);
    assert!(!selection.supported);
    assert_eq!(selection.layout, "legacy_mcr");
    assert!(selection
        .blockers
        .iter()
        .any(|blocker| blocker.contains("legacy")));

    let metadata_only = dir.path().join("metadata-only");
    fs::create_dir_all(&metadata_only).expect("metadata only");
    fs::write(metadata_only.join("level.dat"), level_dat_bytes(TARGET)).expect("level.dat");
    let selection = classify_world_selection(&metadata_only);
    assert_eq!(selection.kind, WorldSelectionKind::WorldFolder);
    assert!(!selection.complete_save);
    assert!(!selection.supported);
    assert_eq!(selection.layout, "level_dat_only");

    let unknown = dir.path().join("unknown");
    fs::create_dir_all(unknown.join("notes")).expect("unknown folder");
    let selection = classify_world_selection(&unknown);
    assert_eq!(selection.kind, WorldSelectionKind::Unknown);
    assert!(!selection.complete_save);
    assert!(!selection.blockers.is_empty());
}

#[test]
fn passthrough_refuses_incomplete_and_unsupported_selections_without_publishing() {
    let dir = TempDir::new("passthrough");

    let region_file = dir.path().join(region_file_name(0, 0));
    fs::write(&region_file, region_bytes(&[(0, 0)])).expect("region file");
    let error = passthrough_java_world(&region_file, dir.path().join("from-file"))
        .expect_err("a region file is not a save");
    assert_eq!(error.code(), "unsupported_format", "{error}");
    assert!(!dir.path().join("from-file").exists());

    let world = dir.path().join("world");
    write_classic_world(&world);
    let before = snapshot(&world);
    let error = passthrough_java_world(world.join("region"), dir.path().join("from-region-dir"))
        .expect_err("a region directory is not a save");
    assert_eq!(error.code(), "unsupported_format", "{error}");
    assert_eq!(snapshot(&world), before);
    assert!(!dir.path().join("from-region-dir").exists());

    let split = dir.path().join("split");
    fs::create_dir_all(split.join("dimensions").join("minecraft").join("overworld"))
        .expect("split dimensions");
    fs::write(split.join("level.dat"), level_dat_bytes(5023)).expect("split level.dat");
    let split_before = snapshot(&split);
    let error = passthrough_java_world(&split, dir.path().join("from-split"))
        .expect_err("split layout is not supported");
    assert_eq!(error.code(), "unsupported_version", "{error}");
    assert_eq!(snapshot(&split), split_before);
    assert!(!dir.path().join("from-split").exists());

    let legacy = dir.path().join("legacy");
    fs::create_dir_all(legacy.join("region")).expect("legacy region");
    fs::write(legacy.join("level.dat"), level_dat_bytes(TARGET)).expect("legacy level.dat");
    fs::write(legacy.join("region").join("r.0.0.mcr"), b"legacy").expect("mcr");
    let legacy_before = snapshot(&legacy);
    let error = passthrough_java_world(&legacy, dir.path().join("from-legacy"))
        .expect_err("legacy layout is not supported");
    assert_eq!(error.code(), "unsupported_version", "{error}");
    assert_eq!(snapshot(&legacy), legacy_before);
    assert!(!dir.path().join("from-legacy").exists());
}
