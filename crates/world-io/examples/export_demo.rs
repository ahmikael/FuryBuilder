//! Builds a tiny original structure in a FuryBuilder world and exports it through the
//! verified Java `.nbt` path. This is the core half of the M6 demo.
//!
//! Run with:
//! `cargo run -p furybuilder-world-io --example export_demo -- <output.nbt>`

use std::fs;
use std::path::PathBuf;

use world_domain::{BlockBox, BlockPos, BlockState, World};
use world_io::prepare_export;

fn block(state: &str) -> BlockState {
    state.parse().expect("demo block state is valid")
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "target/furybuilder-demo.nbt".to_owned()),
    );
    let mut world = World::new_empty();
    let bricks = block("minecraft:stone_bricks");
    let planks = block("minecraft:oak_planks");
    let stairs =
        block("minecraft:oak_stairs[facing=south,half=top,shape=straight,waterlogged=false]");

    for x in 0..5 {
        for z in 0..5 {
            world.set_block("minecraft:overworld", BlockPos::new(x, 64, z), &bricks)?;
        }
    }
    for y in 65..=67 {
        for (x, z) in [(0, 0), (4, 0), (0, 4), (4, 4)] {
            world.set_block("minecraft:overworld", BlockPos::new(x, y, z), &bricks)?;
        }
    }
    for x in 1..=3 {
        for y in 65..=66 {
            if !(x == 2 && y == 65) {
                world.set_block("minecraft:overworld", BlockPos::new(x, y, 0), &planks)?;
            }
        }
    }
    world.set_block("minecraft:overworld", BlockPos::new(2, 65, 0), &stairs)?;

    let bounds = BlockBox::new(BlockPos::new(0, 64, 0), BlockPos::new(4, 67, 4))?;
    let bundle = prepare_export(
        &world,
        "minecraft:overworld",
        Some(bounds),
        "FuryBuilder demo",
    )?;
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&output, &bundle.bytes)?;
    println!("wrote {} bytes to {}", bundle.bytes.len(), output.display());
    println!(
        "size={:?} volume={} nonAir={} palette={}",
        bundle.inspection.size,
        bundle.inspection.volume,
        bundle.inspection.non_air_blocks,
        bundle.inspection.palette_size
    );
    println!("verified by read-back and deterministic re-encoding");
    Ok(())
}
