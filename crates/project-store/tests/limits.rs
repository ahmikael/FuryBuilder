use project_store::{
    CreationMode, EditorState, ProjectStore, SaveKind, CACHE_DIR, DATA_DIR, MAX_CHECKPOINTS,
    MAX_SNAPSHOTS,
};
use world_domain::BlockPos;

mod common;

#[test]
fn snapshots_and_checkpoints_are_bounded_and_only_cache_is_cleared() {
    let root = common::temp_root("limits");
    let store = ProjectStore::new(&root);
    let detail = store
        .create("Limits", CreationMode::Empty, EditorState::default())
        .expect("create");
    let folder = detail.summary.folder.clone();
    let dir = root.join(&folder);
    let (_, mut world, _) = store.open(&folder).expect("open");

    for step in 0..(MAX_SNAPSHOTS + 4) {
        world
            .set_block(
                "minecraft:overworld",
                BlockPos::new(step as i32, 64, 0),
                &"minecraft:stone".parse().unwrap(),
            )
            .unwrap();
        world.bump_revision();
        store
            .save(&folder, &world, EditorState::default(), SaveKind::Manual)
            .expect("save");
    }
    let snapshot_count = std::fs::read_dir(dir.join(DATA_DIR))
        .unwrap()
        .flatten()
        .filter(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            name.starts_with("world-") && name.ends_with(".fwl")
        })
        .count();
    assert_eq!(snapshot_count, MAX_SNAPSHOTS);

    for _ in 0..(MAX_CHECKPOINTS + 3) {
        world.bump_revision();
        store
            .checkpoint(&folder, &world, EditorState::default(), "limit test")
            .expect("checkpoint");
    }
    assert_eq!(
        store.checkpoints(&folder).expect("checkpoints").len(),
        MAX_CHECKPOINTS
    );

    std::fs::write(dir.join(CACHE_DIR).join("recreatable.bin"), b"cache").unwrap();
    store.end_session(&folder).expect("end session");
    assert_eq!(
        std::fs::read_dir(dir.join(CACHE_DIR)).unwrap().count(),
        0,
        "cache is recreatable and is cleared on close"
    );
    assert!(dir.join(project_store::PROJECT_FILE).exists());
    assert!(store.open(&folder).is_ok(), "saved data is untouched");
}
