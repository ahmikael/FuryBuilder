use project_store::{
    checksum, CreationMode, EditorState, ProjectStore, SourceFile, SourceKind, SOURCE_DIR,
};
use world_domain::{BlockPos, World};

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

#[test]
fn imported_project_records_a_verified_immutable_world_tree() {
    let root = common::temp_root("imported-tree");
    let store = ProjectStore::new(&root);
    let mut world = World::new_empty();
    world
        .set_block(
            "minecraft:overworld",
            BlockPos::new(3, 70, -4),
            &"minecraft:stone_bricks".parse().unwrap(),
        )
        .unwrap();
    world.bump_revision();

    assert!(
        store
            .create("Unsupported", CreationMode::Import, EditorState::default())
            .is_err(),
        "the generic creator must not pretend to import a world"
    );

    let detail = store
        .create_imported("Imported World", EditorState::default(), &world, "world")
        .expect("create imported project");
    assert_eq!(detail.summary.creation_mode, CreationMode::Import);
    assert_eq!(detail.source.kind, SourceKind::Imported);
    assert!(detail.source.immutable);
    assert_eq!(detail.source.reference.as_deref(), Some("world"));

    let folder = detail.summary.folder.clone();
    let source_world = root.join(&folder).join(SOURCE_DIR).join("world");
    std::fs::create_dir_all(source_world.join("region")).unwrap();
    std::fs::write(source_world.join("level.dat"), b"level-payload").unwrap();
    std::fs::write(
        source_world.join("region").join("r.0.0.mca"),
        b"region-payload",
    )
    .unwrap();
    let files = vec![
        SourceFile {
            relative_path: "world/level.dat".to_owned(),
            bytes: 13,
            sha256: checksum(b"level-payload"),
        },
        SourceFile {
            relative_path: "world/region/r.0.0.mca".to_owned(),
            bytes: 14,
            sha256: checksum(b"region-payload"),
        },
    ];
    let updated = store
        .record_source_tree(&folder, "world", &files)
        .expect("record source tree");
    assert_eq!(updated.source.reference.as_deref(), Some("world"));
    assert!(updated
        .warnings
        .iter()
        .any(|warning| warning.contains("immutable source")));
    let manifest = store.source_manifest(&folder).expect("source manifest");
    assert_eq!(manifest.files.len(), 2);
    assert!(manifest.immutable);
    assert_eq!(manifest.files[0].relative_path, "world/level.dat");

    let (_, reopened, _) = store.open(&folder).expect("reopen imported project");
    assert_eq!(
        reopened
            .get_block("minecraft:overworld", BlockPos::new(3, 70, -4))
            .unwrap()
            .to_string(),
        "minecraft:stone_bricks"
    );

    for unsafe_path in [
        "world/../escape.dat",
        "other/level.dat",
        "world\\level.dat",
        "/world/level.dat",
    ] {
        let unsafe_files = vec![SourceFile {
            relative_path: unsafe_path.to_owned(),
            bytes: 13,
            sha256: checksum(b"level-payload"),
        }];
        assert!(
            store
                .record_source_tree(&folder, "world", &unsafe_files)
                .is_err(),
            "`{unsafe_path}` must be rejected"
        );
    }

    let missing = vec![SourceFile {
        relative_path: "world/missing.dat".to_owned(),
        bytes: 1,
        sha256: checksum(b"x"),
    }];
    assert!(store
        .record_source_tree(&folder, "world", &missing)
        .is_err());
}
