//! Numeric block/item registry used by Java 1.8–1.12.2 worlds.
//!
//! Pre-flattening saves store a numeric block id plus a 4-bit metadata value,
//! and items store a numeric item id plus a damage value. The same numeric id
//! can describe several block states (for example `wool` metadata 0–15), and a
//! block item can have a different numeric id from its block (`wheat` is block
//! 59 and item 296).
//!
//! The table is the 1.12.2 registry, which is the superset of the supported
//! 1.8–1.12.2 releases. Numeric ids of existing entries are stable across those
//! releases; entries added in a later release are simply absent from older
//! worlds. Catalog annotation only attaches ids whose block or item is present
//! in the exact client JAR, so a 1.8 catalog cannot claim a 1.12 entry.
//!
//! Data provenance: the flattening names were cross-checked against
//! PrismarineJS `minecraft-data` `data/pc/common/legacy.json` (the same
//! registry used by the project for legacy biome ids) and against the blockstate
//! asset names inside the official 1.8.9 and 1.12.2 client JARs. The table is
//! data only: no world bytes, names or values are inferred from modded data.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::sync::OnceLock;

use serde::Deserialize;

use crate::{VersionFamily, VersionId};

/// One numeric block state: id + metadata resolved to a flattened name and its
/// explicit state properties.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyBlockState {
    pub id: u16,
    pub metadata: u16,
    /// Flattened block name, for example `minecraft:oak_sapling`.
    pub name: String,
    /// State properties carried by the flattening table, for example
    /// `stage=1`. Empty for blocks whose metadata does not encode a property.
    pub properties: BTreeMap<String, String>,
}

impl LegacyBlockState {
    /// Returns the flattened state string, for example
    /// `minecraft:oak_sapling[stage=1]`.
    pub fn state_string(&self) -> String {
        if self.properties.is_empty() {
            return self.name.clone();
        }
        let properties = self
            .properties
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join(",");
        format!("{}[{properties}]", self.name)
    }
}

impl fmt::Display for LegacyBlockState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.state_string())
    }
}

/// One numeric item id + damage value resolved to a flattened item name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyItem {
    pub id: u16,
    pub damage: u16,
    pub name: String,
}

/// Read-only lookup of the pre-flattening numeric registry.
pub struct LegacyRegistry {
    blocks: Vec<LegacyBlockState>,
    items: Vec<LegacyItem>,
    blocks_by_name: HashMap<String, Vec<usize>>,
    items_by_name: HashMap<String, Vec<usize>>,
}

#[derive(Deserialize)]
struct RawRegistry {
    blocks: Vec<(u16, u16, String)>,
    items: Vec<(u16, u16, String)>,
}

const REGISTRY_JSON: &str = include_str!("../data/legacy-registry.json");

