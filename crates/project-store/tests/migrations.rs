use project_store::{
    decode_world, migrate_manifest, CreationMode, EditorState, ProjectError, ProjectStore,
    PROJECT_FILE, PROJECT_SCHEMA_VERSION,
};

mod common;

#[test]
fn older_schema_is_migrated_with_a_verbatim_backup() {
    let root = common::temp_root("migrate");
    let store = ProjectStore::new(&root);
    let detail = store
        .create("Migrate", CreationMode::Empty, EditorState::default())
        .expect("create");
    let dir = root.join(&detail.summary.folder);
    let path = dir.join(PROJECT_FILE);
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    value["projectSchemaVersion"] = serde_json::Value::from(0);
    std::fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    let original = std::fs::read(&path).unwrap();

    let (_, world, _) = store
        .open(&detail.summary.folder)
        .expect("migrate and open");
    assert_eq!(world.revision(), 1);
    let migrated: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(
        migrated["projectSchemaVersion"].as_u64(),
        Some(PROJECT_SCHEMA_VERSION as u64)
    );
    assert_eq!(
        std::fs::read(dir.join("project.json.v0.bak")).unwrap(),
        original,
        "migration backup must be byte-identical to the pre-migration manifest"
    );

    let backup = migrate_manifest(value).expect("public migration");
    assert_eq!(backup.project_schema_version, PROJECT_SCHEMA_VERSION);
}

#[test]
fn newer_manifest_and_snapshot_versions_are_refused() {
    let root = common::temp_root("migrate-newer");
    let store = ProjectStore::new(&root);
    let detail = store
        .create("Newer", CreationMode::Empty, EditorState::default())
        .expect("create");
    let dir = root.join(&detail.summary.folder);
    let path = dir.join(PROJECT_FILE);
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    value["projectSchemaVersion"] = serde_json::Value::from(99);
    std::fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    match store.open(&detail.summary.folder) {
        Err(ProjectError::UnsupportedSchema { found: 99, .. }) => {}
        other => panic!("expected unsupported schema, got {other:?}"),
    }

    let mut snapshot = project_store::snapshot_bytes(&world_domain::World::new_empty()).unwrap();
    snapshot[8] = 99; // format version is the two bytes after the 8-byte magic
    match decode_world(&snapshot) {
        Err(ProjectError::UnsupportedSchema { found: 99, .. }) => {}
        other => panic!("expected unsupported snapshot version, got {other:?}"),
    }
}
