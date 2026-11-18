//! Reproducible probe for M9.2 render bundles.
//!
//! Usage:
//!   cargo run -p furybuilder-asset-source --example asset_bundle_probe -- \
//!     --root <launcher-root> [--pack <pack.zip-or-folder> ...] \
//!     --state "minecraft:stone" \
//!     --state "minecraft:oak_stairs[facing=north,half=bottom,shape=straight]" \
//!     [--atlas <out.png>] [--repeat]
//!
//! Only local files are read. The optional `--atlas` writes the generated atlas to a new
//! file for visual inspection; it is never written into the Minecraft installation.

use std::collections::BTreeMap;
use std::path::PathBuf;

use asset_source::{AssetBundleService, AssetSourceSetting, BlockStateRequest};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use serde_json::json;

struct Options {
    kind: String,
    path: String,
    packs: Vec<String>,
    states: Vec<BlockStateRequest>,
    atlas: Option<PathBuf>,
    repeat: bool,
}

fn main() {
    let options = match parse_options() {
        Ok(options) => options,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };
    let source = AssetSourceSetting {
        kind: options.kind.clone(),
        path: options.path.clone(),
    };
    let service = AssetBundleService::new();
    let first = match service.bundle(Some(&source), &options.packs, &options.states) {
        Ok(bundle) => bundle,
        Err(error) => {
            eprintln!("bundle failed ({}): {error}", error.code());
            std::process::exit(1);
        }
    };
    let second = if options.repeat {
        match service.bundle(Some(&source), &options.packs, &options.states) {
            Ok(bundle) => Some(bundle),
            Err(error) => {
                eprintln!("second bundle failed ({}): {error}", error.code());
                std::process::exit(1);
            }
        }
    } else {
        None
    };
    if let Some(path) = &options.atlas {
        let bytes = match BASE64.decode(&first.atlas_png_base64) {
            Ok(bytes) => bytes,
            Err(error) => {
                eprintln!("atlas base64 is invalid: {error}");
                std::process::exit(1);
            }
        };
        if let Err(error) = std::fs::write(path, &bytes) {
            eprintln!("cannot write {}: {error}", path.display());
            std::process::exit(1);
        }
    }
    let summary = json!({
        "tool": { "name": "furybuilder-asset-bundle-probe", "version": 1 },
        "source": { "kind": options.kind, "path": options.path },
        "packs": first.packs,
        "schemaVersion": first.schema_version,
        "cacheKey": first.cache_key,
        "cacheHit": first.cache_hit,
        "secondCacheHit": second.as_ref().map(|bundle| bundle.cache_hit),
        "baseJar": first.base_jar,
        "baseJarSha1": first.base_jar_sha1,
        "atlas": {
            "width": first.atlas_width,
            "height": first.atlas_height,
            "pngBytes": BASE64.decode(&first.atlas_png_base64).map(|bytes| bytes.len()).unwrap_or(0),
            "textureCount": first.textures.len(),
            "missingCount": first.textures.iter().filter(|texture| texture.missing).count(),
        },
        "textures": first.textures.iter().map(|texture| json!({
            "key": texture.key,
            "path": texture.path,
            "x": texture.x,
            "y": texture.y,
            "width": texture.width,
            "height": texture.height,
            "missing": texture.missing,
        })).collect::<Vec<_>>(),
        "blocks": first.blocks.iter().map(|block| json!({
            "name": block.name,
            "properties": block.properties,
            "missingModel": block.missing_model,
            "parts": block.parts.iter().map(|part| json!({
                "model": part.model,
                "rotationX": part.rotation_x,
                "rotationY": part.rotation_y,
                "uvlock": part.uvlock,
                "elements": part.elements.len(),
                "faces": part.elements.iter().map(|element| element.faces.len()).sum::<usize>(),
                "fromTo": part.elements.iter().map(|element| json!({
                    "from": element.from,
                    "to": element.to,
                    "rotation": element.rotation,
                })).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
            "warnings": block.warnings,
            "unsupported": block.unsupported,
        })).collect::<Vec<_>>(),
        "warnings": first.warnings,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&summary).expect("serialize summary")
    );
}

fn parse_options() -> Result<Options, String> {
    let mut args = std::env::args().skip(1);
    let mut kind = "minecraft_root".to_owned();
    let mut path: Option<String> = None;
    let mut packs = Vec::new();
    let mut states = Vec::new();
    let mut atlas = None;
    let mut repeat = false;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--root" => {
                kind = "minecraft_root".to_owned();
                path = args.next();
            }
            "--jar" => {
                kind = "client_jar".to_owned();
                path = args.next();
            }
            "--pack" => {
                packs.push(args.next().ok_or("--pack requires a path")?);
            }
            "--state" => {
                states.push(parse_state(
                    &args.next().ok_or("--state requires a block state")?,
                )?);
            }
            "--atlas" => {
                atlas = Some(PathBuf::from(args.next().ok_or("--atlas requires a path")?));
            }
            "--repeat" => repeat = true,
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    let path = path.ok_or("provide --root <path> or --jar <path>")?;
    if states.is_empty() {
        states.push(parse_state("minecraft:stone")?);
    }
    Ok(Options {
        kind,
        path,
        packs,
        states,
        atlas,
        repeat,
    })
}

fn parse_state(text: &str) -> Result<BlockStateRequest, String> {
    let (name, properties) = match text.split_once('[') {
        Some((name, rest)) => {
            let rest = rest
                .strip_suffix(']')
                .ok_or_else(|| format!("missing closing bracket in {text}"))?;
            (name.trim(), rest)
        }
        None => (text.trim(), ""),
    };
    let mut map = BTreeMap::new();
    if !properties.trim().is_empty() {
        for pair in properties.split(',') {
            let (key, value) = pair
                .split_once('=')
                .ok_or_else(|| format!("invalid property in {text}"))?;
            map.insert(key.trim().to_owned(), value.trim().to_owned());
        }
    }
    Ok(BlockStateRequest {
        name: name.to_owned(),
        properties: map,
    })
}
