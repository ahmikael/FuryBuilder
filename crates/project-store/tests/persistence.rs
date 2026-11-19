use project_store::{CreationMode, EditorState, ProjectStore, SaveKind};
use world_domain::BlockPos;

mod common;

#[test]
fn chunks_metadata_and_editor_state_survive_save_and_reopen() {
    let root = common::temp_root("persistence");
    let store = ProjectStore::new(&root);
    let detail = store
        .create("Persistence", CreationMode::Empty, EditorState::default())
        .expect("create");
    let folder = detail.summary.folder.clone();
    let (_, mut world, recovery) = store.open(&folder).expect("open");
    assert!(recovery.is_none());

    let stairs: world_domain::BlockState =
        "minecraft:oak_stairs[facing=west,half=top,shape=straight,waterlogged=false]"
            .parse()
            .unwrap();
    world
        .set_block("minecraft:overworld", BlockPos::new(-31, -64, 17), &stairs)
        .unwrap();
    world
        .set_block(
            "minecraft:the_nether",
            BlockPos::new(8, 40, -8),
            &"minecraft:nether_bricks".parse().unwrap(),
        )
        .unwrap();
    world.bump_revision();

    let editor_state = EditorState {
        selected_dimension: "minecraft:the_nether".to_owned(),
        load_radius: 3,
        workspace: Default::default(),
        scene: Default::default(),
        parametric: Default::default(),
    };
    let outcome = store
        .save(&folder, &world, editor_state.clone(), SaveKind::Manual)
        .expect("save");
    assert_eq!(outcome.project.summary.world_revision, 2);
    assert_eq!(outcome.project.editor_state, editor_state);
    assert!(outcome.bytes_written > 0);
    assert_eq!(outcome.checksum.len(), 64);

    let (reopened, restored, recovery) = store.open(&folder).expect("reopen");
    assert!(recovery.is_none());
    assert_eq!(restored.revision(), 2);
    assert_eq!(reopened.editor_state, editor_state);
    assert_eq!(
        restored
            .get_block("minecraft:overworld", BlockPos::new(-31, -64, 17))
            .unwrap(),
        stairs
    );
    assert_eq!(
        restored
            .get_block("minecraft:the_nether", BlockPos::new(8, 40, -8))
            .unwrap()
            .name(),
        "minecraft:nether_bricks"
    );
    assert_eq!(restored.chunk_count(), 2);
}
