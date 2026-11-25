//! M9.2 tests: blockstates, models, textures, resource-pack priority and atlas caching.
//!
//! Fixtures are synthetic ZIP archives. The real-JAR render probe lives in
//! `examples/asset_bundle_probe.rs`.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use asset_source::{
    target_profile, AssetBundle, AssetBundleService, AssetSourceSetting, BlockStateRequest,
    BundleError, PackState, ResourcePackInput, ResourcePackStore,
};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use flate2::write::DeflateEncoder;
use flate2::Compression;

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "furybuilder-bundle-{label}-{}-{}",
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

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

fn zip_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut local = Vec::new();
    let mut central = Vec::new();
    for (name, data) in entries {
        let offset = local.len() as u32;
        let crc = crc32(data);
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(data).expect("deflate");
        let compressed = encoder.finish().expect("deflate finish");
        local.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        local.extend_from_slice(&20u16.to_le_bytes());
        local.extend_from_slice(&0u16.to_le_bytes());
        local.extend_from_slice(&8u16.to_le_bytes());
        local.extend_from_slice(&0u16.to_le_bytes());
        local.extend_from_slice(&0u16.to_le_bytes());
        local.extend_from_slice(&crc.to_le_bytes());
        local.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        local.extend_from_slice(&(data.len() as u32).to_le_bytes());
        local.extend_from_slice(&(name.len() as u16).to_le_bytes());
        local.extend_from_slice(&0u16.to_le_bytes());
        local.extend_from_slice(name.as_bytes());
        local.extend_from_slice(&compressed);

        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&8u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        central.extend_from_slice(&(data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(name.len() as u16).to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
    }
    let central_offset = local.len() as u32;
    let mut archive = local;
    archive.extend_from_slice(&central);
    archive.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    archive.extend_from_slice(&0u16.to_le_bytes());
    archive.extend_from_slice(&0u16.to_le_bytes());
    archive.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    archive.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    archive.extend_from_slice(&(central.len() as u32).to_le_bytes());
    archive.extend_from_slice(&central_offset.to_le_bytes());
    archive.extend_from_slice(&0u16.to_le_bytes());
    archive
}

fn png_bytes(width: u32, height: u32, pixel: [u8; 4]) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("png header");
        let rgba = pixel
            .iter()
            .copied()
            .cycle()
            .take((width * height * 4) as usize)
            .collect::<Vec<_>>();
        writer.write_image_data(&rgba).expect("png data");
    }
    bytes
}

fn transparent_png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("png header");
        let mut rgba = vec![0u8; (width * height * 4) as usize];
        for (index, px) in rgba.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            if index.is_multiple_of(2) {
                px.copy_from_slice(&[255, 255, 255, 0]);
            } else {
                px.copy_from_slice(&[255, 255, 255, 255]);
            }
        }
        writer.write_image_data(&rgba).expect("png data");
    }
    bytes
}

fn cube_model() -> String {
    serde_json::json!({
        "parent": "minecraft:block/block",
        "textures": { "particle": "#all", "all": "#all" },
        "elements": [{
            "from": [0, 0, 0],
            "to": [16, 16, 16],
            "faces": {
                "down":  { "uv": [0, 0, 16, 16], "texture": "#all", "cullface": "down" },
                "up":    { "uv": [0, 0, 16, 16], "texture": "#all", "cullface": "up" },
                "north": { "uv": [0, 0, 16, 16], "texture": "#all", "cullface": "north" },
                "south": { "uv": [0, 0, 16, 16], "texture": "#all", "cullface": "south" },
                "west":  { "uv": [0, 0, 16, 16], "texture": "#all", "cullface": "west" },
                "east":  { "uv": [0, 0, 16, 16], "texture": "#all", "cullface": "east" }
            }
        }]
    })
    .to_string()
}

