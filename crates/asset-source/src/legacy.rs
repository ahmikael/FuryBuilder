//! Maps pre-flattening numeric ids into the shared block/item catalog.
//!
//! The exact client JAR remains the authority for which blocks and items exist.
//! This module only translates an asset name to the flattened names in the
//! numeric registry, and filters numeric states by the properties declared by
//! the JAR. A rename without a fixed property match is never guessed: the
//! catalog receives every numeric id that the JAR cannot distinguish.

use std::collections::BTreeMap;

use minecraft_version::{LegacyBlockState, LegacyItem, LegacyRegistry};

use crate::catalog::CatalogLegacyId;

struct BlockAlias {
    names: &'static [&'static str],
    fixed: &'static [(&'static str, &'static str)],
    half_to_type: bool,
}

struct ItemAlias {
    names: &'static [&'static str],
}

/// Resolves the numeric block states that belong to one client-JAR asset name.
pub(crate) fn block_ids(registry: &LegacyRegistry, asset: &str) -> Vec<CatalogLegacyId> {
    matching_block_states(registry, asset, None)
        .into_iter()
        .map(block_id)
        .collect()
}

/// Resolves the exact numeric block states for one catalog variant.
pub(crate) fn variant_ids(
    registry: &LegacyRegistry,
    asset: &str,
    properties: &BTreeMap<String, String>,
) -> Vec<CatalogLegacyId> {
    matching_block_states(registry, asset, Some(properties))
        .into_iter()
        .map(block_id)
        .collect()
}

/// Resolves the numeric item ids that belong to one client-JAR item name.
pub(crate) fn item_ids(registry: &LegacyRegistry, asset: &str) -> Vec<CatalogLegacyId> {
    let alias = item_alias(asset);
    let names = alias
        .map(|alias| alias.names)
        .unwrap_or_else(|| std::slice::from_ref(&asset));
    let mut ids = Vec::new();
    for name in names {
        let name = registry_name(name);
        for item in registry.items_named(&name) {
            ids.push(item_id(item));
        }
    }
    sort_and_dedup(ids)
}

fn registry_name(name: &str) -> String {
    if name.contains(':') {
        name.to_owned()
    } else {
        format!("minecraft:{name}")
    }
}

fn matching_block_states<'a>(
    registry: &'a LegacyRegistry,
    asset: &'a str,
    properties: Option<&BTreeMap<String, String>>,
) -> Vec<&'a LegacyBlockState> {
    let alias = block_alias(asset);
    let names = alias
        .as_ref()
        .map(|alias| alias.names)
        .unwrap_or_else(|| std::slice::from_ref(&asset));
    let fixed = alias.as_ref().map(|alias| alias.fixed).unwrap_or(&[]);
    let half_to_type = alias
        .as_ref()
        .map(|alias| alias.half_to_type)
        .unwrap_or_else(|| asset.ends_with("_slab") && !asset.ends_with("_double_slab"));
    let mut merged = properties.cloned().unwrap_or_default();
    if half_to_type {
        if let Some(half) = merged.remove("half") {
            merged.insert("type".to_owned(), half);
        }
    }
    for (key, value) in fixed {
        merged.insert((*key).to_owned(), (*value).to_owned());
    }

    let mut states: Vec<&LegacyBlockState> = Vec::new();
    for name in names {
        let name = registry_name(name);
        for state in registry.blocks_named(&name) {
            if !fixed
                .iter()
                .all(|(key, value)| state.properties.get(*key).map(String::as_str) == Some(value))
            {
                continue;
            }
            if properties.is_some()
                && !state
                    .properties
                    .iter()
                    .all(|(key, value)| merged.get(key) == Some(value))
            {
                continue;
            }
            states.push(state);
        }
    }
    states.sort_by_key(|state| (state.id, state.metadata));
    states.dedup_by_key(|state| (state.id, state.metadata));
    states
}

fn block_id(state: &LegacyBlockState) -> CatalogLegacyId {
    CatalogLegacyId {
        id: u32::from(state.id),
        metadata: Some(state.metadata),
    }
}

