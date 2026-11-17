use project_store::{CreationMode, EditorState, ProjectStore};
use world_domain::BlockPos;

mod common;

#[test]
fn newer_verified_autosave_is_adopted_after_a_crash_marker() {
    let root = common::temp_root("recovery");
    let store = ProjectStore::new(&root);
    let detail = store
        .create("Crash", CreationMode::Empty, EditorState::default())
        .expect("create");
    let folder = detail.summary.folder.clone();
    store.begin_session(&folder, 1).expect("session marker");

    let (_, mut world, _) = store.open(&folder).expect("open");
    world
        .set_block(
            "minecraft:overworld",
            BlockPos::new(-2, 80, 2),
            &"minecraft:glowstone".parse().unwrap(),
        )
        .unwrap();
    world.bump_revision();
    world.bump_revision();
    store
        .autosave(&folder, &world, EditorState::default(), "")
        .expect("autosave");

    let (detail, recovered, info) = store.open(&folder).expect("recover");
    let info = info.expect("recovery info");
    assert_eq!(info.recovered_revision, 3);
    assert!(detail.summary.recoverable);
    assert_eq!(recovered.revision(), 3);
    assert_eq!(
        recovered
            .get_block("minecraft:overworld", BlockPos::new(-2, 80, 2))
            .unwrap()
            .name(),
        "minecraft:glowstone"
    );

    // A graceful close removes the marker; the autosave alone must not resurrect edits.
    store.end_session(&folder).expect("end session");
    let (_, saved, info) = store.open(&folder).expect("open saved");
    assert!(info.is_none());
    assert_eq!(saved.revision(), 1);
}

#[test]
fn autosave_at_or_below_the_saved_revision_is_not_recovered() {
    let root = common::temp_root("recovery-equal");
    let store = ProjectStore::new(&root);
    let detail = store
        .create("Equal", CreationMode::Empty, EditorState::default())
        .expect("create");
    let folder = detail.summary.folder.clone();
    let (_, mut world, _) = store.open(&folder).expect("open");
    world.bump_revision();
    store
        .save(
            &folder,
            &world,
            EditorState::default(),
            project_store::SaveKind::Manual,
        )
        .expect("save");
    store.begin_session(&folder, 2).expect("marker");
    store
        .autosave(&folder, &world, EditorState::default(), "")
        .expect("autosave at saved revision");
    let (_, opened, info) = store.open(&folder).expect("open");
    assert!(info.is_none());
    assert_eq!(opened.revision(), 2);
}
