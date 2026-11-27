//! Palette icon atlas built from the exact catalog and the ordered resource-pack stack.
//!
//! The catalog itself is registry facts from the base client JAR. Icons are presentation:
//! resource packs may replace a texture without adding or removing a block or item. This
//! module therefore keeps the atlas out of [`CatalogSnapshot`] and caches it separately by
//! `minecraftVersion + clientJarSha1 + packStackHash + catalogSchema`.
//!
//! Only textures referenced by catalog icons are decoded. The same bounded shelf packing,
//! missing checker and animated-texture cropping as the render atlas are reused, so a broken
//! or missing texture never changes world data and never becomes an invisible success.

use std::collections::{BTreeSet, VecDeque};
use std::sync::Mutex;

use minecraft_version::VersionProfile;
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::bundle::{build_atlas_with_limit, BundleError, ResourceStack};
use crate::catalog::CatalogSnapshot;
use crate::{AssetSourceSetting, ResourcePackInput};

/// Version of the icon-atlas DTO and packing output.
pub const CATALOG_ATLAS_SCHEMA_VERSION: u32 = 2;
/// The palette only asks for the icons in its visible window plus overscan. Bounding a batch
/// keeps a malformed caller from turning one lazy request into a full-catalog atlas.
pub const MAX_CATALOG_ATLAS_REQUEST_TEXTURES: usize = 512;

const MAX_CACHE_ENTRIES: usize = 32;
const MAX_CACHE_BYTES: usize = 16 * 1024 * 1024;

/// One decoded catalog icon placed in the atlas.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogAtlasTexture {
    /// Texture id such as `minecraft:block/stone`, matching `iconTexture` in the catalog.
    pub key: String,
    /// Resolved JAR path of the texture, if one was found.
    pub path: Option<String>,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub source_width: u32,
    pub source_height: u32,
    pub missing: bool,
    pub has_alpha: bool,
}

/// A packed PNG atlas for the palette plus the location of every icon in it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogIconAtlas {
    pub schema_version: u32,
    /// `minecraftVersion|clientJarSha1|packStackHash|catalogSchema.atlasSchema`.
    pub cache_key: String,
    pub cache_hit: bool,
    pub catalog_id: String,
    pub catalog_hash: String,
    pub catalog_schema_version: u32,
    pub minecraft_version: String,
    pub client_jar_sha1: String,
    pub pack_stack_hash: String,
    pub width: u32,
    pub height: u32,
    pub png_base64: String,
    pub textures: Vec<CatalogAtlasTexture>,
    /// Number of distinct catalog textures requested for this batch.
    pub requested_texture_count: usize,
    /// Distinct icon textures declared by the whole catalog.
    pub total_icon_texture_count: usize,
    pub warnings: Vec<String>,
    pub pack_issues: Vec<String>,
}

struct CacheEntry {
    key: String,
    atlas: CatalogIconAtlas,
    bytes: usize,
}

/// Bounded process-local icon-atlas cache. The key contains the exact version, JAR checksum,
/// ordered pack stack and catalog schema, so replacing any input rebuilds the atlas. The
/// catalog itself remains pinned by `catalogId`/`catalogHash`; this cache is recreatable.
#[derive(Default)]
pub struct CatalogIconAtlasService {
    cache: Mutex<VecDeque<CacheEntry>>,
}

