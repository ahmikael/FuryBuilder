use project_store::{
    decode_world, CreationMode, EditorState, ProjectError, ProjectStore, SaveKind,
};
use world_domain::BlockPos;

mod common;

fn manifest_json(folder: &std::path::Path) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(folder.join(project_store::PROJECT_FILE)).unwrap())
        .unwrap()
}

#[test]
fn publish_is_atomic_checksum_guarded_and_keeps_the_previous_snapshot() {
    let root = common::temp_root("atomic");
    let store = ProjectStore::new(&root);
    let detail = store
        .create("Atomic", CreationMode::Empty, EditorState::default())
        .expect("create");
    let folder = detail.summary.folder.clone();
    let dir = root.join(&folder);
    let (_, mut world, _) = store.open(&folder).expect("open");
    world
        .set_block(
            "minecraft:overworld",
            BlockPos::new(4, 70, -4),
            &"minecraft:gold_block".parse().unwrap(),
        )
        .unwrap();
    world.bump_revision();
    let outcome = store
        .save(&folder, &world, EditorState::default(), SaveKind::Manual)
        .expect("save");
    assert!(outcome.backup_created);
    assert_eq!(outcome.checksum.len(), 64);

    let json = manifest_json(&dir);
    let world_file = json["worldFile"].as_str().unwrap();
    assert_eq!(world_file, outcome.snapshot_file);
    assert_eq!(json["worldChecksum"].as_str().unwrap(), outcome.checksum);
    assert_eq!(json["worldRevision"].as_u64(), Some(2));

    // Both revisions are present: a crash before the manifest rename can only leave an
    // unreferenced snapshot, never a manifest pointing at partial bytes.
    let snapshots: Vec<_> = std::fs::read_dir(dir.join(project_store::DATA_DIR))
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "fwl"))
        .collect();
    assert_eq!(snapshots.len(), 2);

    let current = dir.join(world_file);
    let mut bytes = std::fs::read(&current).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    std::fs::write(&current, bytes).unwrap();
    match store.open(&folder) {
        Err(ProjectError::ChecksumMismatch { expected, actual }) => {
            assert_eq!(expected, outcome.checksum);
            assert_ne!(actual, expected);
        }
        other => panic!("expected checksum mismatch, got {other:?}"),
    }

    // The older snapshot is independently decodable, which is what makes recovery possible.
    let older = snapshots
        .into_iter()
        .find(|path| path != &current)
        .expect("previous snapshot");
    let decoded = decode_world(&std::fs::read(older).unwrap()).expect("decode previous");
    assert_eq!(decoded.revision(), 1);
}
