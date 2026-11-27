//! Read-only block/item catalog built from the exact client JAR of a project version.
//!
//! The catalog is facts from the JAR, not a curated list: every blockstate file becomes a
//! block entry, every item definition (or item model on older layouts) an item entry, and
//! names, tags and atlas definitions are read from the same archive. Resource packs change
//! rendering, not registry contents, so the catalog is built from the base client JAR only.
//!
//! Exposure is bounded: JSON files, entries, textures, languages, tags and warnings all have
//! explicit limits. A malformed file is skipped with a warning instead of aborting the whole
//! catalog, but a JAR without blockstates is refused rather than reported as an empty catalog.
//!
//! The catalog never writes into the Minecraft installation and never extracts archive
//! entries to disk.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::archive::Archive;
use crate::bundle::normalize_id;
use crate::catalog_flags::classify_flags;
use crate::discovery::{inspect_asset_path, read_version_metadata};
use crate::jar::{client_jar_version, file_sha1};
use crate::settings::AssetSourceSetting;

/// Version of the catalog DTO written by this builder.
pub const CATALOG_SCHEMA_VERSION: u32 = 6;
/// A vanilla client JAR has a few thousand blockstates and items.
pub const MAX_CATALOG_BLOCKS: usize = 32_768;
pub const MAX_CATALOG_ITEMS: usize = 32_768;
pub const MAX_CATALOG_TEXTURES: usize = 65_536;
pub const MAX_CATALOG_TAGS: usize = 16_384;
pub const MAX_CATALOG_LANG_ENTRIES: usize = 65_536;
pub const MAX_CATALOG_ATLAS_SOURCES: usize = 256;
/// Upper bound for one catalog JSON document (`version.json`, blockstate, model, tag, ...).
pub const MAX_CATALOG_JSON_BYTES: u64 = 4 * 1024 * 1024;
/// Warnings are diagnostics, not data; a broken pack must not produce an unbounded response.
pub const MAX_CATALOG_WARNINGS: usize = 256;
/// Budget for the canonical catalog payload held in the backend and sent to the WebView.
/// A 10k+ entry catalog fits comfortably; exceeding this is a hard error rather than runaway
/// memory use in both processes.
pub const MAX_CATALOG_MEMORY_BYTES: usize = 64 * 1024 * 1024;

const MAX_CATALOG_CACHE_ENTRIES: usize = 4;
const MAX_MODEL_DEPTH: usize = 12;
/// Item frames are entities. Their blockstate files exist only to render the entity model,
/// so they must not become placeable block entries. Their item forms stay item-only.
const ENTITY_BACKED_BLOCKSTATES: &[&str] = &["minecraft:item_frame", "minecraft:glow_item_frame"];
const UNVERIFIED_JAR_WARNING: &str =
    "the client JAR SHA-1 was not verified against launcher metadata";

/// One block and/or item from the client JAR.
///
/// A block-only entry has `states` but no `itemForm`; an item-only entry has the reverse.
/// Normal blocks with an item form have both. Facts that cannot be derived from the JAR yet
/// (survival obtainability, block entity, geometry flags) stay `None` in [`CatalogFlags`]
/// instead of being guessed; later catalog passes fill them from official metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntry {
    pub id: String,
    /// Numeric id and optional metadata for legacy versions. `None` for named registries.
    pub legacy_id: Option<CatalogLegacyId>,
    pub display_name: Option<String>,
    /// Category hint assigned by the catalog pass. `None` until categories are known.
    pub category: Option<String>,
    pub tags: Vec<String>,
    /// A texture that exists in the JAR and can be used as the palette icon.
    pub icon_texture: Option<String>,
    /// Older names that resolve to this id. Empty until aliases are known.
    pub aliases: Vec<String>,
    /// Block states declared by the JAR. `None` when the id has no block form.
    pub states: Option<CatalogStates>,
    /// Item form declared by the JAR. `None` for block-only technical entries.
    pub item_form: Option<CatalogItemForm>,
    pub flags: CatalogFlags,
}

/// Numeric id used by pre-flattening versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogLegacyId {
    pub id: u32,
    pub metadata: Option<u16>,
}

/// Every block state property key and value declared by the blockstate file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogStates {
    pub properties: BTreeMap<String, Vec<String>>,
    /// True when the blockstate file enumerates every value of every property it declares.
    /// Multipart conditions and single empty variants only name some states, so they are
    /// incomplete. Properties that never affect a model (for example `waterlogged` on stairs)
    /// are not declared at all and cannot be validated from the blockstate file.
    pub property_values_complete: bool,
    /// The only state declared by the blockstate file. `None` when several variants exist,
    /// because the game default is maintained by game code, not by the blockstate file.
    /// Omitting properties lets Minecraft resolve the exact default state.
    pub default_properties: Option<BTreeMap<String, String>>,
    /// Number of explicit `variants` entries; zero for multipart blocks.
    pub variant_count: usize,
    /// Every explicit variant in file order, including its exact property key and weighted
    /// model choices. Multipart blocks have no finite variant list.
    pub variants: Vec<CatalogVariant>,
    pub multipart: bool,
    /// First model referenced by the blockstate file.
    pub model: Option<String>,
}

/// One explicit entry from the blockstate `variants` object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogVariant {
    /// Property values named by the variant key. Empty when the file uses the single `""` key.
    pub properties: BTreeMap<String, String>,
    /// Weighted model choices declared by the variant. Vanilla usually has one.
    pub models: Vec<CatalogVariantModel>,
}

/// One model choice inside a variant, including the rotation and weight from the JAR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogVariantModel {
    pub model: Option<String>,
    pub x_rotation: i32,
    pub y_rotation: i32,
    pub uvlock: bool,
    pub weight: u32,
}

/// The item form of one catalog id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogItemForm {
    /// True when the entry came from `assets/<namespace>/items/<item>.json`.
    pub item_definition: bool,
    /// Top-level item definition model type, for example `minecraft:select` or
    /// `minecraft:special`. `None` for legacy item models and definitions without a type.
    pub model_type: Option<String>,
    pub model: Option<String>,
}

/// Facts used by palette rules and placement. `None` means the fact is not known yet, not
/// that the answer is false.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogFlags {
    pub placeable: Option<bool>,
    pub has_item: Option<bool>,
    pub obtainable: Option<bool>,
    pub creative_only: Option<bool>,
    pub technical: Option<bool>,
    pub fluid: Option<bool>,
    pub airlike: Option<bool>,
    pub block_entity: Option<bool>,
    pub transparent: Option<bool>,
    pub full_cube: Option<bool>,
    pub gravity: Option<bool>,
    pub waterloggable: Option<bool>,
    pub tinted: Option<bool>,
    pub has_variants: Option<bool>,
}

impl CatalogEntry {
    /// A block entry owns states; an item-only entry does not.
    pub fn is_block(&self) -> bool {
        self.states.is_some()
    }

    /// An entry with an item form can appear in item searches and inventory-like lists.
    pub fn is_item(&self) -> bool {
        self.item_form.is_some()
    }
}

impl CatalogSnapshot {
    /// Distinct icon textures declared by entries. The palette requests only the subset it
    /// currently displays, so the atlas never decodes every icon for a large catalog.
    pub fn icon_texture_keys(&self) -> BTreeSet<String> {
        self.entries
            .iter()
            .filter_map(|entry| entry.icon_texture.clone())
            .collect()
    }
}

/// One atlas definition from `assets/<namespace>/atlases/<atlas>.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogAtlas {
    pub id: String,
    pub sources: Vec<CatalogAtlasSource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogAtlasSource {
    pub kind: String,
    pub source: Option<String>,
    pub prefix: Option<String>,
    pub resource: Option<String>,
}

/// Counts from one JAR inspection, useful before opening the full palette.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogCounts {
    pub blocks: usize,
    pub items: usize,
    pub blocks_with_items: usize,
    pub item_only: usize,
    pub blockstate_files: usize,
    pub block_models: usize,
    pub item_models: usize,
    pub item_definitions: usize,
    pub textures: usize,
    pub languages: usize,
    pub lang_entries: usize,
    pub block_tags: usize,
    pub item_tags: usize,
    pub atlases: usize,
    pub atlas_sources: usize,
}

/// A deterministic block/item catalog for one client JAR.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogSnapshot {
    pub schema_version: u32,
    /// Stable id: `java-<version>-<first 16 hex of catalogHash>`.
    pub catalog_id: String,
    /// SHA-256 over the canonical catalog contents, version and client JAR SHA-1.
    pub catalog_hash: String,
    pub minecraft_version: String,
    pub data_version: i32,
    pub client_jar: String,
    pub client_jar_sha1: Option<String>,
    pub resource_pack_format: Option<u32>,
    pub datapack_format: Option<u32>,
    /// One entry per id, merging its block and item forms when both exist.
    pub entries: Vec<CatalogEntry>,
    pub atlases: Vec<CatalogAtlas>,
    pub counts: CatalogCounts,
    pub warnings: Vec<String>,
    /// Set only on a cache hit; never part of [`Self::catalog_hash`].
    pub cache_hit: bool,
}

#[derive(Debug, Error)]
pub enum CatalogError {
    #[error("client assets are not usable: {0}")]
    InvalidSource(String),
    #[error("client catalog is unavailable: {0}")]
    Unsupported(String),
    #[error("client catalog io error: {0}")]
    Io(String),
    #[error("client catalog limit exceeded: {0}")]
    LimitExceeded(String),
    #[error("client catalog version.json is invalid: {0}")]
    VersionJson(String),
}

impl CatalogError {
    pub fn code(&self) -> &'static str {
        match self {
            CatalogError::InvalidSource(_) => "invalid_argument",
            CatalogError::Unsupported(_) => "unsupported_format",
            CatalogError::Io(_) => "io_error",
            CatalogError::LimitExceeded(_) => "limit_exceeded",
            CatalogError::VersionJson(_) => "invalid_version",
        }
    }
}

struct CatalogCacheEntry {
    key: String,
    catalog: CatalogSnapshot,
}

/// Resolved client JAR plus the official version metadata used to decode it.
///
/// `CatalogBuilder` owns the JAR-to-snapshot conversion: `version.json` inside the archive
/// supplies the exact version, `DataVersion` and pack formats, while launcher metadata
/// supplies the checksum and display path. Caching and project pin comparison stay outside
/// the builder.
pub struct CatalogBuilder {
    resolved: ResolvedClientJar,
}

impl CatalogBuilder {
    pub fn open(source: &AssetSourceSetting) -> Result<Self, CatalogError> {
        Ok(Self {
            resolved: resolve_client_jar(source)?,
        })
    }

    /// Stable cache identity: exact version, client JAR SHA-1 and catalog schema. The path is
    /// deliberately absent, so moving or copying the same JAR reuses the same facts. A JAR
    /// without a readable SHA-1 falls back to its path, size and modification time.
    pub fn cache_key(&self) -> Result<String, CatalogError> {
        if let Some(sha1) = &self.resolved.sha1 {
            return Ok(format!(
                "{}|{}|{}",
                self.resolved.version, sha1, CATALOG_SCHEMA_VERSION
            ));
        }
        let metadata = fs::metadata(&self.resolved.jar_path).map_err(|error| {
            CatalogError::Io(format!(
                "cannot inspect {}: {error}",
                self.resolved.jar_path.display()
            ))
        })?;
        let modified = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        Ok(format!(
            "{}|path:{}|size:{}|mtime:{}|{}",
            self.resolved.version,
            self.resolved.jar_path.display(),
            metadata.len(),
            modified,
            CATALOG_SCHEMA_VERSION
        ))
    }

    /// Builds a fresh snapshot without consulting the service cache.
    pub fn build(&self) -> Result<CatalogSnapshot, CatalogError> {
        build_catalog(&self.resolved)
    }
}

/// Bounded process-local catalog cache. Entries are keyed by exact version, JAR SHA-1 and
/// catalog schema, never by path or "latest". The project records the catalog id and hash
/// independently of this cache.
#[derive(Default)]
pub struct AssetCatalogService {
    cache: Mutex<VecDeque<CatalogCacheEntry>>,
}

impl AssetCatalogService {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&self) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.clear();
        }
    }

    /// Builds (or reuses) the catalog for the selected client assets.
    pub fn catalog(&self, source: &AssetSourceSetting) -> Result<CatalogSnapshot, CatalogError> {
        let builder = CatalogBuilder::open(source)?;
        let key = builder.cache_key()?;
        if let Ok(mut cache) = self.cache.lock() {
            if let Some(index) = cache.iter().position(|entry| entry.key == key) {
                if let Some(entry) = cache.remove(index) {
                    let mut catalog = entry.catalog.clone();
                    catalog.cache_hit = true;
                    apply_cached_source(&mut catalog, &builder.resolved);
                    cache.push_front(entry);
                    return Ok(catalog);
                }
            }
        }
        let catalog = builder.build()?;
        if let Ok(mut cache) = self.cache.lock() {
            cache.retain(|entry| entry.key != key);
            cache.push_front(CatalogCacheEntry {
                key,
                catalog: catalog.clone(),
            });
            while cache.len() > MAX_CATALOG_CACHE_ENTRIES {
                cache.pop_back();
            }
        }
        Ok(catalog)
    }

    /// Builds without consulting or updating the cache. Used by tests and diagnostics.
    pub fn build(&self, source: &AssetSourceSetting) -> Result<CatalogSnapshot, CatalogError> {
        CatalogBuilder::open(source)?.build()
    }
}

#[derive(Debug)]
struct ResolvedClientJar {
    jar_path: PathBuf,
    display_path: String,
    version: String,
    data_version: i32,
    resource_pack_format: Option<u32>,
    datapack_format: Option<u32>,
    sha1: Option<String>,
    checksum_verified: bool,
    warnings: Vec<String>,
}

fn resolve_client_jar(source: &AssetSourceSetting) -> Result<ResolvedClientJar, CatalogError> {
    if source.kind == "resource_pack" {
        return Err(CatalogError::InvalidSource(
            "a resource pack has no block/item registry; select a .minecraft root, version folder or client JAR"
                .to_owned(),
        ));
    }
    let selected = PathBuf::from(&source.path);
    match fs::symlink_metadata(&selected) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(CatalogError::InvalidSource(
                "the selection is a symlink, junction or reparse point; select the real path"
                    .to_owned(),
            ));
        }
        Ok(metadata) if metadata.is_file() => {
            // A direct client JAR is self-describing: `version.json` inside the archive is the
            // exact version authority. The application target is checked by callers against
            // the project profile, so catalogs can be built and compared across versions.
            return resolve_direct_client_jar(selected);
        }
        Ok(metadata) if metadata.is_dir() => {
            // A version folder and a managed client cache are selected directly. Both hold a
            // JAR plus its launcher metadata, so the catalog does not depend on the
            // application target. A .minecraft root still goes through target discovery.
            if let Some(jar_path) = version_folder_jar(&selected) {
                return resolve_direct_client_jar(jar_path);
            }
        }
        Ok(_) => {}
        Err(error) => {
            return Err(CatalogError::Io(format!(
                "cannot inspect {}: {error}",
                selected.display()
            )));
        }
    }
    let report = inspect_asset_path(&source.path);
    if !report.valid || !report.target_match {
        let reason = report
            .errors
            .first()
            .cloned()
            .or_else(|| report.warnings.first().cloned())
            .unwrap_or_else(|| "the selected path is not a compatible client source".to_owned());
        return Err(CatalogError::InvalidSource(reason));
    }
    let jar_path = report
        .version
        .and_then(|version| (!version.jar_path.is_empty()).then_some(version.jar_path))
        .ok_or_else(|| {
            CatalogError::InvalidSource("the selected path has no client JAR".to_owned())
        })?;
    resolve_direct_client_jar(PathBuf::from(jar_path))
}