fn base_jar(stone_pixels: [u8; 4]) -> Vec<u8> {
    let version = serde_json::to_vec(&serde_json::json!({
        "id": "1.21.4",
        "name": "1.21.4",
        "world_version": 4189,
        "series_id": "main",
        "protocol_version": 769,
        "pack_version": { "resource": 46, "data": 61 },
        "java_version": 21,
        "stable": true
    }))
    .expect("version json");
    let blockstate = r#"{"variants":{"":{"model":"minecraft:block/stone"}}}"#.to_string();
    let slab_state =
        r#"{"variants":{"type=bottom":{"model":"minecraft:block/oak_slab"}}}"#.to_string();
    let slab_model = serde_json::json!({
        "parent": "minecraft:block/block",
        "textures": { "particle": "#side", "bottom": "minecraft:block/stone",
                      "side": "minecraft:block/stone", "top": "minecraft:block/stone" },
        "elements": [{
            "from": [0, 0, 0], "to": [16, 8, 16],
            "faces": {
                "down":  { "uv": [0, 0, 16, 16], "texture": "#bottom", "cullface": "down" },
                "up":    { "uv": [0, 0, 16, 16], "texture": "#top" },
                "north": { "uv": [0, 8, 16, 16], "texture": "#side", "cullface": "north" },
                "south": { "uv": [0, 8, 16, 16], "texture": "#side", "cullface": "south" },
                "west":  { "uv": [0, 8, 16, 16], "texture": "#side", "cullface": "west" },
                "east":  { "uv": [0, 8, 16, 16], "texture": "#side", "cullface": "east" }
            }
        }]
    })
    .to_string();
    let stone_model = serde_json::json!({
        "parent": "minecraft:block/cube",
        "textures": { "all": "minecraft:block/stone" }
    })
    .to_string();
    let glass_model = serde_json::json!({
        "parent": "minecraft:block/cube",
        "textures": { "all": "minecraft:block/glass" }
    })
    .to_string();
    let stone_texture = png_bytes(16, 16, stone_pixels);
    let glass_texture = transparent_png(16, 16);
    let animated_texture = png_bytes(16, 32, [30, 80, 220, 200]);
    zip_bytes(&[
        ("version.json", version.as_slice()),
        (
            "assets/minecraft/blockstates/stone.json",
            blockstate.as_bytes(),
        ),
        (
            "assets/minecraft/blockstates/oak_slab.json",
            slab_state.as_bytes(),
        ),
        (
            "assets/minecraft/blockstates/glass.json",
            br#"{"variants":{"":{"model":"minecraft:block/glass"}}}"#,
        ),
        (
            "assets/minecraft/models/block/block.json",
            br##"{"textures":{"particle":"#all"}}"##,
        ),
        (
            "assets/minecraft/models/block/cube.json",
            cube_model().as_bytes(),
        ),
        (
            "assets/minecraft/models/block/stone.json",
            stone_model.as_bytes(),
        ),
        (
            "assets/minecraft/models/block/glass.json",
            glass_model.as_bytes(),
        ),
        (
            "assets/minecraft/models/block/oak_slab.json",
            slab_model.as_bytes(),
        ),
        (
            "assets/minecraft/textures/block/stone.png",
            stone_texture.as_slice(),
        ),
        (
            "assets/minecraft/textures/block/glass.png",
            glass_texture.as_slice(),
        ),
        (
            "assets/minecraft/textures/block/water_still.png",
            animated_texture.as_slice(),
        ),
    ])
}

fn write_jar(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).expect("write jar");
}

fn pack_zip(color: [u8; 4], format: i32) -> Vec<u8> {
    let meta = serde_json::to_vec(&serde_json::json!({
        "pack": { "pack_format": format, "description": "bundle test pack" }
    }))
    .expect("pack meta");
    let texture = png_bytes(16, 16, color);
    zip_bytes(&[
        ("pack.mcmeta", meta.as_slice()),
        (
            "assets/minecraft/textures/block/stone.png",
            texture.as_slice(),
        ),
    ])
}

fn state(name: &str, properties: &[(&str, &str)]) -> BlockStateRequest {
    BlockStateRequest {
        name: name.to_owned(),
        properties: properties
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect(),
    }
}

fn service_bundle(
    service: &AssetBundleService,
    jar: &Path,
    packs: &[String],
    states: &[BlockStateRequest],
) -> AssetBundle {
    service
        .bundle(
            target_profile(),
            Some(&AssetSourceSetting {
                kind: "client_jar".to_owned(),
                path: jar.display().to_string(),
            }),
            None,
            &packs
                .iter()
                .map(ResourcePackInput::local)
                .collect::<Vec<_>>(),
            states,
        )
        .expect("bundle")
}

fn atlas_pixel(bundle: &AssetBundle, key: &str, x: u32, y: u32) -> [u8; 4] {
    let texture = bundle
        .textures
        .iter()
        .find(|texture| texture.key == key)
        .unwrap_or_else(|| panic!("missing texture {key}"));
    let png = BASE64
        .decode(&bundle.atlas_png_base64)
        .expect("atlas base64");
    let decoder = png::Decoder::new(std::io::Cursor::new(png));
    let mut reader = decoder.read_info().expect("atlas png");
    let mut buffer = vec![0u8; reader.output_buffer_size()];
    let frame = reader.next_frame(&mut buffer).expect("atlas frame");
    assert_eq!(frame.color_type, png::ColorType::Rgba);
    let index = (((texture.y + y) * bundle.atlas_width + texture.x + x) * 4) as usize;
    let mut pixel = [0u8; 4];
    pixel.copy_from_slice(&buffer[index..index + 4]);
    pixel
}

