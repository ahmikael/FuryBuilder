//! Render-oriented asset bundles: blockstates, models, textures and a packed atlas (M9.2).
//!
//! All inputs are untrusted resource-pack data. Paths are validated, archives are read in
//! memory, JSON and image sizes are bounded, and a malformed asset is replaced by a clearly
//! marked missing texture instead of silently pretending to be a valid Minecraft asset.
//! World data is never derived from these files.

mod model;
mod stack;
mod texture;

use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::settings::AssetSourceSetting;

pub use stack::BundlePack;
pub use texture::BundleTexture;

pub const ASSET_BUNDLE_SCHEMA_VERSION: u32 = 1;
pub const MAX_BLOCK_STATES: usize = 512;
const MAX_CACHE_ENTRIES: usize = 6;
const MAX_CACHE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockStateRequest {
    pub name: String,
    #[serde(default)]
    pub properties: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetBundle {
    pub schema_version: u32,
    pub cache_key: String,
    pub cache_hit: bool,
    pub base_jar: Option<String>,
    pub base_jar_sha1: Option<String>,
    pub packs: Vec<BundlePack>,
    pub atlas_width: u32,
    pub atlas_height: u32,
    pub atlas_png_base64: String,
    pub textures: Vec<BundleTexture>,
    pub blocks: Vec<BundleBlock>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleBlock {
    pub name: String,
    pub properties: BTreeMap<String, String>,
    pub missing_model: bool,
    pub parts: Vec<BundlePart>,
    pub warnings: Vec<String>,
    pub unsupported: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BundlePart {
    pub model: String,
    pub rotation_x: i32,
    pub rotation_y: i32,
    pub uvlock: bool,
    pub elements: Vec<BundleElement>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleElement {
    pub from: [f32; 3],
    pub to: [f32; 3],
    pub rotation: Option<BundleElementRotation>,
    pub shade: bool,
    pub faces: Vec<BundleFace>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleElementRotation {
    pub origin: [f32; 3],
    pub axis: String,
    pub angle: f32,
    pub rescale: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleFace {
    pub direction: String,
    pub texture: String,
    pub uv: [f32; 4],
    pub rotation: u8,
    pub cullface: Option<String>,
    pub tint_index: Option<u8>,
}

#[derive(Debug, Error)]
pub enum BundleError {
    #[error("invalid asset request: {0}")]
    InvalidArgument(String),
    #[error("asset limit exceeded: {0}")]
    LimitExceeded(String),
    #[error("unsupported asset data: {0}")]
    UnsupportedFormat(String),
    #[error("asset io error: {0}")]
    Io(String),
}

impl BundleError {
    pub fn code(&self) -> &'static str {
        match self {
            BundleError::InvalidArgument(_) => "invalid_argument",
            BundleError::LimitExceeded(_) => "limit_exceeded",
            BundleError::UnsupportedFormat(_) => "unsupported_format",
            BundleError::Io(_) => "io_error",
        }
    }
}

impl From<BundleError> for crate::AssetSettingsError {
    fn from(error: BundleError) -> Self {
        match error {
            BundleError::InvalidArgument(message) => crate::AssetSettingsError::Invalid(message),
            BundleError::Io(message) => crate::AssetSettingsError::Io(message),
            other => crate::AssetSettingsError::UnsupportedPack(other.to_string()),
        }
    }
}

struct CacheEntry {
    key: String,
    bundle: AssetBundle,
    bytes: usize,
}

/// Bounded in-memory atlas cache. The key includes the source/pack fingerprint and the
/// canonical requested block-state list, so editing a pack or changing priority rebuilds it.
pub struct AssetBundleCache {
    entries: Mutex<VecDeque<CacheEntry>>,
    max_entries: usize,
    max_bytes: usize,
}

impl Default for AssetBundleCache {
    fn default() -> Self {
        Self {
            entries: Mutex::new(VecDeque::new()),
            max_entries: MAX_CACHE_ENTRIES,
            max_bytes: MAX_CACHE_BYTES,
        }
    }
}

impl AssetBundleCache {
    pub fn get(&self, key: &str) -> Option<AssetBundle> {
        let mut entries = self.entries.lock().ok()?;
        let position = entries.iter().position(|entry| entry.key == key)?;
        let entry = entries.remove(position)?;
        let mut bundle = entry.bundle.clone();
        bundle.cache_hit = true;
        entries.push_front(CacheEntry {
            key: entry.key,
            bundle: entry.bundle,
            bytes: entry.bytes,
        });
        Some(bundle)
    }

    pub fn insert(&self, key: String, bundle: AssetBundle) -> AssetBundle {
        let bytes = bundle.atlas_png_base64.len() + bundle.blocks.len() * 512;
        if let Ok(mut entries) = self.entries.lock() {
            entries.retain(|entry| entry.key != key);
            entries.push_front(CacheEntry {
                key,
                bundle: bundle.clone(),
                bytes,
            });
            while entries.len() > self.max_entries
                || entries.iter().map(|entry| entry.bytes).sum::<usize>() > self.max_bytes
            {
                if entries.pop_back().is_none() {
                    break;
                }
            }
        }
        bundle
    }

    pub fn clear(&self) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.clear();
        }
    }
}

/// Builds render bundles and owns the process-local atlas cache.
#[derive(Default)]
pub struct AssetBundleService {
    cache: AssetBundleCache,
}

impl AssetBundleService {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear_cache(&self) {
        self.cache.clear();
    }

    pub fn bundle(
        &self,
        source: Option<&AssetSourceSetting>,
        pack_paths: &[String],
        states: &[BlockStateRequest],
    ) -> Result<AssetBundle, BundleError> {
        if states.is_empty() {
            return Err(BundleError::InvalidArgument(
                "at least one block state is required".to_owned(),
            ));
        }
        if states.len() > MAX_BLOCK_STATES {
            return Err(BundleError::LimitExceeded(format!(
                "at most {MAX_BLOCK_STATES} block states can be requested at once"
            )));
        }
        for state in states {
            validate_state(state)?;
        }
        let mut stack = stack::ResourceStack::open(source, pack_paths)?;
        let states_hash = states_fingerprint(states);
        let cache_key = format!("{}:{states_hash}", stack.fingerprint);
        if let Some(bundle) = self.cache.get(&cache_key) {
            return Ok(bundle);
        }

        let mut blocks = Vec::with_capacity(states.len());
        let mut texture_keys = std::collections::BTreeSet::new();
        let mut model_cache = std::collections::HashMap::new();
        let mut warnings = stack.warnings.clone();
        for state in states {
            let resolved = model::resolve_block(&mut stack, state, &mut model_cache)?;
            texture_keys.extend(resolved.textures);
            blocks.push(resolved.block);
        }
        let atlas = texture::build_atlas(&mut stack, &texture_keys)?;
        warnings.extend(atlas.warnings);
        let bundle = AssetBundle {
            schema_version: ASSET_BUNDLE_SCHEMA_VERSION,
            cache_key,
            cache_hit: false,
            base_jar: stack.base_jar_path().map(|path| path.display().to_string()),
            base_jar_sha1: stack.base_jar_sha1().map(ToOwned::to_owned),
            packs: stack.packs_info.clone(),
            atlas_width: atlas.width,
            atlas_height: atlas.height,
            atlas_png_base64: atlas.png_base64,
            textures: atlas.textures,
            blocks,
            warnings,
        };
        Ok(self.cache.insert(bundle.cache_key.clone(), bundle))
    }
}

fn validate_state(state: &BlockStateRequest) -> Result<(), BundleError> {
    if state.name.len() > 128 || state.name.is_empty() {
        return Err(BundleError::InvalidArgument(
            "a block state name is empty or too long".to_owned(),
        ));
    }
    if !valid_id(&state.name) {
        return Err(BundleError::InvalidArgument(format!(
            "invalid block state name: {}",
            state.name
        )));
    }
    if state.properties.len() > 32 {
        return Err(BundleError::LimitExceeded(
            "a block state has too many properties".to_owned(),
        ));
    }
    for (key, value) in &state.properties {
        if key.is_empty() || key.len() > 64 || value.len() > 128 {
            return Err(BundleError::InvalidArgument(
                "a block state property is empty or too long".to_owned(),
            ));
        }
        if !key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
        {
            return Err(BundleError::InvalidArgument(format!(
                "invalid block state property name: {key}"
            )));
        }
    }
    Ok(())
}

pub(crate) fn valid_id(id: &str) -> bool {
    let (namespace, path) = match id.split_once(':') {
        Some((namespace, path)) => (namespace, path),
        None => return false,
    };
    !namespace.is_empty()
        && !path.is_empty()
        && namespace.len() <= 64
        && path.len() <= 128
        && namespace
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '-' | '.'))
        && path.chars().all(|c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '-' | '.' | '/')
        })
        && !path.contains("//")
        && !path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
}

