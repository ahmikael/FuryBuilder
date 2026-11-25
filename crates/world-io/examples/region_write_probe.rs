//! Reproducible probe for the verified region writer (M8.7/M8.9).
//!
//! Usage:
//!   cargo run -p furybuilder-world-io --example region_write_probe -- \
//!     <source-world> <destination-region-file> <dimension> <x> <y> <z> <block-state> \
//!     [<x> <y> <z> <block-state> ...] [--rebuild-derived]
//!
//! The probe opens the source read-only, applies one or more block edits to the overlay,
//! writes a new region file, re-reads every written block with the strict decoder, and
//! confirms that the source region file is byte-identical before and after the run.
//! All edits must belong to the same region file. `--rebuild-derived` selects the
//! `DerivedDataPolicy::VanillaRebuild` hand-off instead of raw preservation.

use std::path::Path;

use serde_json::json;
use sha2::{Digest, Sha256};
use world_domain::BlockState;
use world_io::{
    region_file_name, verified_write_support, write_region_from_overlay_with_policy,
    DerivedDataPolicy, JavaWorld, RegionFile, WorldOverlay, ANVIL_REGION_AXIS_CHUNKS,
};

struct Edit {
    x: i32,
    y: i32,
    z: i32,
    state: BlockState,
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let policy = if args.iter().any(|arg| arg == "--rebuild-derived") {
        args.retain(|arg| arg != "--rebuild-derived");
        DerivedDataPolicy::VanillaRebuild
    } else {
        DerivedDataPolicy::Preserve
    };
    if args.len() < 7 {
        usage();
    }
    let source = args[0].clone();
    let destination = args[1].clone();
    let dimension = args[2].clone();
    let mut edits = Vec::new();
    let mut index = 3;
    while index < args.len() {
        let coords = args.get(index..index + 3).unwrap_or_else(|| usage());
        let Some(state) = args.get(index + 3) else {
            usage();
        };
        let state = state.parse::<BlockState>().unwrap_or_else(|error| {
            eprintln!("invalid block state {state}: {error}");
            std::process::exit(2);
        });
        edits.push(Edit {
            x: parse_i32(Some(coords[0].clone()), "x"),
            y: parse_i32(Some(coords[1].clone()), "y"),
            z: parse_i32(Some(coords[2].clone()), "z"),
            state,
        });
        index += 4;
    }
    if edits.is_empty() {
        usage();
    }
    if let Err(error) = run(
        Path::new(&source),
        Path::new(&destination),
        &dimension,
        &edits,
        policy,
    ) {
        eprintln!("region write probe failed: {error}");
        std::process::exit(1);
    }
}

fn usage() -> ! {
    eprintln!(
        "usage: region_write_probe <source-world> <destination-region-file> <dimension> <x> <y> <z> <block-state> [<x> <y> <z> <block-state> ...] [--rebuild-derived]"
    );
    std::process::exit(2);
}

fn parse_i32(value: Option<String>, name: &str) -> i32 {
    value
        .unwrap_or_else(|| usage())
        .parse()
        .unwrap_or_else(|_| {
            eprintln!("{name} must be an integer");
            std::process::exit(2);
        })
}

fn run(
    source: &Path,
    destination: &Path,
    dimension: &str,
    edits: &[Edit],
    policy: DerivedDataPolicy,
) -> Result<(), Box<dyn std::error::Error>> {
    let first = edits.first().ok_or("no edits")?;
    let chunk_x = first.x.div_euclid(16);
    let chunk_z = first.z.div_euclid(16);
    let region_x = chunk_x.div_euclid(ANVIL_REGION_AXIS_CHUNKS);
    let region_z = chunk_z.div_euclid(ANVIL_REGION_AXIS_CHUNKS);
    for edit in edits {
        if edit.x.div_euclid(16).div_euclid(ANVIL_REGION_AXIS_CHUNKS) != region_x
            || edit.z.div_euclid(16).div_euclid(ANVIL_REGION_AXIS_CHUNKS) != region_z
        {
            return Err(format!(
                "edit ({}, {}, {}) does not belong to region ({region_x}, {region_z})",
                edit.x, edit.y, edit.z
            )
            .into());
        }
    }
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

    let world = JavaWorld::open(source)?;
    let source_region = world
        .dimension(dimension)
        .map(|dimension| {
            dimension
                .region_folder
                .join(region_file_name(region_x, region_z))
        })
        .ok_or("unknown dimension")?;
    let before = sha256_file(&source_region)?;

    let mut overlay = WorldOverlay::open(source)?;
    let mut changes = Vec::new();
    for edit in edits {
        let change = overlay.set_block(dimension, edit.x, edit.y, edit.z, &edit.state)?;
        changes.push(json!({
            "x": edit.x,
            "y": edit.y,
            "z": edit.z,
            "previous": change.previous.to_string(),
            "current": change.current.to_string(),
        }));
    }
    let report = write_region_from_overlay_with_policy(
        &overlay,
        dimension,
        region_x,
        region_z,
        destination,
        policy,
    )?;

    let after = sha256_file(&source_region)?;
    if before != after {
        return Err("the source region file changed".into());
    }

    let written = RegionFile::open(destination)?;
    let mut verified = Vec::new();
    for edit in edits {
        let chunk = written.read_chunk(edit.x.div_euclid(16), edit.z.div_euclid(16))?;
        let decoded = world.decode_chunk(dimension, &chunk)?;
        let local_x = edit.x.rem_euclid(16) as u8;
        let local_z = edit.z.rem_euclid(16) as u8;
        let actual = decoded
            .block_at(local_x, edit.y, local_z)
            .ok_or_else(|| {
                format!(
                    "written chunk has no block at ({}, {}, {})",
                    edit.x, edit.y, edit.z
                )
            })?
            .to_string();
        let expected = edit.state.to_string();
        if actual != expected {
            return Err(format!(
                "written block at ({}, {}, {}) is {actual}, expected {expected}",
                edit.x, edit.y, edit.z
            )
            .into());
        }
        verified.push(json!({
            "x": edit.x,
            "y": edit.y,
            "z": edit.z,
            "state": actual,
            "biome": decoded.biome_at(local_x, edit.y, local_z),
        }));
    }

    let output = json!({
        "tool": { "name": "furybuilder-region-write-probe", "version": 1 },
        "ok": true,
        "support": verified_write_support(),
        "source": source.display().to_string(),
        "sourceRegion": source_region.display().to_string(),
        "sourceRegionSha256Before": before,
        "sourceRegionSha256After": after,
        "sourceUnchanged": true,
        "destination": destination.display().to_string(),
        "dimension": dimension,
        "derivedDataPolicy": policy,
        "target": { "x": first.x, "y": first.y, "z": first.z, "chunkX": chunk_x, "chunkZ": chunk_z, "regionX": region_x, "regionZ": region_z },
        "previousBlock": changes.first().map(|change| change["previous"].clone()),
        "currentBlock": changes.first().map(|change| change["current"].clone()),
        "changes": changes,
        "verified": verified,
        "report": report,
    });
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, std::io::Error> {
    let bytes = std::fs::read(path)?;
    Ok(format!("{:x}", Sha256::digest(&bytes)))
}
