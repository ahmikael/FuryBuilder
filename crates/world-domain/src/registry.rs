use std::collections::BTreeMap;

use crate::block::{BlockState, BlockStateError};
use thiserror::Error;

pub const CATALOG_VERSION: &str = "1.21.4";

type CatalogEntry = (&'static str, &'static [(&'static str, &'static str)]);

/// Curated catalog for the target version. It covers common building blocks and the
/// directional states needed by generators. Unknown imported states remain representable
/// in the world, but writes through the command API must opt in explicitly.
const CATALOG: &[CatalogEntry] = &[
    ("minecraft:air", &[]),
    ("minecraft:stone", &[]),
    ("minecraft:granite", &[]),
    ("minecraft:diorite", &[]),
    ("minecraft:andesite", &[]),
    ("minecraft:deepslate", &[("axis", "y")]),
    ("minecraft:cobblestone", &[]),
    ("minecraft:cobbled_deepslate", &[]),
    ("minecraft:deepslate_bricks", &[]),
    ("minecraft:tuff", &[]),
    ("minecraft:calcite", &[]),
    ("minecraft:dirt", &[]),
    ("minecraft:grass_block", &[("snowy", "false")]),
    ("minecraft:podzol", &[("snowy", "false")]),
    ("minecraft:mycelium", &[("snowy", "false")]),
    ("minecraft:sand", &[]),
    ("minecraft:red_sand", &[]),
    ("minecraft:gravel", &[]),
    ("minecraft:clay", &[]),
    ("minecraft:sandstone", &[]),
    ("minecraft:stone_bricks", &[]),
    ("minecraft:mossy_stone_bricks", &[]),
    ("minecraft:bricks", &[]),
    ("minecraft:oak_planks", &[]),
    ("minecraft:spruce_planks", &[]),
    ("minecraft:birch_planks", &[]),
    ("minecraft:dark_oak_planks", &[]),
    ("minecraft:oak_log", &[("axis", "y")]),
    ("minecraft:spruce_log", &[("axis", "y")]),
    ("minecraft:birch_log", &[("axis", "y")]),
    ("minecraft:dark_oak_log", &[("axis", "y")]),
    (
        "minecraft:oak_leaves",
        &[
            ("distance", "7"),
            ("persistent", "false"),
            ("waterlogged", "false"),
        ],
    ),
    (
        "minecraft:spruce_leaves",
        &[
            ("distance", "7"),
            ("persistent", "false"),
            ("waterlogged", "false"),
        ],
    ),
    ("minecraft:glass", &[]),
    ("minecraft:water", &[("level", "0")]),
    ("minecraft:lava", &[("level", "0")]),
    ("minecraft:ice", &[]),
    ("minecraft:packed_ice", &[]),
    ("minecraft:blue_ice", &[]),
    ("minecraft:snow_block", &[]),
    ("minecraft:snow", &[("layers", "1")]),
    ("minecraft:bedrock", &[]),
    ("minecraft:obsidian", &[]),
    ("minecraft:netherrack", &[]),
    ("minecraft:soul_sand", &[]),
    ("minecraft:glowstone", &[]),
    ("minecraft:sea_lantern", &[]),
    ("minecraft:quartz_block", &[]),
    ("minecraft:smooth_stone", &[]),
    ("minecraft:polished_andesite", &[]),
    ("minecraft:amethyst_block", &[]),
    ("minecraft:copper_block", &[]),
    ("minecraft:gold_block", &[]),
    ("minecraft:iron_block", &[]),
    ("minecraft:diamond_block", &[]),
    ("minecraft:emerald_block", &[]),
    ("minecraft:redstone_block", &[]),
    ("minecraft:coal_block", &[]),
    (
        "minecraft:oak_stairs",
        &[
            ("facing", "north"),
            ("half", "bottom"),
            ("shape", "straight"),
            ("waterlogged", "false"),
        ],
    ),
    (
        "minecraft:stone_stairs",
        &[
            ("facing", "north"),
            ("half", "bottom"),
            ("shape", "straight"),
            ("waterlogged", "false"),
        ],
    ),
    (
        "minecraft:oak_slab",
        &[("type", "bottom"), ("waterlogged", "false")],
    ),
    (
        "minecraft:stone_slab",
        &[("type", "bottom"), ("waterlogged", "false")],
    ),
    (
        "minecraft:oak_fence",
        &[
            ("east", "false"),
            ("north", "false"),
            ("south", "false"),
            ("waterlogged", "false"),
            ("west", "false"),
        ],
    ),
    (
        "minecraft:oak_door",
        &[
            ("facing", "north"),
            ("half", "lower"),
            ("hinge", "left"),
            ("open", "false"),
            ("powered", "false"),
        ],
    ),
    (
        "minecraft:oak_trapdoor",
        &[
            ("facing", "north"),
            ("half", "bottom"),
            ("open", "false"),
            ("powered", "false"),
            ("waterlogged", "false"),
        ],
    ),
    ("minecraft:torch", &[]),
    ("minecraft:wall_torch", &[("facing", "north")]),
    (
        "minecraft:chest",
        &[
            ("facing", "north"),
            ("type", "single"),
            ("waterlogged", "false"),
        ],
    ),
    ("minecraft:crafting_table", &[]),
    (
        "minecraft:furnace",
        &[("facing", "north"), ("lit", "false")],
    ),
    ("minecraft:bookshelf", &[]),
    (
        "minecraft:barrel",
        &[("facing", "north"), ("open", "false")],
    ),
    (
        "minecraft:lantern",
        &[("hanging", "false"), ("waterlogged", "false")],
    ),
    (
        "minecraft:campfire",
        &[
            ("facing", "north"),
            ("lit", "true"),
            ("signal_fire", "false"),
            ("waterlogged", "false"),
        ],
    ),
    (
        "minecraft:glass_pane",
        &[
            ("east", "false"),
            ("north", "false"),
            ("south", "false"),
            ("waterlogged", "false"),
            ("west", "false"),
        ],
    ),
    ("minecraft:white_wool", &[]),
    ("minecraft:red_wool", &[]),
    ("minecraft:blue_wool", &[]),
    ("minecraft:yellow_wool", &[]),
    ("minecraft:green_wool", &[]),
    ("minecraft:black_wool", &[]),
    ("minecraft:white_concrete", &[]),
    ("minecraft:red_concrete", &[]),
    ("minecraft:blue_concrete", &[]),
    ("minecraft:terracotta", &[]),
    ("minecraft:white_terracotta", &[]),
    ("minecraft:red_terracotta", &[]),
    ("minecraft:moss_block", &[]),
    ("minecraft:moss_carpet", &[]),
    ("minecraft:azalea_leaves", &[]),
    ("minecraft:flowering_azalea_leaves", &[]),
];

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RegistryError {
    #[error("unknown block for {CATALOG_VERSION}: `{0}`")]
    UnknownBlock(String),
    #[error("block `{block}` has no property `{property}`")]
    UnknownProperty { block: String, property: String },
    #[error(transparent)]
    InvalidState(#[from] BlockStateError),
}

/// One curated block with its default property set, safe to send to the UI.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockDefinition {
    pub name: String,
    pub properties: BTreeMap<String, String>,
}

