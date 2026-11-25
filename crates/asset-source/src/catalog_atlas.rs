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

use crate::bundle::{build_atlas_with_limit, BundleError, ResourceStack};
use crate::catalog::CatalogSnapshot;
use crate::{AssetSourceSetting, ResourcePackInput};

/// Version of the icon-atlas DTO and packing output.
pub const CATALOG_ATLAS_SCHEMA_VERSION: u32 = 1;
/// A vanilla catalog has a few thousand distinct icon textures.
pub const MAX_CATALOG_ATLAS_TEXTURES: usize = 4_096;

const MAX_CACHE_ENTRIES: usize = 4;
const MAX_CACHE_BYTES: usize = 64 * 1024 * 1024;

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

    /// Builds (or reuses) the icon atlas for a catalog and the current resource-pack stack.
    pub fn atlas(
        &self,
        profile: &VersionProfile,
        source: &AssetSourceSetting,
        packs: &[ResourcePackInput],
        catalog: &CatalogSnapshot,
    ) -> Result<CatalogIconAtlas, BundleError> {
        let mut stack = ResourceStack::open(
            profile,
            Some(source),
            catalog.client_jar_sha1.as_deref(),
            packs,
        )?;
        let key = atlas_cache_key(catalog, &stack)?;
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

        let keys = icon_keys(catalog);
        let built = build_atlas_with_limit(&mut stack, &keys, MAX_CATALOG_ATLAS_TEXTURES + 1)?;
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
            warnings: built.warnings,
            pack_issues: Vec::new(),
        };
        let mut atlas = self.insert(atlas);
        atlas.pack_issues = stack.pack_issues;
        atlas.warnings.splice(..0, stack.warnings);
        Ok(atlas)
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

fn icon_keys(catalog: &CatalogSnapshot) -> BTreeSet<String> {
    catalog
        .blocks
        .iter()
        .filter_map(|block| block.icon_texture.clone())
        .chain(
            catalog
                .items
                .iter()
                .filter_map(|item| item.icon_texture.clone()),
        )
        .collect()
}

fn atlas_cache_key(
    catalog: &CatalogSnapshot,
    stack: &ResourceStack,
) -> Result<String, BundleError> {
    let sha1 = catalog.client_jar_sha1.as_deref().ok_or_else(|| {
        BundleError::UnsupportedFormat(
            "the client JAR SHA-1 is unavailable; select a verified client JAR".to_owned(),
        )
    })?;
    let schema = format!("{}.{CATALOG_ATLAS_SCHEMA_VERSION}", catalog.schema_version);
    Ok(format!(
        "{}|{}|{}|{}",
        catalog.minecraft_version, sha1, stack.pack_stack_hash, schema
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::CATALOG_SCHEMA_VERSION;

    #[test]
    fn icon_keys_are_deduplicated_and_ignore_missing_icons() {
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
            blocks: vec![
                crate::catalog::CatalogBlock {
                    id: "minecraft:stone".to_owned(),
                    display_name: None,
                    properties: Default::default(),
                    property_values_complete: true,
                    default_properties: None,
                    variants: 1,
                    multipart: false,
                    model: None,
                    icon_texture: Some("minecraft:block/stone".to_owned()),
                    tags: Vec::new(),
                    has_item: false,
                },
                crate::catalog::CatalogBlock {
                    id: "minecraft:dirt".to_owned(),
                    display_name: None,
                    properties: Default::default(),
                    property_values_complete: true,
                    default_properties: None,
                    variants: 1,
                    multipart: false,
                    model: None,
                    icon_texture: Some("minecraft:block/stone".to_owned()),
                    tags: Vec::new(),
                    has_item: false,
                },
            ],
            items: vec![crate::catalog::CatalogItem {
                id: "minecraft:sword".to_owned(),
                display_name: None,
                item_definition: true,
                model_type: None,
                model: None,
                icon_texture: None,
                block: None,
                tags: Vec::new(),
            }],
            atlases: Vec::new(),
            counts: Default::default(),
            warnings: Vec::new(),
            cache_hit: false,
        };
        let keys = icon_keys(&catalog);
        assert_eq!(keys.len(), 1);
        assert!(keys.contains("minecraft:block/stone"));
    }
}
