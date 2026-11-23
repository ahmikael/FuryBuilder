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
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

use minecraft_version::JarVersionJson;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::archive::Archive;
use crate::bundle::normalize_id;
use crate::discovery::inspect_asset_path;
use crate::settings::AssetSourceSetting;

/// Version of the catalog DTO written by this builder.
pub const CATALOG_SCHEMA_VERSION: u32 = 1;
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

const MAX_CATALOG_CACHE_ENTRIES: usize = 4;
const MAX_MODEL_DEPTH: usize = 12;

/// One block from the client JAR with the states the JAR declares.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogBlock {
    pub id: String,
    pub display_name: Option<String>,
    /// Every property key and all values declared by variants or multipart conditions.
    pub properties: BTreeMap<String, Vec<String>>,
    /// The first declared variant. `None` for multipart blocks, whose default state is
    /// maintained by the game code, not by the blockstate file.
    pub default_properties: Option<BTreeMap<String, String>>,
    /// Number of explicit `variants` entries; zero for multipart blocks.
    pub variants: usize,
    pub multipart: bool,
    /// First model referenced by the blockstate file.
    pub model: Option<String>,
    /// A texture that exists in the JAR and can be used as the palette icon.
    pub icon_texture: Option<String>,
    pub tags: Vec<String>,
    pub has_item: bool,
}

/// One item from the client JAR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogItem {
    pub id: String,
    pub display_name: Option<String>,
    /// True when the entry came from `assets/<namespace>/items/<item>.json`.
    pub item_definition: bool,
    pub model: Option<String>,
    pub icon_texture: Option<String>,
    /// Present when the item is also a block (`minecraft:stone`).
    pub block: Option<String>,
    pub tags: Vec<String>,
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
    pub blocks: Vec<CatalogBlock>,
    pub items: Vec<CatalogItem>,
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

/// Bounded process-local catalog cache. The key includes the JAR identity, so replacing a
/// JAR with another build of the same version rebuilds the catalog. A persistent cache keyed
/// by `version + jarSha1 + catalogSchema` is added together with the project catalog pin.
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
        let resolved = resolve_client_jar(source)?;
        let key = cache_key(&resolved)?;
        if let Ok(mut cache) = self.cache.lock() {
            if let Some(index) = cache.iter().position(|entry| entry.key == key) {
                if let Some(entry) = cache.remove(index) {
                    let mut catalog = entry.catalog.clone();
                    catalog.cache_hit = true;
                    cache.push_front(entry);
                    return Ok(catalog);
                }
            }
        }
        let catalog = build_catalog(&resolved)?;
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
        build_catalog(&resolve_client_jar(source)?)
    }
}

#[derive(Debug)]
struct ResolvedClientJar {
    jar_path: PathBuf,
    display_path: String,
    version: String,
    data_version: i32,
    sha1: Option<String>,
    checksum_verified: bool,
}

