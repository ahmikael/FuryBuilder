//! Reproducible probe for M9.1 asset discovery.
//!
//! Usage:
//!   cargo run -p furybuilder-asset-source --example asset_source_probe -- discover
//!   cargo run -p furybuilder-asset-source --example asset_source_probe -- inspect <path>
//!
//! `inspect` accepts a launcher root, a `versions/<id>` folder, a client JAR, or a
//! resource pack `.zip`/folder. The probe only reads files and prints JSON.

use std::path::PathBuf;

use asset_source::{discover_assets, inspect_asset_path};
use serde_json::json;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let output = match args.as_slice() {
        [command] if command == "discover" => json!({
            "tool": { "name": "furybuilder-asset-source-probe", "version": 1 },
            "discovery": discover_assets()
        }),
        [command, path] if command == "inspect" => json!({
            "tool": { "name": "furybuilder-asset-source-probe", "version": 1 },
            "inspection": inspect_asset_path(PathBuf::from(path))
        }),
        _ => usage(),
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&output).expect("serialize probe output")
    );
}

fn usage() -> ! {
    eprintln!("usage: asset_source_probe discover");
    eprintln!("       asset_source_probe inspect <path>");
    std::process::exit(2);
}
