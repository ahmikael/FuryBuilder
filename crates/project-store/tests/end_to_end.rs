use project_store::{
    checksum, decode_world, encode_world, snapshot_bytes, CreationMode, EditorState, ProjectStore,
    SaveKind,
};
use world_domain::BlockPos;

mod common;

#[test]
fn end_to_end_reopen_matches_bytes_checksum_and_semantics() {
    let root = common::temp_root("end-to-end");
    let store = ProjectStore::new(&root);
    let detail = store
        .create("End to End", CreationMode::Flat, EditorState::default())
        .expect("create");
    let folder = detail.summary.folder.clone();
    let (_, mut world, _) = store.open(&folder).expect("open");

    let base = snapshot_bytes(&world).unwrap();
    let bricks: world_domain::BlockState = "minecraft:stone_bricks".parse().unwrap();
    for x in -4..=4 {
        world
            .set_block("minecraft:overworld", BlockPos::new(x, 70, 0), &bricks)
            .unwrap();
    }
    world
        .set_block(
            "minecraft:the_end",
            BlockPos::new(0, 40, 0),
            &"minecraft:obsidian".parse().unwrap(),
        )
        .unwrap();
    world.bump_revision();
    assert!(base != snapshot_bytes(&world).unwrap());

    let editor_state = EditorState {
        selected_dimension: "minecraft:overworld".to_owned(),
        load_radius: 4,
        workspace: Default::default(),
    };
    let saved = store
        .save(&folder, &world, editor_state.clone(), SaveKind::Manual)
        .expect("save");
    let expected_bytes = snapshot_bytes(&world).unwrap();
    assert_eq!(saved.checksum, checksum(&expected_bytes));
    assert_eq!(
        decode_world(&expected_bytes).unwrap().block_count(),
        world.block_count()
    );

    let (detail, reopened, recovery) = store.open(&folder).expect("reopen");
    assert!(recovery.is_none());
    assert_eq!(detail.editor_state, editor_state);
    assert_eq!(reopened.revision(), world.revision());
    assert_eq!(snapshot_bytes(&reopened).unwrap(), expected_bytes);
    assert_eq!(encode_world(&reopened).unwrap(), expected_bytes);

    // Simulate a crash: unsaved edits + autosave, then adopt, save and reopen again.
    store
        .begin_session(&folder, reopened.revision())
        .expect("marker");
    let mut crashed = reopened;
    crashed
        .set_block(
            "minecraft:overworld",
            BlockPos::new(12, 71, 12),
            &"minecraft:emerald_block".parse().unwrap(),
        )
        .unwrap();
    crashed.bump_revision();
    store
        .autosave(&folder, &crashed, editor_state.clone(), "")
        .expect("autosave");
    let (_, recovered, info) = store.open(&folder).expect("recover");
    let info = info.expect("recovery info");
    assert_eq!(info.recovered_revision, crashed.revision());
    assert_eq!(
        snapshot_bytes(&recovered).unwrap(),
        snapshot_bytes(&crashed).unwrap()
    );
    store
        .save(&folder, &recovered, editor_state.clone(), SaveKind::Manual)
        .expect("save recovered");
    store.end_session(&folder).expect("end session");
    let (_, final_world, info) = store.open(&folder).expect("final open");
    assert!(info.is_none());
    assert_eq!(
        snapshot_bytes(&final_world).unwrap(),
        snapshot_bytes(&crashed).unwrap()
    );
    assert_eq!(
        final_world
            .get_block("minecraft:overworld", BlockPos::new(12, 71, 12))
            .unwrap()
            .name(),
        "minecraft:emerald_block"
    );
}
