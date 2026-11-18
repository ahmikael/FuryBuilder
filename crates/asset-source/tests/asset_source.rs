//! M9.1 tests: local Minecraft asset discovery, JAR validation and resource pack safety.
//!
//! All fixtures are synthetic except the optional real-JAR probe in
//! `examples/asset_source_probe.rs`. Nothing is launched and nothing is extracted.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use asset_source::{
    discover_root, discover_roots, inspect_asset_path, AssetSelectionKind, AssetSettingsStore,
    AssetSourceSetting, InstallationSource, ResourcePackStatus, VersionStatus,
};
use flate2::write::DeflateEncoder;
use flate2::Compression;
use sha1::{Digest, Sha1};

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "furybuilder-assets-{label}-{}-{}",
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

fn sha1_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
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

fn zip_bytes(entries: &[(&str, &[u8], bool)]) -> Vec<u8> {
    let mut local = Vec::new();
    let mut central = Vec::new();
    for (name, data, deflate) in entries {
        let offset = local.len() as u32;
        let crc = crc32(data);
        let (method, compressed) = if *deflate {
            let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(data).expect("deflate");
            (8u16, encoder.finish().expect("deflate finish"))
        } else {
            (0u16, data.to_vec())
        };
        local.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        local.extend_from_slice(&20u16.to_le_bytes());
        local.extend_from_slice(&0u16.to_le_bytes());
        local.extend_from_slice(&method.to_le_bytes());
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
        central.extend_from_slice(&method.to_le_bytes());
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

fn client_version_json(id: &str, world_version: i32, resource_format: i32) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "id": id,
        "name": id,
        "world_version": world_version,
        "series_id": "main",
        "protocol_version": 769,
        "pack_version": { "resource": resource_format, "data": 61 },
        "java_version": 21,
        "stable": true
    }))
    .expect("version.json")
}

fn client_jar(id: &str, world_version: i32, resource_format: i32) -> Vec<u8> {
    let version_json = client_version_json(id, world_version, resource_format);
    zip_bytes(&[
        ("version.json", version_json.as_slice(), true),
        (
            "assets/minecraft/blockstates/stone.json",
            b"{\"variants\":{}}".as_slice(),
            true,
        ),
        (
            "assets/minecraft/textures/block/stone.png",
            b"not-a-real-png".as_slice(),
            false,
        ),
    ])
}

fn write_target_root(root: &Path, jar_id: &str, world_version: i32) -> Vec<u8> {
    let version_dir = root.join("versions").join("1.21.4");
    fs::create_dir_all(&version_dir).expect("version dir");
    let jar = client_jar(jar_id, world_version, 46);
    let jar_sha1 = sha1_hex(&jar);
    fs::write(version_dir.join("1.21.4.jar"), &jar).expect("jar");
    let asset_index = br#"{"objects":{}}"#.to_vec();
    let index_sha1 = sha1_hex(&asset_index);
    let metadata = serde_json::json!({
        "id": "1.21.4",
        "type": "release",
        "releaseTime": "2024-12-03T10:09:48+00:00",
        "javaVersion": { "component": "java-runtime-delta", "majorVersion": 21 },
        "assetIndex": {
            "id": "19",
            "sha1": index_sha1,
            "size": asset_index.len()
        },
        "downloads": {
            "client": { "sha1": jar_sha1, "size": jar.len() }
        }
    });
    fs::write(
        version_dir.join("1.21.4.json"),
        serde_json::to_vec_pretty(&metadata).expect("metadata"),
    )
    .expect("version json");
    let indexes = root.join("assets").join("indexes");
    fs::create_dir_all(&indexes).expect("asset indexes");
    fs::write(indexes.join("19.json"), &asset_index).expect("asset index");
    jar
}

fn write_resource_pack(root: &Path, name: &str, format: i32) {
    let pack = root.join("resourcepacks").join(name);
    fs::create_dir_all(pack.join("assets").join("minecraft").join("blockstates"))
        .expect("blockstates");
    fs::create_dir_all(
        pack.join("assets")
            .join("minecraft")
            .join("textures")
            .join("block"),
    )
    .expect("textures");
    fs::write(
        pack.join("pack.mcmeta"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "pack": {
                "pack_format": format,
                "description": "Synthetic test pack"
            }
        }))
        .expect("pack meta"),
    )
    .expect("pack.mcmeta");
    fs::write(
        pack.join("assets")
            .join("minecraft")
            .join("blockstates")
            .join("stone.json"),
        b"{}",
    )
    .expect("blockstate");
    fs::write(
        pack.join("assets")
            .join("minecraft")
            .join("textures")
            .join("block")
            .join("stone.png"),
        b"not-a-real-png",
    )
    .expect("texture");
}

