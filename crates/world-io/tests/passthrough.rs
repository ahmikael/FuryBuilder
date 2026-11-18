//! Tests for the byte-preserving Java world copy (M8.4).

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use flate2::write::ZlibEncoder;
use flate2::Compression;
use sha2::{Digest, Sha256};
use world_io::nbt::{self, NbtFile, NbtValue, TAG_COMPOUND, TAG_END, TAG_STRING};
use world_io::passthrough::{
    passthrough_java_world, passthrough_java_world_with_options, PassthroughError,
    PassthroughOptions,
};

const TARGET: i32 = 4189;

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "furybuilder-passthrough-{label}-{}-{}",
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

fn level_dat_bytes() -> Vec<u8> {
    let file = NbtFile::new(
        "",
        NbtValue::compound([(
            "Data",
            NbtValue::compound([
                ("DataVersion", NbtValue::Int(TARGET)),
                (
                    "Version",
                    NbtValue::compound([
                        ("Id", NbtValue::Int(TARGET)),
                        ("Name", NbtValue::String("1.21.4".to_owned())),
                    ]),
                ),
                ("LevelName", NbtValue::String("Passthrough test".to_owned())),
                (
                    "UnknownLevelTag",
                    NbtValue::String("must survive byte-for-byte".to_owned()),
                ),
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

fn chunk_nbt() -> NbtFile {
    NbtFile::new(
        "",
        NbtValue::compound([
            ("DataVersion", NbtValue::Int(TARGET)),
            ("xPos", NbtValue::Int(0)),
            ("zPos", NbtValue::Int(0)),
            ("yPos", NbtValue::Int(-4)),
            ("Status", NbtValue::String("minecraft:full".to_owned())),
            (
                "sections",
                NbtValue::List {
                    kind: TAG_COMPOUND,
                    items: vec![section(-4)],
                },
            ),
            ("block_entities", NbtValue::empty_list(TAG_END)),
            ("entities", NbtValue::empty_list(TAG_END)),
            ("UnknownChunkTag", NbtValue::Long(42)),
        ]),
    )
}

fn zlib(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes).expect("zlib write");
    encoder.finish().expect("zlib finish")
}

fn region_bytes() -> Vec<u8> {
    let raw = nbt::write_uncompressed(&chunk_nbt()).expect("encode chunk");
    let payload = zlib(&raw);
    let record_len = 5 + payload.len();
    let sectors = record_len.div_ceil(4096);
    assert!(sectors < 128);
    let mut file = vec![0u8; 8192];
    file[0..3].copy_from_slice(&[0, 0, 2]);
    file[3] = sectors as u8;
    file.extend_from_slice(&((payload.len() as u32 + 1).to_be_bytes()));
    file.push(2);
    file.extend_from_slice(&payload);
    file.resize(8192 + sectors * 4096, 0);
    file
}

fn write_world(root: &Path) {
    fs::create_dir_all(root.join("region")).expect("create region");
    fs::create_dir_all(root.join("datapacks").join("example")).expect("create datapacks");
    fs::write(root.join("level.dat"), level_dat_bytes()).expect("write level.dat");
    fs::write(root.join("level.dat_old"), b"old level backup").expect("write level.dat_old");
    fs::write(root.join("region").join("r.0.0.mca"), region_bytes()).expect("write region");
    fs::write(
        root.join("datapacks").join("example").join("unknown.bin"),
        [0u8, 1, 2, 3, 0xff, 0x00, 42],
    )
    .expect("write unknown binary");
    fs::write(root.join("custom-unknown.dat"), b"do not drop me").expect("write unknown metadata");
    fs::write(root.join("session.lock"), b"1234567890123456").expect("write session lock");
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut stack = vec![PathBuf::new()];
    while let Some(relative) = stack.pop() {
        let directory = root.join(&relative);
        for entry in fs::read_dir(&directory).expect("read directory") {
            let entry = entry.expect("directory entry");
            let entry_relative = relative.join(entry.file_name());
            let metadata = fs::symlink_metadata(entry.path()).expect("metadata");
            if metadata.is_dir() {
                stack.push(entry_relative);
            } else {
                files.insert(
                    entry_relative,
                    fs::read(entry.path()).expect("read fixture file"),
                );
            }
        }
    }
    files
}

fn temp_entries(root: &Path) -> Vec<String> {
    fs::read_dir(root)
        .expect("read parent")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| name.starts_with(".furybuilder-passthrough-"))
        .collect()
}

fn sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn copies_world_bytes_and_unknown_metadata_without_touching_source() {
    let dir = TempDir::new("copy");
    let source = dir.path().join("source");
    write_world(&source);
    let before = snapshot(&source);
    let destination = dir.path().join("copy");

    let report = passthrough_java_world(&source, &destination).expect("passthrough copy");
    assert_eq!(report.regions, 1);
    assert_eq!(report.chunks, 1);
    assert!(report.chunk_validation);
    assert_eq!(report.skipped.len(), 1);
    assert_eq!(report.skipped[0].relative_path, Path::new("session.lock"));
    assert!(!destination.join("session.lock").exists());
    assert_eq!(snapshot(&source), before, "source must stay byte-identical");

    let after = snapshot(&destination);
    assert_eq!(
        after.keys().cloned().collect::<Vec<_>>(),
        before
            .keys()
            .filter(|path| path.as_path() != Path::new("session.lock"))
            .cloned()
            .collect::<Vec<_>>()
    );
    for (relative, bytes) in &before {
        if relative.as_path() == Path::new("session.lock") {
            continue;
        }
        assert_eq!(
            after.get(relative),
            Some(bytes),
            "copied file differs: {}",
            relative.display()
        );
    }

    let region = destination.join("region").join("r.0.0.mca");
    let copied_region = report
        .files
        .iter()
        .find(|file| file.relative_path == Path::new("region").join("r.0.0.mca"))
        .expect("region report entry");
    assert_eq!(copied_region.bytes, fs::metadata(&region).unwrap().len());
    assert_eq!(copied_region.sha256, sha256(&fs::read(&region).unwrap()));
    assert_eq!(copied_region.sha256.len(), 64);
    assert!(report.bytes() > 0);
    assert!(temp_entries(dir.path()).is_empty());
}

#[test]
fn refuses_existing_and_nested_destinations_without_leaving_temp_dirs() {
    let dir = TempDir::new("destination");
    let source = dir.path().join("source");
    write_world(&source);

    let existing = dir.path().join("existing");
    fs::create_dir(&existing).expect("create existing destination");
    fs::write(existing.join("marker.txt"), b"keep").expect("write marker");
    assert!(matches!(
        passthrough_java_world(&source, &existing),
        Err(PassthroughError::DestinationExists(_))
    ));
    assert_eq!(
        fs::read(existing.join("marker.txt")).expect("marker"),
        b"keep"
    );

    let nested = source.join("nested-copy");
    assert!(matches!(
        passthrough_java_world(&source, &nested),
        Err(PassthroughError::DestinationInsideSource(_))
    ));
    assert!(!nested.exists());
    assert!(temp_entries(dir.path()).is_empty());
}

#[test]
fn corrupt_source_chunk_is_not_published() {
    let dir = TempDir::new("corrupt");
    let source = dir.path().join("source");
    write_world(&source);
    let mut bytes = fs::read(source.join("region").join("r.0.0.mca")).expect("read region");
    bytes[8192..8196].copy_from_slice(&u32::MAX.to_be_bytes());
    fs::write(source.join("region").join("r.0.0.mca"), bytes).expect("corrupt region");
    let destination = dir.path().join("copy");

    let error = passthrough_java_world(&source, &destination).expect_err("must fail closed");
    assert_eq!(error.code(), "unsupported_format", "{error}");
    assert!(!destination.exists());
    assert!(temp_entries(dir.path()).is_empty());
}

#[test]
fn explicit_chunk_validation_opt_out_still_copies_bytes() {
    let dir = TempDir::new("no-validation");
    let source = dir.path().join("source");
    write_world(&source);
    let mut bytes = fs::read(source.join("region").join("r.0.0.mca")).expect("read region");
    bytes[8192..8196].copy_from_slice(&u32::MAX.to_be_bytes());
    fs::write(source.join("region").join("r.0.0.mca"), bytes.clone()).expect("corrupt region");
    let destination = dir.path().join("copy");

    let report = passthrough_java_world_with_options(
        &source,
        &destination,
        PassthroughOptions {
            validate_chunks: false,
            ..PassthroughOptions::default()
        },
    )
    .expect("explicit opt-out");
    assert!(!report.chunk_validation);
    assert_eq!(
        fs::read(destination.join("region").join("r.0.0.mca")).expect("copied region"),
        bytes
    );
}

#[test]
fn file_limit_failure_leaves_no_destination_or_temp_dir() {
    let dir = TempDir::new("limits");
    let source = dir.path().join("source");
    write_world(&source);
    let destination = dir.path().join("copy");

    let error = passthrough_java_world_with_options(
        &source,
        &destination,
        PassthroughOptions {
            max_file_bytes: 4,
            ..PassthroughOptions::default()
        },
    )
    .expect_err("file limit must fail");
    assert_eq!(error.code(), "limit_exceeded", "{error}");
    assert!(!destination.exists());
    assert!(temp_entries(dir.path()).is_empty());
}

#[test]
fn symlinked_unknown_file_is_refused() {
    let dir = TempDir::new("symlink-file");
    let source = dir.path().join("source");
    write_world(&source);
    let target = dir.path().join("outside.txt");
    fs::write(&target, b"outside").expect("write target");
    if !make_symlink_file(&target, &source.join("linked-unknown.txt")) {
        return;
    }
    let destination = dir.path().join("copy");
    assert!(matches!(
        passthrough_java_world(&source, &destination),
        Err(PassthroughError::Symlink(_))
    ));
    assert!(!destination.exists());
    assert!(temp_entries(dir.path()).is_empty());
}

#[test]
fn symlinked_unknown_directory_is_refused() {
    let dir = TempDir::new("symlink-dir");
    let source = dir.path().join("source");
    write_world(&source);
    let target = dir.path().join("outside-dir");
    fs::create_dir(&target).expect("create target");
    fs::write(target.join("file.txt"), b"outside").expect("write target file");
    if !make_symlink_dir(&target, &source.join("linked-unknown-dir")) {
        return;
    }
    let destination = dir.path().join("copy");
    assert!(matches!(
        passthrough_java_world(&source, &destination),
        Err(PassthroughError::Symlink(_))
    ));
    assert!(!destination.exists());
    assert!(temp_entries(dir.path()).is_empty());
}

#[test]
fn symlinked_source_root_is_refused() {
    let dir = TempDir::new("symlink-source");
    let source = dir.path().join("source");
    write_world(&source);
    let link = dir.path().join("source-link");
    if !make_symlink_dir(&source, &link) {
        return;
    }
    let destination = dir.path().join("copy");
    assert!(matches!(
        passthrough_java_world(&link, &destination),
        Err(PassthroughError::Symlink(_))
    ));
    assert!(!destination.exists());
    assert!(temp_entries(dir.path()).is_empty());
}

#[cfg(windows)]
fn make_symlink_file(target: &Path, link: &Path) -> bool {
    std::os::windows::fs::symlink_file(target, link).is_ok()
}

#[cfg(unix)]
fn make_symlink_file(target: &Path, link: &Path) -> bool {
    std::os::unix::fs::symlink(target, link).is_ok()
}

#[cfg(windows)]
fn make_symlink_dir(target: &Path, link: &Path) -> bool {
    std::os::windows::fs::symlink_dir(target, link).is_ok()
}

#[cfg(unix)]
fn make_symlink_dir(target: &Path, link: &Path) -> bool {
    std::os::unix::fs::symlink(target, link).is_ok()
}
