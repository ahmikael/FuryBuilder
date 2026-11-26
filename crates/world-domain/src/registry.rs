use std::collections::{BTreeMap, BTreeSet};

use crate::block::{BlockState, BlockStateError};
use thiserror::Error;

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
    ("minecraft:short_grass", &[]),
    ("minecraft:fern", &[]),
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
    #[error("unknown block: `{0}`")]
    UnknownBlock(String),
    #[error("block `{block}` has no property `{property}`")]
    UnknownProperty { block: String, property: String },
    #[error("block `{block}` property `{property}` has no value `{value}`")]
    UnknownValue {
        block: String,
        property: String,
        value: String,
    },
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

/// One block schema derived from a project catalog. Only property keys and the values the
/// blockstate file declares are needed for write admission; the catalog keeps the full value
/// sets for the state picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockSchemaEntry {
    pub name: String,
    pub properties: BTreeSet<String>,
    pub values: BTreeMap<String, BTreeSet<String>>,
    pub property_values_complete: bool,
    pub default_properties: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
struct RegistryEntry {
    defaults: BTreeMap<String, String>,
    properties: BTreeSet<String>,
    values: BTreeMap<String, BTreeSet<String>>,
    values_complete: bool,
}

/// Versioned catalog of known blocks and their default property sets.
#[derive(Debug, Clone)]
pub struct BlockRegistry {
    blocks: BTreeMap<String, RegistryEntry>,
    /// Property keys declared anywhere in a catalog-derived registry. Blockstate files omit
    /// properties that do not affect a model, so an undeclared but globally known key is
    /// accepted for writes while an unknown key is still rejected.
    known_properties: BTreeSet<String>,
}

impl BlockRegistry {
    /// Version-independent curated quick picks and no-client-JAR fallback list.
    pub fn curated() -> Self {
        Self::target_1_21_4()
    }

    pub fn target_1_21_4() -> Self {
        let mut blocks = BTreeMap::new();
        for (name, defaults) in CATALOG {
            let defaults: BTreeMap<String, String> = defaults
                .iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                .collect();
            let properties = defaults.keys().cloned().collect();
            blocks.insert(
                (*name).to_owned(),
                RegistryEntry {
                    defaults,
                    properties,
                    values: BTreeMap::new(),
                    values_complete: false,
                },
            );
        }
        Self {
            blocks,
            known_properties: BTreeSet::new(),
        }
    }

    /// Builds a registry from an exact project catalog. Catalog blocks may declare no
    /// default properties (ambiguous or multipart blockstates); their declared property
    /// keys still admit writes, and values are checked only when the blockstate file
    /// enumerates all values for the key.
    pub fn from_schema(entries: impl IntoIterator<Item = BlockSchemaEntry>) -> Self {
        let mut known_properties = BTreeSet::new();
        let blocks = entries
            .into_iter()
            .map(|entry| {
                known_properties.extend(entry.properties.iter().cloned());
                known_properties.extend(entry.default_properties.keys().cloned());
                let mut properties = entry.properties;
                properties.extend(entry.default_properties.keys().cloned());
                (
                    entry.name,
                    RegistryEntry {
                        defaults: entry.default_properties,
                        properties,
                        values: entry.values,
                        values_complete: entry.property_values_complete,
                    },
                )
            })
            .collect();
        Self {
            blocks,
            known_properties,
        }
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
        let defaults = &self
            .blocks
            .get(name)
            .ok_or_else(|| RegistryError::UnknownBlock(name.to_owned()))?
            .defaults;
        let properties = defaults
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        Ok(BlockState::new(name, properties)?)
    }

    /// Merges supplied properties over the catalog defaults and rejects unknown keys and,
    /// when the catalog enumerates them, unknown values.
    pub fn canonicalize(
        &self,
        name: &str,
        overrides: BTreeMap<String, String>,
    ) -> Result<BlockState, RegistryError> {
        let entry = self
            .blocks
            .get(name)
            .ok_or_else(|| RegistryError::UnknownBlock(name.to_owned()))?;
        for (key, value) in &overrides {
            self.validate_property(entry, name, key, value)?;
        }
        let mut properties: BTreeMap<String, String> = entry.defaults.clone();
        properties.extend(overrides);
        Ok(BlockState::new(name, properties)?)
    }

    /// Validates a state against the catalog. Missing properties are allowed for imported
    /// data and mean "use the game default". Keys declared by the block are checked against
    /// the declared value set when that set is complete. A catalog-derived registry also
    /// accepts a globally known key that this blockstate file omits (for example
    /// `waterlogged` on stairs), because such properties never affect a model.
    pub fn validate(&self, state: &BlockState) -> Result<(), RegistryError> {
        let entry = self
            .blocks
            .get(state.name())
            .ok_or_else(|| RegistryError::UnknownBlock(state.name().to_owned()))?;
        for (key, value) in state.properties() {
            self.validate_property(entry, state.name(), key, value)?;
        }
        Ok(())
    }