impl LegacyRegistry {
    /// Returns the shared 1.12.2 superset registry.
    pub fn shared() -> &'static Self {
        static REGISTRY: OnceLock<LegacyRegistry> = OnceLock::new();
        REGISTRY.get_or_init(|| {
            LegacyRegistry::from_json(REGISTRY_JSON)
                .expect("the built-in legacy registry must be valid")
        })
    }

    /// Returns the registry for an exact Java release from 1.8 through 1.12.2.
    ///
    /// Snapshots, pre-releases and older/newer releases return `None`: their
    /// numeric registries are not claimed without an exact fixture.
    pub fn for_release(version: &VersionId) -> Option<&'static Self> {
        if !version.is_release() || version.family() != VersionFamily::Numeric {
            return None;
        }
        Some(Self::shared())
    }

    pub fn block(&self, id: u16, metadata: u16) -> Option<&LegacyBlockState> {
        self.blocks
            .binary_search_by_key(&(id, metadata), |block| (block.id, block.metadata))
            .ok()
            .map(|index| &self.blocks[index])
    }

    pub fn item(&self, id: u16, damage: u16) -> Option<&LegacyItem> {
        self.items
            .binary_search_by_key(&(id, damage), |item| (item.id, item.damage))
            .ok()
            .map(|index| &self.items[index])
    }

    /// Every numeric state whose flattened base name is `name`.
    pub fn blocks_named<'a>(
        &'a self,
        name: &str,
    ) -> impl Iterator<Item = &'a LegacyBlockState> + 'a {
        self.blocks_by_name
            .get(name)
            .into_iter()
            .flatten()
            .map(|index| &self.blocks[*index])
    }

    /// Every numeric item whose flattened name is `name`.
    pub fn items_named<'a>(&'a self, name: &str) -> impl Iterator<Item = &'a LegacyItem> + 'a {
        self.items_by_name
            .get(name)
            .into_iter()
            .flatten()
            .map(|index| &self.items[*index])
    }

    fn from_json(json: &str) -> Result<Self, String> {
        let raw: RawRegistry = serde_json::from_str(json)
            .map_err(|error| format!("invalid legacy registry: {error}"))?;
        if raw.blocks.is_empty() || raw.items.is_empty() {
            return Err("the legacy registry must not be empty".to_owned());
        }
        let mut blocks = Vec::with_capacity(raw.blocks.len());
        for (id, metadata, state) in raw.blocks {
            let (name, properties) = parse_state(&state)?;
            blocks.push(LegacyBlockState {
                id,
                metadata,
                name,
                properties,
            });
        }
        let mut items = Vec::with_capacity(raw.items.len());
        for (id, damage, name) in raw.items {
            validate_name(&name)?;
            items.push(LegacyItem { id, damage, name });
        }
        blocks.sort_by_key(|block| (block.id, block.metadata));
        items.sort_by_key(|item| (item.id, item.damage));
        let blocks_by_name = index_by_name(blocks.iter().map(|block| block.name.as_str()));
        let items_by_name = index_by_name(items.iter().map(|item| item.name.as_str()));
        for pair in blocks.windows(2) {
            if (pair[0].id, pair[0].metadata) == (pair[1].id, pair[1].metadata) {
                return Err(format!(
                    "duplicate legacy block id {}:{}",
                    pair[0].id, pair[0].metadata
                ));
            }
        }
        for pair in items.windows(2) {
            if (pair[0].id, pair[0].damage) == (pair[1].id, pair[1].damage) {
                return Err(format!(
                    "duplicate legacy item id {}:{}",
                    pair[0].id, pair[0].damage
                ));
            }
        }
        Ok(Self {
            blocks,
            items,
            blocks_by_name,
            items_by_name,
        })
    }
}

fn index_by_name<'a>(entries: impl Iterator<Item = &'a str>) -> HashMap<String, Vec<usize>> {
    let mut index: HashMap<String, Vec<usize>> = HashMap::new();
    for (position, name) in entries.enumerate() {
        index.entry(name.to_owned()).or_default().push(position);
    }
    index
}

fn parse_state(state: &str) -> Result<(String, BTreeMap<String, String>), String> {
    let (name, properties) = match state.find('[') {
        Some(index) => {
            if !state.ends_with(']') {
                return Err(format!(
                    "legacy state `{state}` has an unterminated property list"
                ));
            }
            (&state[..index], Some(&state[index + 1..state.len() - 1]))
        }
        None => (state, None),
    };
    validate_name(name)?;
    let mut map = BTreeMap::new();
    if let Some(properties) = properties {
        if properties.is_empty() {
            return Err(format!("legacy state `{state}` has an empty property list"));
        }
        for property in properties.split(',') {
            let (key, value) = property
                .split_once('=')
                .ok_or_else(|| format!("legacy state `{state}` has a property without `=`"))?;
            if key.is_empty() || value.is_empty() {
                return Err(format!("legacy state `{state}` has an empty property"));
            }
            if map.insert(key.to_owned(), value.to_owned()).is_some() {
                return Err(format!("legacy state `{state}` repeats property `{key}`"));
            }
        }
    }
    Ok((name.to_owned(), map))
}

