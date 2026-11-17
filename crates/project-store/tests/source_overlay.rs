use project_store::{checksum, CreationMode, EditorState, ProjectStore};

mod common;

#[test]
fn source_files_are_copied_immutably_with_hashes_and_safe_paths() {
    let root = common::temp_root("source");
    let store = ProjectStore::new(&root);
    let detail = store
        .create("Source", CreationMode::Empty, EditorState::default())
        .expect("create");
    let folder = detail.summary.folder.clone();
    let external = root.join("imported.structure");
    std::fs::write(&external, b"structure-payload").unwrap();

    let updated = store
        .attach_source(&folder, &external, "imported.structure")
        .expect("attach");
    assert_eq!(updated.source.kind, project_store::SourceKind::Imported);
    assert!(updated.source.immutable);
    let manifest = store.source_manifest(&folder).expect("source manifest");
    assert_eq!(manifest.files.len(), 1);
    assert_eq!(manifest.files[0].sha256, checksum(b"structure-payload"));
    assert_eq!(manifest.files[0].bytes, 17);

    let copied = root
        .join(&folder)
        .join(project_store::SOURCE_DIR)
        .join("files")
        .join("imported.structure");
    assert_eq!(std::fs::read(&copied).unwrap(), b"structure-payload");
    // The source layer is a copy: deleting the original does not affect the project.
    std::fs::remove_file(&external).unwrap();
    assert_eq!(std::fs::read(&copied).unwrap(), b"structure-payload");

    for unsafe_name in ["../escape.bin", "nested/file.bin", "..", ""] {
        assert!(
            store.attach_source(&folder, &copied, unsafe_name).is_err(),
            "`{unsafe_name}` must be rejected"
        );
    }
}
