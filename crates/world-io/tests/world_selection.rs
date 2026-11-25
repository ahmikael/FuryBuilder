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
    analyze_java_world, classify_world_selection, load_java_world, passthrough_java_world,
    region_file_name, WorldOverlay, WorldSelectionKind,
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
    level_dat_bytes_named(data_version, "1.21.4")
}

fn level_dat_bytes_named(data_version: i32, version_name: &str) -> Vec<u8> {
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
                        ("Name", NbtValue::String(version_name.to_owned())),
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

fn write_split_world(root: &Path, data_version: i32, version: &str) {
    fs::create_dir_all(root.join("dimensions/minecraft/overworld/region")).expect("overworld");
    fs::write(
        root.join("level.dat"),
        level_dat_bytes_named(data_version, version),
    )
    .expect("level.dat");
    let data = root.join("data/minecraft");
    fs::create_dir_all(&data).expect("shared data");
    let world_gen = NbtFile::new(
        "",
        NbtValue::compound([
            ("DataVersion", NbtValue::Int(data_version)),
            (
                "data",
                NbtValue::compound([
                    ("seed", NbtValue::Long(42)),
                    (
                        "dimensions",
                        NbtValue::compound([
                            ("example:deep/caves", NbtValue::Compound(BTreeMap::new())),
                            ("minecraft:overworld", NbtValue::Compound(BTreeMap::new())),
                        ]),
                    ),
                ]),
            ),
        ]),
    );
    fs::write(
        data.join("world_gen_settings.dat"),
        nbt::write_gzip(&world_gen).unwrap(),
    )
    .unwrap();
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
    write_split_world(&split, 5023, "26.3");
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
    assert!(!selection.complete_save);
    assert!(selection.dimensions.is_empty());
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
    write_split_world(&split, 5023, "26.3");
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

#[test]
fn split_inventory_reports_paths_and_terrain_headers_without_admitting_world_io() {
    for (version, data_version) in [
        ("26.1", 4786),
        ("26.1.1", 4788),
        ("26.1.2", 4790),
        ("26.2", 4903),
        ("26.3", 5023),
    ] {
        let dir = TempDir::new(version);
        let root = dir.path().join("world");
        write_split_world(&root, data_version, version);
        for folder in [
            "dimensions/minecraft/the_nether/region",
            "dimensions/minecraft/the_end/region",
            "dimensions/example/deep/caves/region",
            "dimensions/minecraft/overworld/entities",
            "dimensions/minecraft/overworld/poi",
            "players/data",
            "players/advancements",
            "players/stats",
        ] {
            fs::create_dir_all(root.join(folder)).unwrap();
        }
        let mut terrain = region_bytes(&[(0, 0), (1, 0)]);
        // An allocated slot with deliberately undecodable data still counts in a header-only report.
        terrain[8192 + 4] = 0;
        fs::write(
            root.join("dimensions/minecraft/overworld/region/r.0.0.mca"),
            &terrain,
        )
        .unwrap();
        fs::write(
            root.join("dimensions/example/deep/caves/region/r.-1.0.mca"),
            region_bytes(&[(-1, 0)]),
        )
        .unwrap();
        fs::write(
            root.join("dimensions/minecraft/overworld/entities/r.0.0.mca"),
            b"opaque",
        )
        .unwrap();
        fs::write(
            root.join("dimensions/minecraft/overworld/poi/r.0.0.mca"),
            b"opaque",
        )
        .unwrap();
        fs::write(root.join("players/data/player.dat"), b"opaque").unwrap();
        let before = snapshot(&root);

        let selection = classify_world_selection(&root);
        assert!(
            selection.complete_save && !selection.supported,
            "{:?}",
            selection.blockers
        );
        assert_eq!((selection.region_files, selection.chunks), (2, 3));
        assert_eq!(
            selection.related_folders,
            [
                "dimensions",
                "data",
                "players",
                "players/data",
                "players/advancements",
                "players/stats"
            ]
        );
        assert_eq!(
            selection.dimensions[0].folder,
            "dimensions/example/deep/caves"
        );
        let analysis = analyze_java_world(&root);
        assert!(analysis.complete_save && !analysis.supported);
        assert_eq!(analysis.version_id, Some(data_version));
        assert_eq!(analysis.version_name.as_deref(), Some(version));
        assert_eq!(
            analysis.declared_dimensions,
            ["example:deep/caves", "minecraft:overworld"]
        );
        assert_eq!((analysis.regions, analysis.chunks), (2, 3));
        assert_eq!(analysis.dimensions.len(), 4);
        assert_eq!(analysis.dimensions[0].id, "example:deep/caves");
        assert_eq!(
            analysis.dimensions[0].kind,
            world_domain::DimensionKind::Custom
        );
        assert_eq!(analysis.dimensions[0].min_y, None);
        assert_eq!(analysis.block_entities, None);
        assert_eq!(analysis.entities, None);
        assert!(analysis
            .warnings
            .iter()
            .any(|warning| warning.contains("players/data")));
        assert_eq!(
            load_java_world(&root).unwrap_err().code(),
            "unsupported_version"
        );
        assert_eq!(
            WorldOverlay::open(&root)
                .expect_err("split overlay is blocked")
                .code(),
            "unsupported_version"
        );
        assert_eq!(snapshot(&root), before);
    }
}

#[test]
fn split_incomplete_and_contradictory_saves_are_not_complete() {
    let dir = TempDir::new("split-invalid");
    let root = dir.path().join("world");
    write_split_world(&root, 5023, "26.3");
    let overworld = root.join("dimensions/minecraft/overworld");
    let region = overworld.join("region");
    fs::remove_dir(&region).unwrap();
    fs::write(overworld.join("r.0.0.mca"), region_bytes(&[(0, 0)])).unwrap();
    let report = classify_world_selection(&root);
    assert!(!report.complete_save);
    assert_eq!(report.chunks, 0);
    assert!(report
        .blockers
        .iter()
        .any(|b| b.contains("Overworld terrain folder")));

    fs::create_dir(&region).unwrap();
    fs::write(region.join("r.0.0.mca"), b"broken header").unwrap();
    let report = classify_world_selection(&root);
    assert!(!report.complete_save);
    assert!(report
        .blockers
        .iter()
        .any(|b| b.contains("region file is invalid")));

    fs::remove_file(region.join("r.0.0.mca")).unwrap();
    fs::write(root.join("level.dat"), level_dat_bytes_named(5023, "26.2")).unwrap();
    let report = classify_world_selection(&root);
    assert!(!report.complete_save);
    assert!(report
        .blockers
        .iter()
        .any(|b| b.contains("identity disagree")));

    fs::write(root.join("level.dat"), level_dat_bytes_named(5023, "26.3")).unwrap();
    fs::remove_file(root.join("data/minecraft/world_gen_settings.dat")).unwrap();
    let report = classify_world_selection(&root);
    assert!(!report.complete_save);
    assert!(report
        .blockers
        .iter()
        .any(|b| b.contains("world_gen_settings.dat")));

    fs::write(
        root.join("data/minecraft/world_gen_settings.dat"),
        b"not NBT",
    )
    .unwrap();
    let report = classify_world_selection(&root);
    assert!(!report.complete_save);
    assert!(report
        .blockers
        .iter()
        .any(|b| b.contains("world_gen_settings.dat")));
}

#[test]
fn split_metadata_prevents_classic_admission_even_without_split_folders() {
    let dir = TempDir::new("split-metadata");
    fs::write(
        dir.path().join("level.dat"),
        level_dat_bytes_named(4786, "26.1"),
    )
    .unwrap();
    let report = classify_world_selection(dir.path());
    assert_eq!(report.layout, "split_26_1_plus");
    assert!(!report.complete_save);
    fs::create_dir(dir.path().join("DIM-1")).unwrap();
    let report = classify_world_selection(dir.path());
    assert_eq!(report.layout, "mixed");
    assert!(!report.complete_save);
    assert!(!report.supported);
}

#[test]
fn split_terrain_inventory_rejects_conflicting_region_names_and_overdeep_paths() {
    let dir = TempDir::new("split-limits");
    let root = dir.path();
    write_split_world(root, 4786, "26.1");
    let region = root.join("dimensions/minecraft/overworld/region");
    let empty_header = region_bytes(&[]);
    fs::write(region.join("r.0.0.mca"), &empty_header).unwrap();
    fs::write(region.join("r.00.0.mca"), &empty_header).unwrap();
    let report = classify_world_selection(root);
    assert!(!report.complete_save);
    assert!(report
        .blockers
        .iter()
        .any(|b| b.contains("duplicate region coordinates")));

    fs::remove_file(region.join("r.00.0.mca")).unwrap();
    let deep = root.join("dimensions/example").join("a/".repeat(20));
    fs::create_dir_all(deep).unwrap();
    let report = classify_world_selection(root);
    assert!(!report.complete_save);
    assert!(report
        .blockers
        .iter()
        .any(|b| b.contains("split dimension path is too deep")));
}

#[cfg(any(unix, windows))]
#[test]
fn split_inventory_does_not_follow_a_linked_dimension_parent() {
    let dir = TempDir::new("split-link");
    let target = dir.path().join("outside");
    write_split_world(&target, 5023, "26.3");
    let root = dir.path().join("world");
    fs::create_dir_all(root.join("data")).unwrap();
    fs::copy(target.join("level.dat"), root.join("level.dat")).unwrap();
    let link = root.join("dimensions");
    let target = target.join("dimensions");
    #[cfg(windows)]
    assert!(std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&link)
        .arg(&target)
        .output()
        .unwrap()
        .status
        .success());
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let report = classify_world_selection(&root);
    assert_eq!(report.layout, "split_26_1_plus");
    assert!(!report.complete_save);
    assert!(report.dimensions.is_empty());
    assert!(report
        .blockers
        .iter()
        .any(|b| b.contains("link or reparse point")));
    #[cfg(windows)]
    fs::remove_dir(link).unwrap();
    #[cfg(unix)]
    fs::remove_file(link).unwrap();
}
