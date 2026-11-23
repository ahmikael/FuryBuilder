//! Reproducible M9.3 probe. It only touches the paths you pass explicitly.
//!
//! Examples:
//!   cargo run -p furybuilder-runtime-manager --example runtime_probe -- --discover
//!   cargo run -p furybuilder-runtime-manager --example runtime_probe -- --java <java.exe>
//!   cargo run -p furybuilder-runtime-manager --example runtime_probe -- --temurin
//!   cargo run -p furybuilder-runtime-manager --example runtime_probe -- --root <dir> --java <java.exe> --install
//!   cargo run -p furybuilder-runtime-manager --example runtime_probe -- --root <dir> --clear
//!
//! `--install` downloads the official Eclipse Temurin JRE only after you pass it, verifies
//! the announced SHA-256/size, safely extracts it into `<root>` and probes the result.

use std::path::PathBuf;

use furybuilder_runtime_manager::RuntimeManager;

fn main() {
    let mut args = std::env::args().skip(1);
    let mut root: Option<PathBuf> = None;
    let mut java: Option<PathBuf> = None;
    let mut discover = false;
    let mut temurin = false;
    let mut install = false;
    let mut clear = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--root" => root = args.next().map(PathBuf::from),
            "--java" => java = args.next().map(PathBuf::from),
            "--discover" => discover = true,
            "--temurin" => temurin = true,
            "--install" => install = true,
            "--clear" => clear = true,
            other => {
                eprintln!("unknown argument: {other}");
                std::process::exit(2);
            }
        }
    }
    let root = root.unwrap_or_else(|| {
        std::env::temp_dir()
            .join("furybuilder-runtime-probe")
            .join("runtimes")
    });
    let settings = root
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("runtime.json");
    let manager = RuntimeManager::new(&settings, &root);
    if let Err(error) = run(&manager, java.as_deref(), discover, temurin, install, clear) {
        eprintln!("runtime probe failed: {error}");
        std::process::exit(1);
    }
}

fn run(
    manager: &RuntimeManager,
    java: Option<&std::path::Path>,
    discover: bool,
    temurin: bool,
    install: bool,
    clear: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(path) = java {
        let report = manager.select(path)?;
        println!("{}", serde_json::to_string_pretty(&report)?);
    }
    if discover || (!temurin && !install && !clear && java.is_none()) {
        let report = manager.discover();
        println!("{}", serde_json::to_string_pretty(&report)?);
    }
    if temurin || install {
        let release = manager.latest_temurin()?;
        println!("{}", serde_json::to_string_pretty(&release)?);
        if install {
            let cancel = std::sync::atomic::AtomicBool::new(false);
            let installed = manager.install_temurin(&release.version, &cancel, &|progress| {
                eprintln!(
                    "progress {} {}/{}",
                    progress.phase, progress.downloaded, progress.total
                );
            })?;
            println!("{}", serde_json::to_string_pretty(&installed)?);
        }
    }
    if clear {
        let report = manager.clear_managed()?;
        println!("{}", serde_json::to_string_pretty(&report)?);
    }
    Ok(())
}
