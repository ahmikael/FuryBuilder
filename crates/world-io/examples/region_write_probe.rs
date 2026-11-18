//! Reproducible probe for the verified region writer (M8.7).
//!
//! Usage:
//!   cargo run -p furybuilder-world-io --example region_write_probe -- \
//!     <source-world> <destination-region-file> <dimension> <x> <y> <z> <block-state>
//!
//! The probe opens the source read-only, applies one block edit to the overlay, writes a
//! new region file, re-reads the written chunk with the strict decoder, and confirms that
//! the source region file is byte-identical before and after the run.

use std::path::Path;

use serde_json::json;
use sha2::{Digest, Sha256};
use world_domain::BlockState;
use world_io::{
    region_file_name, write_region_from_overlay, JavaWorld, RegionFile, WorldOverlay,
    ANVIL_REGION_AXIS_CHUNKS, VERIFIED_WRITE_SUPPORT,
};

fn main() {
    let mut args = std::env::args().skip(1);
    let source = args.next().unwrap_or_else(usage);
    let destination = args.next().unwrap_or_else(usage);
    let dimension = args.next().unwrap_or_else(usage);
    let x = parse_i32(args.next(), "x");
    let y = parse_i32(args.next(), "y");
    let z = parse_i32(args.next(), "z");
    let state = args.next().unwrap_or_else(usage);
    if let Err(error) = run(
        Path::new(&source),
        Path::new(&destination),
        &dimension,
        x,
        y,
        z,
        &state,
    ) {
        eprintln!("region write probe failed: {error}");
        std::process::exit(1);
    }
}

fn usage() -> String {
    eprintln!(
        "usage: region_write_probe <source-world> <destination-region-file> <dimension> <x> <y> <z> <block-state>"
    );
    std::process::exit(2);
}

fn parse_i32(value: Option<String>, name: &str) -> i32 {
    value.unwrap_or_else(usage).parse().unwrap_or_else(|_| {
        eprintln!("{name} must be an integer");
        std::process::exit(2);
    })
}

fn run(
    source: &Path,
    destination: &Path,
    dimension: &str,
    x: i32,
    y: i32,
    z: i32,
    state: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let state = state.parse::<BlockState>()?;
    let chunk_x = x.div_euclid(16);
    let chunk_z = z.div_euclid(16);
    let region_x = chunk_x.div_euclid(ANVIL_REGION_AXIS_CHUNKS);
    let region_z = chunk_z.div_euclid(ANVIL_REGION_AXIS_CHUNKS);
    if destination.file_name().and_then(|name| name.to_str())
        != Some(region_file_name(region_x, region_z).as_str())
    {
        return Err(format!(
            "destination must be named {}",
            region_file_name(region_x, region_z)
        )
        .into());
    }
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let source_region = JavaWorld::open(source)?
        .dimension(dimension)
        .map(|d| d.region_folder.join(region_file_name(region_x, region_z)));
    let source_region = source_region.ok_or("unknown dimension")?;
    let before = sha256_file(&source_region)?;

    let mut overlay = WorldOverlay::open(source)?;
    let change = overlay.set_block(dimension, x, y, z, &state)?;
    let report = write_region_from_overlay(&overlay, dimension, region_x, region_z, destination)?;

    let after = sha256_file(&source_region)?;
    if before != after {
        return Err("the source region file changed".into());
    }

    let world = JavaWorld::open(source)?;
    let written = RegionFile::open(destination)?;
    let chunk = written.read_chunk(chunk_x, chunk_z)?;
    let decoded = chunk.decode(
        world
            .dimension(dimension)
            .ok_or("unknown dimension after write")?,
    )?;
    let local_x = x.rem_euclid(16) as u8;
    let local_z = z.rem_euclid(16) as u8;
    let actual = decoded
        .block_at(local_x, y, local_z)
        .ok_or("written chunk has no target block")?
        .to_string();
    if actual != state.to_string() {
        return Err(format!("written block is {actual}, expected {state}").into());
    }

    let output = json!({
        "tool": { "name": "furybuilder-region-write-probe", "version": 1 },
        "ok": true,
        "support": VERIFIED_WRITE_SUPPORT,
        "source": source.display().to_string(),
        "sourceRegion": source_region.display().to_string(),
        "sourceRegionSha256Before": before,
        "sourceRegionSha256After": after,
        "sourceUnchanged": true,
        "destination": destination.display().to_string(),
        "dimension": dimension,
        "target": { "x": x, "y": y, "z": z, "chunkX": chunk_x, "chunkZ": chunk_z, "regionX": region_x, "regionZ": region_z },
        "previousBlock": change.previous.to_string(),
        "currentBlock": change.current.to_string(),
        "report": report,
        "writtenBlock": actual,
        "writtenBiome": decoded.biome_at(local_x, y, local_z),
        "writtenStatus": decoded.status,
        "writtenSections": decoded.sections.len(),
        "blockEntities": decoded.block_entities,
        "entities": decoded.entities
    });
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, std::io::Error> {
    let bytes = std::fs::read(path)?;
    Ok(format!("{:x}", Sha256::digest(&bytes)))
}