/// A version folder (`versions/<id>/<id>.jar`) or a managed asset cache (`client.jar`).
/// Neither is the `.minecraft` root; the root has no JAR directly inside it.
fn version_folder_jar(directory: &Path) -> Option<PathBuf> {
    let mut names = Vec::new();
    if let Some(name) = directory.file_name() {
        names.push(format!("{}.jar", name.to_string_lossy()));
    }
    names.push("client.jar".to_owned());
    names.dedup();
    names
        .into_iter()
        .map(|name| directory.join(name))
        .find(|path| is_regular_file(path))
}

fn is_regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
}

fn resolve_direct_client_jar(jar_path: PathBuf) -> Result<ResolvedClientJar, CatalogError> {
    let metadata = fs::symlink_metadata(&jar_path).map_err(|error| {
        CatalogError::Io(format!("cannot inspect {}: {error}", jar_path.display()))
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(CatalogError::InvalidSource(format!(
            "{} is not a regular client JAR",
            jar_path.display()
        )));
    }
    let mut warnings = Vec::new();
    let sidecar_path = jar_path.with_extension("json");
    let sidecar = read_version_metadata(&sidecar_path).map_err(CatalogError::InvalidSource)?;
    let sha1 = file_sha1(&jar_path).map_err(|error| {
        CatalogError::Io(format!(
            "cannot hash the client JAR {}: {error}",
            jar_path.display()
        ))
    })?;
    let bytes = metadata.len();
    let declared = sidecar
        .as_ref()
        .and_then(|sidecar| sidecar.downloads.as_ref())
        .and_then(|downloads| downloads.client.as_ref());
    let checksum_verified = if let Some(client) = declared {
        if let Some(expected) = &client.sha1 {
            if !expected.eq_ignore_ascii_case(&sha1) {
                return Err(CatalogError::InvalidSource(format!(
                    "client JAR SHA-1 is {sha1} but launcher metadata declares {expected}"
                )));
            }
        }
        if let Some(expected) = client.size {
            if expected != bytes {
                return Err(CatalogError::InvalidSource(format!(
                    "client JAR size is {bytes} bytes but launcher metadata declares {expected}"
                )));
            }
        }
        client.sha1.is_some()
    } else {
        false
    };

    let launcher_id = sidecar.as_ref().and_then(|metadata| metadata.id.as_deref());
    let version = client_jar_version(&jar_path, launcher_id, checksum_verified)
        .map_err(CatalogError::VersionJson)?;
    if version
        .pack_formats
        .is_some_and(|formats| formats.minors.is_some())
    {
        return Err(CatalogError::VersionJson(
            minecraft_version::ProfileBuildError::VersionedPackFormatsUnsupported.to_string(),
        ));
    }
    if version.internal_id.is_none() {
        warnings.push(format!(
            "the client JAR has no internal version.json; exact version, DataVersion and pack formats were resolved from {} and the built-in Minecraft {} profile",
            sidecar_path.display(), version.id
        ));
    }

    Ok(ResolvedClientJar {
        display_path: jar_path.display().to_string(),
        version: version.id,
        data_version: version.world_version,
        resource_pack_format: version.pack_formats.map(|formats| formats.resource),
        datapack_format: version.pack_formats.map(|formats| formats.data),
        sha1: Some(sha1),
        checksum_verified,
        warnings,
        jar_path,
    })
}

/// A cache hit may come from a different path or verification state of the same JAR. Keep
/// the facts and hash, but return the current display path and current checksum warning.
fn apply_cached_source(catalog: &mut CatalogSnapshot, resolved: &ResolvedClientJar) {
    catalog.client_jar = resolved.display_path.clone();
    if resolved.checksum_verified {
        catalog
            .warnings
            .retain(|warning| warning != UNVERIFIED_JAR_WARNING);
        return;
    }
    if !catalog
        .warnings
        .iter()
        .any(|warning| warning == UNVERIFIED_JAR_WARNING)
    {
        catalog.warnings.push(UNVERIFIED_JAR_WARNING.to_owned());
        catalog.warnings.sort();
    }
}

#[derive(Default)]
struct BlockStateFacts {
    properties: BTreeMap<String, BTreeSet<String>>,
    default_properties: Option<BTreeMap<String, String>>,
    property_values_complete: bool,
    variant_count: usize,
    variants: Vec<CatalogVariant>,
    multipart: bool,
    first_model: Option<String>,
}

struct BlockFacts {
    display_name: Option<String>,
    states: CatalogStates,
    icon_texture: Option<String>,
    tags: Vec<String>,
}

struct ItemFacts {
    display_name: Option<String>,
    model: Option<String>,
    model_type: Option<String>,
    item_definition: bool,
    icon_texture: Option<String>,
    tags: Vec<String>,
}

fn build_catalog(resolved: &ResolvedClientJar) -> Result<CatalogSnapshot, CatalogError> {
    let mut archive = Archive::open(&resolved.jar_path).map_err(|error| {
        CatalogError::InvalidSource(format!(
            "cannot read the client JAR {}: {error}",
            resolved.jar_path.display()
        ))
    })?;
    let mut warnings = Vec::new();
    if !resolved.checksum_verified {
        push_warning(&mut warnings, UNVERIFIED_JAR_WARNING.to_owned());
    }
    for warning in &resolved.warnings {
        push_warning(&mut warnings, warning.clone());
    }

    let (resource_pack_format, datapack_format) =
        (resolved.resource_pack_format, resolved.datapack_format);

    let entry_names: Vec<String> = archive
        .entries()
        .iter()
        .map(|entry| entry.name.clone())
        .collect();
    let blockstate_entries = collect_asset_entries(&entry_names, "blockstates", ".json");
    if blockstate_entries.is_empty() {
        return Err(CatalogError::Unsupported(
            "the client JAR contains no blockstate JSON files; this version has no JAR-derived block catalog"
                .to_owned(),
        ));
    }
    if blockstate_entries.len() > MAX_CATALOG_BLOCKS {
        return Err(CatalogError::LimitExceeded(format!(
            "the client JAR contains more than {MAX_CATALOG_BLOCKS} blockstates"
        )));
    }
    let model_entries = collect_asset_entries(&entry_names, "models", ".json");
    let definition_entries = collect_asset_entries(&entry_names, "items", ".json");
    if definition_entries.len() > MAX_CATALOG_ITEMS {
        return Err(CatalogError::LimitExceeded(format!(
            "the client JAR contains more than {MAX_CATALOG_ITEMS} item definitions"
        )));
    }
    let texture_entries = collect_asset_entries(&entry_names, "textures", ".png");
    if texture_entries.len() > MAX_CATALOG_TEXTURES {
        return Err(CatalogError::LimitExceeded(format!(
            "the client JAR contains more than {MAX_CATALOG_TEXTURES} textures"
        )));
    }
    let texture_ids: BTreeSet<String> = texture_entries.iter().map(|(id, _)| id.clone()).collect();
    let block_model_count = model_entries
        .iter()
        .filter(|(id, _)| {
            id.split_once(':')
                .is_some_and(|(_, path)| path.starts_with("block/"))
        })
        .count();
    let item_model_count = model_entries
        .iter()
        .filter(|(id, _)| {
            id.split_once(':')
                .is_some_and(|(_, path)| path.starts_with("item/"))
        })
        .count();

    let (lang, languages) = read_language(&mut archive, &entry_names, &mut warnings);

    let mut blocks: BTreeMap<String, BlockFacts> = BTreeMap::new();
    for (id, entry) in &blockstate_entries {
        if ENTITY_BACKED_BLOCKSTATES.contains(&id.as_str()) {
            push_warning(
                &mut warnings,
                format!(
                    "{entry} is an entity render model, not a block; it is not admitted as a placeable block"
                ),
            );
            continue;
        }
        let facts = match read_json(&mut archive, entry, &mut warnings) {
            Some(document) => parse_blockstate(&document, entry, &mut warnings),
            None => BlockStateFacts::default(),
        };
        let states = CatalogStates {
            properties: facts
                .properties
                .iter()
                .map(|(key, values)| (key.clone(), values.iter().cloned().collect()))
                .collect(),
            property_values_complete: facts.property_values_complete,
            default_properties: facts.default_properties,
            variant_count: facts.variant_count,
            variants: facts.variants,
            multipart: facts.multipart,
            model: facts.first_model,
        };
        blocks.insert(
            id.clone(),
            BlockFacts {
                display_name: block_display_name(&lang, id),
                states,
                icon_texture: None,
                tags: Vec::new(),
            },
        );
    }

    let model_index: HashMap<String, String> = model_entries.into_iter().collect();
    let item_model_ids: BTreeSet<String> = model_index
        .keys()
        .filter(|id| {
            id.split_once(':')
                .is_some_and(|(_, path)| path.starts_with("item/"))
        })
        .cloned()
        .collect();
    let mut items: BTreeMap<String, ItemFacts> = BTreeMap::new();
    for (id, entry) in &definition_entries {
        let mut model = None;
        let mut model_type = None;
        if let Some(document) = read_json(&mut archive, entry, &mut warnings) {
            (model, model_type) = item_definition_model(&document);
            if model.is_none() {
                push_warning(
                    &mut warnings,
                    format!("{entry} has no item model reference"),
                );
            }
        }
        items.insert(
            id.clone(),
            ItemFacts {
                display_name: None,
                model,
                model_type,
                item_definition: true,
                icon_texture: None,
                tags: Vec::new(),
            },
        );
    }
    if definition_entries.is_empty() {
        // 1.20.6 and older expose item models instead of item definitions. Item templates
        // such as `item/handheld` are not items, so only block items and item ids backed by a
        // real item model are admitted; a template or an empty placeholder model can never
        // become a fake palette entry.
        let mut model_content: HashMap<String, bool> = HashMap::new();
        for block in blocks.keys() {
            let Some((namespace, path)) = block.split_once(':') else {
                continue;
            };
            if is_wall_fan_variant(block, &blocks) {
                continue;
            }
            let model_id = format!("{namespace}:item/{path}");
            if item_model_ids.contains(&model_id)
                && model_has_content(
                    &mut archive,
                    &model_index,
                    &model_id,
                    &mut model_content,
                    &mut warnings,
                )
            {
                items.insert(
                    block.clone(),
                    ItemFacts {
                        display_name: None,
                        model: Some(model_id),
                        model_type: None,
                        item_definition: false,
                        icon_texture: None,
                        tags: Vec::new(),
                    },
                );
            }
        }
        for key in lang.keys() {
            let Some(rest) = key.strip_prefix("item.") else {
                continue;
            };
            let Some((namespace, path)) = rest.split_once('.') else {
                continue;
            };
            if path.contains('.') {
                continue;
            }
            let id = format!("{namespace}:{path}");
            let model_id = format!("{namespace}:item/{path}");
            if !crate::bundle::valid_id(&id)
                || !item_model_ids.contains(&model_id)
                || !model_has_content(
                    &mut archive,
                    &model_index,
                    &model_id,
                    &mut model_content,
                    &mut warnings,
                )
            {
                continue;
            }
            items.entry(id).or_insert(ItemFacts {
                display_name: None,
                model: Some(model_id),
                model_type: None,
                item_definition: false,
                icon_texture: None,
                tags: Vec::new(),
            });
        }
    }

    let mut resolver = ModelResolver {
        archive: &mut archive,
        models: &model_index,
        cache: HashMap::new(),
    };
    for block in blocks.values_mut() {
        if let Some(model) = &block.states.model {
            block.icon_texture = resolver.icon_texture(model, &texture_ids, &mut warnings);
        }
    }
    for (id, item) in items.iter_mut() {
        let special = item.model_type.as_deref() == Some("minecraft:special");
        item.icon_texture = item.model.as_ref().and_then(|model| {
            resolver.item_icon_texture(model, special, &texture_ids, &mut warnings)
        });
        item.display_name = item_display_name(&lang, id, blocks.contains_key(id));
    }

    let (block_tags, block_tag_files) = parse_tag_folders(
        &mut archive,
        &entry_names,
        &["tags/block", "tags/blocks"],
        &mut warnings,
    );
    let (item_tags, item_tag_files) = parse_tag_folders(
        &mut archive,
        &entry_names,
        &["tags/item", "tags/items"],
        &mut warnings,
    );
    let (atlases, atlas_sources) = read_atlases(&mut archive, &entry_names, &mut warnings);

    for (id, block) in &mut blocks {
        block.tags = tags_for(&block_tags, id);
    }
    for (id, item) in &mut items {
        item.tags = tags_for(&item_tags, id);
    }

    let mut entries: BTreeMap<String, CatalogEntry> = BTreeMap::new();
    for (id, block) in blocks {
        entries.insert(
            id.clone(),
            CatalogEntry {
                id,
                legacy_id: None,
                display_name: block.display_name,
                category: None,
                tags: block.tags,
                icon_texture: block.icon_texture,
                aliases: Vec::new(),
                states: Some(block.states),
                item_form: None,
                flags: CatalogFlags::default(),
            },
        );
    }
    for (id, item) in items {
        let special = item.model_type.as_deref() == Some("minecraft:special");
        let fallback_icon = if special {
            None
        } else {
            entries
                .get(&id)
                .and_then(|entry| entry.icon_texture.clone())
        };
        let icon = item.icon_texture.clone().or(fallback_icon);
        let entry = entries.entry(id.clone()).or_insert_with(|| CatalogEntry {
            id,
            legacy_id: None,
            display_name: None,
            category: None,
            tags: Vec::new(),
            icon_texture: None,
            aliases: Vec::new(),
            states: None,
            item_form: None,
            flags: CatalogFlags::default(),
        });
        entry.display_name = item.display_name.or_else(|| entry.display_name.take());
        entry.item_form = Some(CatalogItemForm {
            item_definition: item.item_definition,
            model_type: item.model_type,
            model: item.model,
        });
        entry.icon_texture = icon.or_else(|| entry.icon_texture.take());
        entry.tags.extend(item.tags);
        entry.tags.sort();
        entry.tags.dedup();
    }
    for entry in entries.values_mut() {
        let has_variants = entry
            .states
            .as_ref()
            .is_some_and(|states| states.multipart || states.variant_count > 1);
        entry.flags = classify_flags(&entry.id, entry.is_block(), entry.is_item(), has_variants);
    }
    let entries: Vec<CatalogEntry> = entries.into_values().collect();

    let mut foreign_namespaces = BTreeSet::new();
    for id in entries.iter().map(|entry| &entry.id) {
        if let Some((namespace, _)) = id.split_once(':') {
            if namespace != "minecraft" {
                foreign_namespaces.insert(namespace.to_owned());
            }
        }
    }
    if !foreign_namespaces.is_empty() {
        push_warning(
            &mut warnings,
            format!(
                "non-vanilla namespaces are present ({}); their entries are facts from this JAR, not part of the vanilla catalog",
                foreign_namespaces.into_iter().collect::<Vec<_>>().join(", ")
            ),
        );
    }

    warnings.sort();
    warnings.dedup();
    let block_count = entries.iter().filter(|entry| entry.is_block()).count();
    let item_count = entries.iter().filter(|entry| entry.is_item()).count();
    let blocks_with_items = entries
        .iter()
        .filter(|entry| entry.is_block() && entry.is_item())
        .count();
    let counts = CatalogCounts {
        blocks: block_count,
        items: item_count,
        blocks_with_items,
        item_only: item_count - blocks_with_items,
        blockstate_files: blockstate_entries.len(),
        block_models: block_model_count,
        item_models: item_model_count,
        item_definitions: definition_entries.len(),
        textures: texture_entries.len(),
        languages,
        lang_entries: lang.len(),
        block_tags: block_tag_files,
        item_tags: item_tag_files,
        atlases: atlases.len(),
        atlas_sources,
    };
    let (catalog_hash, catalog_bytes) = catalog_digest(
        &resolved.version,
        resolved.data_version,
        resolved.sha1.as_deref(),
        &entries,
        &atlases,
    )?;
    if catalog_bytes > MAX_CATALOG_MEMORY_BYTES {
        return Err(CatalogError::LimitExceeded(format!(
            "the catalog serializes to {catalog_bytes} bytes, over the {MAX_CATALOG_MEMORY_BYTES}-byte memory budget"
        )));
    }
    let catalog_id = format!(
        "java-{}-{}",
        safe_version_component(&resolved.version),
        &catalog_hash[..16]
    );
    Ok(CatalogSnapshot {
        schema_version: CATALOG_SCHEMA_VERSION,
        catalog_id,
        catalog_hash,
        minecraft_version: resolved.version.clone(),
        data_version: resolved.data_version,
        client_jar: resolved.display_path.clone(),
        client_jar_sha1: resolved.sha1.clone(),
        resource_pack_format,
        datapack_format,
        entries,
        atlases,
        counts,
        warnings,
        cache_hit: false,
    })
}

fn collect_asset_entries(
    entries: &[String],
    folder: &str,
    extension: &str,
) -> Vec<(String, String)> {
    collect_entries(entries, "assets", folder, extension)
}

fn collect_data_entries(
    entries: &[String],
    folder: &str,
    extension: &str,
) -> Vec<(String, String)> {
    collect_entries(entries, "data", folder, extension)
}

fn collect_entries(
    entries: &[String],
    root: &str,
    folder: &str,
    extension: &str,
) -> Vec<(String, String)> {
    let mut collected = Vec::new();
    for name in entries {
        if let Some((namespace, path)) = parse_namespaced_entry(name, root, folder, extension) {
            let id = format!("{namespace}:{path}");
            if crate::bundle::valid_id(&id) {
                collected.push((id, name.clone()));
            }
        }
    }
    collected.sort();
    collected
}

fn parse_namespaced_entry(
    name: &str,
    root: &str,
    folder: &str,
    extension: &str,
) -> Option<(String, String)> {
    let rest = name.strip_prefix(root)?.strip_prefix('/')?;
    let (namespace, rest) = rest.split_once('/')?;
    let rest = rest.strip_prefix(folder)?.strip_prefix('/')?;
    let path = rest.strip_suffix(extension)?;
    if namespace.is_empty() || path.is_empty() || path.ends_with('/') {
        return None;
    }
    Some((namespace.to_owned(), path.to_owned()))
}

fn read_language(
    archive: &mut Archive,
    entries: &[String],
    warnings: &mut Vec<String>,
) -> (BTreeMap<String, String>, usize) {
    let languages = collect_asset_entries(entries, "lang", ".json");
    let Some((_, entry)) = languages.iter().find(|(id, _)| id == "minecraft:en_us") else {
        push_warning(
            warnings,
            "the client JAR has no assets/minecraft/lang/en_us.json; display names are unavailable"
                .to_owned(),
        );
        return (BTreeMap::new(), languages.len());
    };
    let mut lang = BTreeMap::new();
    if let Some(Value::Object(document)) = read_json(archive, entry, warnings) {
        for (key, value) in document {
            if lang.len() >= MAX_CATALOG_LANG_ENTRIES {
                push_warning(
                    warnings,
                    format!(
                        "en_us has more than {MAX_CATALOG_LANG_ENTRIES} keys; the rest were ignored"
                    ),
                );
                break;
            }
            if let Some(value) = value.as_str() {
                lang.insert(key, value.to_owned());
            }
        }
    }
    (lang, languages.len())
}

fn block_display_name(lang: &BTreeMap<String, String>, id: &str) -> Option<String> {
    let (namespace, path) = id.split_once(':')?;
    lang.get(&format!("block.{namespace}.{path}")).cloned()
}

fn item_display_name(lang: &BTreeMap<String, String>, id: &str, has_block: bool) -> Option<String> {
    let (namespace, path) = id.split_once(':')?;
    lang.get(&format!("item.{namespace}.{path}"))
        .cloned()
        .or_else(|| {
            has_block
                .then(|| lang.get(&format!("block.{namespace}.{path}")).cloned())
                .flatten()
        })
}

fn parse_blockstate(document: &Value, entry: &str, warnings: &mut Vec<String>) -> BlockStateFacts {
    let mut facts = BlockStateFacts::default();
    if let Some(variants) = document.get("variants").and_then(Value::as_object) {
        let mut parsed_variants = 0usize;
        let mut declared_keys: Option<BTreeSet<String>> = None;
        let mut keys_consistent = true;
        for (key, value) in variants {
            facts.variant_count += 1;
            let properties = parse_state_key(key);
            match &properties {
                Some(properties) => {
                    parsed_variants += 1;
                    let keys: BTreeSet<String> = properties.keys().cloned().collect();
                    match &declared_keys {
                        None => declared_keys = Some(keys),
                        Some(expected) if *expected != keys => keys_consistent = false,
                        Some(_) => {}
                    }
                    if facts.variant_count == 1 {
                        facts.default_properties = Some(properties.clone());
                    }
                    merge_properties(&mut facts.properties, properties);
                }
                None => push_warning(warnings, format!("{entry}: invalid variant key `{key}`")),
            }
            let models = parse_variant_models(value);
            if facts.first_model.is_none() {
                facts.first_model = models.iter().find_map(|variant| variant.model.clone());
            }
            facts.variants.push(CatalogVariant {
                properties: properties.unwrap_or_default(),
                models,
            });
        }
        if facts.variant_count != 1 || parsed_variants != 1 {
            facts.default_properties = None;
        }
        facts.property_values_complete = parsed_variants > 0
            && parsed_variants == facts.variant_count
            && keys_consistent
            && declared_keys.as_ref().is_some_and(|keys| !keys.is_empty());
    } else if let Some(parts) = document.get("multipart").and_then(Value::as_array) {
        facts.multipart = true;
        for part in parts {
            if let Some(when) = part.get("when") {
                collect_conditions(when, &mut facts.properties, entry, warnings);
            }
            if facts.first_model.is_none() {
                facts.first_model = part.get("apply").and_then(first_model_ref);
            }
        }
    } else {
        push_warning(
            warnings,
            format!("{entry} has neither variants nor multipart"),
        );
    }
    facts
}

fn parse_state_key(key: &str) -> Option<BTreeMap<String, String>> {
    if key.is_empty() {
        return Some(BTreeMap::new());
    }
    let mut properties = BTreeMap::new();
    for part in key.split(',') {
        let (name, value) = part.split_once('=')?;
        if name.is_empty() || value.is_empty() || value.contains('=') {
            return None;
        }
        if !name.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '.' | '-')
        }) {
            return None;
        }
        properties.insert(name.to_owned(), value.to_owned());
    }
    Some(properties)
}

