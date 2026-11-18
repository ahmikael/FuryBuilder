use project_store::{checksum, CreationMode, EditorState, ProjectError, ProjectStore, EXPORTS_DIR};

mod common;

#[test]
fn exports_are_published_atomically_with_checksums_and_unique_names() {
    let root = common::temp_root("exports");
    let store = ProjectStore::new(&root);
    let detail = store
        .create("Export", CreationMode::Empty, EditorState::default())
        .expect("create");
    let folder = detail.summary.folder.clone();

    let first = store
        .write_export(&folder, "castle-r1.nbt", b"first-structure")
        .expect("export");
    assert_eq!(first.relative_path, "exports/castle-r1.nbt");
    assert_eq!(first.file_name, "castle-r1.nbt");
    assert_eq!(first.bytes, 15);
    assert_eq!(first.checksum, checksum(b"first-structure"));
    let written = root.join(&folder).join(EXPORTS_DIR).join("castle-r1.nbt");
    assert_eq!(std::fs::read(&written).unwrap(), b"first-structure");

    // A second export with the same requested name never overwrites the first.
    let second = store
        .write_export(&folder, "castle-r1.nbt", b"second-structure")
        .expect("second export");
    assert_eq!(second.file_name, "castle-r1-2.nbt");
    assert_eq!(std::fs::read(&written).unwrap(), b"first-structure");
    assert_eq!(
        std::fs::read(root.join(&folder).join(&second.relative_path)).unwrap(),
        b"second-structure"
    );

    // Exports survive a graceful close: they are outputs, not recreatable cache.
    store.end_session(&folder).expect("close");
    assert!(written.exists());

    for unsafe_name in [
        "../escape.nbt",
        "nested/file.nbt",
        "no-extension",
        "bad.txt",
        "",
    ] {
        assert!(
            matches!(
                store.write_export(&folder, unsafe_name, b"x"),
                Err(ProjectError::InvalidPath(_))
            ),
            "`{unsafe_name}` must be rejected"
        );
    }
}

#[test]
fn source_bytes_can_be_attached_without_a_filesystem_path() {
    let root = common::temp_root("source-bytes");
    let store = ProjectStore::new(&root);
    let detail = store
        .create("Source bytes", CreationMode::Empty, EditorState::default())
        .expect("create");
    let folder = detail.summary.folder.clone();
    let updated = store
        .attach_source_bytes(&folder, "imported.schem", b"payload-bytes")
        .expect("attach bytes");
    assert!(updated.source.immutable);
    assert_eq!(updated.source.reference.as_deref(), Some("imported.schem"));
    let manifest = store.source_manifest(&folder).expect("source manifest");
    assert_eq!(manifest.files.len(), 1);
    assert_eq!(manifest.files[0].sha256, checksum(b"payload-bytes"));
    let copied = root
        .join(&folder)
        .join(project_store::SOURCE_DIR)
        .join("files")
        .join("imported.schem");
    assert_eq!(std::fs::read(copied).unwrap(), b"payload-bytes");
    assert!(store
        .attach_source_bytes(&folder, "../escape.schem", b"x")
        .is_err());
}