pub(crate) fn normalize_id(id: &str) -> Option<String> {
    // Minecraft identifiers are canonical lowercase. Reject mixed case instead of silently
    // rewriting it, so a typo cannot resolve to a different asset than the one requested.
    if id.contains(|c: char| c.is_ascii_uppercase()) {
        return None;
    }
    let normalized = if id.contains(':') {
        id.to_owned()
    } else {
        format!("minecraft:{id}")
    };
    valid_id(&normalized).then_some(normalized)
}

fn states_fingerprint(states: &[BlockStateRequest]) -> String {
    let mut hasher = Sha256::new();
    let mut sorted: Vec<(String, Vec<(String, String)>)> = states
        .iter()
        .map(|state| {
            let mut properties: Vec<(String, String)> = state
                .properties
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect();
            properties.sort();
            (state.name.clone(), properties)
        })
        .collect();
    sorted.sort();
    for (name, properties) in sorted {
        hasher.update(name.as_bytes());
        hasher.update([0]);
        for (key, value) in properties {
            hasher.update(key.as_bytes());
            hasher.update(b"=");
            hasher.update(value.as_bytes());
            hasher.update([0]);
        }
        hasher.update([1]);
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut hex, "{byte:02x}");
    }
    hex
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_normalized_and_validated() {
        assert_eq!(normalize_id("stone").as_deref(), Some("minecraft:stone"));
        assert_eq!(
            normalize_id("minecraft:block/oak_stairs").as_deref(),
            Some("minecraft:block/oak_stairs")
        );
        for id in [
            "",
            "minecraft:",
            ":stone",
            "Minecraft:Stone",
            "minecraft:../stone",
            "minecraft:block//stone",
            "minecraft:block/stone!",
        ] {
            assert!(normalize_id(id).is_none(), "{id:?}");
        }
    }

    #[test]
    fn block_state_fingerprint_is_order_independent() {
        let mut first = BTreeMap::new();
        first.insert("facing".to_owned(), "north".to_owned());
        first.insert("half".to_owned(), "bottom".to_owned());
        let a = states_fingerprint(&[BlockStateRequest {
            name: "minecraft:oak_stairs".to_owned(),
            properties: first.clone(),
        }]);
        let mut reversed = BTreeMap::new();
        reversed.insert("half".to_owned(), "bottom".to_owned());
        reversed.insert("facing".to_owned(), "north".to_owned());
        let b = states_fingerprint(&[BlockStateRequest {
            name: "minecraft:oak_stairs".to_owned(),
            properties: reversed,
        }]);
        assert_eq!(a, b);
    }
}