#[test]
fn discovers_and_verifies_the_fixed_target() {
    let dir = TempDir::new("target");
    let root = dir.path();
    write_target_root(root, "1.21.4", 4189);
    write_resource_pack(root, "Example", 46);

    let installation = discover_root(root);
    assert!(installation.versions_dir_present);
    assert!(installation.assets_dir_present);
    assert_eq!(installation.asset_indexes, 1);
    assert_eq!(installation.compatible_versions, ["1.21.4"]);
    let version = &installation.versions[0];
    assert_eq!(version.status, VersionStatus::Compatible);
    assert!(version.checksum_verified);
    assert_eq!(version.world_version, Some(4189));
    assert_eq!(version.resource_pack_format, Some(46));
    assert_eq!(version.java_major, Some(21));
    assert!(version.asset_index_present);
    assert!(version.asset_index_checksum_verified);
    assert_eq!(installation.resource_packs.len(), 1);
    assert_eq!(
        installation.resource_packs[0].status,
        ResourcePackStatus::Compatible
    );
    assert!(installation.resource_packs[0].has_blockstates);
    assert!(installation.resource_packs[0].has_textures);

    let report = inspect_asset_path(root);
    assert_eq!(report.kind, AssetSelectionKind::MinecraftRoot);
    assert!(report.valid, "{:?}", report.errors);
    assert!(report.target_match);
    assert_eq!(
        report.version.as_ref().map(|version| version.id.as_str()),
        Some("1.21.4")
    );

    let version_report = inspect_asset_path(root.join("versions").join("1.21.4"));
    assert_eq!(version_report.kind, AssetSelectionKind::VersionDirectory);
    assert!(version_report.valid, "{:?}", version_report.errors);

    let jar_report = inspect_asset_path(root.join("versions").join("1.21.4").join("1.21.4.jar"));
    assert_eq!(jar_report.kind, AssetSelectionKind::ClientJar);
    assert!(jar_report.valid, "{:?}", jar_report.errors);
    assert!(jar_report
        .version
        .as_ref()
        .is_some_and(|version| version.checksum_verified));
}

#[test]
fn checksum_mismatch_and_wrong_world_version_fail_closed() {
    let dir = TempDir::new("checksums");
    let root = dir.path().join("root");
    let jar = write_target_root(&root, "1.21.4", 4189);
    let jar_path = root.join("versions").join("1.21.4").join("1.21.4.jar");
    let mut mutated = jar;
    mutated.push(0);
    fs::write(&jar_path, mutated).expect("mutate jar");
    let installation = discover_root(&root);
    assert_eq!(
        installation.versions[0].status,
        VersionStatus::ChecksumMismatch
    );
    assert!(installation.compatible_versions.is_empty());
    assert!(!inspect_asset_path(&root).valid);

    let other = dir.path().join("world-version");
    write_target_root(&other, "1.21.4", 4325);
    let installation = discover_root(&other);
    assert_eq!(
        installation.versions[0].status,
        VersionStatus::WorldVersionMismatch
    );
    assert!(installation.compatible_versions.is_empty());
}

#[test]
fn modded_and_wrong_versions_are_reported_but_not_target_matches() {
    let dir = TempDir::new("modded");
    let root = dir.path();
    let modded = root.join("versions").join("fabric-loader-0.19.5-1.21.4");
    fs::create_dir_all(&modded).expect("modded folder");
    fs::write(
        modded.join("fabric-loader-0.19.5-1.21.4.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "id": "fabric-loader-0.19.5-1.21.4",
            "inheritsFrom": "1.21.4",
            "type": "release"
        }))
        .expect("modded json"),
    )
    .expect("modded metadata");
    let old = root.join("versions").join("1.20.1");
    fs::create_dir_all(&old).expect("old folder");
    fs::write(
        old.join("1.20.1.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "id": "1.20.1",
            "type": "release"
        }))
        .expect("old json"),
    )
    .expect("old metadata");
    fs::write(old.join("1.20.1.jar"), b"not-a-jar").expect("old jar");

    let installation = discover_root(root);
    assert_eq!(installation.versions.len(), 2);
    let modded = installation
        .versions
        .iter()
        .find(|version| version.id.starts_with("fabric"))
        .expect("modded row");
    assert_eq!(modded.status, VersionStatus::Modded);
    assert_eq!(modded.inherits_from.as_deref(), Some("1.21.4"));
    let old = installation
        .versions
        .iter()
        .find(|version| version.id == "1.20.1")
        .expect("old row");
    assert_eq!(old.status, VersionStatus::WrongVersion);
    assert!(installation.compatible_versions.is_empty());
    assert!(!inspect_asset_path(root).valid);
}