/// Versioned catalog of known blocks and their default property sets.
#[derive(Debug, Clone)]
pub struct BlockRegistry {
    blocks: BTreeMap<&'static str, BTreeMap<&'static str, &'static str>>,
}

impl BlockRegistry {
    pub fn target_1_21_4() -> Self {
        let mut blocks = BTreeMap::new();
        for (name, defaults) in CATALOG {
            let map = defaults.iter().copied().collect();
            blocks.insert(*name, map);
        }
        Self { blocks }
    }

    pub fn len(&self) -> usize {
        self.blocks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    pub fn is_known(&self, name: &str) -> bool {
        self.blocks.contains_key(name)
    }

    pub fn default_state(&self, name: &str) -> Result<BlockState, RegistryError> {
        let defaults = self
            .blocks
            .get(name)
            .ok_or_else(|| RegistryError::UnknownBlock(name.to_owned()))?;
        let properties = defaults
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect();
        Ok(BlockState::new(name, properties)?)
    }

    /// Merges supplied properties over the catalog defaults and rejects unknown keys.
    pub fn canonicalize(
        &self,
        name: &str,
        overrides: BTreeMap<String, String>,
    ) -> Result<BlockState, RegistryError> {
        let defaults = self
            .blocks
            .get(name)
            .ok_or_else(|| RegistryError::UnknownBlock(name.to_owned()))?;
        for key in overrides.keys() {
            if !defaults.contains_key(key.as_str()) {
                return Err(RegistryError::UnknownProperty {
                    block: name.to_owned(),
                    property: key.clone(),
                });
            }
        }
        let mut properties: BTreeMap<String, String> = defaults
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect();
        properties.extend(overrides);
        Ok(BlockState::new(name, properties)?)
    }

    /// Validates a state against the catalog. Missing properties are allowed for
    /// imported data; unknown blocks/properties are rejected for command writes.
    pub fn validate(&self, state: &BlockState) -> Result<(), RegistryError> {
        let defaults = self
            .blocks
            .get(state.name())
            .ok_or_else(|| RegistryError::UnknownBlock(state.name().to_owned()))?;
        for key in state.properties().keys() {
            if !defaults.contains_key(key.as_str()) {
                return Err(RegistryError::UnknownProperty {
                    block: state.name().to_owned(),
                    property: key.clone(),
                });
            }
        }
        Ok(())
    }

    pub fn names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.blocks.keys().copied()
    }

