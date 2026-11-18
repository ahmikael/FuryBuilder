//! Read-only probe for a classic Minecraft Java world.
//!
//! Usage:
//!   cargo run -p furybuilder-world-io --example anvil_probe -- <world-folder>
//!   cargo run -p furybuilder-world-io --example anvil_probe -- <world-folder> --all-chunks
//!
//! The probe never writes to the world. By default it prints region-level aggregates
//! and a bounded sample of chunks; `--all-chunks` prints every summary. Every mode
//! decodes every allocated chunk, so an exit code of 0 means the whole region was read.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::{json, Value};
use world_io::anvil::{AnvilError, JavaWorld, RegionChunkSummary};

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(root) = args.next() else {
        usage();
    };
    let mut all_chunks = false;
    for argument in args {
        match argument.as_str() {
            "--all-chunks" => all_chunks = true,
            _ => usage(),
        }
    }
    if let Err(error) = probe(Path::new(&root), all_chunks) {
        eprintln!("{}: {error}", error.code());
        std::process::exit(1);
    }
}

fn usage() -> ! {
    eprintln!("usage: anvil_probe <world-folder> [--all-chunks]");
    std::process::exit(2);
}

fn probe(root: &Path, all_chunks: bool) -> Result<(), AnvilError> {
    let world = JavaWorld::open(root)?;
    let mut dimensions = Vec::new();
    for dimension in &world.dimensions {
        let mut regions = Vec::new();
        for (region_x, region_z) in world.region_coordinates(&dimension.id)? {
            let region = world.read_region(&dimension.id, region_x, region_z)?;
            let summaries = region.summarize_all()?;
            let mut status_counts: BTreeMap<String, usize> = BTreeMap::new();
            let mut y_positions = BTreeSet::new();
            let mut sections_min = usize::MAX;
            let mut sections_max = 0usize;
            let mut block_entities = 0usize;
            let mut entities = 0usize;
            for summary in &summaries {
                *status_counts
                    .entry(summary.status.clone().unwrap_or_else(|| "none".to_owned()))
                    .or_default() += 1;
                if let Some(y) = summary.y_pos {
                    y_positions.insert(y);
                }
                sections_min = sections_min.min(summary.sections);
                sections_max = sections_max.max(summary.sections);
                block_entities += summary.block_entities;
                entities += summary.entities;
            }
            if summaries.is_empty() {
                sections_min = 0;
            }
            let chunks = summaries.iter().map(chunk_json).collect::<Vec<Value>>();
            let mut region_value = json!({
                "regionX": region_x,
                "regionZ": region_z,
                "fileBytes": region.file_len(),
                "allocatedChunks": region.entries().len(),
                "statusCounts": status_counts,
                "sections": {
                    "min": sections_min,
                    "max": sections_max,
                },
                "yPositions": y_positions,
                "blockEntities": block_entities,
                "entities": entities,
            });
            if all_chunks {
                region_value["chunks"] = json!(chunks);
            } else {
                region_value["sampleChunks"] = json!(chunks.iter().take(3).collect::<Vec<_>>());
            }
            regions.push(region_value);
        }
        dimensions.push(json!({
            "id": dimension.id,
            "folder": dimension.folder.display().to_string(),
            "regionFiles": dimension.region_files,
            "regions": regions,
        }));
    }

    let output: Value = json!({
        "tool": {
            "name": "furybuilder-anvil-probe",
            "version": 1,
        },
        "worldFolder": world.root.display().to_string(),
        "layout": "classic_anvil",
        "readOnly": true,
        "level": {
            "dataVersion": world.level.data_version,
            "versionId": world.level.version_id,
            "versionName": world.level.version_name,
            "levelName": world.level.level_name,
        },
        "dimensions": dimensions,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&output)
            .map_err(|error| AnvilError::Io(format!("JSON serialization failed: {error}")))?
    );
    Ok(())
}

fn chunk_json(summary: &RegionChunkSummary) -> Value {
    json!({
        "x": summary.x,
        "z": summary.z,
        "dataVersion": summary.data_version,
        "status": summary.status,
        "yPos": summary.y_pos,
        "sections": summary.sections,
        "blockEntities": summary.block_entities,
        "entities": summary.entities,
    })
}