fn item_id(item: &LegacyItem) -> CatalogLegacyId {
    CatalogLegacyId {
        id: u32::from(item.id),
        metadata: Some(item.damage),
    }
}

fn sort_and_dedup(mut ids: Vec<CatalogLegacyId>) -> Vec<CatalogLegacyId> {
    ids.sort_by_key(|id| (id.id, id.metadata));
    ids.dedup_by_key(|id| (id.id, id.metadata));
    ids
}

fn block_alias(asset: &str) -> Option<BlockAlias> {
    let alias = match asset {
        "acacia_double_slab" => BlockAlias::double(&["acacia_slab"]),
        "birch_double_slab" => BlockAlias::double(&["birch_slab"]),
        "brick_double_slab" => BlockAlias::double(&["brick_slab"]),
        "cobblestone_double_slab" => BlockAlias::double(&["cobblestone_slab"]),
        "dark_oak_double_slab" => BlockAlias::double(&["dark_oak_slab"]),
        "jungle_double_slab" => BlockAlias::double(&["jungle_slab"]),
        "nether_brick_double_slab" => BlockAlias::double(&["nether_brick_slab"]),
        "oak_double_slab" => BlockAlias::double(&["oak_slab"]),
        "purpur_double_slab" => BlockAlias::double(&["purpur_slab"]),
        "quartz_double_slab" => BlockAlias::double(&["quartz_slab"]),
        "red_sandstone_double_slab" => BlockAlias::double(&["red_sandstone_slab"]),
        "sandstone_double_slab" => BlockAlias::double(&["sandstone_slab"]),
        "spruce_double_slab" => BlockAlias::double(&["spruce_slab"]),
        "stone_brick_double_slab" => BlockAlias::double(&["stone_brick_slab"]),
        "stone_double_slab" => BlockAlias {
            names: &[
                "stone_slab",
                "sandstone_slab",
                "petrified_oak_slab",
                "cobblestone_slab",
                "brick_slab",
                "stone_brick_slab",
                "nether_brick_slab",
                "quartz_slab",
                "smooth_stone",
                "smooth_sandstone",
                "smooth_quartz",
            ],
            fixed: &[],
            half_to_type: false,
        },
        "wood_old_double_slab" => BlockAlias::double(&["oak_slab"]),
        "wood_old_slab" => BlockAlias {
            names: &["oak_slab"],
            fixed: &[],
            half_to_type: true,
        },
        "black_stained_hardened_clay" => BlockAlias::same(&["black_terracotta"]),
        "blue_stained_hardened_clay" => BlockAlias::same(&["blue_terracotta"]),
        "brown_stained_hardened_clay" => BlockAlias::same(&["brown_terracotta"]),
        "cyan_stained_hardened_clay" => BlockAlias::same(&["cyan_terracotta"]),
        "gray_stained_hardened_clay" => BlockAlias::same(&["gray_terracotta"]),
        "green_stained_hardened_clay" => BlockAlias::same(&["green_terracotta"]),
        "light_blue_stained_hardened_clay" => BlockAlias::same(&["light_blue_terracotta"]),
        "lime_stained_hardened_clay" => BlockAlias::same(&["lime_terracotta"]),
        "magenta_stained_hardened_clay" => BlockAlias::same(&["magenta_terracotta"]),
        "orange_stained_hardened_clay" => BlockAlias::same(&["orange_terracotta"]),
        "pink_stained_hardened_clay" => BlockAlias::same(&["pink_terracotta"]),
        "purple_stained_hardened_clay" => BlockAlias::same(&["purple_terracotta"]),
        "red_stained_hardened_clay" => BlockAlias::same(&["red_terracotta"]),
        "silver_stained_hardened_clay" => BlockAlias::same(&["light_gray_terracotta"]),
        "white_stained_hardened_clay" => BlockAlias::same(&["white_terracotta"]),
        "yellow_stained_hardened_clay" => BlockAlias::same(&["yellow_terracotta"]),
        "hardened_clay" => BlockAlias::same(&["terracotta"]),
        "silver_carpet" => BlockAlias::same(&["light_gray_carpet"]),
        "silver_concrete" => BlockAlias::same(&["light_gray_concrete"]),
        "silver_concrete_powder" => BlockAlias::same(&["light_gray_concrete_powder"]),
        "silver_glazed_terracotta" => BlockAlias::same(&["light_gray_glazed_terracotta"]),
        "silver_stained_glass" => BlockAlias::same(&["light_gray_stained_glass"]),
        "silver_stained_glass_pane" => BlockAlias::same(&["light_gray_stained_glass_pane"]),
        "silver_wool" => BlockAlias::same(&["light_gray_wool"]),
        "brick_block" => BlockAlias::same(&["bricks"]),
        "end_bricks" => BlockAlias::same(&["end_stone_bricks"]),
        "nether_brick" => BlockAlias::same(&["nether_bricks"]),
        "red_nether_brick" => BlockAlias::same(&["red_nether_bricks"]),
        "stonebrick" => BlockAlias::same(&["stone_bricks"]),
        "mossy_stonebrick" => BlockAlias::same(&["mossy_stone_bricks"]),
        "cracked_stonebrick" => BlockAlias::same(&["cracked_stone_bricks"]),
        "chiseled_stonebrick" => BlockAlias::same(&["chiseled_stone_bricks"]),
        "smooth_granite" => BlockAlias::same(&["polished_granite"]),
        "smooth_diorite" => BlockAlias::same(&["polished_diorite"]),
        "smooth_andesite" => BlockAlias::same(&["polished_andesite"]),
        "quartz_column" => BlockAlias::same(&["quartz_pillar"]),
        "quartz_ore" => BlockAlias::same(&["nether_quartz_ore"]),
        "magma" => BlockAlias::same(&["magma_block"]),
        "melon_block" => BlockAlias::same(&["melon"]),
        "mob_spawner" => BlockAlias::same(&["spawner"]),
        "noteblock" => BlockAlias::same(&["note_block"]),
        "reeds" => BlockAlias::same(&["sugar_cane"]),
        "web" => BlockAlias::same(&["cobweb"]),
        "waterlily" => BlockAlias::same(&["lily_pad"]),
        "portal" => BlockAlias::same(&["nether_portal"]),
        "slime" => BlockAlias::same(&["slime_block"]),
        "snow_layer" => BlockAlias::same(&["snow"]),
        "pumpkin" => BlockAlias::same(&["carved_pumpkin"]),
        "lit_pumpkin" => BlockAlias::same(&["jack_o_lantern"]),
        "golden_rail" => BlockAlias::same(&["powered_rail"]),
        "stone_stairs" => BlockAlias::same(&["cobblestone_stairs"]),
        "fence" => BlockAlias::same(&["oak_fence"]),
        "fence_gate" => BlockAlias::same(&["oak_fence_gate"]),
        "trapdoor" => BlockAlias::same(&["oak_trapdoor"]),
        "wooden_button" => BlockAlias::same(&["oak_button"]),
        "wooden_door" => BlockAlias::same(&["oak_door"]),
        "wooden_pressure_plate" => BlockAlias::same(&["oak_pressure_plate"]),
        "stone_monster_egg" => BlockAlias::same(&["infested_stone"]),
        "cobblestone_monster_egg" => BlockAlias::same(&["infested_cobblestone"]),
        "stone_brick_monster_egg" => BlockAlias::same(&["infested_stone_bricks"]),
        "mossy_brick_monster_egg" => BlockAlias::same(&["infested_mossy_stone_bricks"]),
        "cracked_brick_monster_egg" => BlockAlias::same(&["infested_cracked_stone_bricks"]),
        "chiseled_brick_monster_egg" => BlockAlias::same(&["infested_chiseled_stone_bricks"]),
        "houstonia" => BlockAlias::same(&["azure_bluet"]),
        "double_grass" => BlockAlias::same(&["tall_grass"]),
        "double_fern" => BlockAlias::same(&["large_fern"]),
        "double_rose" => BlockAlias::same(&["rose_bush"]),
        "paeonia" => BlockAlias::same(&["peony"]),
        "syringa" => BlockAlias::same(&["lilac"]),
        "bed" => BlockAlias::same(&["red_bed"]),
        "grass" => BlockAlias::same(&["grass_block"]),
        "furnace" => BlockAlias::fixed(&["furnace"], &[("lit", "false")]),
        "lit_furnace" => BlockAlias::fixed(&["furnace"], &[("lit", "true")]),
        "redstone_lamp" => BlockAlias::fixed(&["redstone_lamp"], &[("lit", "false")]),
        "lit_redstone_lamp" => BlockAlias::fixed(&["redstone_lamp"], &[("lit", "true")]),
        "redstone_ore" => BlockAlias::fixed(&["redstone_ore"], &[("lit", "false")]),
        "lit_redstone_ore" => BlockAlias::fixed(&["redstone_ore"], &[("lit", "true")]),
        "redstone_torch" => BlockAlias::fixed(&["redstone_torch"], &[("lit", "true")]),
        "unlit_redstone_torch" => BlockAlias::fixed(&["redstone_torch"], &[("lit", "false")]),
        "daylight_detector" => BlockAlias::fixed(&["daylight_detector"], &[("inverted", "false")]),
        "daylight_detector_inverted" => {
            BlockAlias::fixed(&["daylight_detector"], &[("inverted", "true")])
        }
        "unpowered_comparator" => BlockAlias::fixed(&["comparator"], &[("powered", "false")]),
        "powered_comparator" => BlockAlias::fixed(&["comparator"], &[("powered", "true")]),
        "unpowered_repeater" => BlockAlias::fixed(&["repeater"], &[("powered", "false")]),
        "powered_repeater" => BlockAlias::fixed(&["repeater"], &[("powered", "true")]),
        _ => return None,
    };
    Some(alias)
}

