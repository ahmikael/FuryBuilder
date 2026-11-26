//! Exact server JAR inspection, manual selection and stale-file handling.
//! Archives are synthetic and padded to the minimum real-JAR size; no Mojang binary is
//! committed. Real downloads are exercised by `examples/server_probe.rs`.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use furybuilder_runtime_manager::{
    inspect_server_jar, sha1_file, target_data_version, ServerError, ServerManager, ServerSource,
    ServerTarget,
};

const MIN_SIZE: usize = 10 * 1024 * 1024;

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "furybuilder-server-{label}-{}-{}",
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

    fn manager(&self) -> ServerManager {
        let appdata = self.path.join("appdata");
        ServerManager::new(
            appdata.join("server.json"),
            self.path.join("servers"),
            appdata.join("version-manifest.json"),
        )
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn target(version: &str, data_version: i32, java_major: u32) -> ServerTarget {
    ServerTarget::new(version.parse().expect("version"), data_version, java_major)
        .expect("server target")
}

fn version_json(id: &str, world_version: i32, java_version: u32) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "id": id,
        "name": id,
        "world_version": world_version,
        "series_id": "main",
        "protocol_version": 769,
        "pack_version": { "resource": 46, "data": 61 },
        "build_time": "2024-12-03T10:09:48+00:00",
        "java_component": "java-runtime-delta",
        "java_version": java_version,
        "stable": true,
        "use_editor": false
    }))
    .expect("version json")
}

fn write_jar(path: &Path, version_json: &[u8], include_server_main: bool) {
    let mut entries: Vec<(&str, &[u8])> = vec![("version.json", version_json)];
    if include_server_main {
        entries.push(("net/minecraft/bundler/Main.class", b"synthetic class"));
    }
    write_padded_jar(path, &entries);
}

fn write_padded_jar(path: &Path, entries: &[(&str, &[u8])]) {
    let file = fs::File::create(path).expect("create jar");
    let mut zip = zip::ZipWriter::new(file);
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, bytes) in entries {
        zip.start_file(*name, options).expect("start entry");
        zip.write_all(bytes).expect("write entry");
    }
    zip.start_file("padding.bin", options)
        .expect("start padding");
    let chunk = vec![0u8; 1024 * 1024];
    let mut written = 0usize;
    while written < MIN_SIZE {
        zip.write_all(&chunk).expect("write padding");
        written += chunk.len();
    }
    zip.finish().expect("finish jar");
}

#[test]
fn verifies_classic_entry_point_and_numeric_pack_format() {
    let dir = TempDir::new("classic");
    let jar = dir.path().join("server.jar");
    let entry_point = ("net/minecraft/server/MinecraftServer.class", &b"class"[..]);
    let server_manifest = (
        "META-INF/MANIFEST.MF",
        &b"Manifest-Version: 1.0\r\nMain-Class: net.minecraft.server.MinecraftServer\r\n\r\n"[..],
    );
    let client_manifest = (
        "META-INF/MANIFEST.MF",
        &b"Manifest-Version: 1.0\r\nMain-Class: net.minecraft.client.main.Main\r\n\r\n"[..],
    );
    let metadata = (
        "version.json",
        &br#"{"id":"1.14.4","world_version":1976,"protocol_version":498,"pack_version":4}"#[..],
    );
    let manager = dir.manager();
    let exact = target("1.14.4", 1976, 8);
    write_padded_jar(&jar, &[server_manifest, entry_point, metadata]);
    let report = manager.select_manual(&jar, &exact).expect("classic server");
    assert!(report.ready);
    assert_eq!(report.resource_pack_format, Some(4));
    assert_eq!(report.java_major, None);
    assert_eq!(report.protocol_version, Some(498));
    assert!(manager.selected(&exact).expect("re-inspected server").ready);

    write_padded_jar(&jar, &[client_manifest, entry_point, metadata]);
    assert!(manager.select_manual(&jar, &exact).is_err());

    write_padded_jar(&jar, &[server_manifest, entry_point]);
    let error = manager
        .select_manual(&jar, &target("1.13.2", 1631, 8))
        .expect_err("a class and manifest cannot prove the identity");
    assert!(error.to_string().contains("known official server JAR"));
}