impl CatalogIconAtlasService {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&self) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.clear();
        }
    }

    /// Builds (or reuses) a lazy icon-atlas batch for the requested catalog textures.
    ///
    /// Only textures named by `texture_keys` are decoded. The palette asks for its visible
    /// window plus the selected entry, so opening a 10k+ entry catalog never builds one huge
    /// atlas. Keys must be declared by `catalog`; arbitrary JAR paths are not accepted.
    pub fn atlas(
        &self,
        profile: &VersionProfile,
        source: &AssetSourceSetting,
        packs: &[ResourcePackInput],
        catalog: &CatalogSnapshot,
        texture_keys: &[String],
    ) -> Result<CatalogIconAtlas, BundleError> {
        let mut stack = ResourceStack::open(
            profile,
            Some(source),
            catalog.client_jar_sha1.as_deref(),
            packs,
        )?;
        let available = catalog.icon_texture_keys();
        let requested = requested_keys(&available, texture_keys)?;
        let key = atlas_cache_key(catalog, &stack, &requested)?;
        if let Ok(mut cache) = self.cache.lock() {
            if let Some(index) = cache.iter().position(|entry| entry.key == key) {
                if let Some(entry) = cache.remove(index) {
                    let mut atlas = entry.atlas.clone();
                    atlas.cache_hit = true;
                    atlas.pack_issues = stack.pack_issues;
                    atlas.warnings.splice(..0, stack.warnings);
                    cache.push_front(entry);
                    return Ok(atlas);
                }
            }
        }

        let total_icon_texture_count = available.len();
        let requested_texture_count = requested.len();
        let built = build_atlas_with_limit(
            &mut stack,
            &requested,
            MAX_CATALOG_ATLAS_REQUEST_TEXTURES + 1,
        )?;
        let atlas = CatalogIconAtlas {
            schema_version: CATALOG_ATLAS_SCHEMA_VERSION,
            cache_key: key,
            cache_hit: false,
            catalog_id: catalog.catalog_id.clone(),
            catalog_hash: catalog.catalog_hash.clone(),
            catalog_schema_version: catalog.schema_version,
            minecraft_version: catalog.minecraft_version.clone(),
            client_jar_sha1: catalog.client_jar_sha1.clone().ok_or_else(|| {
                BundleError::UnsupportedFormat(
                    "the client JAR SHA-1 is unavailable; select a verified client JAR".to_owned(),
                )
            })?,
            pack_stack_hash: stack.pack_stack_hash.clone(),
            width: built.width,
            height: built.height,
            png_base64: built.png_base64,
            textures: built
                .textures
                .into_iter()
                .map(|texture| CatalogAtlasTexture {
                    key: texture.key,
                    path: texture.path,
                    x: texture.x,
                    y: texture.y,
                    width: texture.width,
                    height: texture.height,
                    source_width: texture.source_width,
                    source_height: texture.source_height,
                    missing: texture.missing,
                    has_alpha: texture.has_alpha,
                })
                .collect(),
            requested_texture_count,
            total_icon_texture_count,
            warnings: built.warnings,
            pack_issues: Vec::new(),
        };
        let mut atlas = self.insert(atlas);
        atlas.pack_issues = stack.pack_issues;
        atlas.warnings.splice(..0, stack.warnings);
        Ok(atlas)
    }

    #[cfg(test)]
    pub(crate) fn cache_stats(&self) -> (usize, usize) {
        self.cache
            .lock()
            .map(|cache| (cache.len(), cache.iter().map(|entry| entry.bytes).sum()))
            .unwrap_or((0, 0))
    }

    fn insert(&self, atlas: CatalogIconAtlas) -> CatalogIconAtlas {
        let bytes = atlas.png_base64.len() + atlas.textures.len() * 64;
        if let Ok(mut cache) = self.cache.lock() {
            cache.retain(|entry| entry.key != atlas.cache_key);
            cache.push_front(CacheEntry {
                key: atlas.cache_key.clone(),
                atlas: atlas.clone(),
                bytes,
            });
            while cache.len() > MAX_CACHE_ENTRIES
                || cache.iter().map(|entry| entry.bytes).sum::<usize>() > MAX_CACHE_BYTES
            {
                if cache.pop_back().is_none() {
                    break;
                }
            }
        }
        atlas
    }
}

fn requested_keys(
    available: &BTreeSet<String>,
    texture_keys: &[String],
) -> Result<BTreeSet<String>, BundleError> {
    let requested: BTreeSet<String> = texture_keys.iter().cloned().collect();
    if requested.is_empty() {
        return Err(BundleError::InvalidArgument(
            "an icon atlas request needs at least one catalog texture".to_owned(),
        ));
    }
    if requested.len() > MAX_CATALOG_ATLAS_REQUEST_TEXTURES {
        return Err(BundleError::LimitExceeded(format!(
            "an icon atlas batch requests more than {MAX_CATALOG_ATLAS_REQUEST_TEXTURES} textures"
        )));
    }
    if let Some(unknown) = requested
        .iter()
        .find(|texture_key| !available.contains(*texture_key))
    {
        return Err(BundleError::InvalidArgument(format!(
            "texture key {unknown} is not declared by this catalog"
        )));
    }
    Ok(requested)
}

fn atlas_cache_key(
    catalog: &CatalogSnapshot,
    stack: &ResourceStack,
    keys: &BTreeSet<String>,
) -> Result<String, BundleError> {
    let sha1 = catalog.client_jar_sha1.as_deref().ok_or_else(|| {
        BundleError::UnsupportedFormat(
            "the client JAR SHA-1 is unavailable; select a verified client JAR".to_owned(),
        )
    })?;
    let schema = format!("{}.{CATALOG_ATLAS_SCHEMA_VERSION}", catalog.schema_version);
    Ok(format!(
        "{}|{}|{}|{}|{}",
        catalog.minecraft_version,
        sha1,
        stack.pack_stack_hash,
        schema,
        keys_digest(keys)
    ))
}