fn merge_properties(
    target: &mut BTreeMap<String, BTreeSet<String>>,
    properties: &BTreeMap<String, String>,
) {
    for (key, value) in properties {
        target.entry(key.clone()).or_default().insert(value.clone());
    }
}

fn collect_conditions(
    value: &Value,
    properties: &mut BTreeMap<String, BTreeSet<String>>,
    entry: &str,
    warnings: &mut Vec<String>,
) {
    let Some(condition) = value.as_object() else {
        push_warning(
            warnings,
            format!("{entry}: a multipart `when` value is not an object"),
        );
        return;
    };
    for (key, value) in condition {
        if key == "OR" || key == "AND" {
            if let Some(items) = value.as_array() {
                for item in items {
                    collect_conditions(item, properties, entry, warnings);
                }
            } else {
                push_warning(warnings, format!("{entry}: `{key}` is not an array"));
            }
            continue;
        }
        let values: Vec<String> = match value {
            Value::String(text) => text.split('|').map(ToOwned::to_owned).collect(),
            Value::Bool(flag) => vec![flag.to_string()],
            Value::Number(number) => vec![number.to_string()],
            _ => {
                push_warning(
                    warnings,
                    format!("{entry}: unsupported condition value for `{key}`"),
                );
                continue;
            }
        };
        for value in values {
            properties.entry(key.clone()).or_default().insert(value);
        }
    }
}

fn first_model_ref(value: &Value) -> Option<String> {
    match value {
        Value::String(model) => normalize_id(model),
        Value::Object(object) => object
            .get("model")
            .and_then(Value::as_str)
            .and_then(normalize_id),
        Value::Array(items) => items.iter().find_map(first_model_ref),
        _ => None,
    }
}

fn parse_variant_models(value: &Value) -> Vec<CatalogVariantModel> {
    match value {
        Value::Array(items) => items.iter().flat_map(parse_variant_models).collect(),
        Value::String(model) => vec![CatalogVariantModel {
            model: normalize_id(model),
            x_rotation: 0,
            y_rotation: 0,
            uvlock: false,
            weight: 1,
        }],
        Value::Object(object) => {
            let rotation = |key: &str| {
                object
                    .get(key)
                    .and_then(Value::as_i64)
                    .and_then(|value| i32::try_from(value).ok())
                    .unwrap_or(0)
            };
            vec![CatalogVariantModel {
                model: object
                    .get("model")
                    .and_then(Value::as_str)
                    .and_then(normalize_id),
                x_rotation: rotation("x"),
                y_rotation: rotation("y"),
                uvlock: object
                    .get("uvlock")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                weight: object
                    .get("weight")
                    .and_then(Value::as_u64)
                    .and_then(|value| u32::try_from(value).ok())
                    .unwrap_or(1),
            }]
        }
        _ => Vec::new(),
    }
}

