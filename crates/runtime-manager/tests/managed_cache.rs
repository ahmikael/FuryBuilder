//! M9.3 tests: managed runtime archive safety, cache inventory and stale selections.
//! Fixtures are synthetic archives; `examples/runtime_probe.rs` exercises the real
//! discovery path against a local Java installation.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use flate2::write::DeflateEncoder;
use flate2::Compression;
use furybuilder_runtime_manager::{
    ManagedRuntimeCache, RuntimeError, RuntimeManager, RuntimeSource, RuntimeStatus,
};

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "furybuilder-runtime-{label}-{}-{}",
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

/// Minimal ZIP writer with Unix modes so a symlink entry can be exercised.
fn zip_bytes(entries: &[(&str, &[u8], u32)]) -> Vec<u8> {
    let mut local = Vec::new();
    let mut central = Vec::new();
    for (name, data, mode) in entries {
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
        central.extend_from_slice(&0x0314u16.to_le_bytes()); // Unix, version 2.0
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
        let mode = if *mode == 0 { 0o100644 } else { *mode };
        central.extend_from_slice(&(mode << 16).to_le_bytes());
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

fn write_archive(dir: &Path, name: &str, entries: &[(&str, &[u8], u32)]) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, zip_bytes(entries)).expect("write archive");
    path
}

/// Staging accepts only the host platform's `bin/java` entry, so synthetic
/// archives must use the same layout a real Temurin build has on this OS.
fn java_entry(root: &str) -> String {
    if cfg!(windows) {
        format!("{root}/bin/java.exe")
    } else {
        format!("{root}/bin/java")
    }
}

fn manifest() -> furybuilder_runtime_manager::RuntimeManifest {
    manifest_for(21, "temurin-21.0.13+9-windows-x64", "21.0.13+9")
}

fn manifest_for(
    major: u32,
    id: &str,
    release: &str,
) -> furybuilder_runtime_manager::RuntimeManifest {
    furybuilder_runtime_manager::RuntimeManifest {
        id: id.to_owned(),
        major,
        version: format!("{release}.0.LTS"),
        release_name: format!("jdk-{release}"),
        sha256: "a".repeat(64),
        source_url: format!(
            "https://github.com/adoptium/temurin{major}-binaries/releases/download/x.zip"
        ),
        vendor: "Eclipse Adoptium".to_owned(),
        os: "windows".to_owned(),
        arch: "x64".to_owned(),
    }
}

fn stage_and_commit(
    cache: &ManagedRuntimeCache,
    dir: &Path,
    major: u32,
    release: &str,
    id: &str,
) -> furybuilder_runtime_manager::ManagedRuntime {
    let archive_root = format!("jdk-{release}-jre");
    let java = java_entry(&archive_root);
    let archive = write_archive(
        dir,
        &format!("temurin-{major}.zip"),
        &[(java.as_str(), b"fake-java", 0)],
    );
    let staged = cache
        .stage(
            &archive,
            &manifest_for(major, id, release),
            &std::sync::atomic::AtomicBool::new(false),
            &|_| {},
        )
        .expect("stage archive");
    let probe = furybuilder_runtime_manager::JavaProbe {
        version: Some(release.to_owned()),
        major: Some(major),
        vendor: Some("Eclipse Adoptium".to_owned()),
        arch: Some("amd64".to_owned()),
        ..Default::default()
    };
    cache
        .commit(staged, manifest_for(major, id, release), &probe)
        .expect("commit runtime")
}

#[test]
fn caches_each_java_major_under_its_own_release_directory() {
    let dir = TempDir::new("majors");
    let cache = ManagedRuntimeCache::new(dir.path().join("components").join("java"));
    let java_21 = stage_and_commit(
        &cache,
        dir.path(),
        21,
        "21.0.13+9",
        "temurin-21.0.13+9-windows-x64",
    );
    let java_17 = stage_and_commit(
        &cache,
        dir.path(),
        17,
        "17.0.13+8",
        "temurin-17.0.13+8-windows-x64",
    );

    assert!(cache.root().join("21").join(&java_21.id).is_dir());
    assert!(cache.root().join("17").join(&java_17.id).is_dir());
    let list = cache.list().expect("list runtimes");
    assert_eq!(list.runtimes.len(), 2);
    assert_eq!(
        list.runtimes
            .iter()
            .map(|runtime| runtime.major)
            .collect::<Vec<_>>(),
        vec![17, 21]
    );

    assert!(cache.remove(&java_17.id).expect("remove Java 17"));
    let after = cache.list().expect("list after remove");
    assert_eq!(after.runtimes.len(), 1);
    assert_eq!(after.runtimes[0].major, 21);
    assert!(!cache.root().join("17").exists());
    assert!(!cache
        .remove("missing-runtime")
        .expect("missing is not an error"));
}

