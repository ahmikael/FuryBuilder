//! Reproducible M9.4 probe. It only touches the paths you pass explicitly.
//!
//! Examples:
//!   cargo run -p furybuilder-runtime-manager --example server_probe -- --discover
//!   cargo run -p furybuilder-runtime-manager --example server_probe -- --release
//!   cargo run -p furybuilder-runtime-manager --example server_probe -- --root <dir> --install
//!   cargo run -p furybuilder-runtime-manager --example server_probe -- --root <dir> --jar <server.jar>
//!   cargo run -p furybuilder-runtime-manager --example server_probe -- --root <dir> --clear
//!
//! `--install` downloads the exact official Mojang 1.21.4 server JAR only after you pass it,
//! verifies the announced SHA-1/size and the internal `version.json`, and stores it in the
//! managed cache. The JAR is never executed and no EULA is accepted.

use std::path::PathBuf;

use furybuilder_runtime_manager::{ServerManager, ServerTarget};

fn main() {
    let mut args = std::env::args().skip(1);
    let mut root: Option<PathBuf> = None;
    let mut jar: Option<PathBuf> = None;
    let mut discover = false;
    let mut release = false;
    let mut install = false;
    let mut clear = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--root" => root = args.next().map(PathBuf::from),
            "--jar" => jar = args.next().map(PathBuf::from),
            "--discover" => discover = true,
            "--release" => release = true,
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
            .join("furybuilder-server-probe")
            .join("servers")
    });
    let settings = root
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("server.json");
    let manager = ServerManager::new(
        &settings,
        &root,
        root.parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("version-manifest.json"),
    );
    if let Err(error) = run(&manager, jar.as_deref(), discover, release, install, clear) {
        eprintln!("server probe failed: {error}");
        std::process::exit(1);
    }
}

fn run(
    manager: &ServerManager,
    jar: Option<&std::path::Path>,
    discover: bool,
    release: bool,
    install: bool,
    clear: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(path) = jar {
        let report = manager.select_manual(path, &ServerTarget::baseline())?;
        println!("{}", serde_json::to_string_pretty(&report)?);
    }
    if release || install {
        let target = ServerTarget::baseline();
        let info = manager.release(&target)?;
        println!("{}", serde_json::to_string_pretty(&info)?);
        if install {
            let cancel = std::sync::atomic::AtomicBool::new(false);
            let installed = manager.install(&target, &cancel, &|progress| {
                eprintln!(
                    "progress {} {}/{}",
                    progress.phase, progress.downloaded, progress.total
                );
            })?;
            println!("{}", serde_json::to_string_pretty(&installed)?);
        }
    }
    if discover || (!jar.is_some() && !release && !install && !clear) {
        let report = manager.discover();
        println!("{}", serde_json::to_string_pretty(&report)?);
    }
    if clear {
        let report = manager.clear_managed()?;
        println!("{}", serde_json::to_string_pretty(&report)?);
    }
    Ok(())
}
