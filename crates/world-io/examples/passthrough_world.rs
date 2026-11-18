//! CLI probe for the byte-preserving Java world copy.
//!
//! Usage:
//!   cargo run -p furybuilder-world-io --example passthrough_world -- <source> <destination>
//!   cargo run -p furybuilder-world-io --example passthrough_world -- <source> <destination> --all-files
//!
//! The source is opened read-only and validated before the copy. The destination must not
//! exist. `--all-files` adds each relative path, size and SHA-256 to the JSON report.

use std::path::Path;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use world_io::passthrough::{
    passthrough_java_world_with_options, PassthroughError, PassthroughOptions, PassthroughReport,
};

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(source) = args.next() else {
        usage();
    };
    let Some(destination) = args.next() else {
        usage();
    };
    let mut all_files = false;
    for argument in args {
        match argument.as_str() {
            "--all-files" => all_files = true,
            _ => usage(),
        }
    }
    if let Err(error) = run(Path::new(&source), Path::new(&destination), all_files) {
        eprintln!("{}: {error}", error.code());
        std::process::exit(1);
    }
}

fn usage() -> ! {
    eprintln!("usage: passthrough_world <source> <destination> [--all-files]");
    std::process::exit(2);
}

fn run(source: &Path, destination: &Path, all_files: bool) -> Result<(), PassthroughError> {
    let report =
        passthrough_java_world_with_options(source, destination, PassthroughOptions::default())?;
    let output = report_json(&report, all_files);
    println!(
        "{}",
        serde_json::to_string_pretty(&output)
            .map_err(|error| PassthroughError::Io(format!("JSON serialization failed: {error}")))?
    );
    Ok(())
}

fn report_json(report: &PassthroughReport, all_files: bool) -> Value {
    let skipped = report
        .skipped
        .iter()
        .map(|entry| {
            json!({
                "path": entry.relative_path.display().to_string(),
                "reason": entry.reason,
            })
        })
        .collect::<Vec<_>>();
    let mut manifest = Sha256::new();
    for file in &report.files {
        manifest.update(file.relative_path.to_string_lossy().as_bytes());
        manifest.update([0u8]);
        manifest.update(file.sha256.as_bytes());
        manifest.update(b"\n");
    }
    let manifest_sha256 = manifest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();

    let mut output = json!({
        "tool": {
            "name": "furybuilder-passthrough-probe",
            "version": 1,
        },
        "source": report.source.display().to_string(),
        "destination": report.destination.display().to_string(),
        "files": report.files.len(),
        "bytes": report.bytes(),
        "regions": report.regions,
        "chunks": report.chunks,
        "chunkValidation": report.chunk_validation,
        "manifestSha256": manifest_sha256,
        "skipped": skipped,
    });
    if all_files {
        output["fileList"] = json!(report
            .files
            .iter()
            .map(|file| json!({
                "path": file.relative_path.display().to_string(),
                "bytes": file.bytes,
                "sha256": file.sha256,
            }))
            .collect::<Vec<_>>());
    }
    output
}
