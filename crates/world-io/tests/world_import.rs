//! Tests for read-only analysis and editable block import of classic Java worlds (M8.6).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use asset_source::AssetCatalogService;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use minecraft_version::{
    builtin_profiles, flattened_biomes::FlattenedBiomeRegistry, BiomeFormat, VersionFamily,
    VersionProfile,
};
use project_store::{
    AssetsComponent, ComponentMode, EditorState, ImportOptions, ProjectComponents, ProjectStore,
    SaveKind, SourceFile,
};
use sha1::{Digest, Sha1};
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

fn flattened_block_palette(states: &[(&str, &[(&str, &str)])]) -> NbtValue {
    let NbtValue::Compound(mut wrapper) = block_palette(states) else {
        panic!("block palette helper must return a compound");
    };
    wrapper.remove("palette").expect("block palette list")
}

fn flattened_section(y: i8, name: &str, properties: &[(&str, &str)]) -> NbtValue {
    NbtValue::compound([
        ("Y", NbtValue::Byte(y)),
        ("Palette", flattened_block_palette(&[(name, properties)])),
        ("BlockStates", NbtValue::LongArray(vec![0; 256])),
    ])
}

fn flattened_biomes(profile: &VersionProfile) -> NbtValue {
    let registry = FlattenedBiomeRegistry::for_release(&profile.minecraft_version)
        .unwrap_or_else(|| panic!("{}: flattened biome registry", profile.minecraft_version));
    let plains = registry
        .id("minecraft:plains")
        .expect("plains in the flattened biome registry");
    let count = if registry.is_volumetric() {
        16 * 64
    } else {
        256
    };
    NbtValue::IntArray(vec![plains; count])
}

fn flattened_full_status(profile: &VersionProfile) -> &'static str {
    if profile.data_version < 1952 {
        "postprocessed"
    } else {
        "full"
    }
}

fn flattened_chunk_nbt(
    profile: &VersionProfile,
    x: i32,
    z: i32,
    sections: Vec<NbtValue>,
) -> NbtFile {
    NbtFile::new(
        "",
        NbtValue::compound([
            ("DataVersion", NbtValue::Int(profile.data_version)),
            (
                "Level",
                NbtValue::compound([
                    ("xPos", NbtValue::Int(x)),
                    ("zPos", NbtValue::Int(z)),
                    (
                        "Status",
                        NbtValue::String(flattened_full_status(profile).to_owned()),
                    ),
                    (
                        "Sections",
                        NbtValue::List {
                            kind: TAG_COMPOUND,
                            items: sections,
                        },
                    ),
                    ("Biomes", flattened_biomes(profile)),
                    (
                        "TileEntities",
                        NbtValue::List {
                            kind: TAG_COMPOUND,
                            items: vec![NbtValue::compound([(
                                "id",
                                NbtValue::String("minecraft:chest".to_owned()),
                            )])],
                        },
                    ),
                    (
                        "Entities",
                        NbtValue::List {
                            kind: TAG_COMPOUND,
                            items: vec![NbtValue::compound([(
                                "id",
                                NbtValue::String("minecraft:item".to_owned()),
                            )])],
                        },
                    ),
                    ("UnknownLevelTag", NbtValue::String("preserve".to_owned())),
                ]),
            ),
            ("UnknownRootTag", NbtValue::String("preserve".to_owned())),
        ]),
    )
}

fn write_flattened_world_for_profile(root: &Path, profile: &VersionProfile) {
    fs::create_dir_all(root.join("region")).expect("create region");
    fs::write(
        root.join("level.dat"),
        level_dat_bytes_for(profile.data_version, profile.minecraft_version.as_str()),
    )
    .expect("write level.dat");
    fs::write(root.join("session.lock"), b"1234567890123456").expect("write session lock");

    let first = flattened_chunk_nbt(
        profile,
        0,
        0,
        vec![
            flattened_section(0, "minecraft:unknown_machine", &[("powered", "true")]),
            flattened_section(1, "minecraft:stone", &[]),
        ],
    );
    let second = flattened_chunk_nbt(
        profile,
        1,
        0,
        vec![flattened_section(0, "minecraft:dirt", &[])],
    );
    fs::write(
        root.join("region").join("r.0.0.mca"),
        region_bytes(&[(0, 0, first), (1, 0, second)]),
    )
    .expect("write region");
    fs::write(root.join("custom-unknown.dat"), b"keep me").expect("write unknown file");
}

