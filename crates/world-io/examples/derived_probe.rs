//! Read-only probe for Java 1.21.4 chunk-derived data (M8.8).
//!
//! Usage:
//!   cargo run -p furybuilder-world-io --example derived_probe -- <world> --world
//!   cargo run -p furybuilder-world-io --example derived_probe -- <world> --chunk <cx> <cz> [--dimension <id>]
//!   cargo run -p furybuilder-world-io --example derived_probe -- <world> --blocks <x> <y> <z> [<x> <y> <z> ...] [--dimension <id>]
//!
//! The probe never writes to the world. `--world` reports the aggregated inventory,
//! `--chunk` reports one chunk's derived fields and fingerprints, and `--blocks`
//! decodes real block states after a server round-trip.

use std::path::{Path, PathBuf};

use serde_json::json;
use world_io::{
    analyze_java_world_derived, inspect_chunk_derived, region_file_name, JavaWorld, RegionFile,
    ANVIL_REGION_AXIS_CHUNKS,
};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(error) = run(&args) {
        eprintln!("derived probe failed: {error}");
        std::process::exit(1);
    }
}

fn run(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let world = args.first().ok_or_else(usage)?;
    let mut dimension_id = "minecraft:overworld".to_owned();
    let mut mode = "world";
    let mut chunk = None;
    let mut blocks = Vec::new();
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--world" => {
                mode = "world";
                index += 1;
            }
            "--dimension" => {
                dimension_id = args.get(index + 1).ok_or_else(usage)?.clone();
                index += 2;
            }
            "--chunk" => {
                let x = parse_i32(args.get(index + 1), "chunk x")?;
                let z = parse_i32(args.get(index + 2), "chunk z")?;
                chunk = Some((x, z));
                mode = "chunk";
                index += 3;
            }
            "--blocks" => {
                mode = "blocks";
                index += 1;
                while index < args.len() && !args[index].starts_with("--") {
                    let values = args.get(index..index + 3).ok_or_else(usage)?;
                    blocks.push((
                        parse_i32(Some(&values[0]), "block x")?,
                        parse_i32(Some(&values[1]), "block y")?,
                        parse_i32(Some(&values[2]), "block z")?,
                    ));
                    index += 3;
                }
            }
            other => return Err(format!("unknown argument: {other}").into()),
        }
    }

    let world_path = Path::new(world);
    let java_world = JavaWorld::open(world_path)?;
    let dimension = java_world
        .dimension(&dimension_id)
        .ok_or_else(|| format!("unknown dimension: {dimension_id}"))?;

    let output = match mode {
        "world" => {
            let report = analyze_java_world_derived(&java_world)?;
            json!({
                "tool": { "name": "furybuilder-derived-probe", "version": 1 },
                "world": world_path.display().to_string(),
                "mode": "world",
                "report": report
            })
        }
        "chunk" => {
            let (x, z) = chunk.ok_or_else(usage)?;
            let (region, nbt) = read_chunk_nbt(&java_world, &dimension_id, x, z)?;
            let data = inspect_chunk_derived(&nbt, x, z)?;
            json!({
                "tool": { "name": "furybuilder-derived-probe", "version": 1 },
                "world": world_path.display().to_string(),
                "dimension": dimension_id,
                "region": region.display().to_string(),
                "mode": "chunk",
                "chunk": data
            })
        }
        "blocks" => {
            let mut results = Vec::new();
            for (x, y, z) in blocks {
                let chunk_x = x.div_euclid(16);
                let chunk_z = z.div_euclid(16);
                let region_x = chunk_x.div_euclid(ANVIL_REGION_AXIS_CHUNKS);
                let region_z = chunk_z.div_euclid(ANVIL_REGION_AXIS_CHUNKS);
                let region_path = dimension
                    .region_folder
                    .join(region_file_name(region_x, region_z));
                let region_file = RegionFile::open(&region_path)?;
                let chunk_data = region_file.read_chunk(chunk_x, chunk_z)?;
                let decoded = chunk_data.decode(dimension)?;
                let state = decoded
                    .block_at(x.rem_euclid(16) as u8, y, z.rem_euclid(16) as u8)
                    .map(ToString::to_string);
                results.push(json!({
                    "x": x,
                    "y": y,
                    "z": z,
                    "state": state,
                    "nbtBytes": chunk_data.nbt_bytes,
                }));
            }
            json!({
                "tool": { "name": "furybuilder-derived-probe", "version": 1 },
                "world": world_path.display().to_string(),
                "dimension": dimension_id,
                "mode": "blocks",
                "blocks": results
            })
        }
        _ => unreachable!(),
    };
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn read_chunk_nbt(
    world: &JavaWorld,
    dimension_id: &str,
    x: i32,
    z: i32,
) -> Result<(PathBuf, world_io::NbtFile), Box<dyn std::error::Error>> {
    let region_x = x.div_euclid(ANVIL_REGION_AXIS_CHUNKS);
    let region_z = z.div_euclid(ANVIL_REGION_AXIS_CHUNKS);
    let dimension = world
        .dimension(dimension_id)
        .ok_or_else(|| format!("unknown dimension: {dimension_id}"))?;
    let path = dimension
        .region_folder
        .join(region_file_name(region_x, region_z));
    let region = RegionFile::open(&path)?;
    let nbt = region.read_nbt(x, z)?;
    Ok((path, nbt))
}

fn parse_i32(value: Option<&String>, name: &str) -> Result<i32, Box<dyn std::error::Error>> {
    value
        .ok_or_else(usage)?
        .parse()
        .map_err(|_| format!("{name} must be an integer").into())
}

fn usage() -> Box<dyn std::error::Error> {
    "usage: derived_probe <world> --world | --chunk <cx> <cz> | --blocks <x> <y> <z> ... [--dimension <id>]".into()
}