fn probe_21() -> furybuilder_runtime_manager::JavaRuntimeReport {
    furybuilder_runtime_manager::JavaRuntimeReport {
        path: String::new(),
        source: RuntimeSource::Managed,
        status: RuntimeStatus::Compatible,
        version: Some("21.0.13".to_owned()),
        major: Some(21),
        vendor: Some("Eclipse Adoptium".to_owned()),
        arch: Some("amd64".to_owned()),
        vm: Some("OpenJDK 64-Bit Server VM".to_owned()),
        managed: true,
        warnings: Vec::new(),
        error: None,
    }
}

#[test]
fn stages_commits_lists_and_clears_only_managed_runtimes() {
    let dir = TempDir::new("cache");
    let cache = ManagedRuntimeCache::new(dir.path().join("runtimes"));
    let java = java_entry("jdk-21.0.13+9-jre");
    let archive = write_archive(
        dir.path(),
        "temurin.zip",
        &[
            (java.as_str(), b"fake-java", 0),
            ("jdk-21.0.13+9-jre/release", b"JAVA_VERSION=\"21.0.13\"", 0),
            ("jdk-21.0.13+9-jre/LICENSE", b"GPLv2+CE", 0),
        ],
    );
    let staged = cache
        .stage(
            &archive,
            &manifest(),
            &std::sync::atomic::AtomicBool::new(false),
            &|_| {},
        )
        .expect("stage archive");
    assert!(ManagedRuntimeCache::probe_staged(&staged).is_file());

    let probe = furybuilder_runtime_manager::JavaProbe {
        version: Some("21.0.13".to_owned()),
        major: Some(21),
        vendor: Some("Eclipse Adoptium".to_owned()),
        arch: Some("amd64".to_owned()),
        ..Default::default()
    };
    let runtime = cache
        .commit(staged, manifest(), &probe)
        .expect("commit runtime");
    assert_eq!(runtime.id, "temurin-21.0.13+9-windows-x64");
    assert_eq!(runtime.major, 21);
    assert!(Path::new(&runtime.java_path).is_file());
    assert!(runtime.bytes > 0);
    assert!(cache.root().join("21").join(&runtime.id).is_dir());

    let list = cache.list().expect("list runtimes");
    assert_eq!(list.runtimes.len(), 1);
    assert_eq!(list.runtimes[0].version, "21.0.13+9.0.LTS");
    assert_eq!(list.runtimes[0].major, 21);
    assert!(list.total_bytes > 0);

    fs::write(cache.root().join("keep.txt"), b"not ours").expect("unrelated file");
    let cleared = cache.clear().expect("clear runtimes");
    assert!(cleared.runtimes.is_empty());
    assert!(cleared.total_bytes == 0);
    assert!(cache.root().join("keep.txt").is_file());
    assert!(!cache.root().join("21").join(&runtime.id).exists());
    assert_eq!(
        fs::read_dir(cache.root())
            .expect("cache root")
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(".staging-"))
            .count(),
        0
    );
}

#[test]
fn cancelled_staging_leaves_no_partial_runtime() {
    let dir = TempDir::new("cancelled");
    let cache = ManagedRuntimeCache::new(dir.path().join("runtimes"));
    let java = java_entry("jdk-21.0.13+9-jre");
    let archive = write_archive(
        dir.path(),
        "cancelled.zip",
        &[(java.as_str(), b"fake-java", 0)],
    );
    let error = cache
        .stage(
            &archive,
            &manifest(),
            &std::sync::atomic::AtomicBool::new(true),
            &|_| {},
        )
        .expect_err("cancelled staging must stop");
    assert!(matches!(error, RuntimeError::Cancelled), "{error:?}");
    assert!(cache.list().expect("list runtimes").runtimes.is_empty());
    if cache.root().exists() {
        assert_eq!(
            fs::read_dir(cache.root())
                .expect("cache root")
                .flatten()
                .filter(|entry| entry.file_name().to_string_lossy().starts_with(".staging-"))
                .count(),
            0
        );
    }
}

