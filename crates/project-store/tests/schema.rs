//! Current-contract tests for the project manifest and world snapshot versions.
//!
//! FuryBuilder reads exactly the current project schema. Older drafts are not migrated,
//! newer files are not guessed, and a refused manifest is never rewritten.

use project_store::{
    decode_world, snapshot_bytes, CreationMode, EditorState, ProjectError, ProjectStore,
    PROJECT_FILE, PROJECT_SCHEMA_VERSION, SNAPSHOT_FORMAT_VERSION,
};

mod common;

#[test]
fn only_the_current_project_schema_is_read() {
    let root = common::temp_root("schema-current-only");
    let store = ProjectStore::new(&root);
    let detail = store
        .create("Schema", CreationMode::Empty, EditorState::default())
        .expect("create");
    let dir = root.join(&detail.summary.folder);
    let path = dir.join(PROJECT_FILE);
    let original: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(
        original["projectSchemaVersion"].as_u64(),
        Some(PROJECT_SCHEMA_VERSION as u64)
    );

    for version in [0u32, 1, PROJECT_SCHEMA_VERSION + 1] {
        let mut value = original.clone();
        value["projectSchemaVersion"] = serde_json::Value::from(version);
        let bytes = serde_json::to_vec_pretty(&value).unwrap();
        std::fs::write(&path, &bytes).unwrap();

        match store.open(&detail.summary.folder) {
            Err(ProjectError::UnsupportedSchema { found, supported }) => {
                assert_eq!(found, version);
                assert_eq!(supported, PROJECT_SCHEMA_VERSION);
            }
            other => panic!("schema {version} must be refused, got {other:?}"),
        }
        assert_eq!(
            std::fs::read(&path).unwrap(),
            bytes,
            "a refused manifest must not be migrated or rewritten"
        );
        assert!(
            !dir.join(format!("project.json.v{version}.bak")).exists(),
            "schema {version} must not leave a migration backup"
        );
    }
}

#[test]
fn only_the_current_snapshot_version_is_read() {
    let base = snapshot_bytes(&world_domain::World::new_empty()).expect("snapshot");
    for version in [1u16, 2, SNAPSHOT_FORMAT_VERSION + 1] {
        let mut snapshot = base.clone();
        snapshot[8..10].copy_from_slice(&version.to_le_bytes());
        match decode_world(&snapshot) {
            Err(ProjectError::UnsupportedSchema { found, supported }) => {
                assert_eq!(found, u32::from(version));
                assert_eq!(supported, u32::from(SNAPSHOT_FORMAT_VERSION));
            }
            other => panic!("snapshot version {version} must be refused, got {other:?}"),
        }
    }
    assert!(
        decode_world(&base).is_ok(),
        "the current format must decode"
    );
}