fn resolve_client_jar(source: &AssetSourceSetting) -> Result<ResolvedClientJar, CatalogError> {
    if source.kind == "resource_pack" {
        return Err(CatalogError::InvalidSource(
            "a resource pack has no block/item registry; select a .minecraft root, version folder or client JAR"
                .to_owned(),
        ));
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
    let version = report.version.ok_or_else(|| {
        CatalogError::InvalidSource("the selected path has no client JAR version report".to_owned())
    })?;
    if version.jar_path.is_empty() {
        return Err(CatalogError::InvalidSource(
            "the selected path has no client JAR".to_owned(),
        ));
    }
    let jar_path = PathBuf::from(&version.jar_path);
    let metadata = fs::symlink_metadata(&jar_path).map_err(|error| {
        CatalogError::Io(format!("cannot inspect {}: {error}", jar_path.display()))
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(CatalogError::InvalidSource(format!(
            "{} is not a regular client JAR",
            jar_path.display()
        )));
    }
    Ok(ResolvedClientJar {
        display_path: version.jar_path.clone(),
        version: version
            .internal_version_id
            .clone()
            .unwrap_or_else(|| version.id.clone()),
        data_version: version.world_version.ok_or_else(|| {
            CatalogError::Unsupported(format!(
                "{} has no version.json DataVersion",
                jar_path.display()
            ))
        })?,
        sha1: version.actual_sha1.clone(),
        checksum_verified: version.checksum_verified,
        jar_path,
    })
}

fn cache_key(resolved: &ResolvedClientJar) -> Result<String, CatalogError> {
    let metadata = fs::metadata(&resolved.jar_path).map_err(|error| {
        CatalogError::Io(format!(
            "cannot inspect {}: {error}",
            resolved.jar_path.display()
        ))
    })?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    Ok(format!(
        "{}|{}|{}|{}",
        resolved.jar_path.display(),
        metadata.len(),
        modified,
        CATALOG_SCHEMA_VERSION
    ))
}

#[derive(Default)]
struct BlockStateFacts {
    properties: BTreeMap<String, BTreeSet<String>>,
    default_properties: Option<BTreeMap<String, String>>,
    variants: usize,
    multipart: bool,
    first_model: Option<String>,
}

struct ItemFacts {
    model: Option<String>,
    item_definition: bool,
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
        push_warning(
            &mut warnings,
            "the client JAR SHA-1 was not verified against launcher metadata".to_owned(),
        );
    }

    let version_json = read_version_json(&mut archive, &resolved.version)?;
    let (resource_pack_format, datapack_format) = version_json
        .pack_formats()
        .map_err(|error| CatalogError::VersionJson(error.to_string()))?;

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

    let mut blocks: BTreeMap<String, CatalogBlock> = BTreeMap::new();
    for (id, entry) in &blockstate_entries {
        let facts = match read_json(&mut archive, entry, &mut warnings) {
            Some(document) => parse_blockstate(&document, entry, &mut warnings),
            None => BlockStateFacts::default(),
        };
        let properties = facts
            .properties
            .iter()
            .map(|(key, values)| (key.clone(), values.iter().cloned().collect()))
            .collect();
        blocks.insert(
            id.clone(),
            CatalogBlock {
                id: id.clone(),
                display_name: block_display_name(&lang, id),
                properties,
                default_properties: facts.default_properties,
                variants: facts.variants,
                multipart: facts.multipart,
                model: facts.first_model,
                icon_texture: None,
                tags: Vec::new(),
                has_item: false,
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
        if let Some(document) = read_json(&mut archive, entry, &mut warnings) {
            model = item_definition_model(&document);
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
                model,
                item_definition: true,
            },
        );
    }
    if definition_entries.is_empty() {
        // 1.20.6 and older expose item models instead of item definitions. Item templates
        // such as `item/handheld` are not items, so only block items and lang item keys are
        // admitted here; a template can never become a fake palette entry.
        for block in blocks.keys() {
            if let Some((namespace, path)) = block.split_once(':') {
                if item_model_ids.contains(&format!("{namespace}:item/{path}")) {
                    items.insert(
                        block.clone(),
                        ItemFacts {
                            model: Some(format!("{namespace}:item/{path}")),
                            item_definition: false,
                        },
                    );
                }
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
            if crate::bundle::valid_id(&id) {
                items.entry(id.clone()).or_insert(ItemFacts {
                    model: Some(format!("{namespace}:item/{path}")),
                    item_definition: false,
                });
            }
        }
    }

    let mut resolver = ModelResolver {
        archive: &mut archive,
        models: &model_index,
        cache: HashMap::new(),
    };
    for block in blocks.values_mut() {
        if let Some(model) = &block.model {
            block.icon_texture = resolver.icon_texture(model, &texture_ids, &mut warnings);
        }
    }
    let block_icons: HashMap<String, Option<String>> = blocks
        .iter()
        .map(|(id, block)| (id.clone(), block.icon_texture.clone()))
        .collect();
    let mut item_icons: HashMap<String, Option<String>> = HashMap::new();
    for (id, item) in &items {
        let icon = item
            .model
            .as_ref()
            .and_then(|model| resolver.icon_texture(model, &texture_ids, &mut warnings));
        item_icons.insert(id.clone(), icon);
    }

    let (block_tags, block_tag_files) =
        parse_tag_folders(&mut archive, &entry_names, "tags/block", &mut warnings);
    let (item_tags, item_tag_files) =
        parse_tag_folders(&mut archive, &entry_names, "tags/item", &mut warnings);
    let (atlases, atlas_sources) = read_atlases(&mut archive, &entry_names, &mut warnings);

    let mut item_entries: Vec<CatalogItem> = items
        .iter()
        .map(|(id, facts)| {
            let block = blocks.contains_key(id).then(|| id.clone());
            CatalogItem {
                id: id.clone(),
                display_name: item_display_name(&lang, id, block.is_some()),
                item_definition: facts.item_definition,
                model: facts.model.clone(),
                icon_texture: item_icons.get(id).cloned().flatten().or_else(|| {
                    block
                        .as_ref()
                        .and_then(|block| block_icons.get(block).cloned().flatten())
                }),
                block,
                tags: Vec::new(),
            }
        })
        .collect();
    for item in item_entries.iter_mut() {
        item.tags = tags_for(&item_tags, &item.id);
    }
    let item_ids: BTreeSet<&str> = item_entries.iter().map(|item| item.id.as_str()).collect();
    let mut block_entries: Vec<CatalogBlock> = blocks.into_values().collect();
    for block in block_entries.iter_mut() {
        block.tags = tags_for(&block_tags, &block.id);
        block.has_item = item_ids.contains(block.id.as_str());
    }

    let mut foreign_namespaces = BTreeSet::new();
    for id in block_entries
        .iter()
        .map(|block| &block.id)
        .chain(item_entries.iter().map(|item| &item.id))
    {
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
    let blocks_with_items = block_entries.iter().filter(|block| block.has_item).count();
    let counts = CatalogCounts {
        blocks: block_entries.len(),
        items: item_entries.len(),
        blocks_with_items,
        item_only: item_entries
            .iter()
            .filter(|item| item.block.is_none())
            .count(),
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
    let catalog_hash = catalog_digest(
        &resolved.version,
        resolved.data_version,
        resolved.sha1.as_deref(),
        &block_entries,
        &item_entries,
        &atlases,
    )?;
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
        blocks: block_entries,
        items: item_entries,
        atlases,
        counts,
        warnings,
        cache_hit: false,
    })
}

fn read_version_json(
    archive: &mut Archive,
    expected_id: &str,
) -> Result<JarVersionJson, CatalogError> {
    let bytes = archive
        .read_entry("version.json", MAX_CATALOG_JSON_BYTES)
        .map_err(|error| match error {
            crate::archive::ArchiveError::Missing(_) => {
                CatalogError::Unsupported("the client JAR has no version.json".to_owned())
            }
            other => CatalogError::InvalidSource(format!(
                "cannot read version.json from the client JAR: {other}"
            )),
        })?;
    let version: JarVersionJson = serde_json::from_slice(&bytes)
        .map_err(|error| CatalogError::VersionJson(error.to_string()))?;
    if version.id != expected_id {
        return Err(CatalogError::VersionJson(format!(
            "version.json declares `{}`, not `{expected_id}`",
            version.id
        )));
    }
    Ok(version)
}

fn collect_asset_entries(
    entries: &[String],
    folder: &str,
    extension: &str,
) -> Vec<(String, String)> {
    let mut collected = Vec::new();
    for name in entries {
        if let Some((namespace, path)) = parse_asset_entry(name, folder, extension) {
            let id = format!("{namespace}:{path}");
            if crate::bundle::valid_id(&id) {
                collected.push((id, name.clone()));
            }
        }
    }
    collected.sort();
    collected
}

fn parse_asset_entry(name: &str, folder: &str, extension: &str) -> Option<(String, String)> {
    let rest = name.strip_prefix("assets/")?;
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
        for (key, value) in variants {
            facts.variants += 1;
            match parse_state_key(key) {
                Some(properties) => {
                    if facts.default_properties.is_none() {
                        facts.default_properties = Some(properties.clone());
                    }
                    merge_properties(&mut facts.properties, &properties);
                }
                None => push_warning(warnings, format!("{entry}: invalid variant key `{key}`")),
            }
            if facts.first_model.is_none() {
                facts.first_model = first_model_ref(value);
            }
        }
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

fn item_definition_model(document: &Value) -> Option<String> {
    document
        .get("model")
        .and_then(first_model_ref)
        .or_else(|| document.get("parent").and_then(first_model_ref))
}

fn tags_for(tags: &BTreeMap<String, BTreeSet<String>>, id: &str) -> Vec<String> {
    tags.get(id)
        .map(|values| values.iter().cloned().collect())
        .unwrap_or_default()
}

fn parse_tag_folders(
    archive: &mut Archive,
    entries: &[String],
    folder: &str,
    warnings: &mut Vec<String>,
) -> (BTreeMap<String, BTreeSet<String>>, usize) {
    let tag_files = collect_asset_entries(entries, folder, ".json");
    if tag_files.len() > MAX_CATALOG_TAGS {
        push_warning(
            warnings,
            format!("{folder} has more than {MAX_CATALOG_TAGS} tags; the rest were ignored"),
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
        let merged = self.resolve(model, 0, &mut Vec::new(), warnings)?;
        const PREFERRED: [&str; 13] = [
            "layer0", "all", "particle", "top", "texture", "cross", "side", "still", "flow",
            "base", "end", "bottom", "front",
        ];
        for key in PREFERRED {
            if let Some(value) = merged.get(key) {
                if let Some(texture) = resolve_texture_reference(value, &merged, 0) {
                    if textures.contains(&texture) {
                        return Some(texture);
                    }
                }
            }
        }
        for value in merged.values() {
            if let Some(texture) = resolve_texture_reference(value, &merged, 0) {
                if textures.contains(&texture) {
                    return Some(texture);
                }
            }
        }
        push_warning(
            warnings,
            format!("model {model} has no texture present in the client JAR"),
        );
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
    blocks: &[CatalogBlock],
    items: &[CatalogItem],
    atlases: &[CatalogAtlas],
) -> Result<String, CatalogError> {
    #[derive(Serialize)]
    struct Digest<'a> {
        schema_version: u32,
        minecraft_version: &'a str,
        data_version: i32,
        client_jar_sha1: Option<&'a str>,
        blocks: &'a [CatalogBlock],
        items: &'a [CatalogItem],
        atlases: &'a [CatalogAtlas],
    }
    let bytes = serde_json::to_vec(&Digest {
        schema_version: CATALOG_SCHEMA_VERSION,
        minecraft_version: version,
        data_version,
        client_jar_sha1: sha1,
        blocks,
        items,
        atlases,
    })
    .map_err(|error| CatalogError::Io(error.to_string()))?;
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut hex, "{byte:02x}");
    }
    Ok(hex)
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
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 16, 16);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().expect("png header");
            let mut pixels = vec![0u8; 16 * 16 * 4];
            for pixel in pixels.as_chunks_mut::<4>().0 {
                pixel.copy_from_slice(&[255, 0, 255, 255]);
            }
            writer.write_image_data(&pixels).expect("png pixels");
        }
        bytes
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
            r#"{"variants":{"level=0":{"model":"minecraft:block/water"}}}"#,
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
            "assets/minecraft/tags/block/mineable/pickaxe.json",
            r#"{"values":["minecraft:stone","minecraft:oak_stairs","example:ruby_block"]}"#,
        );
        text(
            &mut zip,
            "assets/minecraft/tags/item/swords.json",
            r#"{"values":["minecraft:diamond_sword"]}"#,
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
        assert_eq!(catalog.blocks.len(), 5);
        assert_eq!(catalog.items.len(), 4);
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
        assert_eq!(catalog.counts.item_tags, 1);
        assert_eq!(catalog.counts.atlas_sources, 2);
        assert!(!catalog.cache_hit);

        let stone = catalog
            .blocks
            .iter()
            .find(|block| block.id == "minecraft:stone")
            .expect("stone");
        assert_eq!(stone.display_name.as_deref(), Some("Stone"));
        assert!(stone.properties.is_empty());
        assert_eq!(stone.variants, 1);
        assert!(!stone.multipart);
        assert_eq!(stone.model.as_deref(), Some("minecraft:block/stone"));
        assert_eq!(stone.icon_texture.as_deref(), Some("minecraft:block/stone"));
        assert_eq!(stone.tags, vec!["minecraft:mineable/pickaxe"]);
        assert!(stone.has_item);

        let stairs = catalog
            .blocks
            .iter()
            .find(|block| block.id == "minecraft:oak_stairs")
            .expect("stairs");
        assert_eq!(
            stairs.properties["facing"]
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["east", "north"]
        );
        assert_eq!(
            stairs.properties["waterlogged"]
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["false", "true"]
        );
        assert_eq!(
            stairs
                .default_properties
                .as_ref()
                .expect("default variant")
                .get("half")
                .map(String::as_str),
            Some("bottom")
        );
        assert_eq!(stairs.variants, 2);
        assert_eq!(
            stairs.icon_texture.as_deref(),
            Some("minecraft:block/oak_planks")
        );

        let fence = catalog
            .blocks
            .iter()
            .find(|block| block.id == "minecraft:oak_fence")
            .expect("fence");
        assert!(fence.multipart);
        assert_eq!(fence.variants, 0);
        assert!(fence.default_properties.is_none());
        assert_eq!(
            fence.properties["north"]
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["true"]
        );

        let water = catalog
            .blocks
            .iter()
            .find(|block| block.id == "minecraft:water")
            .expect("water");
        assert!(!water.has_item);
        assert_eq!(
            water.properties["level"]
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["0"]
        );

        let sword = catalog
            .items
            .iter()
            .find(|item| item.id == "minecraft:diamond_sword")
            .expect("sword");
        assert_eq!(sword.display_name.as_deref(), Some("Diamond Sword"));
        assert!(sword.item_definition);
        assert!(sword.block.is_none());
        assert_eq!(
            sword.icon_texture.as_deref(),
            Some("minecraft:item/diamond_sword")
        );
        assert_eq!(sword.tags, vec!["minecraft:swords"]);

        let ruby = catalog
            .blocks
            .iter()
            .find(|block| block.id == "example:ruby_block")
            .expect("ruby");
        assert_eq!(ruby.display_name.as_deref(), Some("Ruby Block"));
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
            .blocks
            .iter()
            .any(|block| block.id == "minecraft:stone"));
        assert!(catalog
            .blocks
            .iter()
            .any(|block| block.id == "minecraft:broken"));
        assert!(catalog
            .warnings
            .iter()
            .any(|warning| warning.contains("is not valid JSON")));
    }
}