impl BlockAlias {
    const fn same(names: &'static [&'static str]) -> Self {
        Self {
            names,
            fixed: &[],
            half_to_type: false,
        }
    }

    const fn fixed(
        names: &'static [&'static str],
        fixed: &'static [(&'static str, &'static str)],
    ) -> Self {
        Self {
            names,
            fixed,
            half_to_type: false,
        }
    }

    const fn double(names: &'static [&'static str]) -> Self {
        Self {
            names,
            fixed: &[("type", "double")],
            half_to_type: false,
        }
    }
}

fn item_alias(asset: &str) -> Option<ItemAlias> {
    let names: &'static [&'static str] = match asset {
        "grass" => &["grass_block"],
        "reeds" => &["sugar_cane"],
        "web" => &["cobweb"],
        "waterlily" => &["lily_pad"],
        "noteblock" => &["note_block"],
        "mob_spawner" => &["spawner"],
        "melon_block" => &["melon"],
        "brick_block" => &["bricks"],
        "nether_brick" => &["nether_bricks"],
        "red_nether_brick" => &["red_nether_bricks"],
        "end_bricks" => &["end_stone_bricks"],
        "stonebrick" => &["stone_bricks"],
        "mossy_stonebrick" => &["mossy_stone_bricks"],
        "cracked_stonebrick" => &["cracked_stone_bricks"],
        "chiseled_stonebrick" => &["chiseled_stone_bricks"],
        "smooth_granite" => &["polished_granite"],
        "smooth_diorite" => &["polished_diorite"],
        "smooth_andesite" => &["polished_andesite"],
        "quartz_column" => &["quartz_pillar"],
        "quartz_ore" => &["nether_quartz_ore"],
        "magma" => &["magma_block"],
        "slime" => &["slime_block"],
        "snow_layer" => &["snow"],
        "pumpkin" => &["carved_pumpkin"],
        "lit_pumpkin" => &["jack_o_lantern"],
        "golden_rail" => &["powered_rail"],
        "stone_stairs" => &["cobblestone_stairs"],
        "fence" => &["oak_fence"],
        "fence_gate" => &["oak_fence_gate"],
        "trapdoor" => &["oak_trapdoor"],
        "wooden_button" => &["oak_button"],
        "wooden_door" => &["oak_door"],
        "wooden_pressure_plate" => &["oak_pressure_plate"],
        "stone_monster_egg" => &["infested_stone"],
        "cobblestone_monster_egg" => &["infested_cobblestone"],
        "stone_brick_monster_egg" => &["infested_stone_bricks"],
        "mossy_brick_monster_egg" => &["infested_mossy_stone_bricks"],
        "cracked_brick_monster_egg" => &["infested_cracked_stone_bricks"],
        "chiseled_brick_monster_egg" => &["infested_chiseled_stone_bricks"],
        "houstonia" => &["azure_bluet"],
        "double_grass" => &["tall_grass"],
        "double_fern" => &["large_fern"],
        "double_rose" => &["rose_bush"],
        "paeonia" => &["peony"],
        "syringa" => &["lilac"],
        "furnace" => &["furnace"],
        "anvil" => &["anvil", "chipped_anvil", "damaged_anvil"],
        "cobblestone_wall" => &["cobblestone_wall", "mossy_cobblestone_wall"],
        "stone_slab" => &["stone_slab"],
        "wooden_slab" => &[
            "oak_slab",
            "spruce_slab",
            "birch_slab",
            "jungle_slab",
            "acacia_slab",
            "dark_oak_slab",
        ],
        "dye" => &[
            "ink_sac",
            "rose_red",
            "cactus_green",
            "cocoa_beans",
            "lapis_lazuli",
            "purple_dye",
            "cyan_dye",
            "light_gray_dye",
            "gray_dye",
            "pink_dye",
            "lime_dye",
            "dandelion_yellow",
            "light_blue_dye",
            "magenta_dye",
            "orange_dye",
            "bone_meal",
        ],
        "bed" => &[
            "white_bed",
            "orange_bed",
            "magenta_bed",
            "light_blue_bed",
            "yellow_bed",
            "lime_bed",
            "pink_bed",
            "gray_bed",
            "light_gray_bed",
            "cyan_bed",
            "purple_bed",
            "blue_bed",
            "brown_bed",
            "green_bed",
            "red_bed",
            "black_bed",
        ],
        "banner" => &[
            "white_banner",
            "orange_banner",
            "magenta_banner",
            "light_blue_banner",
            "yellow_banner",
            "lime_banner",
            "pink_banner",
            "gray_banner",
            "light_gray_banner",
            "cyan_banner",
            "purple_banner",
            "blue_banner",
            "brown_banner",
            "green_banner",
            "red_banner",
            "black_banner",
        ],
        "skull" => &[
            "skeleton_skull",
            "wither_skeleton_skull",
            "zombie_head",
            "player_head",
            "creeper_head",
            "dragon_head",
        ],
        "netherbrick" => &["nether_brick"],
        "boat" => &["oak_boat"],
        "totem" => &["totem_of_undying"],
        "golden_apple" => &["golden_apple", "enchanted_golden_apple"],
        "fish" => &["cod", "salmon", "tropical_fish", "pufferfish"],
        "cooked_fish" => &["cooked_cod", "cooked_salmon"],
        "fireworks" => &["firework_rocket"],
        "firework_charge" => &["firework_star"],
        "speckled_melon" => &["glistering_melon_slice"],
        "chorus_fruit_popped" => &["popped_chorus_fruit"],
        "bottle_splash" => &["splash_potion"],
        "bottle_lingering" => &["lingering_potion"],
        "record_11" => &["music_disc_11"],
        "record_13" => &["music_disc_13"],
        "record_blocks" => &["music_disc_blocks"],
        "record_cat" => &["music_disc_cat"],
        "record_chirp" => &["music_disc_chirp"],
        "record_far" => &["music_disc_far"],
        "record_mall" => &["music_disc_mall"],
        "record_mellohi" => &["music_disc_mellohi"],
        "record_stal" => &["music_disc_stal"],
        "record_strad" => &["music_disc_strad"],
        "record_wait" => &["music_disc_wait"],
        "record_ward" => &["music_disc_ward"],
        _ => {
            let block = block_alias(asset)?;
            return Some(ItemAlias { names: block.names });
        }
    };
    Some(ItemAlias { names })
}