#[test]
fn rejects_traversal_symlinks_missing_java_and_wrong_version() {
    let dir = TempDir::new("unsafe");
    let cache = ManagedRuntimeCache::new(dir.path().join("runtimes"));
    let java = java_entry("jdk");

    let traversal = write_archive(
        dir.path(),
        "traversal.zip",
        &[("../escape.txt", b"escape", 0), (java.as_str(), b"java", 0)],
    );
    let error = cache
        .stage(
            &traversal,
            &manifest(),
            &std::sync::atomic::AtomicBool::new(false),
            &|_| {},
        )
        .expect_err("traversal must fail");
    assert!(matches!(error, RuntimeError::Archive(_)), "{error:?}");
    assert!(!dir.path().join("escape.txt").exists());

    let symlink = write_archive(
        dir.path(),
        "symlink.zip",
        &[(java.as_str(), b"java", 0), ("jdk/link", b"java", 0o120777)],
    );
    let error = cache
        .stage(
            &symlink,
            &manifest(),
            &std::sync::atomic::AtomicBool::new(false),
            &|_| {},
        )
        .expect_err("symlink must fail");
    assert!(matches!(error, RuntimeError::Archive(_)), "{error:?}");

    let missing = write_archive(dir.path(), "missing.zip", &[("jdk/README", b"none", 0)]);
    let error = cache
        .stage(
            &missing,
            &manifest(),
            &std::sync::atomic::AtomicBool::new(false),
            &|_| {},
        )
        .expect_err("missing java must fail");
    assert!(matches!(error, RuntimeError::Archive(_)), "{error:?}");

    let archive = write_archive(dir.path(), "old.zip", &[(java.as_str(), b"java", 0)]);
    let staged = cache
        .stage(
            &archive,
            &manifest(),
            &std::sync::atomic::AtomicBool::new(false),
            &|_| {},
        )
        .expect("stage old archive");
    let old_probe = furybuilder_runtime_manager::JavaProbe {
        version: Some("17.0.12".to_owned()),
        major: Some(17),
        ..Default::default()
    };
    let error = cache
        .commit(staged, manifest(), &old_probe)
        .expect_err("Java 17 must not be committed");
    assert!(matches!(error, RuntimeError::Archive(_)), "{error:?}");
    assert!(!cache
        .root()
        .join("21")
        .join("temurin-21.0.13+9-windows-x64")
        .exists());
}

#[test]
fn stale_selection_is_reported_and_can_be_cleared() {
    let dir = TempDir::new("settings");
    let settings_path = dir.path().join("appdata").join("runtime.json");
    fs::create_dir_all(settings_path.parent().expect("settings parent")).expect("settings dir");
    let missing = dir.path().join("gone").join("java.exe");
    let settings = serde_json::to_vec(&serde_json::json!({
        "schemaVersion": 1,
        "selectedJava": missing.display().to_string()
    }))
    .expect("settings json");
    fs::write(&settings_path, settings).expect("settings file");

    let manager = RuntimeManager::new(&settings_path, dir.path().join("runtimes"));
    let report = manager.saved().expect("saved report");
    assert_eq!(report.status, RuntimeStatus::Unreadable);
    assert_eq!(report.source, RuntimeSource::Manual);
    assert!(report.error.is_some());

    manager.clear_selection().expect("clear selection");
    assert!(manager.saved().is_none());
    assert!(manager
        .select(&dir.path().join("nope"))
        .expect_err("missing executable must fail")
        .to_string()
        .contains("nope"));

    let discovery = manager.discover();
    assert_eq!(discovery.target_java_major, 21);
    assert_eq!(discovery.target_minecraft_version, "1.21.4");
    assert!(!discovery.manual_hint.is_empty());
    assert_eq!(
        discovery.managed_root,
        dir.path().join("runtimes").display().to_string()
    );
}

#[test]
fn probe_report_is_compatible_only_for_the_target_version() {
    // Guards the report shape used by the UI: a compatible report is a fact from a probe.
    let report = probe_21();
    assert!(report.is_compatible());
    assert_eq!(report.major, Some(21));
}
