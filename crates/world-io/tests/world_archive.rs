//! Safety and layout tests for `.zip` world source preparation.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use world_io::world_source::{
    analyze_world_source, prepare_world_source, WorldSourceError, WorldSourceKind,
    MAX_WORLD_ROOT_DEPTH,
};
use zip::write::SimpleFileOptions;

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "furybuilder-archive-{label}-{}-{}",
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

fn zip_entries(archive: &Path, entries: &[(&str, &[u8], Option<u32>)]) {
    let file = fs::File::create(archive).expect("create archive");
    let mut zip = zip::ZipWriter::new(file);
    for (name, bytes, mode) in entries {
        let mut options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored)
            .system(zip::System::Unix);
        if let Some(mode) = mode {
            options = options.external_attributes(*mode << 16);
        }
        zip.start_file(*name, options).expect("start entry");
        zip.write_all(bytes).expect("write entry");
    }
    zip.finish().expect("finish archive");
}

#[test]
fn world_at_the_archive_root_is_found() {
    let dir = TempDir::new("root");
    let archive = dir.path().join("world.zip");
    zip_entries(&archive, &[("level.dat", b"level", None)]);
    let staging = dir.path().join("staging");

    let prepared = prepare_world_source(&archive, &staging).expect("prepare archive");
    assert_eq!(prepared.kind(), WorldSourceKind::Archive);
    assert_eq!(prepared.root(), staging.as_path());
    assert_eq!(prepared.archive().map(|report| report.entries), Some(1));
    assert!(staging.join("level.dat").is_file());
}

#[test]
fn nested_world_root_is_found() {
    let dir = TempDir::new("nested");
    let archive = dir.path().join("world.zip");
    zip_entries(
        &archive,
        &[
            ("My World/level.dat", b"level", None),
            ("My World/region/r.0.0.mca", b"region", None),
        ],
    );
    let staging = dir.path().join("staging");

    let prepared = prepare_world_source(&archive, &staging).expect("prepare archive");
    assert_eq!(prepared.root(), staging.join("My World").as_path());
}

#[test]
fn archive_without_a_world_is_refused_and_staging_is_removed() {
    let dir = TempDir::new("missing");
    let archive = dir.path().join("world.zip");
    zip_entries(&archive, &[("readme.txt", b"nothing", None)]);
    let staging = dir.path().join("staging");

    let error = prepare_world_source(&archive, &staging).expect_err("no world");
    assert!(matches!(error, WorldSourceError::NoWorldFolder));
    assert!(!staging.exists(), "failed extraction must not leak staging");
}

#[test]
fn ambiguous_world_roots_are_refused() {
    let dir = TempDir::new("ambiguous");
    let archive = dir.path().join("world.zip");
    zip_entries(
        &archive,
        &[
            ("First/level.dat", b"one", None),
            ("Second/level.dat", b"two", None),
        ],
    );
    let staging = dir.path().join("staging");

    let error = prepare_world_source(&archive, &staging).expect_err("ambiguous");
    assert!(matches!(error, WorldSourceError::AmbiguousWorldRoot(_)));
    assert!(!staging.exists());
}

#[test]
fn worlds_nested_beyond_the_search_depth_are_refused() {
    let dir = TempDir::new("deep");
    let archive = dir.path().join("world.zip");
    let deep = format!(
        "{}/level.dat",
        std::iter::repeat_n("nested", MAX_WORLD_ROOT_DEPTH + 1)
            .collect::<Vec<_>>()
            .join("/")
    );
    zip_entries(&archive, &[(&deep, b"level", None)]);
    let staging = dir.path().join("staging");

    let error = prepare_world_source(&archive, &staging).expect_err("too deep");
    assert!(matches!(error, WorldSourceError::NoWorldFolder));
}

#[test]
fn parent_directory_entries_are_refused() {
    let dir = TempDir::new("traversal");
    let archive = dir.path().join("world.zip");
    zip_entries(&archive, &[("../escape.txt", b"escape", None)]);
    let staging = dir.path().join("staging");

    let error = prepare_world_source(&archive, &staging).expect_err("traversal");
    assert!(
        matches!(error, WorldSourceError::UnsafeEntry(_)),
        "{error:?}"
    );
    assert!(!dir.path().join("escape.txt").exists());
}

#[test]
fn absolute_entries_are_refused() {
    let dir = TempDir::new("absolute");
    let archive = dir.path().join("world.zip");
    zip_entries(&archive, &[("/escape.txt", b"escape", None)]);
    let staging = dir.path().join("staging");

    let error = prepare_world_source(&archive, &staging).expect_err("absolute");
    assert!(
        matches!(error, WorldSourceError::UnsafeEntry(_)),
        "{error:?}"
    );
}

#[test]
fn symlink_entries_are_refused() {
    let dir = TempDir::new("symlink");
    let archive = dir.path().join("world.zip");
    zip_entries(&archive, &[("world/link", b"target", Some(0o120777))]);
    let staging = dir.path().join("staging");

    let error = prepare_world_source(&archive, &staging).expect_err("symlink");
    assert!(
        matches!(error, WorldSourceError::UnsafeEntry(_)),
        "{error:?}"
    );
}

#[test]
fn invalid_archives_become_analysis_blockers_instead_of_command_errors() {
    let dir = TempDir::new("invalid");
    let archive = dir.path().join("world.zip");
    fs::write(&archive, b"not a zip").expect("write invalid archive");
    let staging = dir.path().join("staging");

    let analysis = analyze_world_source(&archive, &staging);
    assert!(!analysis.supported);
    assert_eq!(analysis.source_kind, WorldSourceKind::Archive);
    assert_eq!(analysis.selection_kind, "world_archive");
    assert_eq!(analysis.layout, "archive");
    assert!(analysis
        .blockers
        .iter()
        .any(|blocker| blocker.contains("not a valid ZIP")));
    assert!(!staging.exists());
}

#[test]
fn non_zip_files_are_not_treated_as_world_archives() {
    let dir = TempDir::new("file");
    let source = dir.path().join("level.dat");
    fs::write(&source, b"level").expect("write file");
    let staging = dir.path().join("staging");

    let error = prepare_world_source(&source, &staging).expect_err("plain file");
    assert!(matches!(error, WorldSourceError::UnsupportedSource(_)));
    assert!(!staging.exists());
}
