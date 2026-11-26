//! Conservative block/item flags for the exact client JAR catalog.
//!
//! The JAR proves names, item forms and block states. It does not contain the game code that
//! defines fluids, block entities or creative-only availability, so this module applies an
//! explicit registry list for the supported versions and derives only facts that follow from
//! the JAR itself. A fact that is not covered stays `None`; it is never guessed as `false`.

use crate::catalog::CatalogFlags;

const FLUIDS: &[&str] = &[
    "minecraft:water",
    "minecraft:lava",
    "minecraft:flowing_water",
    "minecraft:flowing_lava",
];

const AIRLIKE: &[&str] = &["minecraft:air", "minecraft:cave_air", "minecraft:void_air"];

/// Blocks that are functional or hidden machinery even though they have a normal item form.
const TECHNICAL_BLOCKS: &[&str] = &[
    "minecraft:barrier",
    "minecraft:chain_command_block",
    "minecraft:command_block",
    "minecraft:end_gateway",
    "minecraft:end_portal",
    "minecraft:end_portal_frame",
    "minecraft:fire",
    "minecraft:jigsaw",
    "minecraft:light",
    "minecraft:moving_piston",
    "minecraft:nether_portal",
    "minecraft:piston_head",
    "minecraft:repeating_command_block",
    "minecraft:spawner",
    "minecraft:structure_block",
    "minecraft:structure_void",
    "minecraft:soul_fire",
    "minecraft:trial_spawner",
    "minecraft:vault",
];

/// Items or blocks that exist in the creative inventory but have no survival source.
const CREATIVE_ONLY: &[&str] = &[
    "minecraft:barrier",
    "minecraft:bedrock",
    "minecraft:budding_amethyst",
    "minecraft:chain_command_block",
    "minecraft:chorus_plant",
    "minecraft:command_block",
    "minecraft:command_block_minecart",
    "minecraft:debug_stick",
    "minecraft:end_portal_frame",
    "minecraft:jigsaw",
    "minecraft:knowledge_book",
    "minecraft:light",
    "minecraft:petrified_oak_slab",
    "minecraft:reinforced_deepslate",
    "minecraft:repeating_command_block",
    "minecraft:structure_block",
    "minecraft:structure_void",
    "minecraft:trial_spawner",
    "minecraft:vault",
];

/// Blocks that own a block entity in the supported 1.18–1.21.11 range. Colored or wood
/// variants are recognized by their stable suffixes below.
const BLOCK_ENTITIES: &[&str] = &[
    "minecraft:barrel",
    "minecraft:beacon",
    "minecraft:bee_nest",
    "minecraft:beehive",
    "minecraft:bell",
    "minecraft:blast_furnace",
    "minecraft:brewing_stand",
    "minecraft:calibrated_sculk_sensor",
    "minecraft:campfire",
    "minecraft:chain_command_block",
    "minecraft:chest",
    "minecraft:chiseled_bookshelf",
    "minecraft:command_block",
    "minecraft:comparator",
    "minecraft:conduit",
    "minecraft:crafter",
    "minecraft:creaking_heart",
    "minecraft:daylight_detector",
    "minecraft:decorated_pot",
    "minecraft:dispenser",
    "minecraft:dropper",
    "minecraft:enchanting_table",
    "minecraft:end_gateway",
    "minecraft:end_portal",
    "minecraft:ender_chest",
    "minecraft:furnace",
    "minecraft:hopper",
    "minecraft:jigsaw",
    "minecraft:jukebox",
    "minecraft:lectern",
    "minecraft:moving_piston",
    "minecraft:repeating_command_block",
    "minecraft:sculk_sensor",
    "minecraft:sculk_shrieker",
    "minecraft:smoker",
    "minecraft:soul_campfire",
    "minecraft:spawner",
    "minecraft:shulker_box",
    "minecraft:structure_block",
    "minecraft:suspicious_gravel",
    "minecraft:suspicious_sand",
    "minecraft:trapped_chest",
    "minecraft:trial_spawner",
    "minecraft:vault",
];

/// Classifies the facts that are stable for the supported vanilla range. Palette flags are
/// advisory: they never remove an entry from the write registry and never replace state
/// validation.
pub(crate) fn classify_flags(
    id: &str,
    is_block: bool,
    is_item: bool,
    has_variants: bool,
) -> CatalogFlags {
    let vanilla = id.starts_with("minecraft:");
    let fluid = vanilla && is_fluid(id);
    let airlike = vanilla && is_airlike(id);
    let creative_only = vanilla && is_creative_only(id);
    CatalogFlags {
        placeable: Some(is_block),
        has_item: Some(is_item),
        obtainable: vanilla.then_some(is_item && !creative_only),
        creative_only: vanilla.then_some(creative_only),
        technical: vanilla.then_some(is_technical(id, is_block, is_item, fluid, airlike)),
        fluid: vanilla.then_some(fluid),
        airlike: vanilla.then_some(airlike),
        block_entity: vanilla.then_some(is_block && is_block_entity(id)),
        transparent: None,
        full_cube: None,
        gravity: None,
        waterloggable: None,
        tinted: None,
        has_variants: Some(has_variants),
    }
}