#[test]
fn resource_pack_validation_rejects_traversal_and_wrong_formats() {
    let dir = TempDir::new("packs");
    let valid = dir.path().join("valid.zip");
    let pack_meta = serde_json::to_vec(&serde_json::json!({
        "pack": { "pack_format": 46, "description": "zip pack" }
    }))
    .expect("pack meta");
    fs::write(
        &valid,
        zip_bytes(&[
            ("pack.mcmeta", pack_meta.as_slice(), true),
            (
                "assets/minecraft/blockstates/stone.json",
                b"{}".as_slice(),
                true,
            ),
            (
                "assets/minecraft/textures/block/stone.png",
                b"png".as_slice(),
                false,
            ),
        ]),
    )
    .expect("valid pack");
    let report = inspect_asset_path(&valid);
    assert_eq!(report.kind, AssetSelectionKind::ResourcePack);
    assert!(report.valid, "{:?}", report.errors);
    assert!(report
        .resource_pack
        .as_ref()
        .is_some_and(|pack| pack.compatible));

    let ranged = dir.path().join("ranged.zip");
    let ranged_meta = serde_json::to_vec(&serde_json::json!({
        "pack": { "pack_format": 34, "supported_formats": [40, 50] }
    }))
    .expect("ranged meta");
    fs::write(
        &ranged,
        zip_bytes(&[("pack.mcmeta", ranged_meta.as_slice(), true)]),
    )
    .expect("ranged pack");
    assert!(inspect_asset_path(&ranged).valid);

    let wrong = dir.path().join("wrong.zip");
    let wrong_meta = serde_json::to_vec(&serde_json::json!({
        "pack": { "pack_format": 15 }
    }))
    .expect("wrong meta");
    fs::write(
        &wrong,
        zip_bytes(&[("pack.mcmeta", wrong_meta.as_slice(), true)]),
    )
    .expect("wrong pack");
    let report = inspect_asset_path(&wrong);
    assert!(!report.valid);
    assert_eq!(
        report.resource_pack.as_ref().map(|pack| pack.status),
        Some(ResourcePackStatus::IncompatibleFormat)
    );

    let unsafe_pack = dir.path().join("unsafe.zip");
    fs::write(
        &unsafe_pack,
        zip_bytes(&[
            ("pack.mcmeta", pack_meta.as_slice(), true),
            ("../escape.txt", b"escape".as_slice(), false),
        ]),
    )
    .expect("unsafe pack");
    let report = inspect_asset_path(&unsafe_pack);
    assert!(!report.valid);
    assert_eq!(
        report.resource_pack.as_ref().map(|pack| pack.status),
        Some(ResourcePackStatus::UnsafeContents)
    );

    let missing = dir.path().join("missing");
    fs::create_dir_all(missing.join("assets").join("minecraft")).expect("missing pack");
    let report = inspect_asset_path(&missing);
    assert!(!report.valid);
    assert_eq!(
        report.resource_pack.as_ref().map(|pack| pack.status),
        Some(ResourcePackStatus::MissingMetadata)
    );
}

#[test]
fn discovery_reports_missing_roots_without_panicking() {
    let dir = TempDir::new("missing-roots");
    let report = discover_roots(vec![
        (dir.path().join("absent"), InstallationSource::DefaultRoot),
        (dir.path().join("absent"), InstallationSource::Manual),
    ]);
    assert!(report.installations.is_empty());
    assert_eq!(report.searched_roots.len(), 1);
    assert_eq!(report.problems.len(), 1);
    assert!(!report.manual_hint.is_empty());
}

#[test]
fn asset_settings_round_trip_and_clear() {
    let dir = TempDir::new("settings");
    let storage = dir.path().join("config").join("assets.json");
    let store = AssetSettingsStore::new(&storage);
    assert!(store.saved_source().source.is_none());

    let root = dir.path().join("mc");
    write_target_root(&root, "1.21.4", 4189);
    let saved = store
        .save(AssetSourceSetting {
            kind: "minecraft_root".to_owned(),
            path: root.display().to_string(),
        })
        .expect("save source");
    assert!(saved.source.is_some());
    assert!(saved.report.as_ref().is_some_and(|report| report.valid));

    let reopened = AssetSettingsStore::new(&storage);
    let saved = reopened.saved_source();
    assert_eq!(
        saved.source.as_ref().map(|source| source.kind.as_str()),
        Some("minecraft_root")
    );
    assert!(saved.report.as_ref().is_some_and(|report| report.valid));

    let cleared = reopened.clear().expect("clear source");
    assert!(cleared.source.is_none());
    let bytes = fs::read(&storage).expect("settings file");
    let value: serde_json::Value = serde_json::from_slice(&bytes).expect("settings json");
    assert!(value.get("source").is_some_and(|source| source.is_null()));
}