#[test]
fn inspects_structural_facts_and_validates_them_against_an_exact_target() {
    let dir = TempDir::new("inspect");
    let valid = dir.path().join("server.jar");
    write_jar(&valid, &version_json("1.21.4", 4189, 21), true);
    let info = inspect_server_jar(&valid).expect("valid server jar");
    assert_eq!(info.version, "1.21.4");
    assert_eq!(info.data_version, target_data_version());
    assert_eq!(info.java_major, Some(21));
    assert_eq!(info.protocol_version, Some(769));
    assert_eq!(info.resource_pack_format, Some(46));
    assert_eq!(info.size, fs::metadata(&valid).expect("metadata").len());
    assert_eq!(info.sha1, sha1_file(&valid).expect("sha1"));
    assert_eq!(info.sha1.len(), 40);
    info.validate_for(&target("1.21.4", 4189, 21))
        .expect("exact target");

    let wrong_world = dir.path().join("wrong-world.jar");
    write_jar(&wrong_world, &version_json("1.21.4", 5023, 21), true);
    let info = inspect_server_jar(&wrong_world).expect("structural inspection");
    assert!(info.validate_for(&target("1.21.4", 4189, 21)).is_err());

    let wrong_id = dir.path().join("wrong-id.jar");
    write_jar(&wrong_id, &version_json("1.20.6", 3839, 21), true);
    let info = inspect_server_jar(&wrong_id).expect("structural inspection");
    assert!(info.validate_for(&target("1.21.4", 4189, 21)).is_err());

    let wrong_java = dir.path().join("wrong-java.jar");
    write_jar(&wrong_java, &version_json("1.21.4", 4189, 17), true);
    let info = inspect_server_jar(&wrong_java).expect("structural inspection");
    assert!(info.validate_for(&target("1.21.4", 4189, 21)).is_err());

    let client = dir.path().join("client.jar");
    write_jar(&client, &version_json("1.21.4", 4189, 21), false);
    assert!(inspect_server_jar(&client).is_err());

    let tiny = dir.path().join("tiny.jar");
    fs::write(&tiny, b"not a jar").expect("tiny jar");
    assert!(inspect_server_jar(&tiny).is_err());
}

#[test]
fn manual_selection_verifies_reports_and_detects_disk_changes() {
    let dir = TempDir::new("manual");
    let manager = dir.manager();
    let exact = target("1.21.4", 4189, 21);
    let jar = dir.path().join("server.jar");
    write_jar(&jar, &version_json("1.21.4", 4189, 21), true);

    let report = manager
        .select_manual(&jar, &exact)
        .expect("select server jar");
    assert!(report.ready);
    assert_eq!(report.source, ServerSource::Manual);
    assert!(!report.managed);
    assert_eq!(report.version.as_deref(), Some("1.21.4"));
    assert_eq!(report.data_version, Some(4189));
    assert_eq!(report.matches_official, None);
    assert_eq!(report.warnings.len(), 1);

    let discovery = manager.discover(&exact);
    assert_eq!(discovery.target_minecraft_version, "1.21.4");
    assert_eq!(discovery.target_data_version, 4189);
    assert_eq!(discovery.target_java_major, 21);
    assert!(discovery.selected.as_ref().expect("selected").ready);
    assert!(!discovery.manual_hint.is_empty());

    // Append a byte: the stored size/SHA-1 no longer describe the file.
    fs::OpenOptions::new()
        .append(true)
        .open(&jar)
        .expect("open jar")
        .write_all(b"x")
        .expect("append");
    let changed = manager.selected(&exact).expect("saved report");
    assert!(!changed.ready);
    assert!(changed
        .error
        .expect("changed error")
        .contains("changed on disk"));

    manager
        .clear_selection(&exact.minecraft_version)
        .expect("clear selection");
    assert!(manager.selected(&exact).is_none());

    let client = dir.path().join("client.jar");
    write_jar(&client, &version_json("1.21.4", 4189, 21), false);
    assert!(manager.select_manual(&client, &exact).is_err());
    assert!(manager.selected(&exact).is_none());
}