#[cfg(test)]
mod tests {
    use super::*;
    use minecraft_version::LegacyRegistry;

    #[test]
    fn numeric_blocks_and_items_join_the_catalog_by_flattened_name() {
        let registry = LegacyRegistry::shared();
        assert_eq!(
            block_ids(registry, "white_wool"),
            vec![CatalogLegacyId {
                id: 35,
                metadata: Some(0)
            }]
        );
        assert_eq!(
            block_ids(registry, "oak_slab"),
            vec![
                CatalogLegacyId {
                    id: 125,
                    metadata: Some(0)
                },
                CatalogLegacyId {
                    id: 126,
                    metadata: Some(0)
                },
                CatalogLegacyId {
                    id: 126,
                    metadata: Some(8)
                },
            ]
        );
        assert_eq!(
            item_ids(registry, "wheat"),
            vec![CatalogLegacyId {
                id: 296,
                metadata: Some(0)
            }]
        );
        assert_eq!(
            item_ids(registry, "dye").len(),
            16,
            "all sixteen dye damages map to the shared dye item"
        );
        assert_eq!(
            item_ids(registry, "netherbrick"),
            vec![CatalogLegacyId {
                id: 405,
                metadata: Some(0)
            }]
        );
        assert_eq!(
            item_ids(registry, "totem"),
            vec![CatalogLegacyId {
                id: 449,
                metadata: Some(0)
            }]
        );
        assert_eq!(
            item_ids(registry, "boat"),
            vec![CatalogLegacyId {
                id: 333,
                metadata: Some(0)
            }]
        );
    }