const TINY_PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0a, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae,
    0x42, 0x60, 0x82,
];

fn catalog_json(value: serde_json::Value) -> Vec<u8> {
    serde_json::to_vec(&value).expect("serialize catalog fixture JSON")
}

fn catalog_display_name(id: &str) -> &'static str {
    match id {
        "stone" => "Stone",
        "dirt" => "Dirt",
        "gold_block" => "Block of Gold",
        "oak_stairs" => "Oak Stairs",
        "water" => "Water",
        "item_frame" => "Item Frame",
        "diamond_sword" => "Diamond Sword",
        "beehive" => "Beehive",
        "netherite_block" => "Block of Netherite",
        "copper_block" => "Block of Copper",
        other => panic!("no catalog display name for {other}"),
    }
}

fn write_catalog_launcher_metadata(jar: &Path, profile: &VersionProfile) {
    let bytes = fs::read(jar).expect("read catalog JAR for launcher metadata");
    let digest = Sha1::digest(&bytes);
    let sha1: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    let metadata = serde_json::json!({
        "id": profile.minecraft_version.as_str(),
        "type": "release",
        "downloads": {
            "client": {
                "sha1": sha1,
                "size": bytes.len(),
                "url": "https://piston-data.mojang.com/v1/objects/client.jar",
            }
        }
    });
    fs::write(
        jar.with_extension("json"),
        serde_json::to_vec(&metadata).expect("catalog launcher metadata JSON"),
    )
    .expect("write catalog launcher metadata");
}