fn item_definition_model(document: &Value) -> (Option<String>, Option<String>) {
    let model = document.get("model");
    let model_type = model
        .and_then(Value::as_object)
        .and_then(|object| object.get("type"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let model = model
        .and_then(item_model_ref)
        .or_else(|| document.get("parent").and_then(item_model_ref));
    (model, model_type)
}

/// Resolves the first concrete model referenced by a modern item definition. Definitions
/// may dispatch on a property (`select`, `range_dispatch`, `condition`), use a special
/// renderer with a `base` model, or compose several models. The first branch in declaration
/// order is the palette icon; the full definition remains available in the JAR.
fn item_model_ref(value: &Value) -> Option<String> {
    match value {
        Value::String(model) => normalize_id(model),
        Value::Array(items) => items.iter().find_map(item_model_ref),
        Value::Object(object) => {
            for key in ["model", "base", "on_false", "on_true"] {
                if let Some(model) = object.get(key).and_then(item_model_ref) {
                    return Some(model);
                }
            }
            for key in ["cases", "entries", "models"] {
                if let Some(model) = object.get(key).and_then(item_model_ref) {
                    return Some(model);
                }
            }
            None
        }
        _ => None,
    }
}

/// Wall coral fans use the item model of their base fan but are not items themselves.
/// Requiring the base fan block makes the rule explicit instead of matching every suffix.
fn is_wall_fan_variant(id: &str, blocks: &BTreeMap<String, BlockFacts>) -> bool {
    id.strip_suffix("_wall_fan")
        .is_some_and(|base| blocks.contains_key(&format!("{base}_fan")))
}

/// An item model that is an empty JSON object carries no geometry or texture. Vanilla uses
/// such a placeholder for air; it must not turn a block into an item.
fn model_has_content(
    archive: &mut Archive,
    models: &HashMap<String, String>,
    model_id: &str,
    cache: &mut HashMap<String, bool>,
    warnings: &mut Vec<String>,
) -> bool {
    if let Some(content) = cache.get(model_id) {
        return *content;
    }
    let content = models.get(model_id).is_some_and(|entry| {
        match read_json(archive, entry, warnings) {
            Some(Value::Object(object)) => !object.is_empty(),
            Some(_) => true,
            // A malformed model is reported by read_json; keep the id instead of silently
            // dropping a potentially real item.
            None => true,
        }
    });
    cache.insert(model_id.to_owned(), content);
    content
}

fn tags_for(tags: &BTreeMap<String, BTreeSet<String>>, id: &str) -> Vec<String> {
    tags.get(id)
        .map(|values| values.iter().cloned().collect())
        .unwrap_or_default()
}

fn parse_tag_folders(
    archive: &mut Archive,
    entries: &[String],
    folders: &[&str],
    warnings: &mut Vec<String>,
) -> (BTreeMap<String, BTreeSet<String>>, usize) {
    // Block/item tags are data-pack content. 1.21+ uses the singular `block`/`item`
    // directories; older releases use `blocks`/`items`. Only one layout exists in a real
    // JAR, but reading both keeps the extractor honest across the supported releases.
    let mut tag_files: BTreeMap<String, String> = BTreeMap::new();
    for folder in folders {
        for (tag_id, entry) in collect_data_entries(entries, folder, ".json") {
            tag_files.entry(tag_id).or_insert(entry);
        }
    }
    if tag_files.len() > MAX_CATALOG_TAGS {
        push_warning(
            warnings,
            format!(
                "{} tag files exceed the {MAX_CATALOG_TAGS} limit; the rest were ignored",
                folders.join("/")
            ),
        );
    }
    let mut by_entry: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (tag_id, entry) in tag_files.iter().take(MAX_CATALOG_TAGS) {
        let Some(document) = read_json(archive, entry, warnings) else {
            continue;
        };
        let Some(values) = document.get("values").and_then(Value::as_array) else {
            push_warning(warnings, format!("{entry} has no tag values array"));
            continue;
        };
        for value in values {
            let id = match value {
                Value::String(id) => Some(id.as_str()),
                Value::Object(object) => object.get("id").and_then(Value::as_str),
                _ => None,
            };
            let Some(id) = id else {
                continue;
            };
            if id.starts_with('#') {
                // Tag references are transitive and version-dependent. The catalog stores
                // direct membership only; the tag graph remains available in the JAR.
                continue;
            }
            if let Some(id) = normalize_id(id) {
                by_entry.entry(id).or_default().insert(tag_id.clone());
            }
        }
    }
    (by_entry, tag_files.len())
}

fn read_atlases(
    archive: &mut Archive,
    entries: &[String],
    warnings: &mut Vec<String>,
) -> (Vec<CatalogAtlas>, usize) {
    let atlas_files = collect_asset_entries(entries, "atlases", ".json");
    let mut atlases = Vec::new();
    let mut source_count = 0;
    for (id, entry) in atlas_files {
        let Some(document) = read_json(archive, &entry, warnings) else {
            continue;
        };
        let mut sources = Vec::new();
        if let Some(values) = document.get("sources").and_then(Value::as_array) {
            for value in values.iter().take(MAX_CATALOG_ATLAS_SOURCES) {
                let Some(object) = value.as_object() else {
                    continue;
                };
                let kind = object
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .to_owned();
                sources.push(CatalogAtlasSource {
                    kind,
                    source: string_field(object, "source"),
                    prefix: string_field(object, "prefix"),
                    resource: string_field(object, "resource"),
                });
                source_count += 1;
            }
        } else {
            push_warning(warnings, format!("{entry} has no atlas sources array"));
        }
        atlases.push(CatalogAtlas { id, sources });
    }
    (atlases, source_count)
}

fn string_field(object: &Map<String, Value>, key: &str) -> Option<String> {
    object
        .get(key)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

struct ModelResolver<'a> {
    archive: &'a mut Archive,
    models: &'a HashMap<String, String>,
    cache: HashMap<String, Option<BTreeMap<String, String>>>,
}

impl ModelResolver<'_> {
    fn icon_texture(
        &mut self,
        model: &str,
        textures: &BTreeSet<String>,
        warnings: &mut Vec<String>,
    ) -> Option<String> {
        self.icon_texture_inner(model, textures, warnings, true, true)
    }

    /// Item icons must not fall back to a model's `particle` texture: special items such as
    /// shields and heads use an entity renderer and only carry a particle placeholder.
    fn item_icon_texture(
        &mut self,
        model: &str,
        special: bool,
        textures: &BTreeSet<String>,
        warnings: &mut Vec<String>,
    ) -> Option<String> {
        self.icon_texture_inner(model, textures, warnings, false, !special)
    }

    fn icon_texture_inner(
        &mut self,
        model: &str,
        textures: &BTreeSet<String>,
        warnings: &mut Vec<String>,
        allow_particle: bool,
        warn_missing: bool,
    ) -> Option<String> {
        let merged = self.resolve(model, 0, &mut Vec::new(), warnings)?;
        const PREFERRED: [&str; 13] = [
            "layer0", "all", "particle", "top", "texture", "cross", "side", "still", "flow",
            "base", "end", "bottom", "front",
        ];
        for key in PREFERRED {
            if key == "particle" && !allow_particle {
                continue;
            }
            if let Some(value) = merged.get(key) {
                if let Some(texture) = resolve_texture_reference(value, &merged, 0) {
                    if textures.contains(&texture) {
                        return Some(texture);
                    }
                }
            }
        }
        for (key, value) in &merged {
            if key == "particle" && !allow_particle {
                continue;
            }
            if let Some(texture) = resolve_texture_reference(value, &merged, 0) {
                if textures.contains(&texture) {
                    return Some(texture);
                }
            }
        }
        if warn_missing {
            push_warning(
                warnings,
                format!("model {model} has no texture present in the client JAR"),
            );
        }
        None
    }

    fn resolve(
        &mut self,
        model: &str,
        depth: usize,
        chain: &mut Vec<String>,
        warnings: &mut Vec<String>,
    ) -> Option<BTreeMap<String, String>> {
        if depth > MAX_MODEL_DEPTH {
            push_warning(
                warnings,
                format!("model {model} exceeds the {MAX_MODEL_DEPTH}-level parent limit"),
            );
            return None;
        }
        if chain.iter().any(|candidate| candidate == model) {
            push_warning(warnings, format!("model parent cycle at {model}"));
            return None;
        }
        if let Some(cached) = self.cache.get(model).cloned() {
            return cached;
        }
        let entry = self.models.get(model)?.clone();
        let document = read_json(self.archive, &entry, warnings)?;
        if !document.is_object() {
            push_warning(warnings, format!("{entry} is not a JSON object"));
            return None;
        }
        chain.push(model.to_owned());
        let mut merged = BTreeMap::new();
        if let Some(parent) = document
            .get("parent")
            .and_then(Value::as_str)
            .and_then(normalize_id)
        {
            if let Some(parent_textures) = self.resolve(&parent, depth + 1, chain, warnings) {
                merged.extend(parent_textures);
            }
        }
        if let Some(textures) = document.get("textures").and_then(Value::as_object) {
            for (key, value) in textures {
                if let Some(value) = value.as_str() {
                    merged.insert(key.clone(), value.to_owned());
                }
            }
        }
        chain.pop();
        self.cache.insert(model.to_owned(), Some(merged.clone()));
        Some(merged)
    }
}

fn resolve_texture_reference(
    value: &str,
    textures: &BTreeMap<String, String>,
    depth: usize,
) -> Option<String> {
    if depth > MAX_MODEL_DEPTH {
        return None;
    }
    let value = value.trim();
    if let Some(reference) = value.strip_prefix('#') {
        let next = textures.get(reference)?;
        resolve_texture_reference(next, textures, depth + 1)
    } else {
        normalize_id(value)
    }
}

fn catalog_digest(
    version: &str,
    data_version: i32,
    sha1: Option<&str>,
    entries: &[CatalogEntry],
    atlases: &[CatalogAtlas],
) -> Result<(String, usize), CatalogError> {
    #[derive(Serialize)]
    struct Digest<'a> {
        schema_version: u32,
        minecraft_version: &'a str,
        data_version: i32,
        client_jar_sha1: Option<&'a str>,
        entries: &'a [CatalogEntry],
        atlases: &'a [CatalogAtlas],
    }
    let bytes = serde_json::to_vec(&Digest {
        schema_version: CATALOG_SCHEMA_VERSION,
        minecraft_version: version,
        data_version,
        client_jar_sha1: sha1,
        entries,
        atlases,
    })
    .map_err(|error| CatalogError::Io(error.to_string()))?;
    let bytes_len = bytes.len();
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut hex, "{byte:02x}");
    }
    Ok((hex, bytes_len))
}

fn safe_version_component(version: &str) -> String {
    version
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character
            } else {
                '-'
            }
        })
        .collect()
}

fn read_json(archive: &mut Archive, entry: &str, warnings: &mut Vec<String>) -> Option<Value> {
    let bytes = match archive.read_entry(entry, MAX_CATALOG_JSON_BYTES) {
        Ok(bytes) => bytes,
        Err(error) => {
            push_warning(warnings, format!("cannot read {entry}: {error}"));
            return None;
        }
    };
    match serde_json::from_slice(&bytes) {
        Ok(value) => Some(value),
        Err(error) => {
            push_warning(warnings, format!("{entry} is not valid JSON: {error}"));
            None
        }
    }
}

