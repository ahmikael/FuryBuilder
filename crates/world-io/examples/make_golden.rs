//! One-off fixture generator for the committed Java structure golden.
//! Run with: cargo run -p furybuilder-world-io --example make_golden

use std::fs;

use world_domain::BlockState;
use world_io::{default_version_profile, write_java_structure, Structure, StructureBlock};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let stairs: BlockState =
        "minecraft:oak_stairs[facing=west,half=top,shape=straight,waterlogged=false]".parse()?;
    let bricks: BlockState = "minecraft:stone_bricks".parse()?;
    let structure = Structure::new(
        default_version_profile().data_version,
        [3, 2, 2],
        vec![BlockState::default(), stairs, bricks],
        vec![
            StructureBlock {
                pos: [0, 0, 0],
                palette_index: 1,
            },
            StructureBlock {
                pos: [2, 1, 1],
                palette_index: 2,
            },
        ],
    )?;
    let bytes = write_java_structure(&structure, "FuryBuilder golden fixture")?;
    fs::create_dir_all("crates/world-io/tests/goldens")?;
    fs::write(
        "crates/world-io/tests/goldens/java-structure-1.21.4.nbt",
        &bytes,
    )?;
    println!("wrote {} bytes", bytes.len());
    Ok(())
}