    /// Full curated catalog with canonical defaults, ordered by block id.
    pub fn definitions(&self) -> Vec<BlockDefinition> {
        self.blocks
            .iter()
            .map(|(name, defaults)| BlockDefinition {
                name: (*name).to_owned(),
                properties: defaults
                    .iter()
                    .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                    .collect(),
            })
            .collect()
    }
}

impl Default for BlockRegistry {
    fn default() -> Self {
        Self::target_1_21_4()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_has_common_blocks_and_defaults() {
        let registry = BlockRegistry::target_1_21_4();
        assert!(registry.len() >= 60);
        assert!(registry.is_known("minecraft:stone_bricks"));
        assert!(registry.is_known("minecraft:oak_stairs"));

        let stairs = registry.default_state("minecraft:oak_stairs").unwrap();
        assert_eq!(
            stairs.properties().get("facing").map(String::as_str),
            Some("north")
        );
        assert_eq!(
            stairs.properties().get("half").map(String::as_str),
            Some("bottom")
        );
    }

    #[test]
    fn canonicalize_merges_overrides_and_rejects_unknown_properties() {
        let registry = BlockRegistry::target_1_21_4();
        let mut overrides = BTreeMap::new();
        overrides.insert("facing".to_owned(), "east".to_owned());
        let state = registry
            .canonicalize("minecraft:oak_stairs", overrides)
            .unwrap();
        assert_eq!(
            state.properties().get("facing").map(String::as_str),
            Some("east")
        );
        assert_eq!(
            state.properties().get("shape").map(String::as_str),
            Some("straight")
        );

        let mut bad = BTreeMap::new();
        bad.insert("axis".to_owned(), "x".to_owned());
        assert!(matches!(
            registry.canonicalize("minecraft:stone", bad),
            Err(RegistryError::UnknownProperty { .. })
        ));
    }

    #[test]
    fn validate_accepts_imported_states_without_all_properties() {
        let registry = BlockRegistry::target_1_21_4();
        assert!(registry
            .validate(&"minecraft:oak_stairs[facing=south]".parse().unwrap())
            .is_ok());
        assert!(registry
            .validate(&"minecraft:modded_block[facing=south]".parse().unwrap())
            .is_err());
    }

    #[test]
    fn definitions_are_sorted_and_carry_canonical_defaults() {
        let registry = BlockRegistry::target_1_21_4();
        let definitions = registry.definitions();
        assert_eq!(definitions.len(), registry.len());
        let names: Vec<&str> = definitions
            .iter()
            .map(|entry| entry.name.as_str())
            .collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted);
        let stairs = definitions
            .iter()
            .find(|entry| entry.name == "minecraft:oak_stairs")
            .expect("stairs are curated");
        assert_eq!(
            stairs.properties.get("facing").map(String::as_str),
            Some("north")
        );
        assert_eq!(
            stairs.properties.get("waterlogged").map(String::as_str),
            Some("false")
        );
    }
}
