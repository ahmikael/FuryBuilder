//! Tests for read-only analysis and editable block import of classic Java worlds (M8.6).

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use asset_source::AssetCatalogService;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use minecraft_version::builtin_profiles;
use project_store::{
    AssetsComponent, ComponentMode, EditorState, ImportOptions, ProjectComponents, ProjectStore,
    SaveKind, SourceFile,
};
use world_domain::BlockPos;
use world_io::nbt::{self, NbtFile, NbtValue, TAG_COMPOUND, TAG_STRING};
use world_io::world_import::{
    analyze_java_world, load_java_world, load_java_world_with_options, WorldImportOptions,
};
use world_io::world_source::{analyze_world_source, WorldSourceKind};
use world_io::{export_java_world, WorldOverlay};
use zip::write::SimpleFileOptions;

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
    level_dat_bytes_for(data_version, "test")
}

fn level_dat_bytes_for(data_version: i32, version_name: &str) -> Vec<u8> {
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
    chunk_nbt_for(TARGET, "minecraft:full", x, z, sections)
}

fn chunk_nbt_for(
    data_version: i32,
    status: &str,
    x: i32,
    z: i32,
    sections: Vec<NbtValue>,
) -> NbtFile {
    NbtFile::new(
        "",
        NbtValue::compound([
            ("DataVersion", NbtValue::Int(data_version)),
            ("xPos", NbtValue::Int(x)),
            ("zPos", NbtValue::Int(z)),
            ("yPos", NbtValue::Int(-4)),
            ("Status", NbtValue::String(status.to_owned())),
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

fn write_world_for_profile(root: &Path, profile: &minecraft_version::VersionProfile) {
    fs::create_dir_all(root.join("region")).expect("create region");
    fs::write(
        root.join("level.dat"),
        level_dat_bytes_for(profile.data_version, profile.minecraft_version.as_str()),
    )
    .expect("write level.dat");
    fs::write(root.join("session.lock"), b"1234567890123456").expect("write session lock");

    let version = profile.minecraft_version.as_str();
    let status = if version.starts_with("1.18") || version.starts_with("1.19") {
        "full"
    } else {
        "minecraft:full"
    };
    let first = chunk_nbt_for(
        profile.data_version,
        status,
        0,
        0,
        vec![
            section(-4, "minecraft:unknown_machine", &[("powered", "true")]),
            section(-3, "minecraft:stone", &[]),
        ],
    );
    let second = chunk_nbt_for(
        profile.data_version,
        status,
        1,
        0,
        vec![section(-4, "minecraft:dirt", &[])],
    );
    fs::write(
        root.join("region").join("r.0.0.mca"),
        region_bytes(&[(0, 0, first), (1, 0, second)]),
    )
    .expect("write region");
    fs::write(root.join("custom-unknown.dat"), b"keep me").expect("write unknown file");
}

fn write_catalog_jar(root: &Path, profile: &minecraft_version::VersionProfile) -> PathBuf {
    let path = root.join(format!("catalog-{}.jar", profile.minecraft_version));
    let file = fs::File::create(&path).expect("create catalog JAR");
    let mut zip = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default();
    let version = serde_json::json!({
        "id": profile.minecraft_version.as_str(),
        "name": profile.minecraft_version.as_str(),
        "world_version": profile.data_version,
        "pack_version": {
            "resource": profile.resource_pack_format.unwrap_or(0),
            "data": profile.datapack_format.unwrap_or(0),
        },
    });
    let mut files = vec![(
        "version.json".to_owned(),
        serde_json::to_vec(&version).expect("catalog version JSON"),
    )];
    for (id, model) in [
        ("stone", "minecraft:block/stone"),
        ("dirt", "minecraft:block/dirt"),
        ("gold_block", "minecraft:block/gold_block"),
    ] {
        files.push((
            format!("assets/minecraft/blockstates/{id}.json"),
            format!(r#"{{"variants":{{"":{{"model":"{model}"}}}}}}"#).into_bytes(),
        ));
        files.push((
            format!("assets/minecraft/models/block/{id}.json"),
            format!(r#"{{"textures":{{"all":"minecraft:block/{id}"}}}}"#).into_bytes(),
        ));
    }
    files.push((
        "assets/minecraft/lang/en_us.json".to_owned(),
        serde_json::to_vec(&serde_json::json!({
            "block.minecraft.stone": "Stone",
            "block.minecraft.dirt": "Dirt",
            "block.minecraft.gold_block": "Block of Gold",
        }))
        .expect("catalog language JSON"),
    ));
    for (name, bytes) in files {
        zip.start_file(name, options).expect("start catalog entry");
        zip.write_all(&bytes).expect("write catalog entry");
    }
    zip.finish().expect("finish catalog JAR");
    path
}

fn add_zip_directory(
    zip: &mut zip::ZipWriter<fs::File>,
    root: &Path,
    relative: &Path,
    prefix: &str,
    options: SimpleFileOptions,
) {
    for entry in fs::read_dir(root.join(relative)).expect("read archive source directory") {
        let entry = entry.expect("archive source entry");
        let child = relative.join(entry.file_name());
        let metadata = fs::symlink_metadata(entry.path()).expect("archive source metadata");
        if metadata.is_dir() {
            add_zip_directory(zip, root, &child, prefix, options);
        } else {
            let name = child.to_string_lossy().replace('\\', "/");
            zip.start_file(format!("{prefix}/{name}"), options)
                .expect("start archive entry");
            zip.write_all(&fs::read(entry.path()).expect("read archive source file"))
                .expect("write archive entry");
        }
    }
}

fn zip_world(world: &Path, prefix: &str, archive: &Path) {
    let file = fs::File::create(archive).expect("create archive");
    let mut zip = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    add_zip_directory(&mut zip, world, Path::new(""), prefix, options);
    zip.finish().expect("finish archive");
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
    assert_eq!(analysis.block_entities, Some(2));
    assert_eq!(analysis.entities, Some(2));
    assert_eq!(analysis.dimensions.len(), 1);
    assert_eq!(analysis.dimensions[0].id, OVERWORLD);
    assert_eq!(analysis.dimensions[0].region_files, 1);
    assert_eq!(analysis.dimensions[0].chunks, 2);
    assert_eq!(analysis.source_kind, WorldSourceKind::Folder);
    assert_eq!(analysis.source_path, root.display().to_string());
    assert!(analysis.source_bytes > 0);
    assert!(analysis.archive.is_none());
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
fn archive_analysis_extracts_a_nested_world_without_writing_the_archive() {
    let dir = TempDir::new("archive");
    let root = dir.path().join("world");
    write_world(&root, TARGET);
    let archive = dir.path().join("My World.zip");
    zip_world(&root, "My World", &archive);
    let before = fs::read(&archive).expect("read archive");
    let staging = dir.path().join("staging");

    let analysis = analyze_world_source(&archive, &staging);
    assert!(analysis.supported, "blockers: {:?}", analysis.blockers);
    assert_eq!(analysis.source_kind, WorldSourceKind::Archive);
    assert_eq!(analysis.selection_kind, "world_archive");
    assert_eq!(analysis.source_path, archive.display().to_string());
    assert_eq!(analysis.root, archive.display().to_string());
    assert_eq!(analysis.chunks, 2);
    assert!(analysis.source_bytes > 0);
    assert_eq!(
        analysis.archive.as_ref().map(|archive| archive.entries),
        Some(4)
    );
    assert!(analysis
        .warnings
        .iter()
        .any(|warning| warning.contains("temporary folder")));
    assert_eq!(fs::read(&archive).expect("read archive"), before);
    assert!(!staging.exists(), "preview staging must be removed");
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
fn imports_every_modern_classic_release_end_to_end() {
    let mut tested = 0;
    for profile in builtin_profiles()
        .iter()
        .filter(|profile| profile.family == minecraft_version::VersionFamily::Modern)
    {
        let label = profile.minecraft_version.as_str();
        let dir = TempDir::new(&format!("modern-{label}"));
        let root = dir.path().join("world");
        write_world_for_profile(&root, profile);
        let before = snapshot(&root);

        let analysis = analyze_java_world(&root);
        assert!(analysis.supported, "{label}: {:?}", analysis.blockers);
        assert_eq!(analysis.data_version, Some(profile.data_version), "{label}");
        assert_eq!(analysis.version_name.as_deref(), Some(label), "{label}");

        let (world, report) =
            load_java_world(&root).unwrap_or_else(|error| panic!("{label}: {error}"));
        assert_eq!(
            world.version().data_version(),
            profile.data_version,
            "{label}"
        );
        assert_eq!(
            world.version().minecraft_version(),
            label,
            "{label} version identity"
        );
        let expected = profile
            .dimension_profiles
            .iter()
            .find(|dimension| dimension.kind == minecraft_version::DimensionKind::Overworld)
            .expect("overworld profile");
        let dimension = world.dimension(OVERWORLD).expect("overworld");
        assert_eq!(
            (dimension.min_y(), dimension.height()),
            (expected.min_y, expected.height),
            "{label} bounds"
        );
        assert_eq!(report.chunks, 2, "{label}");
        assert_eq!(snapshot(&root), before, "{label}: import must not write");
        tested += 1;
    }
    assert_eq!(tested, 27, "every modern classic release must be covered");
}

#[test]
fn exports_every_modern_classic_release_same_version_and_verifies() {
    let mut tested = 0;
    for profile in builtin_profiles()
        .iter()
        .filter(|profile| profile.family == minecraft_version::VersionFamily::Modern)
    {
        let label = profile.minecraft_version.as_str();
        let dir = TempDir::new(&format!("export-{label}"));
        let root = dir.path().join("world");
        write_world_for_profile(&root, profile);
        let before = snapshot(&root);

        let mut overlay = WorldOverlay::open(&root)
            .unwrap_or_else(|error| panic!("{label}: open overlay: {error}"));
        overlay
            .set_block(
                OVERWORLD,
                0,
                -64,
                0,
                &"minecraft:gold_block".parse().unwrap(),
            )
            .unwrap_or_else(|error| panic!("{label}: edit block: {error}"));
        let destination = dir.path().join("exported");
        let report = export_java_world(&overlay, &destination)
            .unwrap_or_else(|error| panic!("{label}: export: {error}"));

        assert!(report.source_unchanged, "{label}: source changed");
        assert!(report.source_tree_verified, "{label}: source not hashed");
        assert!(
            report.copied_files_match_source,
            "{label}: copied files differ"
        );
        assert!(report.output_revalidated, "{label}: output not revalidated");
        assert_eq!(report.replaced_chunks, 1, "{label}");
        assert_eq!(report.skipped_files, ["session.lock"], "{label}");
        assert_eq!(report.files, before.len() - 1, "{label}");
        assert_eq!(snapshot(&root), before, "{label}: export wrote the source");
        for untouched in ["level.dat", "custom-unknown.dat"] {
            let exported_bytes = fs::read(destination.join(untouched)).expect("read exported file");
            let source_bytes = before
                .get(Path::new(untouched))
                .unwrap_or_else(|| panic!("{label}: missing {untouched}"));
            assert_eq!(
                &exported_bytes, source_bytes,
                "{label}: {untouched} must stay byte-for-byte"
            );
        }

        let (exported, _) = load_java_world(&destination)
            .unwrap_or_else(|error| panic!("{label}: reopen export: {error}"));
        assert_eq!(
            exported.version().data_version(),
            profile.data_version,
            "{label}"
        );
        assert_eq!(
            exported
                .get_block(OVERWORLD, BlockPos::new(0, -64, 0))
                .expect("edited block")
                .to_string(),
            "minecraft:gold_block",
            "{label}"
        );
        assert_eq!(
            exported
                .get_block(OVERWORLD, BlockPos::new(16, -64, 0))
                .expect("untouched neighbour")
                .to_string(),
            "minecraft:dirt",
            "{label}: untouched chunk must survive byte-for-byte"
        );
        tested += 1;
    }
    assert_eq!(tested, 27, "every modern classic release must be covered");
}

#[test]
fn catalog_families_create_edit_save_reopen_export_and_verify() {
    let profiles = builtin_profiles()
        .iter()
        .filter(|profile| {
            matches!(
                profile.minecraft_version.as_str(),
                "1.18.2" | "1.19.4" | "1.20.6" | "1.21.4"
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(profiles.len(), 4, "one representative per modern family");

    for profile in profiles {
        let label = profile.minecraft_version.as_str();
        let dir = TempDir::new(&format!("catalog-family-{label}"));
        let root = dir.path().join("world");
        write_world_for_profile(&root, profile);
        let jar = write_catalog_jar(dir.path(), profile);
        let catalog = AssetCatalogService::new()
            .catalog(&asset_source::AssetSourceSetting {
                kind: "client_jar".to_owned(),
                path: jar.to_string_lossy().into_owned(),
            })
            .unwrap_or_else(|error| panic!("{label}: build catalog: {error}"));
        assert_eq!(catalog.minecraft_version, label, "{label}");
        assert_eq!(catalog.data_version, profile.data_version, "{label}");
        for id in ["minecraft:stone", "minecraft:dirt", "minecraft:gold_block"] {
            let entry = catalog
                .entries
                .iter()
                .find(|entry| entry.id == id)
                .unwrap_or_else(|| panic!("{label}: catalog is missing {id}"));
            assert!(entry.states.is_some(), "{label}: {id} must be a block");
        }
        assert!(
            catalog
                .entries
                .iter()
                .all(|entry| entry.id != "minecraft:unknown_machine"),
            "{label}: non-vanilla state must stay outside the catalog"
        );

        let (world, _) = load_java_world(&root).unwrap_or_else(|error| panic!("{label}: {error}"));
        assert_eq!(
            world.version().data_version(),
            profile.data_version,
            "{label}: editable world version"
        );

        let assets = AssetsComponent {
            mode: ComponentMode::Manual,
            client_jar_sha1: catalog.client_jar_sha1.clone(),
            catalog_id: Some(catalog.catalog_id.clone()),
            catalog_hash: Some(catalog.catalog_hash.clone()),
            pack_stack: Vec::new(),
        };
        let pins = ProjectComponents {
            assets: Some(assets),
            ..ProjectComponents::default()
        };
        let store = ProjectStore::new(dir.path().join("projects"));
        let folder = format!("{label}.furybuilder");
        let detail = store
            .create_imported(
                label,
                EditorState::default(),
                &world,
                "world",
                ImportOptions {
                    folder: Some(&folder),
                    components: pins.clone(),
                },
            )
            .unwrap_or_else(|error| panic!("{label}: create project: {error}"));

        let project_dir = store.root().join(&detail.summary.folder);
        let source_world = project_dir.join(project_store::SOURCE_DIR).join("world");
        let copied = world_io::passthrough_java_world(&root, &source_world)
            .unwrap_or_else(|error| panic!("{label}: copy source: {error}"));
        let files = copied
            .files
            .iter()
            .map(|file| SourceFile {
                relative_path: format!(
                    "world/{}",
                    file.relative_path.to_string_lossy().replace('\\', "/")
                ),
                bytes: file.bytes,
                sha256: file.sha256.clone(),
            })
            .collect::<Vec<_>>();
        store
            .record_source_tree(&folder, "world", &files)
            .unwrap_or_else(|error| panic!("{label}: record source: {error}"));

        let (_, mut opened, _) = store
            .open(&folder)
            .unwrap_or_else(|error| panic!("{label}: open project: {error}"));
        let gold: world_domain::BlockState = "minecraft:gold_block".parse().unwrap();
        opened
            .set_block(OVERWORLD, BlockPos::new(0, -48, 0), &gold)
            .unwrap_or_else(|error| panic!("{label}: edit project: {error}"));
        opened.bump_revision();
        store
            .save(&folder, &opened, EditorState::default(), SaveKind::Manual)
            .unwrap_or_else(|error| panic!("{label}: save project: {error}"));

        let (reopened_detail, reopened, _) = store
            .open(&folder)
            .unwrap_or_else(|error| panic!("{label}: reopen project: {error}"));
        assert_eq!(reopened_detail.components, pins, "{label}: catalog pin");
        let rebuilt = AssetCatalogService::new()
            .catalog(&asset_source::AssetSourceSetting {
                kind: "client_jar".to_owned(),
                path: jar.to_string_lossy().into_owned(),
            })
            .unwrap_or_else(|error| panic!("{label}: rebuild catalog from pin: {error}"));
        let pinned = reopened_detail.components.assets.as_ref().unwrap();
        assert_eq!(
            Some(rebuilt.catalog_id.clone()),
            pinned.catalog_id,
            "{label}: pinned catalog id"
        );
        assert_eq!(
            Some(rebuilt.catalog_hash.clone()),
            pinned.catalog_hash,
            "{label}: pinned catalog hash"
        );
        assert_eq!(
            rebuilt.client_jar_sha1,
            pinned.client_jar_sha1.clone(),
            "{label}: pinned client JAR"
        );
        assert_eq!(
            rebuilt.minecraft_version, label,
            "{label}: rebuilt catalog version"
        );
        assert_eq!(
            rebuilt.data_version, profile.data_version,
            "{label}: rebuilt catalog DataVersion"
        );
        assert_eq!(
            reopened
                .get_block(OVERWORLD, BlockPos::new(0, -48, 0))
                .unwrap(),
            gold,
            "{label}: saved edit"
        );
        assert_eq!(
            reopened
                .get_block(OVERWORLD, BlockPos::new(1, -64, 0))
                .unwrap()
                .name(),
            "minecraft:unknown_machine",
            "{label}: unknown imported state must survive"
        );

        let before = snapshot(&source_world);
        let mut overlay = WorldOverlay::open(&source_world)
            .unwrap_or_else(|error| panic!("{label}: open overlay: {error}"));
        overlay
            .set_block(OVERWORLD, 0, -48, 0, &gold)
            .unwrap_or_else(|error| panic!("{label}: overlay edit: {error}"));
        let destination = dir.path().join("export");
        let report = export_java_world(&overlay, &destination)
            .unwrap_or_else(|error| panic!("{label}: export: {error}"));
        assert!(report.source_unchanged, "{label}: source changed");
        assert!(report.output_revalidated, "{label}: export not revalidated");
        assert_eq!(snapshot(&source_world), before, "{label}: source bytes");

        let (exported, _) = load_java_world(&destination)
            .unwrap_or_else(|error| panic!("{label}: reopen export: {error}"));
        assert_eq!(
            exported
                .get_block(OVERWORLD, BlockPos::new(0, -48, 0))
                .unwrap(),
            gold,
            "{label}: exported edit"
        );
        assert_eq!(
            exported
                .get_block(OVERWORLD, BlockPos::new(1, -64, 0))
                .unwrap()
                .name(),
            "minecraft:unknown_machine",
            "{label}: exported unknown state"
        );
        assert!(
            catalog
                .entries
                .iter()
                .any(|entry| entry.id == "minecraft:gold_block" && entry.states.is_some()),
            "{label}: edited state must be in the exact catalog"
        );
    }
}

#[test]
fn load_preserves_non_vanilla_biomes_and_reports_one_bounded_warning() {
    let dir = TempDir::new("non-vanilla-biomes");
    let root = dir.path().join("world");
    write_world(&root, TARGET);
    let sections = [
        (-4, "example:caves/deep"),
        (-3, "minecraft:mountains"),
        (-2, "minecraft:pale_garden"),
    ]
    .map(|(y, biome)| {
        let NbtValue::Compound(mut fields) = section(y, "minecraft:stone", &[]) else {
            panic!("fixture section must be a compound");
        };
        fields.insert("biomes".to_owned(), biome_palette(biome));
        NbtValue::Compound(fields)
    });
    fs::write(
        root.join("region/r.0.0.mca"),
        region_bytes(&[(0, 0, chunk_nbt(0, 0, sections.into()))]),
    )
    .unwrap();
    let before = snapshot(&root);

    let (world, report) = load_java_world(&root).unwrap();
    for (y, expected) in [
        (-64, "example:caves/deep"),
        (-48, "minecraft:mountains"),
        (-32, "minecraft:pale_garden"),
    ] {
        assert_eq!(
            world
                .get_biome(OVERWORLD, BlockPos::new(0, y, 0))
                .unwrap()
                .unwrap()
                .as_str(),
            expected
        );
    }
    let warnings: Vec<_> = report
        .warnings
        .iter()
        .filter(|warning| warning.contains("biome IDs outside the vanilla"))
        .collect();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].starts_with("2 section(s)"));
    assert!(warnings[0].contains("example:caves/deep"));
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
    write_world(&root, 2730);

    let analysis = analyze_java_world(&root);
    assert!(!analysis.supported);
    assert_eq!(analysis.data_version, Some(2730));
    assert!(analysis
        .blockers
        .iter()
        .any(|blocker| blocker.contains("DataVersion 2730")));
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
