//! Read-only probe for a classic Minecraft Java world.
//!
//! Usage:
//!   cargo run -p furybuilder-world-io --example anvil_probe -- <world-folder>
//!   cargo run -p furybuilder-world-io --example anvil_probe -- <world-folder> --decode
//!   cargo run -p furybuilder-world-io --example anvil_probe -- <world-folder> --decode --all-chunks
//!
//! The probe never writes to the world. Every mode decodes every allocated chunk, so an
//! exit code of 0 means the whole region was read. `--decode` additionally turns each
//! chunk into block sections and biome palettes and reports aggregates; `--all-chunks`
//! prints every per-chunk summary instead of a bounded sample.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::{json, Value};
use world_domain::BlockState;
use world_io::anvil::{AnvilError, JavaWorld, RegionChunkSummary};

#[derive(Default)]
struct DecodeStats {
    chunks: usize,
    sections: usize,
    non_empty_sections: usize,
    non_air_blocks: usize,
    missing_sections: usize,
    distinct_blocks: BTreeSet<BlockState>,
    distinct_biomes: BTreeSet<String>,
}

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(root) = args.next() else {
        usage();
    };
    let mut all_chunks = false;
    let mut decode = false;
    for argument in args {
        match argument.as_str() {
            "--all-chunks" => all_chunks = true,
            "--decode" => decode = true,
            _ => usage(),
        }
    }
    if let Err(error) = probe(Path::new(&root), all_chunks, decode) {
        eprintln!("{}: {error}", error.code());
        std::process::exit(1);
    }
}

fn usage() -> ! {
    eprintln!("usage: anvil_probe <world-folder> [--decode] [--all-chunks]");
    std::process::exit(2);
}

fn probe(root: &Path, all_chunks: bool, decode: bool) -> Result<(), AnvilError> {
    let world = JavaWorld::open(root)?;
    let mut dimensions = Vec::new();
    let mut world_stats = DecodeStats::default();
    for dimension in &world.dimensions {
        let mut regions = Vec::new();
        let mut dimension_stats = DecodeStats::default();
        for (region_x, region_z) in world.region_coordinates(&dimension.id)? {
            let region = world.read_region(&dimension.id, region_x, region_z)?;
            let mut status_counts: BTreeMap<String, usize> = BTreeMap::new();
            let mut y_positions = BTreeSet::new();
            let mut sections_min = usize::MAX;
            let mut sections_max = 0usize;
            let mut block_entities = 0usize;
            let mut entities = 0usize;
            let mut chunks = Vec::new();
            let mut region_stats = DecodeStats::default();

            for entry in region.entries() {
                let chunk = region.read_chunk(entry.x, entry.z)?;
                let summary = &chunk.summary;
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
                chunks.push(chunk_json(summary));

                if decode {
                    let decoded = chunk.decode(dimension)?;
                    let non_empty = decoded
                        .sections
                        .values()
                        .filter(|section| !section.blocks.is_empty())
                        .count();
                    let non_air: usize = decoded
                        .sections
                        .values()
                        .map(|section| section.blocks.non_air_count())
                        .sum();
                    region_stats.chunks += 1;
                    region_stats.sections += decoded.sections.len();
                    region_stats.non_empty_sections += non_empty;
                    region_stats.non_air_blocks += non_air;
                    region_stats.missing_sections += decoded.missing_sections.len();
                    for section in decoded.sections.values() {
                        region_stats
                            .distinct_blocks
                            .extend(section.blocks.palette().iter().cloned());
                        region_stats
                            .distinct_biomes
                            .extend(section.biomes.palette().iter().cloned());
                    }
                }
            }
            if chunks.is_empty() {
                sections_min = 0;
            }

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
            if decode {
                region_value["decode"] = decode_json(&region_stats);
                dimension_stats.chunks += region_stats.chunks;
                dimension_stats.sections += region_stats.sections;
                dimension_stats.non_empty_sections += region_stats.non_empty_sections;
                dimension_stats.non_air_blocks += region_stats.non_air_blocks;
                dimension_stats.missing_sections += region_stats.missing_sections;
                dimension_stats
                    .distinct_blocks
                    .extend(region_stats.distinct_blocks.iter().cloned());
                dimension_stats
                    .distinct_biomes
                    .extend(region_stats.distinct_biomes.iter().cloned());
            }
            if all_chunks {
                region_value["chunks"] = json!(chunks);
            } else {
                region_value["sampleChunks"] = json!(chunks.iter().take(3).collect::<Vec<_>>());
            }
            regions.push(region_value);
        }

        let mut dimension_value = json!({
            "id": dimension.id,
            "kind": dimension.kind,
            "minY": dimension.min_y,
            "height": dimension.height,
            "folder": dimension.folder.display().to_string(),
            "regionFiles": dimension.region_files,
            "regions": regions,
        });
        if decode {
            dimension_value["decode"] = decode_json(&dimension_stats);
            world_stats.chunks += dimension_stats.chunks;
            world_stats.sections += dimension_stats.sections;
            world_stats.non_empty_sections += dimension_stats.non_empty_sections;
            world_stats.non_air_blocks += dimension_stats.non_air_blocks;
            world_stats.missing_sections += dimension_stats.missing_sections;
            world_stats
                .distinct_blocks
                .extend(dimension_stats.distinct_blocks);
            world_stats
                .distinct_biomes
                .extend(dimension_stats.distinct_biomes);
        }
        dimensions.push(dimension_value);
    }

    let mut output = json!({
        "tool": {
            "name": "furybuilder-anvil-probe",
            "version": 2,
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
        "declaredDimensions": world.declared_dimensions,
        "dimensions": dimensions,
    });
    if decode {
        output["decode"] = decode_json(&world_stats);
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&output)
            .map_err(|error| AnvilError::Io(format!("JSON serialization failed: {error}")))?
    );
    Ok(())
}

fn decode_json(stats: &DecodeStats) -> Value {
    json!({
        "chunks": stats.chunks,
        "sections": stats.sections,
        "nonEmptySections": stats.non_empty_sections,
        "nonAirBlocks": stats.non_air_blocks,
        "missingSections": stats.missing_sections,
        "distinctBlockStates": stats.distinct_blocks.len(),
        "distinctBiomes": stats.distinct_biomes.len(),
        "biomes": stats.distinct_biomes,
    })
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