#[test]
fn resolves_parent_models_textures_atlas_and_cache() {
    let dir = TempDir::new("resolve");
    let jar = dir.path().join("client.jar");
    write_jar(&jar, &base_jar([180, 40, 40, 255]));
    let service = AssetBundleService::new();
    let states = [
        state("minecraft:stone", &[]),
        state(
            "minecraft:oak_slab",
            &[("type", "bottom"), ("waterlogged", "false")],
        ),
        state("minecraft:glass", &[]),
    ];
    let bundle = service_bundle(&service, &jar, &[], &states);

    assert_eq!(bundle.schema_version, 1);
    assert!(!bundle.cache_hit);
    assert_eq!(bundle.blocks.len(), 3);
    assert!(bundle.blocks.iter().all(|block| !block.missing_model));
    let stone = bundle
        .blocks
        .iter()
        .find(|block| block.name == "minecraft:stone")
        .expect("stone block");
    assert_eq!(stone.parts.len(), 1);
    assert_eq!(stone.parts[0].elements.len(), 1);
    assert_eq!(stone.parts[0].elements[0].faces.len(), 6);
    assert!(stone.unsupported.is_empty(), "{:?}", stone.unsupported);
    let slab = bundle
        .blocks
        .iter()
        .find(|block| block.name == "minecraft:oak_slab")
        .expect("slab block");
    assert_eq!(slab.parts[0].elements[0].to[1], 8.0);

    let stone_texture = bundle
        .textures
        .iter()
        .find(|texture| texture.key == "minecraft:block/stone")
        .expect("stone texture");
    assert!(!stone_texture.missing);
    assert_eq!((stone_texture.width, stone_texture.height), (16, 16));
    assert_eq!(
        atlas_pixel(&bundle, "minecraft:block/stone", 0, 0),
        [180, 40, 40, 255]
    );

    let glass = bundle
        .textures
        .iter()
        .find(|texture| texture.key == "minecraft:block/glass")
        .expect("glass texture");
    assert!(glass.has_alpha);

    let again = service_bundle(&service, &jar, &[], &states);
    assert!(again.cache_hit);
    assert_eq!(again.cache_key, bundle.cache_key);
}

#[test]
fn animated_texture_is_cropped_to_first_frame_and_missing_assets_are_marked() {
    let dir = TempDir::new("animated");
    let jar = dir.path().join("client.jar");
    let version = serde_json::to_vec(&serde_json::json!({
        "id": "1.21.4", "world_version": 4189,
        "pack_version": { "resource": 46, "data": 61 }, "java_version": 21
    }))
    .expect("version json");
    let animated_texture = png_bytes(16, 32, [30, 80, 220, 200]);
    let jar_bytes = zip_bytes(&[
        ("version.json", version.as_slice()),
        (
            "assets/minecraft/blockstates/water.json",
            br#"{"variants":{"":{"model":"minecraft:block/water"}}}"#,
        ),
        (
            "assets/minecraft/models/block/water.json",
            br##"{"textures":{"particle":"#all","all":"minecraft:block/water_still"},"elements":[{"from":[0,0,0],"to":[16,16,16],"faces":{"up":{"uv":[0,0,16,16],"texture":"#all"}}}]}"##,
        ),
        (
            "assets/minecraft/blockstates/missing_model.json",
            br#"{"variants":{"":{"model":"minecraft:block/nope"}}}"#,
        ),
        (
            "assets/minecraft/blockstates/missing_texture.json",
            br#"{"variants":{"":{"model":"minecraft:block/missing_texture"}}}"#,
        ),
        (
            "assets/minecraft/models/block/missing_texture.json",
            br##"{"textures":{"particle":"minecraft:block/nope"},"elements":[{"from":[0,0,0],"to":[16,16,16],"faces":{"up":{"texture":"#nope"}}}]}"##,
        ),
        (
            "assets/minecraft/textures/block/water_still.png",
            animated_texture.as_slice(),
        ),
    ]);
    write_jar(&jar, &jar_bytes);

    let service = AssetBundleService::new();
    let bundle = service_bundle(
        &service,
        &jar,
        &[],
        &[
            state("minecraft:water", &[]),
            state("minecraft:missing_model", &[]),
            state("minecraft:missing_texture", &[]),
        ],
    );
    let water = bundle
        .textures
        .iter()
        .find(|texture| texture.key == "minecraft:block/water_still")
        .expect("water texture");
    assert_eq!((water.width, water.height), (16, 16));
    assert_eq!((water.source_width, water.source_height), (16, 32));
    let missing_model = bundle
        .blocks
        .iter()
        .find(|block| block.name == "minecraft:missing_model")
        .expect("missing model block");
    assert!(missing_model.missing_model);
    assert!(missing_model.parts.is_empty());
    let missing_texture = bundle
        .blocks
        .iter()
        .find(|block| block.name == "minecraft:missing_texture")
        .expect("missing texture block");
    assert!(!missing_texture.missing_model);
    assert_eq!(
        missing_texture.parts[0].elements[0].faces[0].texture,
        "__missing__"
    );
    let checker = bundle
        .textures
        .iter()
        .find(|texture| texture.key == "__missing__")
        .expect("missing checker");
    assert!(checker.missing);
}