fn push_warning(warnings: &mut Vec<String>, message: String) {
    if warnings.len() < MAX_CATALOG_WARNINGS {
        warnings.push(message);
    } else if warnings.len() == MAX_CATALOG_WARNINGS {
        warnings.push(format!(
            "additional catalog warnings were suppressed after {MAX_CATALOG_WARNINGS}"
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};
    use zip::write::SimpleFileOptions;

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "furybuilder-catalog-{label}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|duration| duration.as_nanos())
                    .unwrap_or(0)
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("create temp directory");
            Self { path }
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn tiny_png() -> Vec<u8> {
        colored_png([255, 0, 255, 255])
    }

    fn colored_png(color: [u8; 4]) -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 16, 16);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().expect("png header");
            let mut pixels = vec![0u8; 16 * 16 * 4];
            for pixel in pixels.as_chunks_mut::<4>().0 {
                pixel.copy_from_slice(&color);
            }
            writer.write_image_data(&pixels).expect("png pixels");
        }
        bytes
    }

    fn write_pack(dir: &TempDir, label: &str, stone: &[u8]) -> PathBuf {
        let path = dir.path.join(format!("{label}.zip"));
        let file = fs::File::create(&path).expect("create pack");
        let mut zip = zip::ZipWriter::new(file);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        zip.start_file("pack.mcmeta", options).expect("pack meta");
        zip.write_all(br#"{"pack":{"pack_format":46,"description":"test pack"}}"#)
            .expect("write pack meta");
        zip.start_file("assets/minecraft/textures/block/stone.png", options)
            .expect("pack texture");
        zip.write_all(stone).expect("write pack texture");
        zip.finish().expect("finish pack");
        path
    }

    fn atlas_pixel(
        atlas: &crate::CatalogIconAtlas,
        texture: &crate::CatalogAtlasTexture,
        x: u32,
        y: u32,
    ) -> [u8; 4] {
        use base64::Engine as _;
        let png = base64::engine::general_purpose::STANDARD
            .decode(&atlas.png_base64)
            .expect("atlas base64");
        let decoder = png::Decoder::new(std::io::Cursor::new(png));
        let mut reader = decoder.read_info().expect("atlas png");
        let mut buffer = vec![0u8; reader.output_buffer_size()];
        let frame = reader.next_frame(&mut buffer).expect("atlas frame");
        let index = (((texture.y + y) * atlas.width + texture.x + x) * 4) as usize;
        let mut pixel = [0u8; 4];
        pixel.copy_from_slice(&buffer[index..index + 4]);
        assert_eq!(frame.color_type, png::ColorType::Rgba);
        pixel
    }

    fn version_json() -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "id": "1.21.4",
            "world_version": 4189,
            "pack_version": { "resource": 46, "data": 61 },
            "stable": true
        }))
        .expect("version json")
    }

    #[test]
    fn ten_thousand_entries_fit_the_catalog_memory_budget() {
        let entries: Vec<CatalogEntry> = (0..10_000)
            .map(|index| CatalogEntry {
                id: format!("example:block_{index}"),
                legacy_id: None,
                display_name: Some(format!("Test Block {index}")),
                category: None,
                tags: vec!["example:generated".to_owned()],
                icon_texture: Some(format!("example:block/icon_{}", index % 256)),
                aliases: Vec::new(),
                states: Some(CatalogStates {
                    properties: BTreeMap::new(),
                    property_values_complete: true,
                    default_properties: None,
                    variant_count: 1,
                    variants: Vec::new(),
                    multipart: false,
                    model: Some(format!("example:block/block_{index}")),
                }),
                item_form: Some(CatalogItemForm {
                    item_definition: true,
                    model_type: Some("minecraft:model".to_owned()),
                    model: Some(format!("example:block/block_{index}")),
                }),
                flags: CatalogFlags {
                    placeable: Some(true),
                    has_item: Some(true),
                    ..CatalogFlags::default()
                },
            })
            .collect();
        let (_, bytes) =
            catalog_digest("1.21.4", 4189, Some("sha1"), &entries, &[]).expect("catalog digest");
        assert!(
            bytes < MAX_CATALOG_MEMORY_BYTES,
            "10k-entry catalog uses {bytes} bytes, over the {MAX_CATALOG_MEMORY_BYTES}-byte budget"
        );
    }

    #[test]
    fn icon_texture_keys_are_deduplicated_per_catalog() {
        let mut catalog = CatalogSnapshot {
            schema_version: CATALOG_SCHEMA_VERSION,
            catalog_id: "java-1.21.4-test".to_owned(),
            catalog_hash: "hash".to_owned(),
            minecraft_version: "1.21.4".to_owned(),
            data_version: 4189,
            client_jar: "client.jar".to_owned(),
            client_jar_sha1: Some("sha1".to_owned()),
            resource_pack_format: Some(46),
            datapack_format: Some(61),
            entries: Vec::new(),
            atlases: Vec::new(),
            counts: Default::default(),
            warnings: Vec::new(),
            cache_hit: false,
        };
        catalog.entries = vec![
            CatalogEntry {
                id: "minecraft:stone".to_owned(),
                legacy_id: None,
                display_name: None,
                category: None,
                tags: Vec::new(),
                icon_texture: Some("minecraft:block/stone".to_owned()),
                aliases: Vec::new(),
                states: None,
                item_form: None,
                flags: CatalogFlags::default(),
            },
            CatalogEntry {
                id: "minecraft:smooth_stone".to_owned(),
                legacy_id: None,
                display_name: None,
                category: None,
                tags: Vec::new(),
                icon_texture: Some("minecraft:block/stone".to_owned()),
                aliases: Vec::new(),
                states: None,
                item_form: None,
                flags: CatalogFlags::default(),
            },
        ];
        assert_eq!(
            catalog.icon_texture_keys(),
            BTreeSet::from(["minecraft:block/stone".to_owned()])
        );
    }

    fn write_jar(dir: &TempDir) -> PathBuf {
        let path = dir.path.join("client.jar");
        let file = fs::File::create(&path).expect("create jar");
        let mut zip = zip::ZipWriter::new(file);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let png = tiny_png();
        let text = |zip: &mut zip::ZipWriter<fs::File>, name: &str, value: &str| {
            zip.start_file(name, options).expect("start entry");
            zip.write_all(value.as_bytes()).expect("write entry");
        };
        let texture = |zip: &mut zip::ZipWriter<fs::File>, name: &str| {
            zip.start_file(name, options).expect("start texture");
            zip.write_all(&png).expect("write texture");
        };

        zip.start_file("version.json", options)
            .expect("version entry");
        zip.write_all(&version_json()).expect("write version.json");
        text(
            &mut zip,
            "assets/minecraft/blockstates/stone.json",
            r#"{"variants":{"":{"model":"minecraft:block/stone"}}}"#,
        );
        text(
            &mut zip,
            "assets/minecraft/blockstates/oak_stairs.json",
            r#"{"variants":{
                "facing=east,half=bottom,shape=straight,waterlogged=false":{"model":"minecraft:block/oak_stairs"},
                "facing=north,half=top,shape=outer_left,waterlogged=true":{"model":"minecraft:block/oak_stairs", "y":90}
            }}"#,
        );
        text(
            &mut zip,
            "assets/minecraft/blockstates/oak_fence.json",
            r#"{"multipart":[
                {"when":{"north":"true"},"apply":{"model":"minecraft:block/oak_fence_post"}},
                {"apply":{"model":"minecraft:block/oak_fence_post"}}
            ]}"#,
        );
        text(
            &mut zip,
            "assets/minecraft/blockstates/water.json",
            r#"{"variants":{"":{"model":"minecraft:block/water"}}}"#,
        );
        text(
            &mut zip,
            "assets/example/blockstates/ruby_block.json",
            r#"{"variants":{"":{"model":"example:block/ruby_block"}}}"#,
        );
        text(
            &mut zip,
            "assets/minecraft/models/block/stone.json",
            r#"{"parent":"minecraft:block/cube_all","textures":{"all":"minecraft:block/stone"}}"#,
        );
        text(
            &mut zip,
            "assets/minecraft/models/block/cube_all.json",
            r##"{"textures":{"particle":"#all"}}"##,
        );
        text(
            &mut zip,
            "assets/minecraft/models/block/oak_stairs.json",
            r#"{"parent":"minecraft:block/stairs","textures":{"bottom":"minecraft:block/oak_planks","top":"minecraft:block/oak_planks","side":"minecraft:block/oak_planks"}}"#,
        );
        text(
            &mut zip,
            "assets/minecraft/models/block/stairs.json",
            r##"{"textures":{"particle":"#side"}}"##,
        );
        text(
            &mut zip,
            "assets/minecraft/models/block/oak_fence_post.json",
            r#"{"textures":{"texture":"minecraft:block/oak_planks"}}"#,
        );
        text(
            &mut zip,
            "assets/minecraft/models/block/water.json",
            r#"{"textures":{"particle":"minecraft:block/water_still"}}"#,
        );
        text(
            &mut zip,
            "assets/example/models/block/ruby_block.json",
            r#"{"textures":{"all":"example:block/ruby_block"}}"#,
        );
        text(
            &mut zip,
            "assets/minecraft/items/stone.json",
            r#"{"model":{"type":"minecraft:model","model":"minecraft:block/stone"}}"#,
        );
        text(
            &mut zip,
            "assets/minecraft/items/oak_stairs.json",
            r#"{"model":{"type":"minecraft:model","model":"minecraft:block/oak_stairs"}}"#,
        );
        text(
            &mut zip,
            "assets/minecraft/items/oak_fence.json",
            r#"{"model":{"type":"minecraft:model","model":"minecraft:block/oak_fence_post"}}"#,
        );
        text(
            &mut zip,
            "assets/minecraft/items/diamond_sword.json",
            r#"{"model":{"type":"minecraft:model","model":"minecraft:item/diamond_sword"}}"#,
        );
        text(
            &mut zip,
            "assets/minecraft/models/item/diamond_sword.json",
            r#"{"parent":"minecraft:item/handheld","textures":{"layer0":"minecraft:item/diamond_sword"}}"#,
        );
        text(
            &mut zip,
            "assets/minecraft/models/item/handheld.json",
            r#"{"textures":{"layer0":"minecraft:item/handheld"}}"#,
        );
        texture(&mut zip, "assets/minecraft/textures/block/stone.png");
        texture(&mut zip, "assets/minecraft/textures/block/oak_planks.png");
        texture(&mut zip, "assets/minecraft/textures/block/water_still.png");
        texture(&mut zip, "assets/minecraft/textures/item/diamond_sword.png");
        texture(&mut zip, "assets/example/textures/block/ruby_block.png");
        text(
            &mut zip,
            "assets/minecraft/lang/en_us.json",
            r#"{
                "block.minecraft.stone":"Stone",
                "block.minecraft.oak_stairs":"Oak Stairs",
                "block.minecraft.oak_fence":"Oak Fence",
                "block.minecraft.water":"Water",
                "block.example.ruby_block":"Ruby Block",
                "item.minecraft.diamond_sword":"Diamond Sword"
            }"#,
        );
        text(
            &mut zip,
            "assets/minecraft/lang/ru_ru.json",
            r#"{"block.minecraft.stone":"Камень"}"#,
        );
        text(
            &mut zip,
            "data/minecraft/tags/block/mineable/pickaxe.json",
            r#"{"values":["minecraft:stone","minecraft:oak_stairs","example:ruby_block"]}"#,
        );
        text(
            &mut zip,
            "data/minecraft/tags/item/swords.json",
            r#"{"values":["minecraft:diamond_sword"]}"#,
        );
        text(
            &mut zip,
            "data/minecraft/tags/item/stone_tool_materials.json",
            r#"{"values":["minecraft:stone"]}"#,
        );
        text(
            &mut zip,
            "assets/minecraft/atlases/blocks.json",
            r#"{"sources":[
                {"type":"directory","source":"block","prefix":"block/"},
                {"type":"directory","source":"item","prefix":"item/"}
            ]}"#,
        );
        zip.finish().expect("finish jar");
        path
    }

    fn source(path: &Path) -> AssetSourceSetting {
        AssetSourceSetting {
            kind: "client_jar".to_owned(),
            path: path.display().to_string(),
        }
    }

    /// Checks the invariants every built catalog must satisfy, independent of the fixture.
    fn assert_catalog_contract(catalog: &CatalogSnapshot) {
        assert_eq!(catalog.schema_version, CATALOG_SCHEMA_VERSION);
        assert_eq!(catalog.catalog_hash.len(), 64);
        assert!(catalog
            .catalog_hash
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit()));

        let mut previous: Option<&str> = None;
        let mut blocks = 0usize;
        let mut items = 0usize;
        let mut blocks_with_items = 0usize;
        for entry in &catalog.entries {
            if let Some(previous) = previous {
                assert!(
                    previous < entry.id.as_str(),
                    "catalog entries must be sorted and unique: {previous} then {}",
                    entry.id
                );
            }
            previous = Some(entry.id.as_str());
            assert!(
                crate::bundle::valid_id(&entry.id),
                "invalid catalog id {}",
                entry.id
            );
            assert!(
                entry.is_block() || entry.is_item(),
                "{} has neither a block nor an item form",
                entry.id
            );
            assert_eq!(
                entry.flags.placeable,
                Some(entry.is_block()),
                "{} placeable flag must follow its block form",
                entry.id
            );
            assert_eq!(
                entry.flags.has_item,
                Some(entry.is_item()),
                "{} hasItem flag must follow its item form",
                entry.id
            );

            let mut tags = entry.tags.clone();
            tags.sort();
            tags.dedup();
            assert_eq!(
                tags, entry.tags,
                "{} tags must be sorted and unique",
                entry.id
            );
            assert!(
                entry.tags.iter().all(|tag| crate::bundle::valid_id(tag)),
                "{} has an invalid tag",
                entry.id
            );

            if let Some(states) = &entry.states {
                blocks += 1;
                assert!(
                    !states.multipart || states.variants.is_empty(),
                    "{} multipart states cannot have a finite variant list",
                    entry.id
                );
                assert_eq!(
                    states.variant_count,
                    states.variants.len(),
                    "{} variantCount must match its variant list",
                    entry.id
                );
                for (key, values) in &states.properties {
                    assert!(!key.is_empty(), "{} has an empty property key", entry.id);
                    assert!(
                        !values.is_empty(),
                        "{} property {key} has no declared values",
                        entry.id
                    );
                    assert!(
                        values.windows(2).all(|window| window[0] < window[1]),
                        "{} property {key} values must be sorted and unique",
                        entry.id
                    );
                }
                for variant in &states.variants {
                    for (key, value) in &variant.properties {
                        assert!(
                            states
                                .properties
                                .get(key)
                                .is_some_and(|values| values.contains(value)),
                            "{} variant uses undeclared {key}={value}",
                            entry.id
                        );
                    }
                }
                if let Some(default) = &states.default_properties {
                    assert_eq!(
                        states.variant_count, 1,
                        "{} defaultProperties require exactly one declared variant",
                        entry.id
                    );
                    assert_eq!(
                        Some(default),
                        states.variants.first().map(|variant| &variant.properties),
                        "{} defaultProperties must equal the single declared variant",
                        entry.id
                    );
                    for (key, value) in default {
                        assert!(
                            states
                                .properties
                                .get(key)
                                .is_some_and(|values| values.contains(value)),
                            "{} default uses undeclared {key}={value}",
                            entry.id
                        );
                    }
                }
                if states.property_values_complete {
                    let declared: Vec<&String> = states.properties.keys().collect();
                    for variant in &states.variants {
                        assert_eq!(
                            variant.properties.keys().collect::<Vec<_>>(),
                            declared,
                            "{} complete state must declare the same keys in every variant",
                            entry.id
                        );
                    }
                }
            }

            if entry.item_form.is_some() {
                items += 1;
                if entry.is_block() {
                    blocks_with_items += 1;
                }
            }
        }

        assert_eq!(catalog.counts.blocks, blocks);
        assert_eq!(catalog.counts.items, items);
        assert_eq!(catalog.counts.blocks_with_items, blocks_with_items);
        assert_eq!(catalog.counts.item_only, items - blocks_with_items);
    }

    #[test]
    fn catalog_entries_are_valid_sorted_and_counted() {
        let dir = TempDir::new("contract");
        let path = write_jar(&dir);
        let catalog = AssetCatalogService::new()
            .catalog(&source(&path))
            .expect("catalog");

        assert_catalog_contract(&catalog);
        assert_eq!(
            catalog
                .entries
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            vec![
                "example:ruby_block",
                "minecraft:diamond_sword",
                "minecraft:oak_fence",
                "minecraft:oak_stairs",
                "minecraft:stone",
                "minecraft:water",
            ]
        );
    }

    fn json_bytes(value: Value) -> Vec<u8> {
        serde_json::to_vec(&value).expect("serialize test JSON")
    }

    fn write_entries_jar(dir: &TempDir, label: &str, entries: &[(&str, Vec<u8>)]) -> PathBuf {
        write_entries_jar_in(&dir.path, label, entries)
    }

    fn write_launcher_metadata(jar: &Path, id: &str) {
        let sha1 = crate::jar::file_sha1(jar).expect("jar sha1");
        let size = fs::metadata(jar).expect("jar metadata").len();
        let path = jar.with_extension("json");
        fs::write(
            path,
            json_bytes(serde_json::json!({
                "id": id,
                "type": "release",
                "downloads": {
                    "client": {
                        "sha1": sha1,
                        "size": size,
                        "url": "https://piston-data.mojang.com/v1/objects/client.jar"
                    }
                }
            })),
        )
        .expect("write launcher metadata");
    }

    fn write_minimal_jar(dir: &TempDir, label: &str, files: &[(&str, &str)]) -> PathBuf {
        let path = dir.path.join(format!("{label}.jar"));
        let file = fs::File::create(&path).expect("create jar");
        let mut zip = zip::ZipWriter::new(file);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        zip.start_file("version.json", options)
            .expect("version entry");
        zip.write_all(&version_json()).expect("write version.json");
        zip.start_file("assets/minecraft/blockstates/stone.json", options)
            .expect("blockstate entry");
        zip.write_all(br#"{"variants":{"":{"model":"minecraft:block/stone"}}}"#)
            .expect("write blockstate");
        for (name, text) in files {
            zip.start_file(*name, options).expect("start file");
            zip.write_all(text.as_bytes()).expect("write file");
        }
        for name in [
            "assets/minecraft/textures/item/clock_00.png",
            "assets/minecraft/textures/block/soul_sand.png",
        ] {
            zip.start_file(name, options).expect("start texture");
            zip.write_all(&tiny_png()).expect("write texture");
        }
        zip.finish().expect("finish jar");
        path
    }

    #[test]
    fn catalog_is_built_from_the_client_jar() {
        let dir = TempDir::new("baseline");
        let path = write_jar(&dir);
        let service = AssetCatalogService::new();
        let catalog = service.catalog(&source(&path)).expect("catalog");

        assert_eq!(catalog.minecraft_version, "1.21.4");
        assert_eq!(catalog.data_version, 4189);
        assert_eq!(catalog.resource_pack_format, Some(46));
        assert_eq!(catalog.datapack_format, Some(61));
        assert_eq!(catalog.schema_version, CATALOG_SCHEMA_VERSION);
        assert_eq!(catalog.catalog_hash.len(), 64);
        assert!(catalog
            .catalog_hash
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit()));
        assert!(catalog.catalog_id.starts_with("java-1.21.4-"));
        assert_eq!(catalog.entries.len(), 6);
        assert_eq!(catalog.atlases.len(), 1);
        assert_eq!(catalog.atlases[0].id, "minecraft:blocks");
        assert_eq!(catalog.atlases[0].sources.len(), 2);
        assert_eq!(catalog.atlases[0].sources[0].kind, "directory");
        assert_eq!(
            catalog.atlases[0].sources[0].source.as_deref(),
            Some("block")
        );
        assert_eq!(
            catalog.atlases[0].sources[0].prefix.as_deref(),
            Some("block/")
        );
        let serialized = serde_json::to_value(&catalog).expect("catalog json");
        assert!(serialized.get("entries").is_some());
        assert!(serialized.get("blocks").is_none());
        assert!(serialized.get("items").is_none());

        assert_eq!(catalog.counts.blocks, 5);
        assert_eq!(catalog.counts.items, 4);
        assert_eq!(catalog.counts.blocks_with_items, 3);
        assert_eq!(catalog.counts.item_only, 1);
        assert_eq!(catalog.counts.blockstate_files, 5);
        assert_eq!(catalog.counts.item_definitions, 4);
        assert_eq!(catalog.counts.block_models, 7);
        assert_eq!(catalog.counts.item_models, 2);
        assert_eq!(catalog.counts.textures, 5);
        assert_eq!(catalog.counts.languages, 2);
        assert_eq!(catalog.counts.lang_entries, 6);
        assert_eq!(catalog.counts.block_tags, 1);
        assert_eq!(catalog.counts.item_tags, 2);
        assert_eq!(catalog.counts.atlas_sources, 2);
        assert!(!catalog.cache_hit);

        let stone = catalog
            .entries
            .iter()
            .find(|entry| entry.id == "minecraft:stone")
            .expect("stone");
        let stone_states = stone.states.as_ref().expect("stone states");
        assert_eq!(stone.display_name.as_deref(), Some("Stone"));
        assert!(stone.legacy_id.is_none());
        assert!(stone.category.is_none());
        assert!(stone.aliases.is_empty());
        assert!(stone_states.properties.is_empty());
        assert!(!stone_states.property_values_complete);
        assert_eq!(stone_states.default_properties, Some(BTreeMap::new()));
        assert_eq!(stone_states.variant_count, 1);
        assert_eq!(stone_states.variants.len(), 1);
        assert!(stone_states.variants[0].properties.is_empty());
        assert_eq!(
            stone_states.variants[0].models[0].model.as_deref(),
            Some("minecraft:block/stone")
        );
        assert!(!stone_states.multipart);
        assert_eq!(stone_states.model.as_deref(), Some("minecraft:block/stone"));
        assert_eq!(stone.icon_texture.as_deref(), Some("minecraft:block/stone"));
        assert_eq!(
            stone.tags,
            vec![
                "minecraft:mineable/pickaxe",
                "minecraft:stone_tool_materials"
            ]
        );
        assert!(stone.item_form.is_some());
        assert_eq!(stone.flags.placeable, Some(true));
        assert_eq!(stone.flags.has_item, Some(true));
        assert_eq!(stone.flags.has_variants, Some(false));
        assert_eq!(stone.flags.obtainable, Some(true));
        assert_eq!(stone.flags.creative_only, Some(false));
        assert_eq!(stone.flags.technical, Some(false));
        assert_eq!(stone.flags.fluid, Some(false));
        assert_eq!(stone.flags.airlike, Some(false));
        assert_eq!(stone.flags.block_entity, Some(false));

        let stairs = catalog
            .entries
            .iter()
            .find(|entry| entry.id == "minecraft:oak_stairs")
            .expect("stairs");
        let stairs_states = stairs.states.as_ref().expect("stairs states");
        assert_eq!(
            stairs_states.properties["facing"]
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["east", "north"]
        );
        assert_eq!(
            stairs_states.properties["waterlogged"]
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["false", "true"]
        );
        assert!(stairs_states.property_values_complete);
        assert!(stairs_states.default_properties.is_none());
        assert_eq!(stairs_states.variant_count, 2);
        assert_eq!(stairs_states.variants.len(), 2);
        let north = stairs_states
            .variants
            .iter()
            .find(|variant| variant.properties.get("facing").map(String::as_str) == Some("north"))
            .expect("north variant");
        assert_eq!(north.models[0].y_rotation, 90);
        assert_eq!(
            north.models[0].model.as_deref(),
            Some("minecraft:block/oak_stairs")
        );
        assert_eq!(stairs.flags.has_variants, Some(true));
        assert_eq!(
            stairs.icon_texture.as_deref(),
            Some("minecraft:block/oak_planks")
        );

        let fence = catalog
            .entries
            .iter()
            .find(|entry| entry.id == "minecraft:oak_fence")
            .expect("fence");
        let fence_states = fence.states.as_ref().expect("fence states");
        assert!(fence_states.multipart);
        assert_eq!(fence_states.variant_count, 0);
        assert!(fence_states.variants.is_empty());
        assert!(fence_states.default_properties.is_none());
        assert!(!fence_states.property_values_complete);
        assert_eq!(fence.flags.has_variants, Some(true));
        assert_eq!(
            fence_states.properties["north"]
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["true"]
        );

        let water = catalog
            .entries
            .iter()
            .find(|entry| entry.id == "minecraft:water")
            .expect("water");
        let water_states = water.states.as_ref().expect("water states");
        assert!(water.item_form.is_none());
        assert_eq!(water.flags.has_item, Some(false));
        assert_eq!(water.flags.fluid, Some(true));
        assert_eq!(water.flags.technical, Some(false));
        assert_eq!(water.flags.obtainable, Some(false));
        assert!(water_states.properties.is_empty());
        assert!(!water_states.property_values_complete);
        assert_eq!(water_states.default_properties, Some(BTreeMap::new()));

        let sword = catalog
            .entries
            .iter()
            .find(|entry| entry.id == "minecraft:diamond_sword")
            .expect("sword");
        let sword_form = sword.item_form.as_ref().expect("sword item form");
        assert_eq!(sword.display_name.as_deref(), Some("Diamond Sword"));
        assert!(sword_form.item_definition);
        assert!(sword.states.is_none());
        assert_eq!(sword.flags.placeable, Some(false));
        assert_eq!(sword.flags.has_item, Some(true));
        assert_eq!(sword.flags.has_variants, Some(false));
        assert_eq!(sword.flags.obtainable, Some(true));
        assert_eq!(sword.flags.creative_only, Some(false));
        assert_eq!(sword.flags.technical, Some(false));
        assert_eq!(sword_form.model_type.as_deref(), Some("minecraft:model"));
        assert_eq!(
            sword_form.model.as_deref(),
            Some("minecraft:item/diamond_sword")
        );
        assert_eq!(
            sword.icon_texture.as_deref(),
            Some("minecraft:item/diamond_sword")
        );
        assert_eq!(sword.tags, vec!["minecraft:swords"]);

        let ruby = catalog
            .entries
            .iter()
            .find(|entry| entry.id == "example:ruby_block")
            .expect("ruby");
        assert_eq!(ruby.display_name.as_deref(), Some("Ruby Block"));
        assert!(ruby.states.is_some());
        assert!(ruby.item_form.is_none());
        assert_eq!(ruby.flags.technical, None);
        assert_eq!(ruby.flags.obtainable, None);
        assert_eq!(ruby.flags.block_entity, None);
        assert!(catalog
            .warnings
            .iter()
            .any(|warning| warning.contains("non-vanilla namespaces")));
    }

    #[test]
    fn repeated_catalog_requests_hit_the_cache_and_tampering_rebuilds() {
        let dir = TempDir::new("cache");
        let path = write_jar(&dir);
        let service = AssetCatalogService::new();
        let first = service.catalog(&source(&path)).expect("first");
        let second = service.catalog(&source(&path)).expect("second");
        assert!(!first.cache_hit);
        assert!(second.cache_hit);
        assert_eq!(first.catalog_hash, second.catalog_hash);

        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open jar");
        file.write_all(b"extra").expect("append");
        drop(file);
        let third = service.catalog(&source(&path)).expect("third");
        assert!(!third.cache_hit);

        // A replacement can preserve both the byte length and the timestamp. It still
        // needs a different catalog identity, since its source checksum changed.
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        let mut bytes = fs::read(&path).unwrap();
        *bytes.last_mut().unwrap() = b'!';
        fs::write(&path, bytes).unwrap();
        fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(modified)
            .unwrap();
        let fourth = service.catalog(&source(&path)).expect("fourth");
        assert!(!fourth.cache_hit);
        assert_ne!(third.client_jar_sha1, fourth.client_jar_sha1);
        assert_ne!(third.catalog_hash, fourth.catalog_hash);
    }

    #[test]
    fn catalog_cache_key_is_content_based_and_tracks_the_current_path() {
        let dir = TempDir::new("cache-key");
        let path = write_jar(&dir);
        let service = AssetCatalogService::new();
        let first = service.catalog(&source(&path)).expect("first");
        let builder = CatalogBuilder::open(&source(&path)).expect("builder");
        assert_eq!(
            builder.cache_key().expect("cache key"),
            format!(
                "{}|{}|{}",
                first.minecraft_version,
                first.client_jar_sha1.clone().expect("sha1"),
                CATALOG_SCHEMA_VERSION
            )
        );

        let copy = dir.path.join("copy.jar");
        fs::copy(&path, &copy).expect("copy jar");
        let copied = service.catalog(&source(&copy)).expect("copy catalog");
        assert!(copied.cache_hit);
        assert_eq!(copied.client_jar, copy.display().to_string());
        assert_eq!(copied.catalog_id, first.catalog_id);
        assert_eq!(copied.catalog_hash, first.catalog_hash);
    }

    #[test]
    fn a_direct_client_jar_selects_its_own_exact_version() {
        let profile = minecraft_version::builtin_profiles()
            .iter()
            .find(|profile| profile.minecraft_version.as_str() == "1.18.2")
            .expect("1.18.2 profile");
        let dir = TempDir::new("direct-cross-version");
        let path = dir.path.join("client-1.18.2.jar");
        let file = fs::File::create(&path).expect("create jar");
        let mut zip = zip::ZipWriter::new(file);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let version = serde_json::to_vec(&serde_json::json!({
            "id": profile.minecraft_version.as_str(),
            "world_version": profile.data_version,
            "pack_version": {
                "resource": profile.resource_pack_format.unwrap_or(0),
                "data": profile.datapack_format.unwrap_or(0),
            },
        }))
        .expect("version json");
        zip.start_file("version.json", options)
            .expect("version entry");
        zip.write_all(&version).expect("write version.json");
        zip.start_file("assets/minecraft/blockstates/stone.json", options)
            .expect("blockstate entry");
        zip.write_all(br#"{"variants":{"":{"model":"minecraft:block/stone"}}}"#)
            .expect("write blockstate");
        zip.finish().expect("finish jar");

        let catalog = AssetCatalogService::new()
            .catalog(&source(&path))
            .expect("catalog");
        assert_eq!(catalog.minecraft_version, "1.18.2");
        assert_eq!(catalog.data_version, profile.data_version);
        assert!(catalog.catalog_id.starts_with("java-1.18.2-"));
        assert!(catalog
            .warnings
            .iter()
            .any(|warning| warning.contains("SHA-1 was not verified")));
    }

    #[test]
    fn a_resource_pack_is_not_a_catalog_source() {
        let error = resolve_client_jar(&AssetSourceSetting {
            kind: "resource_pack".to_owned(),
            path: "pack.zip".to_owned(),
        })
        .expect_err("resource pack");
        assert!(matches!(error, CatalogError::InvalidSource(_)));
        assert_eq!(error.code(), "invalid_argument");
    }

    #[test]
    fn a_jar_without_blockstates_is_refused() {
        let dir = TempDir::new("no-blockstates");
        let path = dir.path.join("client.jar");
        let file = fs::File::create(&path).expect("jar");
        let mut zip = zip::ZipWriter::new(file);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        zip.start_file("version.json", options).expect("version");
        zip.write_all(&version_json()).expect("version bytes");
        zip.start_file("assets/minecraft/lang/en_us.json", options)
            .expect("lang");
        zip.write_all(b"{}").expect("lang bytes");
        zip.finish().expect("finish");
        let error = AssetCatalogService::new()
            .build(&source(&path))
            .expect_err("no catalog");
        assert!(matches!(error, CatalogError::Unsupported(_)), "{error:?}");
    }

    #[test]
    fn malformed_blockstates_warn_without_dropping_the_block_id() {
        let dir = TempDir::new("malformed");
        let path = dir.path.join("client.jar");
        let file = fs::File::create(&path).expect("jar");
        let mut zip = zip::ZipWriter::new(file);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        zip.start_file("version.json", options).expect("version");
        zip.write_all(&version_json()).expect("version bytes");
        zip.start_file("assets/minecraft/blockstates/stone.json", options)
            .expect("blockstate");
        zip.write_all(br#"{"variants":{"":{"model":"minecraft:block/stone"}}}"#)
            .expect("blockstate bytes");
        zip.start_file("assets/minecraft/blockstates/broken.json", options)
            .expect("broken");
        zip.write_all(b"{").expect("broken bytes");
        zip.finish().expect("finish");
        let catalog = AssetCatalogService::new()
            .build(&source(&path))
            .expect("catalog");
        assert!(catalog
            .entries
            .iter()
            .any(|entry| entry.id == "minecraft:stone"));
        assert!(catalog
            .entries
            .iter()
            .any(|entry| entry.id == "minecraft:broken"));
        assert!(catalog
            .warnings
            .iter()
            .any(|warning| warning.contains("is not valid JSON")));
    }

    #[test]
    fn tags_are_read_from_data_pack_singular_folders() {
        let dir = TempDir::new("data-tags");
        let path = write_minimal_jar(
            &dir,
            "data-tags",
            &[
                (
                    "data/minecraft/tags/block/mineable/pickaxe.json",
                    r##"{"values":["minecraft:stone","#minecraft:needs_stone_tool"]}"##,
                ),
                (
                    "data/minecraft/tags/item/tools.json",
                    r#"{"values":["minecraft:stone"]}"#,
                ),
            ],
        );
        let catalog = AssetCatalogService::new()
            .build(&source(&path))
            .expect("catalog");
        assert_eq!(catalog.counts.block_tags, 1);
        assert_eq!(catalog.counts.item_tags, 1);
        let stone = catalog
            .entries
            .iter()
            .find(|entry| entry.id == "minecraft:stone")
            .expect("stone");
        assert_eq!(stone.tags, vec!["minecraft:mineable/pickaxe"]);
        assert!(stone.item_form.is_none());
        assert_eq!(stone.flags.has_item, Some(false));
        assert!(catalog
            .warnings
            .iter()
            .all(|warning| !warning.contains("tags/block")));
    }

    #[test]
    fn tags_are_read_from_legacy_plural_data_folders() {
        let dir = TempDir::new("legacy-tags");
        let path = write_minimal_jar(
            &dir,
            "legacy-tags",
            &[
                (
                    "data/minecraft/tags/blocks/mineable/pickaxe.json",
                    r#"{"values":["minecraft:stone"]}"#,
                ),
                (
                    "data/minecraft/tags/items/tools.json",
                    r#"{"values":["minecraft:stone"]}"#,
                ),
            ],
        );
        let catalog = AssetCatalogService::new()
            .build(&source(&path))
            .expect("catalog");
        assert_eq!(catalog.counts.block_tags, 1);
        assert_eq!(catalog.counts.item_tags, 1);
        let stone = catalog
            .entries
            .iter()
            .find(|entry| entry.id == "minecraft:stone")
            .expect("stone");
        assert_eq!(stone.tags, vec!["minecraft:mineable/pickaxe"]);
    }

    #[test]
    fn item_models_are_the_fallback_when_item_definitions_are_absent() {
        let dir = TempDir::new("item-models");
        let path = write_minimal_jar(
            &dir,
            "item-models",
            &[
                (
                    "assets/minecraft/models/item/diamond_sword.json",
                    r#"{"parent":"minecraft:item/handheld","textures":{"layer0":"minecraft:item/diamond_sword"}}"#,
                ),
                (
                    "assets/minecraft/models/item/handheld.json",
                    r#"{"textures":{"layer0":"minecraft:item/handheld"}}"#,
                ),
                (
                    "assets/minecraft/lang/en_us.json",
                    r#"{"item.minecraft.diamond_sword":"Diamond Sword"}"#,
                ),
                (
                    "data/minecraft/tags/item/swords.json",
                    r#"{"values":["minecraft:diamond_sword"]}"#,
                ),
            ],
        );
        let catalog = AssetCatalogService::new()
            .build(&source(&path))
            .expect("catalog");
        assert_eq!(catalog.counts.item_definitions, 0);
        let sword = catalog
            .entries
            .iter()
            .find(|entry| entry.id == "minecraft:diamond_sword")
            .expect("sword");
        let sword_form = sword.item_form.as_ref().expect("sword item form");
        assert!(!sword_form.item_definition);
        assert!(sword.states.is_none());
        assert!(sword_form.model_type.is_none());
        assert_eq!(sword.display_name.as_deref(), Some("Diamond Sword"));
        assert_eq!(sword.tags, vec!["minecraft:swords"]);
    }

    #[test]
    fn flattened_catalog_uses_launcher_metadata_when_version_json_is_absent() {
        let dir = TempDir::new("flat-1.13");
        let entries = vec![
            (
                "assets/minecraft/blockstates/stone.json",
                json_bytes(serde_json::json!({
                    "variants": {"": {"model": "minecraft:block/stone"}}
                })),
            ),
            (
                "assets/minecraft/blockstates/air.json",
                json_bytes(serde_json::json!({
                    "variants": {"": {"model": "minecraft:block/air"}}
                })),
            ),
            (
                "assets/minecraft/blockstates/item_frame.json",
                json_bytes(serde_json::json!({
                    "variants": {
                        "map=false": {"model": "minecraft:block/item_frame"},
                        "map=true": {"model": "minecraft:block/item_frame_map"}
                    }
                })),
            ),
            (
                "assets/minecraft/blockstates/brain_coral_wall_fan.json",
                json_bytes(serde_json::json!({
                    "variants": {"": {"model": "minecraft:block/brain_coral_wall_fan"}}
                })),
            ),
            (
                "assets/minecraft/blockstates/brain_coral_fan.json",
                json_bytes(serde_json::json!({
                    "variants": {"": {"model": "minecraft:block/brain_coral_fan"}}
                })),
            ),
            (
                "assets/minecraft/blockstates/sign.json",
                json_bytes(serde_json::json!({
                    "variants": {"": {"model": "minecraft:block/sign"}}
                })),
            ),
            (
                "assets/minecraft/models/block/stone.json",
                json_bytes(serde_json::json!({
                    "textures": {"all": "minecraft:block/stone"}
                })),
            ),
            (
                "assets/minecraft/models/block/air.json",
                json_bytes(serde_json::json!({})),
            ),
            (
                "assets/minecraft/models/block/item_frame.json",
                json_bytes(serde_json::json!({
                    "textures": {"all": "minecraft:block/stone"}
                })),
            ),
            (
                "assets/minecraft/models/block/item_frame_map.json",
                json_bytes(serde_json::json!({
                    "textures": {"all": "minecraft:block/stone"}
                })),
            ),
            (
                "assets/minecraft/models/block/brain_coral_wall_fan.json",
                json_bytes(serde_json::json!({
                    "textures": {"all": "minecraft:block/brain_coral_fan"}
                })),
            ),
            (
                "assets/minecraft/models/block/brain_coral_fan.json",
                json_bytes(serde_json::json!({
                    "textures": {"all": "minecraft:block/brain_coral_fan"}
                })),
            ),
            (
                "assets/minecraft/models/block/sign.json",
                json_bytes(serde_json::json!({
                    "textures": {"all": "minecraft:block/stone"}
                })),
            ),
            (
                "assets/minecraft/models/item/stone.json",
                json_bytes(serde_json::json!({"parent": "minecraft:block/stone"})),
            ),
            (
                "assets/minecraft/models/item/air.json",
                json_bytes(serde_json::json!({})),
            ),
            (
                "assets/minecraft/models/item/item_frame.json",
                json_bytes(serde_json::json!({
                    "parent": "minecraft:item/generated",
                    "textures": {"layer0": "minecraft:item/item_frame"}
                })),
            ),
            (
                "assets/minecraft/models/item/clock.json",
                json_bytes(serde_json::json!({
                    "parent": "minecraft:item/generated",
                    "textures": {"layer0": "minecraft:item/clock_00"},
                    "overrides": [
                        {"predicate": {"time": 0.0}, "model": "minecraft:item/clock_01"}
                    ]
                })),
            ),
            (
                "assets/minecraft/models/item/generated.json",
                json_bytes(serde_json::json!({
                    "textures": {"layer0": "minecraft:item/generated"}
                })),
            ),
            (
                "assets/minecraft/models/item/shield.json",
                json_bytes(serde_json::json!({
                    "parent": "minecraft:builtin/entity",
                    "textures": {"particle": "minecraft:block/dark_oak_planks"}
                })),
            ),
            (
                "assets/minecraft/models/item/brain_coral_wall_fan.json",
                json_bytes(serde_json::json!({
                    "parent": "minecraft:item/generated",
                    "textures": {"layer0": "minecraft:block/brain_coral_fan"}
                })),
            ),
            (
                "assets/minecraft/models/item/brain_coral_fan.json",
                json_bytes(serde_json::json!({
                    "parent": "minecraft:item/generated",
                    "textures": {"layer0": "minecraft:block/brain_coral_fan"}
                })),
            ),
            (
                "assets/minecraft/models/item/sign.json",
                json_bytes(serde_json::json!({
                    "parent": "minecraft:item/generated",
                    "textures": {"layer0": "minecraft:item/sign"}
                })),
            ),
            ("assets/minecraft/textures/block/stone.png", tiny_png()),
            (
                "assets/minecraft/textures/block/brain_coral_fan.png",
                tiny_png(),
            ),
            (
                "assets/minecraft/textures/block/dark_oak_planks.png",
                tiny_png(),
            ),
            ("assets/minecraft/textures/item/clock_00.png", tiny_png()),
            ("assets/minecraft/textures/item/item_frame.png", tiny_png()),
            ("assets/minecraft/textures/item/sign.png", tiny_png()),
            (
                "assets/minecraft/lang/en_us.json",
                json_bytes(serde_json::json!({
                    "block.minecraft.air": "Air",
                    "block.minecraft.brain_coral_fan": "Brain Coral Fan",
                    "block.minecraft.brain_coral_wall_fan": "Brain Coral Wall Fan",
                    "block.minecraft.item_frame": "Item Frame",
                    "block.minecraft.sign": "Sign",
                    "block.minecraft.stone": "Stone",
                    "item.minecraft.clock": "Clock",
                    "item.minecraft.item_frame": "Item Frame",
                    "item.minecraft.lodestone_compass": "Lodestone Compass",
                    "item.minecraft.shield": "Shield",
                    "item.minecraft.sign": "Sign",
                    "item.modifiers.mainhand": "When in main hand:"
                })),
            ),
            (
                "data/minecraft/tags/blocks/base_stone.json",
                json_bytes(serde_json::json!({"values": ["minecraft:stone"]})),
            ),
            (
                "data/minecraft/tags/items/clocks.json",
                json_bytes(serde_json::json!({"values": ["minecraft:clock"]})),
            ),
        ];
        let jar = write_entries_jar(&dir, "1.13.2", &entries);
        write_launcher_metadata(&jar, "1.13.2");
        let service = AssetCatalogService::new();
        let source = source(&jar);
        let catalog = service.catalog(&source).expect("1.13 catalog");

        assert_eq!(catalog.minecraft_version, "1.13.2");
        assert_eq!(catalog.data_version, 1631);
        assert_eq!(catalog.resource_pack_format, Some(4));
        assert_eq!(catalog.datapack_format, Some(4));
        assert!(catalog.catalog_id.starts_with("java-1.13.2-"));
        assert!(!catalog.cache_hit);
        assert!(catalog
            .warnings
            .iter()
            .any(|warning| warning.contains("no internal version.json")));
        assert!(catalog
            .warnings
            .iter()
            .any(|warning| warning.contains("entity render model")));
        assert!(!catalog
            .warnings
            .iter()
            .any(|warning| warning.contains("SHA-1 was not verified")));

        let entry = |id: &str| {
            catalog
                .entries
                .iter()
                .find(|entry| entry.id == id)
                .unwrap_or_else(|| panic!("missing catalog entry {id}"))
        };

        let stone = entry("minecraft:stone");
        assert!(stone.is_block() && stone.is_item());
        assert_eq!(stone.icon_texture.as_deref(), Some("minecraft:block/stone"));
        assert_eq!(stone.display_name.as_deref(), Some("Stone"));
        assert_eq!(stone.tags, vec!["minecraft:base_stone"]);
        assert_eq!(
            stone
                .states
                .as_ref()
                .and_then(|states| states.model.as_deref()),
            Some("minecraft:block/stone")
        );

        let clock = entry("minecraft:clock");
        assert!(!clock.is_block() && clock.is_item());
        assert_eq!(
            clock.icon_texture.as_deref(),
            Some("minecraft:item/clock_00")
        );
        assert_eq!(
            clock
                .item_form
                .as_ref()
                .and_then(|form| form.model.as_deref()),
            Some("minecraft:item/clock")
        );
        assert_eq!(clock.tags, vec!["minecraft:clocks"]);

        let shield = entry("minecraft:shield");
        assert!(!shield.is_block() && shield.is_item());
        assert!(
            shield.icon_texture.is_none(),
            "a special item's particle texture must not become its icon"
        );

        let frame = entry("minecraft:item_frame");
        assert!(frame.item_form.is_some());
        assert!(
            frame.states.is_none(),
            "item frames are entities, not blocks"
        );
        assert_eq!(frame.flags.placeable, Some(false));
        assert_eq!(
            frame.icon_texture.as_deref(),
            Some("minecraft:item/item_frame")
        );

        let wall_fan = entry("minecraft:brain_coral_wall_fan");
        assert!(wall_fan.is_block());
        assert!(!wall_fan.is_item());
        assert!(wall_fan.item_form.is_none());

        let fan = entry("minecraft:brain_coral_fan");
        assert!(fan.is_block() && fan.is_item());

        let air = entry("minecraft:air");
        assert!(air.is_block());
        assert!(
            !air.is_item(),
            "an empty item/air model must not become an item"
        );

        let sign = entry("minecraft:sign");
        assert!(sign.is_block() && sign.is_item());

        assert!(!catalog
            .entries
            .iter()
            .any(|entry| entry.id == "minecraft:lodestone_compass"));
        assert!(!catalog
            .entries
            .iter()
            .any(|entry| entry.id.starts_with("modifiers:")));
        assert!(catalog
            .entries
            .iter()
            .all(|entry| entry.id != "minecraft:generated"));

        assert_eq!(catalog.counts.blockstate_files, 6);
        assert_eq!(catalog.counts.blocks, 5);
        assert_eq!(catalog.counts.items, 6);
        assert_eq!(catalog.counts.blocks_with_items, 3);
        assert_eq!(catalog.counts.item_only, 3);
        assert_eq!(catalog.counts.item_models, 9);
        assert_eq!(catalog.counts.item_definitions, 0);
        assert_eq!(catalog.counts.textures, 6);
        assert_eq!(catalog.counts.block_tags, 1);
        assert_eq!(catalog.counts.item_tags, 1);
        assert_eq!(catalog.counts.atlases, 0);

        let cached = service.catalog(&source).expect("cached 1.13 catalog");
        assert!(cached.cache_hit);
        assert_eq!(cached.catalog_hash, catalog.catalog_hash);
    }

    #[test]
    fn every_flattened_profile_supplies_formats_without_internal_version_json() {
        let dir = TempDir::new("flat-profiles");
        // 1.13.2-1.16.5 client JARs have no root version.json. Launcher metadata names the
        // exact release; the built-in profile supplies the remaining catalog facts.
        for (version, data_version, pack_format) in [
            ("1.13.2", 1631, 4),
            ("1.14.4", 1976, 4),
            ("1.15.2", 2230, 5),
            ("1.16.5", 2586, 6),
            ("1.17.1", 2730, 7),
        ] {
            let entries = vec![
                (
                    "assets/minecraft/blockstates/stone.json",
                    json_bytes(serde_json::json!({
                        "variants": {"": {"model": "minecraft:block/stone"}}
                    })),
                ),
                (
                    "assets/minecraft/models/block/stone.json",
                    json_bytes(serde_json::json!({
                        "textures": {"all": "minecraft:block/stone"}
                    })),
                ),
                ("assets/minecraft/textures/block/stone.png", tiny_png()),
            ];
            let jar = write_entries_jar(&dir, version, &entries);
            write_launcher_metadata(&jar, version);
            let catalog = AssetCatalogService::new()
                .build(&source(&jar))
                .unwrap_or_else(|error| panic!("{version} catalog: {error}"));
            assert_eq!(catalog.minecraft_version, version);
            assert_eq!(catalog.data_version, data_version);
            assert_eq!(catalog.resource_pack_format, Some(pack_format));
            assert_eq!(catalog.datapack_format, Some(pack_format));
            assert!(
                catalog
                    .warnings
                    .iter()
                    .any(|warning| warning.contains("no internal version.json")),
                "{version}: {:?}",
                catalog.warnings
            );
        }
    }

    #[test]
    fn flattened_catalog_reads_weighted_variants_from_internal_version_json() {
        let dir = TempDir::new("flat-1.14");
        let version = json_bytes(serde_json::json!({
            "id": "1.14.4",
            "world_version": 1976,
            "pack_version": 4,
            "stable": true
        }));
        let entries = vec![
            ("version.json", version),
            (
                "assets/minecraft/blockstates/stone.json",
                json_bytes(serde_json::json!({
                    "variants": {"": [
                        {"model": "minecraft:block/stone", "weight": 2},
                        {"model": "minecraft:block/stone_mirrored", "y": 180}
                    ]}
                })),
            ),
            (
                "assets/minecraft/models/block/stone.json",
                json_bytes(serde_json::json!({
                    "textures": {"all": "minecraft:block/stone"}
                })),
            ),
            (
                "assets/minecraft/models/block/stone_mirrored.json",
                json_bytes(serde_json::json!({
                    "textures": {"all": "minecraft:block/stone"}
                })),
            ),
            (
                "assets/minecraft/models/item/stone.json",
                json_bytes(serde_json::json!({"parent": "minecraft:block/stone"})),
            ),
            ("assets/minecraft/textures/block/stone.png", tiny_png()),
            (
                "assets/minecraft/lang/en_us.json",
                json_bytes(serde_json::json!({
                    "block.minecraft.stone": "Stone"
                })),
            ),
        ];
        let jar = write_entries_jar(&dir, "1.14.4", &entries);
        write_launcher_metadata(&jar, "1.14.4");
        let catalog = AssetCatalogService::new()
            .build(&source(&jar))
            .expect("1.14 catalog");

        assert_eq!(catalog.minecraft_version, "1.14.4");
        assert_eq!(catalog.data_version, 1976);
        assert_eq!(catalog.resource_pack_format, Some(4));
        assert_eq!(catalog.datapack_format, Some(4));
        assert!(!catalog
            .warnings
            .iter()
            .any(|warning| warning.contains("no internal version.json")));
        let stone = catalog
            .entries
            .iter()
            .find(|entry| entry.id == "minecraft:stone")
            .expect("stone");
        let states = stone.states.as_ref().expect("stone states");
        assert_eq!(states.variant_count, 1);
        assert_eq!(states.variants[0].models.len(), 2);
        assert_eq!(
            states.variants[0].models[0].model.as_deref(),
            Some("minecraft:block/stone")
        );
        assert_eq!(states.variants[0].models[0].weight, 2);
        assert_eq!(
            states.variants[0].models[1].model.as_deref(),
            Some("minecraft:block/stone_mirrored")
        );
        assert_eq!(states.variants[0].models[1].y_rotation, 180);
        assert_eq!(stone.icon_texture.as_deref(), Some("minecraft:block/stone"));
    }

    #[test]
    fn a_client_jar_without_version_json_or_launcher_metadata_is_refused() {
        let dir = TempDir::new("flat-no-metadata");
        let jar = write_entries_jar(
            &dir,
            "orphan",
            &[(
                "assets/minecraft/blockstates/stone.json",
                json_bytes(serde_json::json!({
                    "variants": {"": {"model": "minecraft:block/stone"}}
                })),
            )],
        );
        let error = AssetCatalogService::new()
            .build(&source(&jar))
            .expect_err("a JAR without version identity must be refused");
        let message = error.to_string();
        assert!(message.contains("no internal version.json"), "{message}");
        assert!(
            message.contains("no adjacent launcher metadata"),
            "{message}"
        );
    }

    #[test]
    fn launcher_metadata_checksum_and_id_are_checked() {
        let dir = TempDir::new("flat-metadata-guard");
        let jar = write_entries_jar(
            &dir,
            "1.13.2",
            &[
                (
                    "assets/minecraft/blockstates/stone.json",
                    json_bytes(serde_json::json!({
                        "variants": {"": {"model": "minecraft:block/stone"}}
                    })),
                ),
                (
                    "assets/minecraft/models/block/stone.json",
                    json_bytes(serde_json::json!({
                        "textures": {"all": "minecraft:block/stone"}
                    })),
                ),
                ("assets/minecraft/textures/block/stone.png", tiny_png()),
            ],
        );
        let size = fs::metadata(&jar).expect("jar metadata").len();
        let sidecar = jar.with_extension("json");
        fs::write(
            &sidecar,
            json_bytes(serde_json::json!({
                "id": "1.13.2",
                "downloads": {"client": {
                    "sha1": "0".repeat(40),
                    "size": size,
                    "url": "https://piston-data.mojang.com/v1/objects/client.jar"
                }}
            })),
        )
        .expect("write bad metadata");
        let error = AssetCatalogService::new()
            .build(&source(&jar))
            .expect_err("checksum mismatch must be refused");
        assert!(error.to_string().contains("SHA-1"), "{error}");

        write_launcher_metadata(&jar, "1.12.2");
        let error = AssetCatalogService::new()
            .build(&source(&jar))
            .expect_err("an unknown exact profile must be refused");
        assert!(
            error.to_string().contains("no exact built-in profile"),
            "{error}"
        );

        let internal = write_entries_jar(
            &dir,
            "internal",
            &[
                (
                    "version.json",
                    json_bytes(serde_json::json!({
                        "id": "1.14.4",
                        "world_version": 1976,
                        "pack_version": 4,
                        "stable": true
                    })),
                ),
                (
                    "assets/minecraft/blockstates/stone.json",
                    json_bytes(serde_json::json!({
                        "variants": {"": {"model": "minecraft:block/stone"}}
                    })),
                ),
            ],
        );
        write_launcher_metadata(&internal, "1.13.2");
        let error = AssetCatalogService::new()
            .build(&source(&internal))
            .expect_err("internal and launcher ids must agree");
        assert!(
            error.to_string().contains("version.json declares"),
            "{error}"
        );
    }

    #[test]
    fn version_folder_and_managed_cache_expose_the_jar_with_its_metadata() {
        let dir = TempDir::new("flat-folders");
        let entries = vec![
            (
                "assets/minecraft/blockstates/stone.json",
                json_bytes(serde_json::json!({
                    "variants": {"": {"model": "minecraft:block/stone"}}
                })),
            ),
            (
                "assets/minecraft/models/block/stone.json",
                json_bytes(serde_json::json!({
                    "textures": {"all": "minecraft:block/stone"}
                })),
            ),
            ("assets/minecraft/textures/block/stone.png", tiny_png()),
        ];
        let source_dir = dir.path.join("versions").join("1.13.2");
        fs::create_dir_all(&source_dir).expect("version folder");
        let jar = write_entries_jar_in(&source_dir, "1.13.2", &entries);
        write_launcher_metadata(&jar, "1.13.2");
        let catalog = AssetCatalogService::new()
            .build(&AssetSourceSetting {
                kind: "version_folder".to_owned(),
                path: source_dir.display().to_string(),
            })
            .expect("version folder catalog");
        assert_eq!(catalog.minecraft_version, "1.13.2");
        assert_eq!(catalog.data_version, 1631);

        let cache_dir = dir.path.join("cache").join("1.13.2");
        fs::create_dir_all(&cache_dir).expect("cache folder");
        let jar = write_entries_jar_in(&cache_dir, "client", &entries);
        write_launcher_metadata(&jar, "1.13.2");
        let catalog = AssetCatalogService::new()
            .build(&AssetSourceSetting {
                kind: "client_jar".to_owned(),
                path: cache_dir.display().to_string(),
            })
            .expect("managed cache catalog");
        assert_eq!(catalog.minecraft_version, "1.13.2");
    }

    fn write_entries_jar_in(directory: &Path, label: &str, entries: &[(&str, Vec<u8>)]) -> PathBuf {
        let path = directory.join(format!("{label}.jar"));
        let file = fs::File::create(&path).expect("create jar");
        let mut zip = zip::ZipWriter::new(file);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, bytes) in entries {
            zip.start_file(*name, options).expect("start file");
            zip.write_all(bytes).expect("write file");
        }
        zip.finish().expect("finish jar");
        path
    }

    #[test]
    fn nested_item_definitions_resolve_the_first_concrete_model() {
        let dir = TempDir::new("nested-items");
        let path = write_minimal_jar(
            &dir,
            "nested-items",
            &[
                (
                    "assets/minecraft/items/clock.json",
                    r#"{"model":{"type":"minecraft:select","property":"minecraft:time","cases":[
                        {"model":{"type":"minecraft:range_dispatch","entries":[
                            {"threshold":0,"model":{"type":"minecraft:model","model":"minecraft:item/clock_00"}}
                        ]}},
                        {"model":{"type":"minecraft:model","model":"minecraft:item/clock_01"}}
                    ]}}"#,
                ),
                (
                    "assets/minecraft/items/player_head.json",
                    r#"{"model":{"type":"minecraft:special","base":"minecraft:item/template_skull","model":{"type":"minecraft:head","kind":"player"}}}"#,
                ),
                (
                    "assets/minecraft/models/item/clock_00.json",
                    r#"{"textures":{"layer0":"minecraft:item/clock_00"}}"#,
                ),
                (
                    "assets/minecraft/models/item/template_skull.json",
                    r#"{"textures":{"layer0":"minecraft:item/template_skull"}}"#,
                ),
                (
                    "assets/minecraft/lang/en_us.json",
                    r#"{"item.minecraft.clock":"Clock","item.minecraft.player_head":"Player Head"}"#,
                ),
            ],
        );
        let catalog = AssetCatalogService::new()
            .build(&source(&path))
            .expect("catalog");
        let clock = catalog
            .entries
            .iter()
            .find(|entry| entry.id == "minecraft:clock")
            .expect("clock");
        let clock_form = clock.item_form.as_ref().expect("clock item form");
        assert!(clock_form.item_definition);
        assert_eq!(clock_form.model_type.as_deref(), Some("minecraft:select"));
        assert_eq!(clock_form.model.as_deref(), Some("minecraft:item/clock_00"));
        assert_eq!(
            clock.icon_texture.as_deref(),
            Some("minecraft:item/clock_00")
        );
        let head = catalog
            .entries
            .iter()
            .find(|entry| entry.id == "minecraft:player_head")
            .expect("player head");
        let head_form = head.item_form.as_ref().expect("head item form");
        assert_eq!(head_form.model_type.as_deref(), Some("minecraft:special"));
        assert_eq!(
            head_form.model.as_deref(),
            Some("minecraft:item/template_skull")
        );
        assert!(
            head.icon_texture.is_none(),
            "a special item's particle texture is not its icon"
        );
        assert!(!catalog
            .warnings
            .iter()
            .any(|warning| warning.contains("item model reference")));
    }

    #[test]
    fn blockstate_defaults_are_known_only_for_a_single_declared_state() {
        let mut warnings = Vec::new();
        let single = parse_blockstate(
            &serde_json::json!({
                "variants":{"facing=north,waterlogged=false":{"model":"minecraft:block/x"}}
            }),
            "x.json",
            &mut warnings,
        );
        assert_eq!(
            single
                .default_properties
                .as_ref()
                .and_then(|properties| properties.get("facing"))
                .map(String::as_str),
            Some("north")
        );
        assert!(single.property_values_complete);
        assert_eq!(single.variant_count, 1);
        assert_eq!(single.variants.len(), 1);
        assert_eq!(single.variants[0].properties["facing"], "north");
        assert_eq!(single.variants[0].models.len(), 1);
        assert_eq!(
            single.variants[0].models[0].model.as_deref(),
            Some("minecraft:block/x")
        );
        assert_eq!(single.variants[0].models[0].weight, 1);

        let ambiguous = parse_blockstate(
            &serde_json::json!({"variants":{
                "facing=north":{"model":"minecraft:block/x"},
                "facing=south":{"model":"minecraft:block/x"}
            }}),
            "x.json",
            &mut warnings,
        );
        assert!(ambiguous.default_properties.is_none());
        assert!(ambiguous.property_values_complete);
        assert_eq!(ambiguous.variant_count, 2);
        assert_eq!(ambiguous.variants.len(), 2);

        let inconsistent = parse_blockstate(
            &serde_json::json!({"variants":{
                "facing=north":{"model":"minecraft:block/x"},
                "facing=south,waterlogged=false":{"model":"minecraft:block/x"}
            }}),
            "x.json",
            &mut warnings,
        );
        assert!(inconsistent.default_properties.is_none());
        assert!(!inconsistent.property_values_complete);
        assert_eq!(inconsistent.variant_count, 2);

        let weighted = parse_blockstate(
            &serde_json::json!({"variants":{"":[
                {"model":"minecraft:block/x","weight":2},
                {"model":"minecraft:block/y","weight":3,"y":90}
            ]}}),
            "x.json",
            &mut warnings,
        );
        assert_eq!(weighted.variant_count, 1);
        assert_eq!(weighted.variants[0].models.len(), 2);
        assert_eq!(weighted.variants[0].models[0].weight, 2);
        assert_eq!(weighted.variants[0].models[1].y_rotation, 90);
        assert_eq!(weighted.variants[0].models[1].weight, 3);

        let multipart = parse_blockstate(
            &serde_json::json!({"multipart":[
                {"when":{"north":"true"},"apply":{"model":"minecraft:block/x"}}
            ]}),
            "x.json",
            &mut warnings,
        );
        assert!(multipart.default_properties.is_none());
        assert!(!multipart.property_values_complete);
        assert_eq!(multipart.variant_count, 0);
        assert!(multipart.variants.is_empty());
    }

    #[test]
    fn malformed_tag_and_atlas_files_warn_without_dropping_the_catalog() {
        let dir = TempDir::new("malformed-extra");
        let path = write_minimal_jar(
            &dir,
            "malformed-extra",
            &[
                (
                    "data/minecraft/tags/block/valid.json",
                    r#"{"values":["minecraft:stone"]}"#,
                ),
                ("data/minecraft/tags/block/broken.json", "{"),
                ("assets/minecraft/atlases/broken.json", "{"),
            ],
        );
        let catalog = AssetCatalogService::new()
            .build(&source(&path))
            .expect("catalog");
        assert_eq!(catalog.entries.len(), 1);
        assert_eq!(catalog.counts.block_tags, 2);
        assert!(catalog
            .warnings
            .iter()
            .any(|warning| warning.contains("tags/block/broken.json")));
        assert!(catalog
            .warnings
            .iter()
            .any(|warning| warning.contains("atlases/broken.json")));
    }

    #[test]
    fn catalog_icon_atlas_is_keyed_by_jar_and_pack_stack() {
        use crate::catalog_atlas::{CatalogIconAtlasService, CATALOG_ATLAS_SCHEMA_VERSION};

        let dir = TempDir::new("atlas");
        let path = write_jar(&dir);
        let catalog = AssetCatalogService::new()
            .build(&source(&path))
            .expect("catalog");
        let service = CatalogIconAtlasService::new();
        let profile = crate::target_profile();
        let store = crate::ResourcePackStore::new(dir.path.join("cache"));
        let stone_keys = vec!["minecraft:block/stone".to_owned()];

        let first = service
            .atlas(profile, &source(&path), &[], &catalog, &stone_keys)
            .expect("first atlas");
        assert!(!first.cache_hit);
        assert_eq!(first.schema_version, CATALOG_ATLAS_SCHEMA_VERSION);
        assert_eq!(first.catalog_id, catalog.catalog_id);
        assert_eq!(first.catalog_hash, catalog.catalog_hash);
        assert_eq!(first.minecraft_version, "1.21.4");
        assert_eq!(
            first.client_jar_sha1,
            catalog.client_jar_sha1.clone().expect("jar sha1")
        );
        assert_eq!(first.cache_key.split('|').count(), 5);
        assert!(first.cache_key.contains(&first.pack_stack_hash));
        assert_eq!(first.requested_texture_count, 1);
        assert_eq!(
            first.total_icon_texture_count,
            catalog.icon_texture_keys().len()
        );
        assert_eq!(
            first.textures.len(),
            2,
            "requested icon plus missing checker"
        );
        let stone = first
            .textures
            .iter()
            .find(|texture| texture.key == "minecraft:block/stone")
            .expect("stone icon");
        assert!(!stone.missing);
        assert_eq!((stone.width, stone.height), (16, 16));
        assert_eq!(atlas_pixel(&first, stone, 0, 0), [255, 0, 255, 255]);

        let second = service
            .atlas(profile, &source(&path), &[], &catalog, &stone_keys)
            .expect("second atlas");
        assert!(second.cache_hit);
        assert_eq!(second.cache_key, first.cache_key);

        let other = service
            .atlas(
                profile,
                &source(&path),
                &[],
                &catalog,
                &["minecraft:block/oak_planks".to_owned()],
            )
            .expect("different lazy batch");
        assert!(!other.cache_hit);
        assert_ne!(other.cache_key, first.cache_key);
        assert_eq!(other.requested_texture_count, 1);
        assert!(matches!(
            service.atlas(
                profile,
                &source(&path),
                &[],
                &catalog,
                &["minecraft:block/not_an_icon".to_owned()],
            ),
            Err(crate::bundle::BundleError::InvalidArgument(_))
        ));
        let (entries, bytes) = service.cache_stats();
        assert_eq!(entries, 2);
        assert!(bytes > 0);

        let pack = write_pack(&dir, "green-stone", &colored_png([0, 255, 0, 255]));
        let pin = store.import(&pack, profile).expect("import pack");
        let inputs = store.inputs(std::slice::from_ref(&pin));
        let third = service
            .atlas(profile, &source(&path), &inputs, &catalog, &stone_keys)
            .expect("pack atlas");
        assert!(!third.cache_hit);
        assert_ne!(third.cache_key, first.cache_key);
        assert_ne!(third.pack_stack_hash, first.pack_stack_hash);
        let stone = third
            .textures
            .iter()
            .find(|texture| texture.key == "minecraft:block/stone")
            .expect("pack stone icon");
        assert_eq!(atlas_pixel(&third, stone, 0, 0), [0, 255, 0, 255]);

        let fourth = service
            .atlas(profile, &source(&path), &inputs, &catalog, &stone_keys)
            .expect("pack cache hit");
        assert!(fourth.cache_hit);
        assert_eq!(fourth.cache_key, third.cache_key);
        std::fs::remove_file(dir.path.join("cache").join(&pin.path)).expect("delete pinned pack");
        let fallback = service
            .atlas(profile, &source(&path), &inputs, &catalog, &stone_keys)
            .expect("cached vanilla atlas");
        assert!(fallback.cache_hit);
        assert_eq!(fallback.cache_key, first.cache_key);
        assert_eq!(fallback.catalog_id, catalog.catalog_id);
        assert_eq!(fallback.catalog_hash, catalog.catalog_hash);
        assert_eq!(fallback.pack_issues.len(), 1);
        assert!(service
            .atlas(profile, &source(&path), &[], &catalog, &stone_keys)
            .expect("other project with no packs")
            .pack_issues
            .is_empty());
    }
}
