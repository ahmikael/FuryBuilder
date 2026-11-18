//! M9.4 tests: exact server JAR inspection, manual selection and stale-file handling.
//! Archives are synthetic and padded to the minimum real-JAR size; no Mojang binary is
//! committed. The real official 1.21.4 download is exercised by `examples/server_probe.rs`.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use furybuilder_runtime_manager::{
    inspect_server_jar, sha1_file, ServerManager, ServerSource, TARGET_DATA_VERSION,
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
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
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
    let file = fs::File::create(path).expect("create jar");
    let mut zip = zip::ZipWriter::new(file);
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    zip.start_file("version.json", options)
        .expect("start version.json");
    zip.write_all(version_json).expect("write version.json");
    if include_server_main {
        zip.start_file("net/minecraft/bundler/Main.class", options)
            .expect("start main");
        zip.write_all(b"not-a-real-main-class").expect("write main");
        zip.start_file("META-INF/versions.list", options)
            .expect("start versions.list");
        zip.write_all(
            b"1.21.4\t4707d00eb834b446575d89a61a11b5d548d8c001\tversions/1.21.4/server-1.21.4.jar",
        )
        .expect("write versions.list");
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
fn inspects_exact_version_and_rejects_non_server_archives() {
    let dir = TempDir::new("inspect");
    let valid = dir.path().join("server.jar");
    write_jar(&valid, &version_json("1.21.4", 4189, 21), true);
    let info = inspect_server_jar(&valid).expect("valid server jar");
    assert_eq!(info.version, "1.21.4");
    assert_eq!(info.data_version, TARGET_DATA_VERSION);
    assert_eq!(info.java_major, Some(21));
    assert_eq!(info.protocol_version, Some(769));
    assert_eq!(info.resource_pack_format, Some(46));
    assert_eq!(info.size, fs::metadata(&valid).expect("metadata").len());
    assert_eq!(info.sha1, sha1_file(&valid).expect("sha1"));
    assert_eq!(info.sha1.len(), 40);

    let client = dir.path().join("client.jar");
    write_jar(&client, &version_json("1.21.4", 4189, 21), false);
    assert!(inspect_server_jar(&client).is_err());

    let wrong_world = dir.path().join("wrong-world.jar");
    write_jar(&wrong_world, &version_json("1.21.4", 5023, 21), true);
    assert!(inspect_server_jar(&wrong_world).is_err());

    let wrong_id = dir.path().join("wrong-id.jar");
    write_jar(&wrong_id, &version_json("1.21.3", 4189, 21), true);
    assert!(inspect_server_jar(&wrong_id).is_err());

    let wrong_java = dir.path().join("wrong-java.jar");
    write_jar(&wrong_java, &version_json("1.21.4", 4189, 17), true);
    assert!(inspect_server_jar(&wrong_java).is_err());

    let tiny = dir.path().join("tiny.jar");
    fs::write(&tiny, b"not a jar").expect("tiny jar");
    assert!(inspect_server_jar(&tiny).is_err());
}

#[test]
fn manual_selection_verifies_reports_and_detects_disk_changes() {
    let dir = TempDir::new("manual");
    let jar = dir.path().join("server.jar");
    write_jar(&jar, &version_json("1.21.4", 4189, 21), true);
    let settings = dir.path().join("appdata").join("server.json");
    fs::create_dir_all(settings.parent().expect("settings parent")).expect("settings dir");
    let manager = ServerManager::new(&settings, dir.path().join("servers"));

    let report = manager.select_manual(&jar).expect("select server jar");
    assert!(report.ready);
    assert_eq!(report.source, ServerSource::Manual);
    assert!(!report.managed);
    assert_eq!(report.version.as_deref(), Some("1.21.4"));
    assert_eq!(report.data_version, Some(4189));
    assert_eq!(report.matches_official, None);
    assert_eq!(report.warnings.len(), 1);

    let discovery = manager.discover();
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
    let changed = manager.selected().expect("saved report");
    assert!(!changed.ready);
    assert!(changed
        .error
        .expect("changed error")
        .contains("changed on disk"));

    manager.clear_selection().expect("clear selection");
    assert!(manager.selected().is_none());

    let client = dir.path().join("client.jar");
    write_jar(&client, &version_json("1.21.4", 4189, 21), false);
    assert!(manager.select_manual(&client).is_err());
    assert!(manager.selected().is_none());
}

#[test]
fn missing_manual_jar_is_reported_not_hidden() {
    let dir = TempDir::new("missing");
    let settings = dir.path().join("server.json");
    let missing = dir.path().join("gone").join("server.jar");
    fs::write(
        &settings,
        serde_json::to_vec(&serde_json::json!({
            "schemaVersion": 1,
            "selected": {
                "path": missing.display().to_string(),
                "sha1": "4707d00eb834b446575d89a61a11b5d548d8c001",
                "size": 56880250,
                "dataVersion": 4189
            }
        }))
        .expect("settings json"),
    )
    .expect("settings file");
    let manager = ServerManager::new(&settings, dir.path().join("servers"));
    let report = manager.selected().expect("saved report");
    assert!(!report.ready);
    assert!(report.error.is_some());
    assert_eq!(report.source, ServerSource::Manual);
}