    fn validate_property(
        &self,
        entry: &RegistryEntry,
        block: &str,
        key: &str,
        value: &str,
    ) -> Result<(), RegistryError> {
        if entry.properties.contains(key) {
            if entry.values_complete {
                if let Some(values) = entry.values.get(key) {
                    if !values.contains(value) {
                        return Err(RegistryError::UnknownValue {
                            block: block.to_owned(),
                            property: key.to_owned(),
                            value: value.to_owned(),
                        });
                    }
                }
            }
            return Ok(());
        }
        if !self.known_properties.is_empty() && self.known_properties.contains(key) {
            return Ok(());
        }
        Err(RegistryError::UnknownProperty {
            block: block.to_owned(),
            property: key.to_owned(),
        })
    }

    pub fn names(&self) -> impl Iterator<Item = &str> + '_ {
        self.blocks.keys().map(String::as_str)
    }

    /// Full curated catalog with canonical defaults, ordered by block id.
    pub fn definitions(&self) -> Vec<BlockDefinition> {
        self.blocks
            .iter()
            .map(|(name, entry)| BlockDefinition {
                name: name.clone(),
                properties: entry.defaults.clone(),
            })
            .collect()
    }
}

impl Default for BlockRegistry {
    fn default() -> Self {
        Self::curated()
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

    #[test]
    fn schema_registry_admits_catalog_blocks_and_property_keys() {
        let registry = BlockRegistry::from_schema([
            BlockSchemaEntry {
                name: "minecraft:deepslate_bricks".to_owned(),
                properties: BTreeSet::new(),
                values: BTreeMap::new(),
                property_values_complete: false,
                default_properties: BTreeMap::new(),
            },
            BlockSchemaEntry {
                name: "minecraft:oak_fence".to_owned(),
                properties: ["east", "north", "south", "west"]
                    .into_iter()
                    .map(ToOwned::to_owned)
                    .collect(),
                values: BTreeMap::new(),
                property_values_complete: false,
                default_properties: BTreeMap::new(),
            },
        ]);
        assert!(registry.is_known("minecraft:deepslate_bricks"));
        assert!(registry
            .validate(&"minecraft:deepslate_bricks".parse().unwrap())
            .is_ok());
        assert!(registry
            .validate(&"minecraft:oak_fence[north=true]".parse().unwrap())
            .is_ok());
        assert!(matches!(
            registry.validate(&"minecraft:oak_fence[axis=x]".parse().unwrap()),
            Err(RegistryError::UnknownProperty { .. })
        ));
        assert!(registry
            .validate(&"minecraft:oak_stairs".parse().unwrap())
            .is_err());
    }

    #[test]
    fn schema_registry_checks_values_only_when_the_blockstate_is_complete() {
        let mut stairs_values = BTreeMap::new();
        stairs_values.insert(
            "facing".to_owned(),
            ["north", "south"]
                .into_iter()
                .map(ToOwned::to_owned)
                .collect(),
        );
        stairs_values.insert(
            "waterlogged".to_owned(),
            ["false", "true"]
                .into_iter()
                .map(ToOwned::to_owned)
                .collect(),
        );
        let mut fence_values = BTreeMap::new();
        fence_values.insert(
            "north".to_owned(),
            ["true"].into_iter().map(ToOwned::to_owned).collect(),
        );
        let registry = BlockRegistry::from_schema([
            BlockSchemaEntry {
                name: "minecraft:oak_stairs".to_owned(),
                properties: stairs_values.keys().cloned().collect(),
                values: stairs_values,
                property_values_complete: true,
                default_properties: BTreeMap::new(),
            },
            BlockSchemaEntry {
                name: "minecraft:oak_fence".to_owned(),
                properties: ["north"].into_iter().map(ToOwned::to_owned).collect(),
                values: fence_values,
                property_values_complete: false,
                default_properties: BTreeMap::new(),
            },
        ]);

        assert!(registry
            .validate(
                &"minecraft:oak_stairs[facing=south,waterlogged=true]"
                    .parse()
                    .unwrap()
            )
            .is_ok());
        assert!(matches!(
            registry.validate(&"minecraft:oak_stairs[facing=up]".parse().unwrap()),
            Err(RegistryError::UnknownValue { .. })
        ));
        // Multipart conditions only name `true`, so `false` is valid even though the
        // blockstate file does not enumerate it.
        assert!(registry
            .validate(&"minecraft:oak_fence[north=false]".parse().unwrap())
            .is_ok());
        // A key declared by another catalog block is accepted because blockstates omit
        // properties that do not affect a model.
        assert!(registry
            .validate(&"minecraft:oak_fence[waterlogged=true]".parse().unwrap())
            .is_ok());
        assert!(matches!(
            registry.validate(&"minecraft:oak_fence[axis=x]".parse().unwrap()),
            Err(RegistryError::UnknownProperty { .. })
        ));
        let overrides = BTreeMap::from([("facing".to_owned(), "up".to_owned())]);
        assert!(matches!(
            registry.canonicalize("minecraft:oak_stairs", overrides),
            Err(RegistryError::UnknownValue { .. })
        ));
    }
}
