//! End-to-end probe for the `Import World` pipeline.
//!
//! Usage:
//!   cargo run -p furybuilder-desktop --example import_world_probe -- \
//!     <source-world> <empty-project-root> [project-name]
//!   cargo run -p furybuilder-desktop --example import_world_probe -- \
//!     <world-folder> --analysis-only
//!
//! The probe performs the same library steps as the Tauri command: read-only analysis,
//! bounded block decoding, project creation, byte-preserving source copy, source manifest
//! recording and a reopen check. It never writes to the source world.

use std::path::Path;

use project_store::{EditorState, ProjectStore, SourceFile};
use serde_json::json;
use world_io::{analyze_java_world, load_java_world, passthrough_java_world};

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(source) = args.next() else {
        eprintln!("usage: import_world_probe <source-world> <empty-project-root> [project-name]");
        eprintln!("       import_world_probe <world-folder> --analysis-only");
        std::process::exit(2);
    };
    let second = args.next();
    if second.as_deref() == Some("--analysis-only") {
        let analysis = analyze_java_world(&source);
        println!(
            "{}",
            serde_json::to_string_pretty(&analysis).expect("serialize analysis")
        );
        return;
    }
    let Some(project_root) = second else {
        eprintln!("usage: import_world_probe <source-world> <empty-project-root> [project-name]");
        eprintln!("       import_world_probe <world-folder> --analysis-only");
        std::process::exit(2);
    };
    let name = args.next().unwrap_or_else(|| "Imported World".to_owned());
    if let Err(error) = run(Path::new(&source), Path::new(&project_root), &name) {
        eprintln!("import failed: {error}");
        std::process::exit(1);
    }
}

fn run(source: &Path, project_root: &Path, name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let analysis = analyze_java_world(source);
    if !analysis.supported {
        return Err(format!("analysis blockers: {}", analysis.blockers.join("; ")).into());
    }
    let (mut world, load) = load_java_world(source)?;
    world.bump_revision();

    let store = ProjectStore::new(project_root);
    let detail = store.create_imported(
        name,
        EditorState::default(),
        &world,
        "world",
        project_store::ImportOptions::default(),
    )?;
    let folder = detail.summary.folder.clone();
    let project_dir = project_root.join(&folder);
    let source_world = project_dir.join(project_store::SOURCE_DIR).join("world");
    let copy = passthrough_java_world(source, &source_world)?;
    let files = copy
        .files
        .iter()
        .map(|file| {
            let relative = file.relative_path.to_string_lossy().replace('\\', "/");
            SourceFile {
                relative_path: format!("world/{relative}"),
                bytes: file.bytes,
                sha256: file.sha256.clone(),
            }
        })
        .collect::<Vec<_>>();
    let detail = store.record_source_tree(&folder, "world", &files)?;
    let (_, reopened, _) = store.open(&folder)?;
    let source_manifest = store.source_manifest(&folder)?;

    let output = json!({
        "tool": { "name": "furybuilder-import-world-probe", "version": 1 },
        "source": source.display().to_string(),
        "projectFolder": folder,
        "analysis": analysis,
        "load": load,
        "copy": {
            "files": copy.files.len(),
            "bytes": copy.bytes(),
            "skipped": copy
                .skipped
                .iter()
                .map(|entry| json!({
                    "path": entry.relative_path.display().to_string(),
                    "reason": entry.reason,
                }))
                .collect::<Vec<_>>(),
            "regions": copy.regions,
            "chunks": copy.chunks,
            "chunkValidation": copy.chunk_validation,
        },
        "project": {
            "name": detail.summary.name,
            "creationMode": detail.summary.creation_mode,
            "revision": detail.summary.world_revision,
            "source": detail.source,
            "sourceFiles": source_manifest.files.len(),
            "editableChunks": reopened.chunk_count(),
            "editableBlocks": reopened.block_count(),
            "editableSections": reopened.section_count(),
        }
    });
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}