fn is_fluid(id: &str) -> bool {
    FLUIDS.contains(&id)
}

fn is_airlike(id: &str) -> bool {
    AIRLIKE.contains(&id)
}

fn is_creative_only(id: &str) -> bool {
    CREATIVE_ONLY.contains(&id) || id.starts_with("minecraft:infested_")
}

fn is_technical(id: &str, is_block: bool, is_item: bool, fluid: bool, airlike: bool) -> bool {
    if !is_block {
        return false;
    }
    TECHNICAL_BLOCKS.contains(&id)
        || id.starts_with("minecraft:infested_")
        || (is_block && !is_item && !fluid && !airlike)
}

fn is_block_entity(id: &str) -> bool {
    BLOCK_ENTITIES.contains(&id)
        || id.ends_with("_sign")
        || id.ends_with("_banner")
        || id.ends_with("_bed")
        || id.ends_with("_shulker_box")
        || id.ends_with("_skull")
        || (id.ends_with("_head") && id != "minecraft:piston_head")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clear_flags_are_classified_and_unknown_geometry_stays_unknown() {
        let stone = classify_flags("minecraft:stone", true, true, false);
        assert_eq!(stone.placeable, Some(true));
        assert_eq!(stone.has_item, Some(true));
        assert_eq!(stone.obtainable, Some(true));
        assert_eq!(stone.technical, Some(false));
        assert_eq!(stone.fluid, Some(false));
        assert_eq!(stone.airlike, Some(false));
        assert_eq!(stone.block_entity, Some(false));
        assert_eq!(stone.transparent, None);
        assert_eq!(stone.full_cube, None);
        assert_eq!(stone.gravity, None);
        assert_eq!(stone.waterloggable, None);
        assert_eq!(stone.tinted, None);

        let water = classify_flags("minecraft:water", true, false, false);
        assert_eq!(water.fluid, Some(true));
        assert_eq!(water.technical, Some(false));
        assert_eq!(water.obtainable, Some(false));

        let air = classify_flags("minecraft:air", true, false, false);
        assert_eq!(air.airlike, Some(true));
        assert_eq!(air.fluid, Some(false));
        assert_eq!(air.obtainable, Some(false));

        let barrier = classify_flags("minecraft:barrier", true, true, false);
        assert_eq!(barrier.technical, Some(true));
        assert_eq!(barrier.creative_only, Some(true));
        assert_eq!(barrier.obtainable, Some(false));

        let bedrock = classify_flags("minecraft:bedrock", true, true, false);
        assert_eq!(bedrock.creative_only, Some(true));
        assert_eq!(bedrock.technical, Some(false));
        assert_eq!(bedrock.obtainable, Some(false));

        let staff = classify_flags("minecraft:debug_stick", false, true, false);
        assert_eq!(staff.placeable, Some(false));
        assert_eq!(staff.technical, Some(false));
        assert_eq!(staff.creative_only, Some(true));
        assert_eq!(staff.obtainable, Some(false));

        let foreign = classify_flags("example:ruby_block", true, false, false);
        assert_eq!(foreign.placeable, Some(true));
        assert_eq!(foreign.has_item, Some(false));
        assert_eq!(foreign.obtainable, None);
        assert_eq!(foreign.creative_only, None);
        assert_eq!(foreign.technical, None);
        assert_eq!(foreign.fluid, None);
        assert_eq!(foreign.airlike, None);
        assert_eq!(foreign.block_entity, None);
    }

    #[test]
    fn block_entity_variants_use_stable_suffixes() {
        for id in [
            "minecraft:chest",
            "minecraft:oak_sign",
            "minecraft:oak_wall_hanging_sign",
            "minecraft:white_banner",
            "minecraft:red_bed",
            "minecraft:blue_shulker_box",
            "minecraft:shulker_box",
            "minecraft:skeleton_skull",
            "minecraft:player_wall_head",
            "minecraft:decorated_pot",
            "minecraft:crafter",
            "minecraft:creaking_heart",
        ] {
            assert_eq!(
                classify_flags(id, true, true, false).block_entity,
                Some(true),
                "{id}"
            );
        }
        assert_eq!(
            classify_flags("minecraft:piston_head", true, false, false).block_entity,
            Some(false)
        );
        assert_eq!(
            classify_flags("minecraft:piston", true, true, false).block_entity,
            Some(false)
        );
    }

    #[test]
    fn every_block_without_an_item_is_technical_unless_it_is_fluid_or_airlike() {
        assert_eq!(
            classify_flags("minecraft:potted_oak_sapling", true, false, false).technical,
            Some(true)
        );
        assert_eq!(
            classify_flags("minecraft:water", true, false, false).technical,
            Some(false)
        );
        assert_eq!(
            classify_flags("minecraft:void_air", true, false, false).technical,
            Some(false)
        );
    }
}
