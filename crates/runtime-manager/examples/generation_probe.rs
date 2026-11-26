//! Reproducible M9.6 probe for the isolated Vanilla generation process.
//!
//! Examples:
//!   cargo run -p furybuilder-runtime-manager --example generation_probe -- \
//!     --root <dir> --version <release> --java <java.exe> --jar <server.jar> --accept-eula --consent
//!
//! `--accept-eula` records the official EULA acceptance; `--consent` confirms starting the
//! local Java server for this run. Neither happens implicitly. Optional:
//! `--datapack <pack.zip>` (repeatable) and `--no-structures`.
//!
//! The probe starts the official server, generates a small area, saves it, stops and prints
//! the final report. Use `--cancel-after <seconds>` to exercise cancellation.

use std::path::PathBuf;
use std::time::Duration;

use furybuilder_runtime_manager::{
    DatapackSelection, GenerationRequest, GenerationTarget, ServerManager, WorldPreset,
};

fn main() {
    if let Err(error) = run() {
        eprintln!("generation probe failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let mut root: Option<PathBuf> = None;
    let mut java: Option<PathBuf> = None;
    let mut jar: Option<PathBuf> = None;
    let mut version = None;
    let mut seed = "FuryBuilder".to_owned();
    let mut preset = WorldPreset::Normal;
    let mut center_x = 0i32;
    let mut center_z = 0i32;
    let mut radius_chunks = 1u32;
    let mut time_limit_seconds = 180u32;
    let mut memory_mb = 2048u32;
    let mut view_distance = 3u8;
    let mut cpu_threads = 2u8;
    let mut accept_eula = false;
    let mut consent = false;
    let mut generate_structures = true;
    let mut datapacks: Vec<DatapackSelection> = Vec::new();
    let mut cancel_after: Option<u64> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--root" => root = args.next().map(PathBuf::from),
            "--java" => java = args.next().map(PathBuf::from),
            "--jar" => jar = args.next().map(PathBuf::from),
            "--version" => version = Some(args.next().ok_or("--version needs a release id")?),
            "--seed" => seed = args.next().unwrap_or_default(),
            "--preset" => {
                preset = match args.next().as_deref() {
                    Some("flat") => WorldPreset::Flat,
                    Some("large_biomes") => WorldPreset::LargeBiomes,
                    Some("amplified") => WorldPreset::Amplified,
                    _ => WorldPreset::Normal,
                }
            }
            "--center-x" => center_x = args.next().unwrap_or_default().parse()?,
            "--center-z" => center_z = args.next().unwrap_or_default().parse()?,
            "--radius" => radius_chunks = args.next().unwrap_or_default().parse()?,
            "--time" => time_limit_seconds = args.next().unwrap_or_default().parse()?,
            "--memory" => memory_mb = args.next().unwrap_or_default().parse()?,
            "--view" => view_distance = args.next().unwrap_or_default().parse()?,
            "--cpu" => cpu_threads = args.next().unwrap_or_default().parse()?,
            "--cancel-after" => cancel_after = args.next().unwrap_or_default().parse().ok(),
            "--accept-eula" => accept_eula = true,
            "--consent" => consent = true,
            "--no-structures" => generate_structures = false,
            "--datapack" => {
                if let Some(path) = args.next() {
                    datapacks.push(DatapackSelection { path });
                }
            }
            other => {
                eprintln!("unknown argument: {other}");
                std::process::exit(2);
            }
        }
    }
    let root = root.ok_or("--root is required")?;
    let java = java.ok_or("--java is required")?;
    std::fs::create_dir_all(&root)?;
    let manager = ServerManager::new(
        root.join("server.json"),
        root.join("servers"),
        root.join("version-manifest.json"),
    );
    let profile = match version {
        Some(version) => minecraft_version::profile_for_version(&version.parse()?)
            .ok_or_else(|| format!("unknown built-in Minecraft version: {version}"))?,
        None => minecraft_version::default_profile(),
    };
    let generation_target = GenerationTarget::for_profile(&profile)?;
    let server_target = generation_target.server_target();
    if let Some(jar) = jar {
        let report = manager.select_manual(&jar, &server_target)?;
        eprintln!(
            "selected server JAR: {} {} DV {:?}",
            report.version.as_deref().unwrap_or("?"),
            report.sha1.as_deref().unwrap_or("?"),
            report.data_version
        );
    }
    if accept_eula {
        let eula = manager.accept_eula(true, &server_target)?;
        eprintln!(
            "EULA explicitly accepted for {} sha1 {}",
            eula.minecraft_version, eula.jar_sha1
        );
    }
    let request = GenerationRequest {
        seed,
        preset,
        center_x,
        center_z,
        radius_chunks,
        time_limit_seconds,
        memory_mb,
        view_distance,
        cpu_threads,
        generate_structures,
        consent,
        datapacks,
    };
    let snapshot = manager.start_generation(&java, &generation_target, request)?;
    eprintln!("{}", serde_json::to_string_pretty(&snapshot)?);
    if let Some(seconds) = cancel_after {
        std::thread::sleep(Duration::from_secs(seconds));
        eprintln!("cancel requested: {}", manager.cancel_generation());
    }
    loop {
        let status = manager.generation_status();
        if let Some(active) = &status.active {
            eprintln!(
                "{} {} chunks {}/{} elapsed {} ms",
                active.phase as u8,
                active.message,
                active.generated_chunks,
                active.expected_chunks,
                active.elapsed_ms
            );
        } else {
            let report = status.last.ok_or("generation ended without a report")?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            return Ok(());
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}