fn validate_name(name: &str) -> Result<(), String> {
    let Some((namespace, path)) = name.split_once(':') else {
        return Err(format!("legacy name `{name}` is not namespaced"));
    };
    if namespace.is_empty() || path.is_empty() {
        return Err(format!("legacy name `{name}` is incomplete"));
    }
    if !name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-' | b':'))
    {
        return Err(format!(
            "legacy name `{name}` contains an invalid character"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(id: &str) -> VersionId {
        id.parse().expect("version id")
    }

    #[test]
    fn the_registry_exposes_every_numeric_block_state() {
        let registry = LegacyRegistry::shared();
        assert_eq!(registry.blocks.len(), 1682);
        assert_eq!(registry.items.len(), 644);
        assert!(registry.blocks.iter().all(|block| block.id <= 255));
        assert!(registry.blocks.iter().all(|block| block.metadata <= 15));
        assert!(registry
            .blocks
            .iter()
            .all(|block| block.name.starts_with("minecraft:")));
        assert!(registry
            .items
            .iter()
            .all(|item| item.name.starts_with("minecraft:")));
    }

    #[test]
    fn resolves_blocks_across_metadata_variants() {
        let registry = LegacyRegistry::shared();
        let stone = registry.block(1, 0).expect("stone");
        assert_eq!(stone.name, "minecraft:stone");
        assert!(stone.properties.is_empty());

        let granite = registry.block(1, 1).expect("granite");
        assert_eq!(granite.name, "minecraft:granite");

        let grass = registry.block(2, 0).expect("grass");
        assert_eq!(grass.name, "minecraft:grass_block");
        assert_eq!(
            grass.properties.get("snowy").map(String::as_str),
            Some("false")
        );

        let sapling = registry.block(6, 8).expect("oak sapling stage 1");
        assert_eq!(sapling.name, "minecraft:oak_sapling");
        assert_eq!(
            sapling.properties.get("stage").map(String::as_str),
            Some("1")
        );

        let structure = registry.block(255, 0).expect("structure block");
        assert_eq!(
            structure.state_string(),
            "minecraft:structure_block[mode=save]"
        );

        let wool = registry
            .blocks_named("minecraft:white_wool")
            .collect::<Vec<_>>();
        assert_eq!(wool.len(), 1);
        assert_eq!((wool[0].id, wool[0].metadata), (35, 0));
    }

    #[test]
    fn resolves_item_ids_separately_from_block_ids() {
        let registry = LegacyRegistry::shared();
        let wheat_item = registry.item(296, 0).expect("wheat item");
        assert_eq!(wheat_item.name, "minecraft:wheat");
        let wheat_block = registry.block(59, 0).expect("wheat block");
        assert_eq!(wheat_block.name, "minecraft:wheat");

        let charcoal = registry.item(263, 1).expect("charcoal");
        assert_eq!(charcoal.name, "minecraft:charcoal");
        let record = registry.item(2258, 0).expect("music disc");
        assert_eq!(record.name, "minecraft:music_disc_blocks");

        assert!(registry.block(59, 99).is_none());
        assert!(registry.item(296, 1).is_none());
    }

    #[test]
    fn name_indexes_survive_unsorted_input() {
        let registry = LegacyRegistry::from_json(
            r#"{
                "blocks": [[2, 0, "minecraft:second"], [1, 0, "minecraft:first"], [1, 1, "minecraft:first[stage=1]"]],
                "items": [[300, 0, "minecraft:later"], [200, 0, "minecraft:earlier"]]
            }"#,
        )
        .expect("unsorted registry");
        let first = registry
            .blocks_named("minecraft:first")
            .map(|block| (block.id, block.metadata))
            .collect::<Vec<_>>();
        assert_eq!(first, vec![(1, 0), (1, 1)]);
        assert_eq!(
            registry
                .items_named("minecraft:later")
                .map(|item| item.id)
                .collect::<Vec<_>>(),
            vec![300]
        );
        assert_eq!(
            registry.block(2, 0).map(|block| block.name.as_str()),
            Some("minecraft:second")
        );
    }

    #[test]
    fn only_exact_numeric_releases_get_the_registry() {
        for id in [
            "1.8", "1.8.9", "1.9.4", "1.10.2", "1.11.2", "1.12", "1.12.2",
        ] {
            assert!(LegacyRegistry::for_release(&version(id)).is_some(), "{id}");
        }
        for id in ["1.7.10", "1.13", "1.13.2", "1.21.4", "1.12-pre4"] {
            assert!(LegacyRegistry::for_release(&version(id)).is_none(), "{id}");
        }
    }
}