#[test]
fn pack_priority_cache_key_and_fail_closed_validation_work() {
    let dir = TempDir::new("packs");
    let jar = dir.path().join("client.jar");
    write_jar(&jar, &base_jar([180, 40, 40, 255]));
    let green = dir.path().join("green.zip");
    let blue = dir.path().join("blue.zip");
    fs::write(&green, pack_zip([20, 200, 40, 255], 46)).expect("green pack");
    fs::write(&blue, pack_zip([20, 40, 220, 255], 46)).expect("blue pack");
    let green_path = green.display().to_string();
    let blue_path = blue.display().to_string();
    let service = AssetBundleService::new();
    let states = [state("minecraft:stone", &[])];

    let bundle = service_bundle(
        &service,
        &jar,
        &[green_path.clone(), blue_path.clone()],
        &states,
    );
    assert_eq!(
        atlas_pixel(&bundle, "minecraft:block/stone", 4, 4),
        [20, 200, 40, 255]
    );
    let first_key = bundle.cache_key.clone();

    let swapped = service_bundle(&service, &jar, &[blue_path, green_path], &states);
    assert_eq!(
        atlas_pixel(&swapped, "minecraft:block/stone", 4, 4),
        [20, 40, 220, 255]
    );
    assert_ne!(swapped.cache_key, first_key);

    let wrong = dir.path().join("wrong.zip");
    fs::write(&wrong, pack_zip([0, 0, 0, 255], 15)).expect("wrong pack");
    let error = service
        .bundle(
            target_profile(),
            Some(&AssetSourceSetting {
                kind: "client_jar".to_owned(),
                path: jar.display().to_string(),
            }),
            None,
            &[ResourcePackInput::local(wrong)],
            &states,
        )
        .expect_err("wrong pack format must fail");
    assert!(
        matches!(error, BundleError::UnsupportedFormat(_)),
        "{error:?}"
    );

    let malformed = dir.path().join("malformed.zip");
    let meta = serde_json::to_vec(&serde_json::json!({
        "pack": { "pack_format": 46 }
    }))
    .expect("meta");
    fs::write(
        &malformed,
        zip_bytes(&[
            ("pack.mcmeta", meta.as_slice()),
            (
                "assets/minecraft/blockstates/stone.json",
                b"{not json".as_slice(),
            ),
        ]),
    )
    .expect("malformed pack");
    let error = service
        .bundle(
            target_profile(),
            Some(&AssetSourceSetting {
                kind: "client_jar".to_owned(),
                path: jar.display().to_string(),
            }),
            None,
            &[ResourcePackInput::local(malformed)],
            &states,
        )
        .expect_err("malformed model JSON must fail");
    assert!(
        matches!(error, BundleError::UnsupportedFormat(_)),
        "{error:?}"
    );
}

