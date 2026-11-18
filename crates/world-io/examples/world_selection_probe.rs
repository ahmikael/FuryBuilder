//! Reproducible probe for world selection classification (M8.11).
//!
//! Usage:
//!   cargo run -p furybuilder-world-io --example world_selection_probe -- \
//!     <path> [--passthrough <destination>]
//!
//! Prints the classifier report and the matching import analysis as JSON. With
//! `--passthrough`, it also attempts a byte-preserving copy and records either the copied
//! file/region counts or the structured refusal. The probe never writes to the selection.

use std::path::PathBuf;

use serde_json::json;
use world_io::{
    analyze_java_world, classify_world_selection, passthrough_java_world, WorldSelectionKind,
};

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let passthrough = if let Some(index) = args.iter().position(|arg| arg == "--passthrough") {
        if index + 1 >= args.len() {
            usage();
        }
        let destination = args.remove(index + 1);
        args.remove(index);
        Some(PathBuf::from(destination))
    } else {
        None
    };
    if args.len() != 1 {
        usage();
    }
    let path = PathBuf::from(&args[0]);
    let selection = classify_world_selection(&path);
    let analysis = analyze_java_world(&path);
    let passthrough_result =
        passthrough.map(
            |destination| match passthrough_java_world(&path, &destination) {
                Ok(report) => json!({
                    "ok": true,
                    "destination": destination.display().to_string(),
                    "files": report.files.len(),
                    "bytes": report.bytes(),
                    "regions": report.regions,
                    "chunks": report.chunks,
                }),
                Err(error) => json!({
                    "ok": false,
                    "destination": destination.display().to_string(),
                    "code": error.code(),
                    "error": error.to_string(),
                }),
            },
        );
    let output = json!({
        "tool": { "name": "furybuilder-world-selection-probe", "version": 1 },
        "selection": selection,
        "analysis": {
            "selectionKind": analysis.selection_kind,
            "completeSave": analysis.complete_save,
            "layout": analysis.layout,
            "supported": analysis.supported,
            "dataVersion": analysis.data_version,
            "versionName": analysis.version_name,
            "regions": analysis.regions,
            "chunks": analysis.chunks,
            "blockers": analysis.blockers,
        },
        "passthrough": passthrough_result,
        "isRegionFile": selection.kind == WorldSelectionKind::RegionFile,
        "isCompleteSave": selection.complete_save,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&output).expect("serialize probe output")
    );
}

fn usage() -> ! {
    eprintln!("usage: world_selection_probe <path> [--passthrough <destination>]");
    std::process::exit(2);
}