/// Synthetic exact-version client JAR. It follows the real asset layout of the profile:
/// 1.13.x has no internal `version.json` and relies on adjacent launcher metadata, 1.14–1.14.2
/// carries a build-suffixed id, 1.14–1.16 uses a numeric `pack_version`, and 1.17+ uses the
/// split resource/data object. Blockstates, item models, plural tags and entity-backed
/// blockstates are all represented so the catalog can be checked end to end.
fn write_catalog_jar(root: &Path, profile: &VersionProfile) -> PathBuf {
    let path = root.join(format!("catalog-{}.jar", profile.minecraft_version));
    let file = fs::File::create(&path).expect("create catalog JAR");
    let mut zip = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default();
    let version = profile.minecraft_version.as_str();
    let flattened = profile.family == VersionFamily::Flattened;
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();

    // 1.13.x client JARs are the only flattened releases without a root version.json.
    if !(flattened && version.starts_with("1.13")) {
        let internal_id = if flattened && matches!(version, "1.14" | "1.14.1" | "1.14.2") {
            let build = match version {
                "1.14" => "5dac5567e13e46bdb0c1d90aa8d8b3f7",
                "1.14.1" => "a8f78b0d43c74598a199d6d80cda413f",
                _ => "f647ba8dc371474797bee24b2b312ff4",
            };
            format!("{version} / {build}")
        } else {
            version.to_owned()
        };
        let pack_version = if flattened && profile.data_version < 2724 {
            serde_json::json!(profile.resource_pack_format.unwrap_or(0))
        } else {
            serde_json::json!({
                "resource": profile.resource_pack_format.unwrap_or(0),
                "data": profile.datapack_format.unwrap_or(0),
            })
        };
        files.push((
            "version.json".to_owned(),
            catalog_json(serde_json::json!({
                "id": internal_id,
                "name": version,
                "world_version": profile.data_version,
                "pack_version": pack_version,
            })),
        ));
    }

    let mut extras = Vec::new();
    if profile.data_version >= 2225 {
        extras.push(("beehive", "beehive_side"));
    }
    if profile.data_version >= 2566 {
        extras.push(("netherite_block", "netherite_block"));
    }
    if profile.data_version >= 2724 {
        extras.push(("copper_block", "copper_block"));
    }
    let mut blocks = vec![
        "stone",
        "dirt",
        "gold_block",
        "oak_stairs",
        "water",
        "item_frame",
    ];
    blocks.extend(extras.iter().map(|(id, _)| *id));

    let mut lang = serde_json::Map::new();
    for id in blocks.iter().copied() {
        let model = format!("minecraft:block/{id}");
        let blockstate = if id == "oak_stairs" {
            let mut variants = serde_json::Map::new();
            for facing in ["north", "east", "south", "west"] {
                for half in ["bottom", "top"] {
                    for shape in [
                        "straight",
                        "inner_left",
                        "inner_right",
                        "outer_left",
                        "outer_right",
                    ] {
                        for waterlogged in ["false", "true"] {
                            variants.insert(
                                format!(
                                    "facing={facing},half={half},shape={shape},waterlogged={waterlogged}"
                                ),
                                serde_json::json!({ "model": model.clone() }),
                            );
                        }
                    }
                }
            }
            serde_json::json!({ "variants": variants })
        } else {
            serde_json::json!({ "variants": { "": { "model": model.clone() } } })
        };
        files.push((
            format!("assets/minecraft/blockstates/{id}.json"),
            catalog_json(blockstate),
        ));

        let texture = extras
            .iter()
            .find(|(extra_id, _)| *extra_id == id)
            .map(|(_, texture)| *texture)
            .unwrap_or(match id {
                "oak_stairs" => "oak_planks",
                "water" => "water_still",
                other => other,
            });
        files.push((
            format!("assets/minecraft/models/block/{id}.json"),
            catalog_json(serde_json::json!({
                "textures": { "all": format!("minecraft:block/{texture}") }
            })),
        ));
        lang.insert(
            format!("block.minecraft.{id}"),
            serde_json::json!(catalog_display_name(id)),
        );
        if id != "water" {
            let item_model = if id == "item_frame" {
                serde_json::json!({
                    "textures": { "layer0": "minecraft:item/item_frame" }
                })
            } else {
                serde_json::json!({ "parent": model })
            };
            files.push((
                format!("assets/minecraft/models/item/{id}.json"),
                catalog_json(item_model),
            ));
            lang.insert(
                format!("item.minecraft.{id}"),
                serde_json::json!(catalog_display_name(id)),
            );
        }
    }

    files.push((
        "assets/minecraft/models/item/diamond_sword.json".to_owned(),
        catalog_json(serde_json::json!({
            "textures": { "layer0": "minecraft:item/diamond_sword" }
        })),
    ));
    lang.insert(
        "item.minecraft.diamond_sword".to_owned(),
        serde_json::json!("Diamond Sword"),
    );
    // An empty item model must never become an item.
    files.push((
        "assets/minecraft/models/item/air.json".to_owned(),
        catalog_json(serde_json::json!({})),
    ));
    lang.insert("item.minecraft.air".to_owned(), serde_json::json!("Air"));

    let mut textures = vec![
        "block/stone".to_owned(),
        "block/dirt".to_owned(),
        "block/gold_block".to_owned(),
        "block/oak_planks".to_owned(),
        "block/water_still".to_owned(),
        "block/item_frame".to_owned(),
        "item/diamond_sword".to_owned(),
        "item/item_frame".to_owned(),
    ];
    for (_, texture) in &extras {
        textures.push(format!("block/{texture}"));
    }
    for texture in &textures {
        files.push((
            format!("assets/minecraft/textures/{texture}.png"),
            TINY_PNG.to_vec(),
        ));
    }

    let block_tag_folder = if flattened { "blocks" } else { "block" };
    files.push((
        format!("data/minecraft/tags/{block_tag_folder}/stairs.json"),
        catalog_json(serde_json::json!({ "values": ["minecraft:oak_stairs"] })),
    ));
    let item_tag_folder = if flattened { "items" } else { "item" };
    files.push((
        format!("data/minecraft/tags/{item_tag_folder}/planks.json"),
        catalog_json(serde_json::json!({ "values": ["minecraft:oak_stairs"] })),
    ));

    files.push((
        "assets/minecraft/lang/en_us.json".to_owned(),
        catalog_json(serde_json::Value::Object(lang)),
    ));

    for (name, bytes) in files {
        zip.start_file(name, options).expect("start catalog entry");
        zip.write_all(&bytes).expect("write catalog entry");
    }
    zip.finish().expect("finish catalog JAR");
    write_catalog_launcher_metadata(&path, profile);
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
fn imports_every_flattened_classic_release_end_to_end() {
    let mut tested = 0;
    for profile in builtin_profiles()
        .iter()
        .filter(|profile| profile.family == VersionFamily::Flattened)
    {
        let label = profile.minecraft_version.as_str();
        let dir = TempDir::new(&format!("flattened-import-{label}"));
        let root = dir.path().join("world");
        write_flattened_world_for_profile(&root, profile);
        let before = snapshot(&root);

        let analysis = analyze_java_world(&root);
        assert!(analysis.supported, "{label}: {:?}", analysis.blockers);
        assert_eq!(analysis.data_version, Some(profile.data_version), "{label}");
        assert_eq!(analysis.version_name.as_deref(), Some(label), "{label}");
        assert_eq!(analysis.block_entities, Some(2), "{label}");
        assert_eq!(analysis.entities, Some(2), "{label}");

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
        assert_eq!(report.non_full_chunks, 0, "{label}");
        assert_eq!(report.sections, 3, "{label}");
        assert_eq!(report.biome_sections, 3, "{label}");
        assert_eq!(report.block_entities, 2, "{label}");
        assert_eq!(report.entities, 2, "{label}");
        assert_eq!(
            world
                .get_block(OVERWORLD, BlockPos::new(0, 0, 0))
                .expect("unknown block")
                .to_string(),
            "minecraft:unknown_machine[powered=true]",
            "{label}"
        );
        assert_eq!(
            world
                .get_block(OVERWORLD, BlockPos::new(1, 17, 3))
                .expect("stone")
                .to_string(),
            "minecraft:stone",
            "{label}"
        );
        assert_eq!(
            world
                .get_biome(OVERWORLD, BlockPos::new(1, 17, 3))
                .expect("biome")
                .expect("biome data")
                .as_str(),
            "minecraft:plains",
            "{label}"
        );
        assert_eq!(
            snapshot(&root),
            before,
            "{label}: import must not write the source"
        );
        tested += 1;
    }
    assert_eq!(
        tested, 19,
        "every flattened classic release must be covered"
    );
}

#[test]
fn flattened_mixed_columns_warn_after_center_sampling() {
    let profile = builtin_profiles()
        .iter()
        .find(|profile| profile.minecraft_version.as_str() == "1.13.2")
        .expect("1.13.2 profile");
    let dir = TempDir::new("flattened-mixed");
    let root = dir.path().join("world");
    fs::create_dir_all(root.join("region")).expect("create region");
    fs::write(
        root.join("level.dat"),
        level_dat_bytes_for(profile.data_version, profile.minecraft_version.as_str()),
    )
    .expect("write level.dat");

    let registry = FlattenedBiomeRegistry::for_release(&profile.minecraft_version)
        .expect("1.13.2 biome registry");
    let plains = registry.id("minecraft:plains").expect("plains");
    let desert = registry.id("minecraft:desert").expect("desert");
    let mut biomes = vec![plains; 256];
    biomes[0] = desert;
    let mut chunk = flattened_chunk_nbt(
        profile,
        0,
        0,
        vec![flattened_section(0, "minecraft:stone", &[])],
    );
    let NbtValue::Compound(root_map) = &mut chunk.root else {
        panic!("chunk root compound");
    };
    let NbtValue::Compound(level) = root_map.get_mut("Level").expect("Level") else {
        panic!("Level compound");
    };
    level.insert("Biomes".to_owned(), NbtValue::IntArray(biomes));
    fs::write(
        root.join("region/r.0.0.mca"),
        region_bytes(&[(0, 0, chunk)]),
    )
    .expect("write region");

    let (world, report) = load_java_world(&root).expect("import mixed columns");
    assert_eq!(
        world
            .get_biome(OVERWORLD, BlockPos::new(0, 0, 0))
            .expect("biome")
            .expect("biome data")
            .as_str(),
        "minecraft:plains",
        "the center column wins"
    );
    let warnings = report
        .warnings
        .iter()
        .filter(|warning| warning.contains("mixed 1.13-1.14"))
        .collect::<Vec<_>>();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("1 mixed"));
}

