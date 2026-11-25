//! Reproducible probe for complete Java world-folder export (M8.10).
//!
//! Usage:
//!   cargo run -p furybuilder-world-io --example world_export_probe -- \
//!     <source-world> <destination-world> [--rebuild-derived] \
//!     [<dimension> <x> <y> <z> <block-state> ...]
//!
//! The probe opens the source world read-only, applies optional edits to an in-memory
//! overlay, exports a new complete world folder and re-reads every edited block from the
//! exported save. The source world is never written. With no edits the probe performs a
//! byte-preserving folder copy.

use std::path::{Path, PathBuf};

use serde_json::json;
use world_domain::BlockState;
use world_io::{
    export_java_world_with_options, region_file_name, DerivedDataPolicy, JavaWorld,
    WorldExportOptions, WorldOverlay, ANVIL_REGION_AXIS_CHUNKS,
};

struct Edit {
    dimension: String,
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
    if args.len() < 2 {
        usage();
    }
    let source = PathBuf::from(&args[0]);
    let destination = PathBuf::from(&args[1]);
    let mut edits = Vec::new();
    let mut index = 2;
    while index < args.len() {
        let values = args.get(index..index + 5).unwrap_or_else(|| usage());
        let state = values[4].parse::<BlockState>().unwrap_or_else(|error| {
            eprintln!("invalid block state {}: {error}", values[4]);
            std::process::exit(2);
        });
        edits.push(Edit {
            dimension: values[0].clone(),
            x: parse_i32(Some(values[1].clone()), "x"),
            y: parse_i32(Some(values[2].clone()), "y"),
            z: parse_i32(Some(values[3].clone()), "z"),
            state,
        });
        index += 5;
    }
    if let Err(error) = run(&source, &destination, &edits, policy) {
        eprintln!("world export probe failed: {error}");
        std::process::exit(1);
    }
}

fn usage() -> ! {
    eprintln!(
        "usage: world_export_probe <source-world> <destination-world> [--rebuild-derived] [<dimension> <x> <y> <z> <block-state> ...]"
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
    edits: &[Edit],
    policy: DerivedDataPolicy,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut overlay = WorldOverlay::open(source)?;
    let mut changes = Vec::new();
    for edit in edits {
        let change = overlay.set_block(&edit.dimension, edit.x, edit.y, edit.z, &edit.state)?;
        changes.push(json!({
            "dimension": edit.dimension,
            "x": edit.x,
            "y": edit.y,
            "z": edit.z,
            "previous": change.previous.to_string(),
            "current": change.current.to_string(),
        }));
    }

    let options = WorldExportOptions {
        derived_data_policy: policy,
        ..WorldExportOptions::default()
    };
    let report = export_java_world_with_options(&overlay, destination, options)?;

    let exported = JavaWorld::open(destination)?;
    let mut verified = Vec::new();
    for edit in edits {
        let dimension = exported
            .dimension(&edit.dimension)
            .ok_or_else(|| format!("exported world has no dimension {}", edit.dimension))?;
        let region_x = edit.x.div_euclid(16).div_euclid(ANVIL_REGION_AXIS_CHUNKS);
        let region_z = edit.z.div_euclid(16).div_euclid(ANVIL_REGION_AXIS_CHUNKS);
        let region_path = dimension
            .region_folder
            .join(region_file_name(region_x, region_z));
        let region = world_io::RegionFile::open(&region_path)?;
        let chunk = region.read_chunk(edit.x.div_euclid(16), edit.z.div_euclid(16))?;
        let decoded = chunk.decode(dimension, &exported.profile)?;
        let actual = decoded
            .block_at(
                edit.x.rem_euclid(16) as u8,
                edit.y,
                edit.z.rem_euclid(16) as u8,
            )
            .ok_or_else(|| {
                format!(
                    "exported chunk has no block at ({}, {}, {})",
                    edit.x, edit.y, edit.z
                )
            })?
            .to_string();
        let expected = edit.state.to_string();
        if actual != expected {
            return Err(format!(
                "exported block at ({}, {}, {}) is {actual}, expected {expected}",
                edit.x, edit.y, edit.z
            )
            .into());
        }
        verified.push(json!({
            "dimension": edit.dimension,
            "x": edit.x,
            "y": edit.y,
            "z": edit.z,
            "state": actual,
            "biome": decoded.biome_at(edit.x.rem_euclid(16) as u8, edit.y, edit.z.rem_euclid(16) as u8),
        }));
    }

    let output = json!({
        "tool": { "name": "furybuilder-world-export-probe", "version": 1 },
        "ok": true,
        "source": source.display().to_string(),
        "destination": destination.display().to_string(),
        "derivedDataPolicy": policy,
        "changes": changes,
        "verified": verified,
        "report": report,
    });
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}