#[test]
fn selections_are_kept_per_exact_version() {
    let dir = TempDir::new("versions");
    let manager = dir.manager();
    let first = target("1.21.4", 4189, 21);
    let second = target("1.20.6", 3839, 21);
    let first_jar = dir.path().join("server-first.jar");
    let second_jar = dir.path().join("server-second.jar");
    write_jar(&first_jar, &version_json("1.21.4", 4189, 21), true);
    write_jar(&second_jar, &version_json("1.20.6", 3839, 21), true);

    manager
        .select_manual(&first_jar, &first)
        .expect("select first version");
    manager
        .select_manual(&second_jar, &second)
        .expect("select second version");

    assert!(manager.selected(&first).expect("first").ready);
    assert!(manager.selected(&second).expect("second").ready);
    assert_eq!(
        manager
            .selected(&second)
            .expect("second")
            .version
            .as_deref(),
        Some("1.20.6")
    );

    // A JAR for another version is refused and never replaces an existing selection.
    assert!(manager.select_manual(&first_jar, &second).is_err());
    assert!(manager.selected(&second).expect("second").ready);
    assert!(manager.selected(&first).expect("first").ready);

    manager
        .clear_selection(&second.minecraft_version)
        .expect("clear second");
    assert!(manager.selected(&second).is_none());
    assert!(manager.selected(&first).is_some());
}

#[test]
fn eula_acceptance_is_explicit_and_tied_to_the_exact_jar_and_version() {
    let dir = TempDir::new("eula");
    let manager = dir.manager();
    let exact = target("1.21.4", 4189, 21);
    let second = target("1.20.6", 3839, 21);
    let jar = dir.path().join("server.jar");
    let second_jar = dir.path().join("server-second.jar");
    write_jar(&jar, &version_json("1.21.4", 4189, 21), true);
    write_jar(&second_jar, &version_json("1.20.6", 3839, 21), true);

    assert!(manager.accept_eula(false, &exact).is_err());
    assert!(manager.eula_for(&exact).is_none());
    assert!(
        manager.accept_eula(true, &exact).is_err(),
        "no selected JAR yet"
    );

    let report = manager
        .select_manual(&jar, &exact)
        .expect("select server jar");
    let sha1 = report.sha1.clone().expect("sha1");
    assert!(manager.discover(&exact).eula.is_none());
    assert!(!manager.discover(&exact).eula_matches_selected);

    let eula = manager
        .accept_eula(true, &exact)
        .expect("explicit acceptance");
    assert_eq!(eula.minecraft_version, "1.21.4");
    assert_eq!(eula.jar_sha1, sha1);
    assert_eq!(eula.eula_url, "https://aka.ms/MinecraftEULA");
    assert!(eula.matches("1.21.4", &sha1));
    assert!(!eula.matches("1.20.6", &sha1));
    assert!(!eula.matches("1.21.4", "0000000000000000000000000000000000000000"));
    let discovery = manager.discover(&exact);
    assert!(discovery.eula_matches_selected);
    assert_eq!(
        discovery
            .eula
            .as_ref()
            .expect("matching eula")
            .minecraft_version,
        "1.21.4"
    );

    // A second version gets its own selection and acceptance; the first is not reused.
    manager
        .select_manual(&second_jar, &second)
        .expect("select second version");
    assert!(manager.eula_for(&second).is_none());
    assert!(manager.discover(&exact).eula_matches_selected);
    let second_eula = manager
        .accept_eula(true, &second)
        .expect("second explicit acceptance");
    assert_eq!(second_eula.minecraft_version, "1.20.6");
    assert_ne!(second_eula.jar_sha1, eula.jar_sha1);
    assert!(manager.eula_for(&exact).is_some());
    assert!(manager.eula_for(&second).is_some());

    manager
        .clear_selection(&exact.minecraft_version)
        .expect("clear first selection");
    let discovery = manager.discover(&exact);
    assert!(discovery.selected.is_none());
    assert!(
        discovery.eula.is_none(),
        "no JAR means no matching acceptance"
    );
    assert!(!discovery.eula_matches_selected);
    assert!(manager.eula_for(&second).is_some());

    manager
        .select_manual(&jar, &exact)
        .expect("reselect server jar");
    assert!(
        manager.discover(&exact).eula_matches_selected,
        "the acceptance survives reselecting the same JAR"
    );
    manager.clear_eula(&exact).expect("withdraw eula");
    assert!(manager.eula_for(&exact).is_none());
    assert!(!manager.discover(&exact).eula_matches_selected);
    assert!(
        manager.eula_for(&second).is_some(),
        "withdrawing the baseline acceptance keeps another JAR's record"
    );
}