#[test]
fn flattened_column_edit_refuses_different_values_by_height() {
    let profile = builtin_profiles()
        .iter()
        .find(|profile| profile.minecraft_version.as_str() == "1.13.2")
        .expect("1.13.2 profile");
    let dir = TempDir::new("flattened-column-conflict");
    let root = dir.path().join("world");
    write_flattened_world_for_profile(&root, profile);

    let mut overlay = WorldOverlay::open(&root).expect("open overlay");
    overlay
        .set_biome(OVERWORLD, 0, 0, 0, &"minecraft:desert".parse().unwrap())
        .expect("lower biome edit");
    overlay
        .set_biome(OVERWORLD, 0, 16, 0, &"minecraft:forest".parse().unwrap())
        .expect("upper biome edit");
    let error = export_java_world(&overlay, dir.path().join("exported"))
        .expect_err("a column cannot store two biomes by height");
    assert!(error.to_string().contains("different values"), "{error}");
}

#[test]
fn exports_every_flattened_classic_release_same_version_and_verifies() {
    let mut tested = 0;
    for profile in builtin_profiles()
        .iter()
        .filter(|profile| profile.family == VersionFamily::Flattened)
    {
        let label = profile.minecraft_version.as_str();
        let dir = TempDir::new(&format!("flattened-export-{label}"));
        let root = dir.path().join("world");
        write_flattened_world_for_profile(&root, profile);
        let before = snapshot(&root);

        let mut overlay = WorldOverlay::open(&root)
            .unwrap_or_else(|error| panic!("{label}: open overlay: {error}"));
        overlay
            .set_block(OVERWORLD, 0, 0, 0, &"minecraft:gold_block".parse().unwrap())
            .unwrap_or_else(|error| panic!("{label}: edit block: {error}"));
        overlay
            .set_biome(OVERWORLD, 0, 0, 0, &"minecraft:desert".parse().unwrap())
            .unwrap_or_else(|error| panic!("{label}: edit biome: {error}"));
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

        let written = world_io::RegionFile::open(destination.join("region/r.0.0.mca"))
            .unwrap_or_else(|error| panic!("{label}: open written region: {error}"));
        let chunk = written
            .read_chunk(0, 0)
            .unwrap_or_else(|error| panic!("{label}: read written chunk: {error}"));
        let root_nbt = chunk.nbt.root.as_compound().expect("chunk root compound");
        assert_eq!(
            root_nbt.get("UnknownRootTag").and_then(NbtValue::as_string),
            Some("preserve"),
            "{label}: unknown root tag"
        );
        let level = root_nbt
            .get("Level")
            .and_then(NbtValue::as_compound)
            .expect("Level compound");
        assert_eq!(
            level.get("UnknownLevelTag").and_then(NbtValue::as_string),
            Some("preserve"),
            "{label}: unknown Level tag"
        );
        assert_eq!(
            level.get("Status").and_then(NbtValue::as_string),
            Some(flattened_full_status(profile)),
            "{label}: Status"
        );

        let (exported, exported_report) = load_java_world(&destination)
            .unwrap_or_else(|error| panic!("{label}: reopen export: {error}"));
        assert_eq!(
            exported.version().data_version(),
            profile.data_version,
            "{label}"
        );
        assert_eq!(exported_report.block_entities, 2, "{label}");
        assert_eq!(exported_report.entities, 2, "{label}");
        assert_eq!(
            exported
                .get_block(OVERWORLD, BlockPos::new(0, 0, 0))
                .expect("edited block")
                .to_string(),
            "minecraft:gold_block",
            "{label}"
        );
        assert_eq!(
            exported
                .get_block(OVERWORLD, BlockPos::new(1, 0, 0))
                .expect("unknown block")
                .to_string(),
            "minecraft:unknown_machine[powered=true]",
            "{label}: unknown imported state must survive"
        );
        assert_eq!(
            exported
                .get_block(OVERWORLD, BlockPos::new(16, 0, 0))
                .expect("untouched neighbour")
                .to_string(),
            "minecraft:dirt",
            "{label}: untouched chunk must survive byte-for-byte"
        );
        assert_eq!(
            exported
                .get_biome(OVERWORLD, BlockPos::new(0, 0, 0))
                .expect("edited biome")
                .expect("biome data")
                .as_str(),
            "minecraft:desert",
            "{label}"
        );
        let upper_biome = exported
            .get_biome(OVERWORLD, BlockPos::new(0, 16, 0))
            .expect("upper biome")
            .expect("biome data");
        assert_eq!(
            upper_biome.as_str(),
            if profile.biome_format == BiomeFormat::Column {
                "minecraft:desert"
            } else {
                "minecraft:plains"
            },
            "{label}: column edits propagate through all sections"
        );
        tested += 1;
    }
    assert_eq!(
        tested, 19,
        "every flattened classic release must be covered"
    );
}

