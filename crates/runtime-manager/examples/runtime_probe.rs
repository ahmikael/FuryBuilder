//! Reproducible M9.3 probe. It only touches the paths you pass explicitly.
//!
//! Examples:
//!   cargo run -p furybuilder-runtime-manager --example runtime_probe -- --discover
//!   cargo run -p furybuilder-runtime-manager --example runtime_probe -- --java <java.exe>
//!   cargo run -p furybuilder-runtime-manager --example runtime_probe -- --temurin
//!   cargo run -p furybuilder-runtime-manager --example runtime_probe -- --java-major 17 --install
//!   cargo run -p furybuilder-runtime-manager --example runtime_probe -- --root <dir> --java <java.exe> --install
//!   cargo run -p furybuilder-runtime-manager --example runtime_probe -- --root <dir> --clear
//!
//! `--install` downloads official Eclipse Temurin only after you pass it, verifies
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
    let mut java_major = furybuilder_runtime_manager::target_java_major();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--root" => root = args.next().map(PathBuf::from),
            "--java" => java = args.next().map(PathBuf::from),
            "--java-major" => {
                java_major = args
                    .next()
                    .and_then(|value| value.parse().ok())
                    .expect("--java-major needs a number");
            }
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
    if let Err(error) = run(
        &manager,
        java.as_deref(),
        java_major,
        discover,
        temurin,
        install,
        clear,
    ) {
        eprintln!("runtime probe failed: {error}");
        std::process::exit(1);
    }
}

fn run(
    manager: &RuntimeManager,
    java: Option<&std::path::Path>,
    java_major: u32,
    discover: bool,
    temurin: bool,
    install: bool,
    clear: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(path) = java {
        let report = manager.select_for_major(path, java_major)?;
        println!("{}", serde_json::to_string_pretty(&report)?);
    }
    if discover || (!temurin && !install && !clear && java.is_none()) {
        let report = manager.discover();
        println!("{}", serde_json::to_string_pretty(&report)?);
    }
    if temurin || install {
        let release = manager.latest_temurin(java_major)?;
        println!("{}", serde_json::to_string_pretty(&release)?);
        if install {
            let cancel = std::sync::atomic::AtomicBool::new(false);
            let installed =
                manager.install_temurin(java_major, &release.version, &cancel, &|progress| {
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