fn keys_digest(keys: &BTreeSet<String>) -> String {
    let mut hasher = Sha256::new();
    for key in keys {
        hasher.update(key.as_bytes());
        hasher.update([0]);
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(16);
    for byte in digest.iter().take(8) {
        use std::fmt::Write as _;
        let _ = write!(&mut hex, "{byte:02x}");
    }
    hex
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::CATALOG_SCHEMA_VERSION;

    #[test]
    fn catalog_icon_keys_are_deduplicated_and_ignore_missing_icons() {
        let catalog = CatalogSnapshot {
            schema_version: CATALOG_SCHEMA_VERSION,
            catalog_id: "java-1.21.4-test".to_owned(),
            catalog_hash: "hash".to_owned(),
            minecraft_version: "1.21.4".to_owned(),
            data_version: 4189,
            client_jar: "client.jar".to_owned(),
            client_jar_sha1: Some("sha1".to_owned()),
            resource_pack_format: Some(46),
            datapack_format: Some(61),
            entries: vec![
                crate::catalog::CatalogEntry {
                    id: "minecraft:stone".to_owned(),
                    legacy_ids: Vec::new(),
                    display_name: None,
                    category: None,
                    tags: Vec::new(),
                    icon_texture: Some("minecraft:block/stone".to_owned()),
                    aliases: Vec::new(),
                    states: Some(crate::catalog::CatalogStates {
                        properties: Default::default(),
                        property_values_complete: true,
                        default_properties: None,
                        variant_count: 1,
                        variants: Vec::new(),
                        multipart: false,
                        model: None,
                    }),
                    item_form: None,
                    flags: crate::catalog::CatalogFlags::default(),
                },
                crate::catalog::CatalogEntry {
                    id: "minecraft:dirt".to_owned(),
                    legacy_ids: Vec::new(),
                    display_name: None,
                    category: None,
                    tags: Vec::new(),
                    icon_texture: Some("minecraft:block/stone".to_owned()),
                    aliases: Vec::new(),
                    states: Some(crate::catalog::CatalogStates {
                        properties: Default::default(),
                        property_values_complete: true,
                        default_properties: None,
                        variant_count: 1,
                        variants: Vec::new(),
                        multipart: false,
                        model: None,
                    }),
                    item_form: None,
                    flags: crate::catalog::CatalogFlags::default(),
                },
                crate::catalog::CatalogEntry {
                    id: "minecraft:sword".to_owned(),
                    legacy_ids: Vec::new(),
                    display_name: None,
                    category: None,
                    tags: Vec::new(),
                    icon_texture: None,
                    aliases: Vec::new(),
                    states: None,
                    item_form: Some(crate::catalog::CatalogItemForm {
                        item_definition: true,
                        model_type: None,
                        model: None,
                        legacy_ids: Vec::new(),
                    }),
                    flags: crate::catalog::CatalogFlags::default(),
                },
            ],
            atlases: Vec::new(),
            counts: Default::default(),
            warnings: Vec::new(),
            cache_hit: false,
        };
        let keys = catalog.icon_texture_keys();
        assert_eq!(keys.len(), 1);
        assert!(keys.contains("minecraft:block/stone"));
    }

    #[test]
    fn lazy_atlas_requests_are_deduplicated_and_bounded_to_catalog_keys() {
        let available = BTreeSet::from([
            "minecraft:block/stone".to_owned(),
            "minecraft:block/dirt".to_owned(),
        ]);
        let requested = requested_keys(
            &available,
            &[
                "minecraft:block/stone".to_owned(),
                "minecraft:block/stone".to_owned(),
            ],
        )
        .expect("deduplicated request");
        assert_eq!(
            requested,
            BTreeSet::from(["minecraft:block/stone".to_owned()])
        );

        assert!(matches!(
            requested_keys(&available, &[]),
            Err(BundleError::InvalidArgument(_))
        ));
        assert!(matches!(
            requested_keys(&available, &["minecraft:block/not_a_texture".to_owned()]),
            Err(BundleError::InvalidArgument(_))
        ));

        let too_many: Vec<String> = (0..=MAX_CATALOG_ATLAS_REQUEST_TEXTURES)
            .map(|index| format!("minecraft:block/texture_{index}"))
            .collect();
        assert!(matches!(
            requested_keys(&available, &too_many),
            Err(BundleError::LimitExceeded(_))
        ));
    }

    #[test]
    fn atlas_cache_enforces_entry_and_byte_budgets() {
        let service = CatalogIconAtlasService::new();
        for index in 0..20 {
            service.insert(CatalogIconAtlas {
                schema_version: CATALOG_ATLAS_SCHEMA_VERSION,
                cache_key: format!("atlas-{index}"),
                cache_hit: false,
                catalog_id: "java-1.21.4-test".to_owned(),
                catalog_hash: "hash".to_owned(),
                catalog_schema_version: CATALOG_SCHEMA_VERSION,
                minecraft_version: "1.21.4".to_owned(),
                client_jar_sha1: "sha1".to_owned(),
                pack_stack_hash: "packs".to_owned(),
                width: 16,
                height: 16,
                png_base64: "x".repeat(1_100_000),
                textures: Vec::new(),
                requested_texture_count: 1,
                total_icon_texture_count: 1_000,
                warnings: Vec::new(),
                pack_issues: Vec::new(),
            });
        }
        let (entries, bytes) = service.cache_stats();
        assert!(entries < 20, "byte budget must evict older batches");
        assert!(entries <= MAX_CACHE_ENTRIES);
        assert!(bytes <= MAX_CACHE_BYTES);
    }
}