#[test]
fn pinned_pack_priority_and_health_are_isolated_even_on_cache_hits() {
    let dir = TempDir::new("pack-stack");
    let cache = dir.path().join("cache");
    let store = ResourcePackStore::new(&cache);
    let profile = target_profile();
    let green = dir.path().join("green.zip");
    let blue = dir.path().join("blue.zip");
    let green_png = [20, 200, 40, 255];
    let blue_png = [20, 40, 220, 255];
    fs::write(&green, pack_zip(green_png, 46)).expect("green pack");
    fs::write(&blue, pack_zip(blue_png, 46)).expect("blue pack");
    let green_pin = store.import(&green, profile).expect("pin green");
    let blue_pin = store.import(&blue, profile).expect("pin blue");
    let jar = dir.path().join("client.jar");
    write_jar(&jar, &base_jar([180, 40, 40, 255]));
    let service = AssetBundleService::new();
    let source = AssetSourceSetting {
        kind: "client_jar".to_owned(),
        path: jar.display().to_string(),
    };
    let resolve = |pins: &[component_model::AssetsPack]| {
        service
            .bundle(
                profile,
                Some(&source),
                None,
                &store.inputs(pins),
                &[state("minecraft:stone", &[])],
            )
            .expect("bundle")
    };
    let a = [green_pin.clone(), blue_pin.clone()];
    let b = [blue_pin.clone(), green_pin.clone()];
    let first = resolve(&a);
    let second = resolve(&b);
    assert_eq!(
        atlas_pixel(&first, "minecraft:block/stone", 4, 4),
        green_png
    );
    assert_eq!(
        atlas_pixel(&second, "minecraft:block/stone", 4, 4),
        blue_png
    );
    assert_ne!(first.cache_key, second.cache_key);
    let only_blue = resolve(std::slice::from_ref(&blue_pin));

    fs::remove_file(&green).expect("delete original; cached copy still works");
    assert_eq!(store.status(&a, profile)[0].state, PackState::Ready);
    let cached_green = cache.join(&green_pin.path);
    let green_bytes = fs::read(&cached_green).expect("read cache");
    let mut same_size_corruption = green_bytes.clone();
    same_size_corruption[0] ^= 1;
    fs::write(&cached_green, same_size_corruption).expect("corrupt cached bytes");
    assert_eq!(store.status(&a, profile)[0].state, PackState::Changed);
    let degraded = resolve(&a);
    assert!(degraded.cache_hit);
    assert_eq!(degraded.cache_key, only_blue.cache_key);
    assert_eq!(degraded.packs[0].priority, 1);
    assert_eq!(degraded.pack_issues.len(), 1);
    assert!(degraded.pack_issues[0].contains("checksum"));
    assert!(resolve(std::slice::from_ref(&blue_pin))
        .pack_issues
        .is_empty());
    fs::remove_file(cached_green).expect("delete cached pack");
    assert_eq!(store.status(&a, profile)[0].state, PackState::Missing);
    fs::write(&green, green_bytes).expect("restore original");
    assert_eq!(
        store.import(&green, profile).expect("repair pin"),
        green_pin
    );
    assert_eq!(resolve(&a).cache_key, first.cache_key);
}

#[test]
fn importing_packs_uses_the_exact_profile_and_rejects_unsafe_entries() {
    let dir = TempDir::new("pack-format");
    let store = ResourcePackStore::new(dir.path());
    let older = minecraft_version::profile_for_version(&"1.20.4".parse().expect("version"))
        .expect("older profile");
    let folder = dir.path().join("local folder");
    fs::create_dir(&folder).expect("pack folder");
    let format = older.resource_pack_format.expect("older pack format");
    fs::write(
        folder.join("pack.mcmeta"),
        format!(r#"{{"pack":{{"pack_format":{format}}}}}"#),
    )
    .expect("pack metadata");
    let incompatible = store
        .import(&folder, target_profile())
        .expect_err("wrong profile");
    assert_eq!(incompatible.state, PackState::Incompatible);
    let pin = store.import(&folder, &older).expect("older pack");
    assert_eq!(
        store.status(std::slice::from_ref(&pin), target_profile())[0].state,
        PackState::Incompatible
    );
    assert_eq!(
        store.status(std::slice::from_ref(&pin), &older)[0].state,
        PackState::Ready
    );
    assert_eq!(
        store.import(&folder, &older).expect("deterministic import"),
        pin
    );
    let unsafe_pack = dir.path().join("unsafe.zip");
    let meta = serde_json::to_vec(&serde_json::json!({ "pack": { "pack_format": 46 } }))
        .expect("pack meta");
    fs::write(
        &unsafe_pack,
        zip_bytes(&[
            ("pack.mcmeta", meta.as_slice()),
            ("../escape.txt", b"nope".as_slice()),
        ]),
    )
    .expect("unsafe pack");
    let unsafe_error = store
        .import(&unsafe_pack, target_profile())
        .expect_err("unsafe archive entry must fail");
    assert_eq!(unsafe_error.state, PackState::Invalid);
}