#[test]
fn flattened_catalog_projects_create_edit_save_reopen_export_and_verify() {
    let mut tested = 0;
    for profile in builtin_profiles()
        .iter()
        .filter(|profile| profile.family == VersionFamily::Flattened)
    {
        let label = profile.minecraft_version.as_str();
        let dir = TempDir::new(&format!("flattened-catalog-{label}"));
        let root = dir.path().join("world");
        write_flattened_world_for_profile(&root, profile);
        let jar = write_catalog_jar(dir.path(), profile);
        let catalog = AssetCatalogService::new()
            .catalog(&asset_source::AssetSourceSetting {
                kind: "client_jar".to_owned(),
                path: jar.to_string_lossy().into_owned(),
            })
            .unwrap_or_else(|error| panic!("{label}: build catalog: {error}"));

        assert_eq!(catalog.minecraft_version, label, "{label}: version");
        assert_eq!(
            catalog.data_version, profile.data_version,
            "{label}: DataVersion"
        );
        assert_eq!(
            catalog.resource_pack_format, profile.resource_pack_format,
            "{label}: resource pack format"
        );
        assert_eq!(
            catalog.datapack_format, profile.datapack_format,
            "{label}: data pack format"
        );
        assert!(
            catalog.catalog_id.starts_with(&format!("java-{label}-")),
            "{label}: catalog id"
        );
        assert_eq!(catalog.catalog_hash.len(), 64, "{label}: catalog hash");
        assert_eq!(
            catalog
                .warnings
                .iter()
                .any(|warning| warning.contains("no internal version.json")),
            label.starts_with("1.13"),
            "{label}: only 1.13.x relies on launcher metadata"
        );
        assert!(
            !catalog
                .warnings
                .iter()
                .any(|warning| warning.contains("SHA-1 was not verified")),
            "{label}: launcher metadata must verify the JAR"
        );

        let mut expected = BTreeSet::from([
            "minecraft:diamond_sword",
            "minecraft:dirt",
            "minecraft:gold_block",
            "minecraft:item_frame",
            "minecraft:oak_stairs",
            "minecraft:stone",
            "minecraft:water",
        ]);
        let mut extra_count = 0;
        for (threshold, id) in [
            (2225, "minecraft:beehive"),
            (2566, "minecraft:netherite_block"),
            (2724, "minecraft:copper_block"),
        ] {
            if profile.data_version >= threshold {
                expected.insert(id);
                extra_count += 1;
            }
        }
        let ids = catalog
            .entries
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>();
        assert!(
            ids.windows(2).all(|window| window[0] < window[1]),
            "{label}: catalog entries must be sorted and unique"
        );
        let actual = ids.iter().copied().collect::<BTreeSet<_>>();
        assert_eq!(
            actual.len(),
            catalog.entries.len(),
            "{label}: duplicate catalog ids"
        );
        assert_eq!(actual, expected, "{label}: exact catalog entries");
        assert_eq!(catalog.counts.blocks, 5 + extra_count, "{label}: blocks");
        assert_eq!(catalog.counts.items, 6 + extra_count, "{label}: items");
        assert_eq!(
            catalog.counts.blocks_with_items,
            4 + extra_count,
            "{label}: block items"
        );
        assert_eq!(catalog.counts.item_only, 2, "{label}: item-only entries");
        assert_eq!(
            catalog.counts.item_definitions, 0,
            "{label}: flattened JARs use item models"
        );
        assert_eq!(catalog.counts.block_tags, 1, "{label}: block tags");
        assert_eq!(catalog.counts.item_tags, 1, "{label}: item tags");

        let find_entry = |id: &str| {
            catalog
                .entries
                .iter()
                .find(|entry| entry.id == id)
                .unwrap_or_else(|| panic!("{label}: catalog is missing {id}"))
        };

        let stairs = find_entry("minecraft:oak_stairs");
        let states = stairs.states.as_ref().expect("stairs block states");
        assert!(states.property_values_complete, "{label}: complete stairs");
        assert_eq!(states.variant_count, 80, "{label}: stairs variants");
        assert_eq!(states.variants.len(), 80, "{label}: stairs variant list");
        assert!(
            states.default_properties.is_none(),
            "{label}: multiple variants have no single default"
        );
        assert_eq!(
            states
                .properties
                .get("facing")
                .expect("facing")
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["east", "north", "south", "west"],
            "{label}: facing values"
        );
        assert_eq!(
            states
                .properties
                .get("half")
                .expect("half")
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["bottom", "top"],
            "{label}: half values"
        );
        assert_eq!(
            states
                .properties
                .get("shape")
                .expect("shape")
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec![
                "inner_left",
                "inner_right",
                "outer_left",
                "outer_right",
                "straight"
            ],
            "{label}: shape values"
        );
        assert_eq!(
            states
                .properties
                .get("waterlogged")
                .expect("waterlogged")
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["false", "true"],
            "{label}: waterlogged values"
        );
        assert_eq!(
            stairs.tags,
            vec!["minecraft:planks".to_owned(), "minecraft:stairs".to_owned()],
            "{label}: direct block and item tags"
        );
        assert_eq!(
            stairs
                .item_form
                .as_ref()
                .and_then(|form| form.model.as_deref()),
            Some("minecraft:item/oak_stairs"),
            "{label}: stairs item model"
        );

        let water = find_entry("minecraft:water");
        assert!(water.states.is_some(), "{label}: water block");
        assert!(water.item_form.is_none(), "{label}: water has no item");
        let frame = find_entry("minecraft:item_frame");
        assert!(frame.states.is_none(), "{label}: item frame is not a block");
        assert!(frame.item_form.is_some(), "{label}: item frame item");
        assert!(
            catalog
                .warnings
                .iter()
                .any(|warning| warning.contains("item_frame")
                    && warning.contains("entity render model")),
            "{label}: entity blockstate warning"
        );
        assert!(
            catalog
                .entries
                .iter()
                .all(|entry| entry.id != "minecraft:air"),
            "{label}: empty item model must not become an item"
        );

        for (threshold, id, texture) in [
            (2225, "minecraft:beehive", "beehive_side"),
            (2566, "minecraft:netherite_block", "netherite_block"),
            (2724, "minecraft:copper_block", "copper_block"),
        ] {
            if profile.data_version >= threshold {
                let entry = find_entry(id);
                assert!(entry.states.is_some(), "{label}: {id} block");
                assert!(entry.item_form.is_some(), "{label}: {id} item");
                assert_eq!(
                    entry.icon_texture,
                    Some(format!("minecraft:block/{texture}")),
                    "{label}: {id} icon"
                );
            } else {
                assert!(
                    catalog.entries.iter().all(|entry| entry.id.as_str() != id),
                    "{label}: {id} must not exist before its release"
                );
            }
        }

        let (world, _) =
            load_java_world(&root).unwrap_or_else(|error| panic!("{label}: import: {error}"));
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
        let desert: world_domain::BiomeState = "minecraft:desert".parse().unwrap();
        opened
            .set_block(OVERWORLD, BlockPos::new(0, 0, 0), &gold)
            .unwrap_or_else(|error| panic!("{label}: edit block: {error}"));
        opened
            .set_biome(OVERWORLD, BlockPos::new(0, 0, 0), &desert)
            .unwrap_or_else(|error| panic!("{label}: edit biome: {error}"));
        opened.bump_revision();
        store
            .save(&folder, &opened, EditorState::default(), SaveKind::Manual)
            .unwrap_or_else(|error| panic!("{label}: save project: {error}"));

        let (reopened_detail, reopened, _) = store
            .open(&folder)
            .unwrap_or_else(|error| panic!("{label}: reopen project: {error}"));
        assert_eq!(reopened_detail.components, pins, "{label}: catalog pin");
        assert_eq!(
            reopened_detail.target.profile_id, profile.id,
            "{label}: profile pin"
        );
        let rebuilt = AssetCatalogService::new()
            .catalog(&asset_source::AssetSourceSetting {
                kind: "client_jar".to_owned(),
                path: jar.to_string_lossy().into_owned(),
            })
            .unwrap_or_else(|error| panic!("{label}: rebuild catalog from pin: {error}"));
        assert_eq!(
            rebuilt.catalog_id, catalog.catalog_id,
            "{label}: rebuilt catalog id"
        );
        assert_eq!(
            rebuilt.catalog_hash, catalog.catalog_hash,
            "{label}: rebuilt catalog hash"
        );
        assert_eq!(
            rebuilt.client_jar_sha1, catalog.client_jar_sha1,
            "{label}: rebuilt client JAR"
        );
        assert_eq!(
            rebuilt.resource_pack_format, profile.resource_pack_format,
            "{label}: rebuilt resource pack format"
        );
        assert_eq!(
            rebuilt.datapack_format, profile.datapack_format,
            "{label}: rebuilt data pack format"
        );
        assert_eq!(
            rebuilt.entries, catalog.entries,
            "{label}: rebuilt catalog entries"
        );
        assert_eq!(
            reopened
                .get_block(OVERWORLD, BlockPos::new(0, 0, 0))
                .unwrap(),
            gold,
            "{label}: saved block edit"
        );
        assert_eq!(
            reopened
                .get_biome(OVERWORLD, BlockPos::new(0, 0, 0))
                .unwrap()
                .unwrap(),
            desert,
            "{label}: saved biome edit"
        );
        assert_eq!(
            reopened
                .get_block(OVERWORLD, BlockPos::new(1, 0, 0))
                .unwrap()
                .name(),
            "minecraft:unknown_machine",
            "{label}: unknown imported state must survive"
        );

        let before = snapshot(&source_world);
        let mut overlay = WorldOverlay::open(&source_world)
            .unwrap_or_else(|error| panic!("{label}: open overlay: {error}"));
        overlay
            .set_block(OVERWORLD, 0, 0, 0, &gold)
            .unwrap_or_else(|error| panic!("{label}: overlay block edit: {error}"));
        overlay
            .set_biome(OVERWORLD, 0, 0, 0, &desert)
            .unwrap_or_else(|error| panic!("{label}: overlay biome edit: {error}"));
        let destination = dir.path().join("export");
        let report = export_java_world(&overlay, &destination)
            .unwrap_or_else(|error| panic!("{label}: export: {error}"));
        assert!(report.source_unchanged, "{label}: source changed");
        assert!(report.source_tree_verified, "{label}: source not hashed");
        assert!(
            report.copied_files_match_source,
            "{label}: copied files differ"
        );
        assert!(report.output_revalidated, "{label}: export not revalidated");
        assert_eq!(snapshot(&source_world), before, "{label}: source bytes");

        let (exported, _) = load_java_world(&destination)
            .unwrap_or_else(|error| panic!("{label}: reopen export: {error}"));
        assert_eq!(
            exported
                .get_block(OVERWORLD, BlockPos::new(0, 0, 0))
                .unwrap(),
            gold,
            "{label}: exported edit"
        );
        assert_eq!(
            exported
                .get_block(OVERWORLD, BlockPos::new(1, 0, 0))
                .unwrap()
                .name(),
            "minecraft:unknown_machine",
            "{label}: exported unknown state"
        );
        assert_eq!(
            exported
                .get_block(OVERWORLD, BlockPos::new(16, 0, 0))
                .unwrap()
                .name(),
            "minecraft:dirt",
            "{label}: untouched neighbour chunk"
        );
        assert_eq!(
            exported
                .get_biome(OVERWORLD, BlockPos::new(0, 0, 0))
                .unwrap()
                .unwrap()
                .as_str(),
            "minecraft:desert",
            "{label}: exported biome edit"
        );
        let upper_biome = exported
            .get_biome(OVERWORLD, BlockPos::new(0, 16, 0))
            .unwrap()
            .unwrap();
        assert_eq!(
            upper_biome.as_str(),
            if profile.biome_format == BiomeFormat::Column {
                "minecraft:desert"
            } else {
                "minecraft:plains"
            },
            "{label}: column edits propagate through all sections"
        );
        tested += 1;
    }
    assert_eq!(
        tested, 19,
        "every flattened classic release must be covered"
    );
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
    write_world(&root, 1343);

    let analysis = analyze_java_world(&root);
    assert!(!analysis.supported);
    assert_eq!(analysis.data_version, Some(1343));
    assert!(analysis
        .blockers
        .iter()
        .any(|blocker| blocker.contains("DataVersion 1343")));
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
