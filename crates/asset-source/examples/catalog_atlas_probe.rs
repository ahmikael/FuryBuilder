//! Reproducible probe for the palette icon atlas.
//!
//! Usage:
//!   cargo run -p furybuilder-asset-source --example catalog_atlas_probe -- \
//!     --jar <client.jar> [--pack <pack.zip-or-folder> ...] [--atlas <out.png>] [--repeat]
//!
//! Only local files are read. The optional `--atlas` writes the generated atlas to a new
//! file for visual inspection; it is never written into the Minecraft installation.

use std::path::PathBuf;

use asset_source::{
    AssetCatalogService, AssetSourceSetting, CatalogIconAtlasService, CATALOG_ATLAS_SCHEMA_VERSION,
};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use serde_json::json;

struct Options {
    jar: String,
    packs: Vec<String>,
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
        kind: "client_jar".to_owned(),
        path: options.jar.clone(),
    };
    let catalog = match AssetCatalogService::new().build(&source) {
        Ok(catalog) => catalog,
        Err(error) => {
            eprintln!("catalog failed ({}): {error}", error.code());
            std::process::exit(1);
        }
    };
    let service = CatalogIconAtlasService::new();
    let version = catalog
        .minecraft_version
        .parse()
        .expect("valid catalog version");
    let profile = minecraft_version::profile_for_version(&version).expect("known version profile");
    let packs: Vec<_> = options
        .packs
        .iter()
        .map(asset_source::ResourcePackInput::local)
        .collect();
    let texture_keys: Vec<String> = catalog.icon_texture_keys().into_iter().take(256).collect();
    let first = match service.atlas(&profile, &source, &packs, &catalog, &texture_keys) {
        Ok(atlas) => atlas,
        Err(error) => {
            eprintln!("atlas failed ({}): {error}", error.code());
            std::process::exit(1);
        }
    };
    let second = if options.repeat {
        match service.atlas(&profile, &source, &packs, &catalog, &texture_keys) {
            Ok(atlas) => Some(atlas),
            Err(error) => {
                eprintln!("second atlas failed ({}): {error}", error.code());
                std::process::exit(1);
            }
        }
    } else {
        None
    };
    if let Some(path) = &options.atlas {
        let bytes = match BASE64.decode(&first.png_base64) {
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
        "tool": { "name": "furybuilder-catalog-atlas-probe", "version": 2 },
        "source": { "kind": "client_jar", "path": options.jar },
        "packs": options.packs,
        "schemaVersion": first.schema_version,
        "expectedSchemaVersion": CATALOG_ATLAS_SCHEMA_VERSION,
        "catalogId": first.catalog_id,
        "catalogHash": first.catalog_hash,
        "catalogSchemaVersion": first.catalog_schema_version,
        "minecraftVersion": first.minecraft_version,
        "clientJarSha1": first.client_jar_sha1,
        "packStackHash": first.pack_stack_hash,
        "cacheKey": first.cache_key,
        "cacheHit": first.cache_hit,
        "secondCacheHit": second.as_ref().map(|atlas| atlas.cache_hit),
        "atlas": {
            "width": first.width,
            "height": first.height,
            "pngBytes": BASE64.decode(&first.png_base64).map(|bytes| bytes.len()).unwrap_or(0),
            "textureCount": first.textures.len(),
            "requestedTextureCount": first.requested_texture_count,
            "totalIconTextureCount": first.total_icon_texture_count,
            "missingCount": first.textures.iter().filter(|texture| texture.missing).count(),
        },
        "sample": first.textures.iter().find(|texture| texture.key == "minecraft:block/stone"),
        "warnings": first.warnings,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&summary).expect("serialize summary")
    );
}

fn parse_options() -> Result<Options, String> {
    let mut args = std::env::args().skip(1);
    let mut jar = None;
    let mut packs = Vec::new();
    let mut atlas = None;
    let mut repeat = false;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--jar" => jar = args.next(),
            "--pack" => packs.push(args.next().ok_or("--pack requires a path")?),
            "--atlas" => atlas = Some(PathBuf::from(args.next().ok_or("--atlas requires a path")?)),
            "--repeat" => repeat = true,
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(Options {
        jar: jar.ok_or("provide --jar <client.jar>")?,
        packs,
        atlas,
        repeat,
    })
}