    #[test]
    fn variants_use_exact_metadata_and_ignore_jar_only_properties() {
        let registry = LegacyRegistry::shared();
        let mut bottom = BTreeMap::new();
        bottom.insert("half".to_owned(), "bottom".to_owned());
        assert_eq!(
            variant_ids(registry, "oak_slab", &bottom),
            vec![CatalogLegacyId {
                id: 126,
                metadata: Some(0)
            }]
        );

        let mut variant = BTreeMap::new();
        variant.insert("half".to_owned(), "top".to_owned());
        variant.insert("variant".to_owned(), "default".to_owned());
        assert_eq!(
            variant_ids(registry, "purpur_slab", &variant),
            vec![CatalogLegacyId {
                id: 205,
                metadata: Some(8)
            }]
        );

        let mut facing = BTreeMap::new();
        facing.insert("facing".to_owned(), "north".to_owned());
        assert_eq!(
            variant_ids(registry, "lit_furnace", &facing),
            vec![CatalogLegacyId {
                id: 62,
                metadata: Some(2)
            }]
        );
    }

    #[test]
    fn shared_double_slabs_keep_every_numeric_variant() {
        let registry = LegacyRegistry::shared();
        let ids = block_ids(registry, "stone_double_slab");
        assert!(ids.iter().any(|id| (id.id, id.metadata) == (43, Some(0))));
        assert!(ids.iter().any(|id| (id.id, id.metadata) == (43, Some(8))));
        assert!(ids.iter().any(|id| (id.id, id.metadata) == (43, Some(15))));
        assert!(ids.len() > 8);
    }

    #[test]
    fn unknown_assets_never_invent_numeric_ids() {
        let registry = LegacyRegistry::shared();
        assert!(block_ids(registry, "not_a_block").is_empty());
        assert!(item_ids(registry, "not_an_item").is_empty());
    }
}
