//! Read-only probe for the block-edit overlay over an imported Java world.
//!
//! Usage:
//!   cargo run -p furybuilder-world-io --example overlay_probe -- \
//!     <world-folder> <dimension> <x> <y> <z> <block-state> [--document]
//!
//! The source world is opened read-only. The probe applies one block edit in memory,
//! materialises the patched chunk NBT, re-decodes it and prints a JSON report. It never
//! writes to the source folder. `--document` includes the full overlay JSON document.

use std::path::Path;

use serde_json::json;
use sha2::{Digest, Sha256};
use world_domain::BlockState;
use world_io::nbt;
use world_io::overlay::{OverlayError, WorldOverlay};

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(root) = args.next() else {
        usage();
    };
    let Some(dimension) = args.next() else {
        usage();
    };
    let Some(x) = args.next().and_then(|value| value.parse::<i32>().ok()) else {
        usage();
    };
    let Some(y) = args.next().and_then(|value| value.parse::<i32>().ok()) else {
        usage();
    };
    let Some(z) = args.next().and_then(|value| value.parse::<i32>().ok()) else {
        usage();
    };
    let Some(state) = args.next() else {
        usage();
    };
    let mut include_document = false;
    for argument in args {
        match argument.as_str() {
            "--document" => include_document = true,
            _ => usage(),
        }
    }
    let state = match state.parse::<BlockState>() {
        Ok(state) => state,
        Err(error) => {
            eprintln!("invalid_argument: invalid block state `{state}`: {error}");
            std::process::exit(2);
        }
    };
    if let Err(error) = run(
        Path::new(&root),
        &dimension,
        x,
        y,
        z,
        &state,
        include_document,
    ) {
        eprintln!("{}: {error}", error.code());
        std::process::exit(1);
    }
}

fn usage() -> ! {
    eprintln!(
        "usage: overlay_probe <world-folder> <dimension> <x> <y> <z> <block-state> [--document]"
    );
    std::process::exit(2);
}

fn run(
    root: &Path,
    dimension: &str,
    x: i32,
    y: i32,
    z: i32,
    state: &BlockState,
    include_document: bool,
) -> Result<(), OverlayError> {
    let mut overlay = WorldOverlay::open(root)?;
    let previous = overlay.get_block(dimension, x, y, z)?;
    let change = overlay.set_block(dimension, x, y, z, state)?;
    let merged = overlay.read_chunk(dimension, x.div_euclid(16), z.div_euclid(16))?;
    let patched = overlay.materialize_chunk_nbt(dimension, x.div_euclid(16), z.div_euclid(16))?;
    let document = overlay.to_json_pretty()?;
    let document_sha256 = Sha256::digest(document.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();

    let mut materialized = json!(null);
    if let Some(patched) = &patched {
        let decoded =
            overlay
                .world()
                .decode_chunk_nbt(dimension, merged.chunk.x, merged.chunk.z, patched)?;
        let round_trip = decoded.block_at(x.rem_euclid(16) as u8, y, z.rem_euclid(16) as u8);
        let raw = nbt::write_uncompressed(patched).map_err(|error| {
            OverlayError::VerificationFailed(format!("patched NBT is not encodable: {error}"))
        })?;
        materialized = json!({
            "present": true,
            "nbtBytes": raw.len(),
            "roundTripBlock": round_trip.map(ToString::to_string),
            "roundTripOk": round_trip == Some(state),
        });
    }

    let dirty_chunks = overlay
        .dirty_chunks()
        .map(|key| {
            json!({
                "dimension": key.dimension,
                "x": key.x,
                "z": key.z,
            })
        })
        .collect::<Vec<_>>();
    let dirty_sections = merged.dirty_sections.iter().copied().collect::<Vec<_>>();

    let mut document_summary = json!({
        "schemaVersion": world_io::overlay::OVERLAY_SCHEMA_VERSION,
        "dataVersion": overlay.world().level.data_version,
        "chunks": overlay.dirty_chunks().count(),
        "sections": overlay.dirty_section_count(),
        "jsonBytes": document.len(),
        "sha256": document_sha256,
    });
    if include_document {
        document_summary["json"] = json!(document);
    }

    let output = json!({
        "tool": {
            "name": "furybuilder-overlay-probe",
            "version": 1,
        },
        "worldFolder": overlay.world().root.display().to_string(),
        "readOnly": true,
        "dataVersion": overlay.world().level.data_version,
        "dimension": dimension,
        "position": { "x": x, "y": y, "z": z },
        "previous": previous.to_string(),
        "current": change.current.to_string(),
        "changed": change.changed,
        "changeCount": overlay.change_count(),
        "dirtyChunkCount": overlay.dirty_chunks().count(),
        "dirtySectionCount": overlay.dirty_section_count(),
        "dirtyChunks": dirty_chunks,
        "merged": {
            "chunkX": merged.chunk.x,
            "chunkZ": merged.chunk.z,
            "overridden": merged.overridden,
            "changes": merged.changes,
            "dirtySections": dirty_sections,
            "blockEntities": merged.chunk.block_entities,
            "entities": merged.chunk.entities,
            "biomeAtPosition": merged.chunk.biome_at(
                x.rem_euclid(16) as u8,
                y,
                z.rem_euclid(16) as u8,
            ),
        },
        "materialized": materialized,
        "document": document_summary,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&output)
            .map_err(|error| OverlayError::Io(format!("JSON serialization failed: {error}")))?
    );
    Ok(())
}