#[test]
fn a_changed_jar_is_not_covered_by_its_old_eula_acceptance() {
    let dir = TempDir::new("eula-stale");
    let manager = dir.manager();
    let exact = target("1.21.4", 4189, 21);
    let jar = dir.path().join("server.jar");
    write_jar(&jar, &version_json("1.21.4", 4189, 21), true);
    manager
        .select_manual(&jar, &exact)
        .expect("select server jar");
    manager
        .accept_eula(true, &exact)
        .expect("explicit acceptance");
    assert!(manager.discover(&exact).eula_matches_selected);

    fs::OpenOptions::new()
        .append(true)
        .open(&jar)
        .expect("open jar")
        .write_all(b"tampered")
        .expect("append");
    let discovery = manager.discover(&exact);
    assert!(!discovery.selected.as_ref().expect("selection").ready);
    assert!(!discovery.eula_matches_selected);
    assert!(discovery.eula.is_none());
    assert!(
        manager.eula_for(&exact).is_some(),
        "the record still exists, but it no longer authorizes the changed file"
    );
    assert!(manager.accept_eula(true, &exact).is_err());
}

#[test]
fn missing_manual_jar_is_reported_not_hidden() {
    let dir = TempDir::new("missing");
    let manager = dir.manager();
    let missing = dir.path().join("gone").join("server.jar");
    fs::create_dir_all(manager.settings_path().parent().expect("settings parent"))
        .expect("settings dir");
    fs::write(
        manager.settings_path(),
        serde_json::to_vec(&serde_json::json!({
            "schemaVersion": 3,
            "selected": [{
                "minecraftVersion": "1.21.4",
                "path": missing.display().to_string(),
                "sha1": "4707d00eb834b446575d89a61a11b5d548d8c001",
                "size": 56880250,
                "dataVersion": 4189
            }]
        }))
        .expect("settings json"),
    )
    .expect("settings file");
    let manager = dir.manager();
    let report = manager
        .selected(&target("1.21.4", 4189, 21))
        .expect("saved report");
    assert!(!report.ready);
    assert!(report.error.is_some());
    assert_eq!(report.source, ServerSource::Manual);
}

#[test]
fn offline_server_resolution_is_refused_without_network() {
    let dir = TempDir::new("offline");
    let appdata = dir.path().join("appdata");
    let manager = ServerManager::with_client(
        appdata.join("server.json"),
        dir.path().join("servers"),
        appdata.join("version-manifest.json"),
        download_pipeline::DownloadClient::new(download_pipeline::NetworkConfig::offline()),
    );
    let error = manager
        .release(&target("1.21.4", 4189, 21))
        .expect_err("offline");
    assert!(matches!(error, ServerError::Offline), "{error:?}");
}
