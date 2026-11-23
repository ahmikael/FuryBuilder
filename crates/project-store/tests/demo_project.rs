//! Validates the committed demo project without the desktop app.
//!
//! The demo is a real FuryBuilder project folder, so it must open through the normal
//! versioned store, verify its checksum and contain the documented self-authored scene.
//! The test also enforces the legal boundary: no Minecraft JAR, region, level.dat or
//! texture is allowed inside the demo folder.

use std::path::{Path, PathBuf};

use project_store::{CreationMode, ProjectStore};
use world_domain::BlockPos;

fn examples_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
}

fn demo_folder() -> &'static str {
    "furybuilder-demo.furybuilder"
}

fn block_name(world: &world_domain::World, x: i32, y: i32, z: i32) -> String {
    world
        .get_block("minecraft:overworld", BlockPos::new(x, y, z))
        .expect("block inside the demo world")
        .name()
        .to_owned()
}

#[test]
fn bundled_demo_project_opens_through_the_versioned_store() {
    let store = ProjectStore::new(examples_root());
    let (detail, world, recovery) = store
        .open(demo_folder())
        .expect("the committed demo project must open");

    assert_eq!(detail.summary.folder, demo_folder());
    assert_eq!(detail.summary.name, "FuryBuilder Demo");
    assert_eq!(detail.summary.creation_mode, CreationMode::Empty);
    assert_eq!(detail.summary.target_minecraft_version, "1.21.4");
    assert_eq!(detail.summary.target_data_version, 4189);
    assert_eq!(detail.summary.world_revision, 1);
    assert_eq!(detail.summary.schema_version, 2);
    assert!(
        recovery.is_none(),
        "a clean demo project must not offer crash recovery"
    );

    assert_eq!(world.revision(), 1);
    assert_eq!(world.chunk_count(), 9);
    assert_eq!(world.section_count(), 81);
    assert_eq!(world.block_count(), 297_719);
}

#[test]
fn bundled_demo_project_contains_the_documented_scene() {
    let store = ProjectStore::new(examples_root());
    let (_, world, _) = store.open(demo_folder()).expect("demo project");

    assert_eq!(block_name(&world, 0, 64, 0), "minecraft:grass_block");
    assert_eq!(block_name(&world, 2, 66, -8), "minecraft:oak_planks");
    assert_eq!(block_name(&world, 6, 66, -8), "minecraft:air");
    assert_eq!(block_name(&world, 4, 67, -8), "minecraft:glass");
    assert_eq!(block_name(&world, 12, 64, 7), "minecraft:water");
    assert_eq!(block_name(&world, -6, 65, -6), "minecraft:oak_log");
    assert_eq!(block_name(&world, 1, 70, -9), "minecraft:dark_oak_planks");
    assert_eq!(block_name(&world, 4, 70, -6), "minecraft:sea_lantern");
    let stairs = world
        .get_block("minecraft:overworld", BlockPos::new(1, 71, -9))
        .expect("stair border");
    assert_eq!(stairs.name(), "minecraft:oak_stairs");
}

#[test]
fn bundled_demo_project_contains_no_minecraft_game_files() {
    let mut pending = vec![examples_root().join(demo_folder())];
    let mut files = Vec::new();
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).expect("demo directory") {
            let path = entry.expect("directory entry").path();
            if path.is_dir() {
                pending.push(path);
            } else {
                files.push(path);
            }
        }
    }
    assert!(!files.is_empty(), "the demo project must not be empty");

    for path in files {
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .expect("UTF-8 file name");
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        assert!(
            !matches!(
                extension.as_str(),
                "jar" | "mca" | "mcr" | "dat" | "png" | "jpg" | "jpeg" | "ogg" | "zip"
            ),
            "Minecraft or binary asset file is not allowed in the demo: {name}"
        );
        assert!(
            name != "level.dat" && name != "client.jar" && name != "server.jar",
            "Minecraft game file is not allowed in the demo: {name}"
        );
    }
}
