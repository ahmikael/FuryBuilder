/**
 * Creative-style palette grouping derived from the exact project catalog.
 *
 * The client JAR does not carry Minecraft's creative-tab assignment, so these groups are
 * built from direct block/item tags, flags and conservative id patterns. They are hints for
 * finding entries, not write permissions; Search/Recent/Favorites and the flag filters sit
 * on top of the same catalog.
 */

import type { CatalogFlags } from "@/assets/types";

export type PaletteGroupId =
  | "building"
  | "colored"
  | "natural"
  | "functional"
  | "redstone"
  | "technical"
  | "other";

export type PaletteFilterId = "all" | "recent" | "favorites" | PaletteGroupId;

export const PALETTE_CATEGORIES = [
  { id: "all", label: "All" },
  { id: "recent", label: "Recent" },
  { id: "favorites", label: "Favorites" },
  { id: "building", label: "Building Blocks" },
  { id: "colored", label: "Colored Blocks" },
  { id: "natural", label: "Natural Blocks" },
  { id: "functional", label: "Functional Blocks" },
  { id: "redstone", label: "Redstone Blocks" },
  { id: "technical", label: "Technical" },
  { id: "other", label: "Other" },
] as const satisfies readonly { id: PaletteFilterId; label: string }[];

export function paletteCategoryLabel(id: PaletteFilterId): string {
  return PALETTE_CATEGORIES.find((category) => category.id === id)?.label ?? id;
}

export type PaletteFlagFilterId =
  | "any"
  | "placeable"
  | "has_item"
  | "technical"
  | "creative_only"
  | "fluid"
  | "block_entity";

export const PALETTE_FLAG_FILTERS = [
  { id: "any", label: "Any block" },
  { id: "placeable", label: "Placeable" },
  { id: "has_item", label: "With item" },
  { id: "technical", label: "Technical" },
  { id: "creative_only", label: "Creative only" },
  { id: "fluid", label: "Fluid" },
  { id: "block_entity", label: "Block entity" },
] as const satisfies readonly { id: PaletteFlagFilterId; label: string }[];

export function paletteFlagFilterLabel(id: PaletteFlagFilterId): string {
  return PALETTE_FLAG_FILTERS.find((filter) => filter.id === id)?.label ?? id;
}

/** Matches a catalog flag filter. Unknown flags never satisfy a specific filter. */
export function matchesPaletteFlagFilter(
  flags: CatalogFlags | null | undefined,
  filter: PaletteFlagFilterId,
): boolean {
  if (filter === "any") return true;
  if (!flags) return false;
  switch (filter) {
    case "placeable":
      return flags.placeable === true;
    case "has_item":
      return flags.hasItem === true;
    case "technical":
      return flags.technical === true;
    case "creative_only":
      return flags.creativeOnly === true;
    case "fluid":
      return flags.fluid === true;
    case "block_entity":
      return flags.blockEntity === true;
  }
}

const BUILDING_TAGS = new Set([
  "minecraft:planks",
  "minecraft:stairs",
  "minecraft:wooden_stairs",
  "minecraft:slabs",
  "minecraft:wooden_slabs",
  "minecraft:walls",
  "minecraft:fences",
  "minecraft:wooden_fences",
  "minecraft:fence_gates",
  "minecraft:doors",
  "minecraft:wooden_doors",
  "minecraft:trapdoors",
  "minecraft:wooden_trapdoors",
  "minecraft:wool",
  "minecraft:wool_carpets",
  "minecraft:terracotta",
  "minecraft:concrete_powder",
  "minecraft:stone_bricks",
  "minecraft:impermeable",
  "minecraft:base_stone_overworld",
  "minecraft:base_stone_nether",
]);

const BUILDING_SUFFIXES = [
  "_planks",
  "_stairs",
  "_slab",
  "_wall",
  "_fence",
  "_fence_gate",
  "_door",
  "_trapdoor",
  "_pane",
  "_carpet",
  "_wool",
  "_terracotta",
  "_concrete",
  "_concrete_powder",
  "_glazed_terracotta",
  "_stained_glass",
  "_stained_glass_pane",
  "_bricks",
  "_tiles",
  "_pillar",
];

const BUILDING_PREFIXES = [
  "chiseled_",
  "cut_",
  "polished_",
  "smooth_",
  "mossy_",
  "cracked_",
  "waxed_",
  "exposed_",
  "weathered_",
  "oxidized_",
];

const COLOR_PREFIXES = [
  "light_blue_",
  "light_gray_",
  "white_",
  "orange_",
  "magenta_",
  "yellow_",
  "lime_",
  "pink_",
  "gray_",
  "cyan_",
  "purple_",
  "blue_",
  "brown_",
  "green_",
  "red_",
  "black_",
];

/** Families that Minecraft groups as color variants instead of separate materials. */
const COLORED_SUFFIXES = [
  "_wool",
  "_carpet",
  "_concrete",
  "_concrete_powder",
  "_terracotta",
  "_glazed_terracotta",
  "_stained_glass",
  "_stained_glass_pane",
  "_bed",
  "_banner",
  "_shulker_box",
  "_candle",
];

const BUILDING_KEYWORDS = [
  "cobblestone",
  "mossy_cobblestone",
  "smooth_stone",
  "sandstone",
  "red_sandstone",
  "quartz",
  "purpur",
  "prismarine",
  "copper",
  "brick",
  "deepslate",
  "mud",
  "resin",
  "glass",
  "terracotta",
  "concrete",
  "wool",
  "carpet",
  "amethyst_block",
  "obsidian",
  "dripstone_block",
  "calcite",
  "basalt",
  "blackstone",
  "netherrack",
  "end_stone",
  "bamboo_mosaic",
  "iron_block",
  "gold_block",
  "diamond_block",
  "emerald_block",
  "lapis_block",
  "netherite_block",
  "coal_block",
  "raw_iron_block",
  "raw_copper_block",
  "raw_gold_block",
  "iron_bars",
  "chain",
  "scaffolding",
  "hay_block",
  "bone_block",
  "dried_kelp_block",
  "honeycomb_block",
  "sponge",
];

const NATURAL_TAGS = new Set([
  "minecraft:leaves",
  "minecraft:saplings",
  "minecraft:flowers",
  "minecraft:small_flowers",
  "minecraft:crops",
  "minecraft:coral_plants",
  "minecraft:corals",
  "minecraft:coral_blocks",
  "minecraft:wall_corals",
  "minecraft:underwater_bonemeals",
  "minecraft:dirt",
  "minecraft:sand",
  "minecraft:ice",
  "minecraft:snow",
  "minecraft:nylium",
  "minecraft:logs",
  "minecraft:logs_that_burn",
  "minecraft:acacia_logs",
  "minecraft:birch_logs",
  "minecraft:cherry_logs",
  "minecraft:dark_oak_logs",
  "minecraft:jungle_logs",
  "minecraft:mangrove_logs",
  "minecraft:oak_logs",
  "minecraft:pale_oak_logs",
  "minecraft:spruce_logs",
  "minecraft:crimson_stems",
  "minecraft:warped_stems",
  "minecraft:bamboo_blocks",
  "minecraft:overworld_natural_logs",
  "minecraft:redstone_ores",
  "minecraft:coal_ores",
  "minecraft:copper_ores",
  "minecraft:diamond_ores",
  "minecraft:emerald_ores",
  "minecraft:gold_ores",
  "minecraft:iron_ores",
  "minecraft:lapis_ores",
  "minecraft:wart_blocks",
]);

const NATURAL_SUFFIXES = [
  "_log",
  "_wood",
  "_stem",
  "_hyphae",
  "_leaves",
  "_sapling",
  "_flower",
  "_tulip",
  "_orchid",
  "_allium",
  "_bluet",
  "_daisy",
  "_rose",
  "_bush",
  "_grass",
  "_fern",
  "_vine",
  "_vines",
  "_kelp",
  "_seagrass",
  "_mushroom",
  "_fungus",
  "_roots",
  "_sprouts",
  "_wart",
  "_cactus",
  "_bamboo",
  "_sugar_cane",
  "_wheat",
  "_carrots",
  "_potatoes",
  "_beetroots",
  "_melon",
  "_pumpkin",
  "_berry",
  "_moss",
  "_azalea",
  "_dripleaf",
  "_propagule",
  "_petals",
  "_blossom",
  "_eyeblossom",
  "_chorus",
  "_mycelium",
  "_podzol",
  "_dirt",
  "_sand",
  "_gravel",
  "_clay",
  "_snow",
  "_ice",
  "_magma",
  "_obsidian",
  "_netherrack",
  "_soul_soil",
  "_soul_sand",
  "_nylium",
  "_shroomlight",
  "_glowstone",
  "_amethyst",
  "_dripstone",
  "_calcite",
  "_ore",
  "_coral",
  "_froglight",
  "_egg",
  "_pickle",
  "_lichen",
  "_hanging_roots",
  "_spore_blossom",
  "_torchflower",
  "_pitcher_plant",
];

const NATURAL_IDS = new Set([
  "amethyst_block",
  "ancient_debris",
  "bamboo",
  "bee_nest",
  "budding_amethyst",
  "cactus",
  "calcite",
  "chorus_plant",
  "clay",
  "cobweb",
  "cocoa",
  "creaking_heart",
  "dead_bush",
  "dirt_path",
  "dragon_egg",
  "dripstone_block",
  "farmland",
  "fern",
  "frogspawn",
  "glowstone",
  "gravel",
  "kelp",
  "kelp_plant",
  "lily_pad",
  "magma_block",
  "melon",
  "moss_carpet",
  "netherrack",
  "obsidian",
  "pale_moss_carpet",
  "pumpkin",
  "sculk",
  "sculk_catalyst",
  "sculk_sensor",
  "calibrated_sculk_sensor",
  "sculk_shrieker",
  "sculk_vein",
  "seagrass",
  "shroomlight",
  "sniffer_egg",
  "soul_soil",
  "sugar_cane",
  "suspicious_gravel",
  "turtle_egg",
  "vine",
]);

const NATURAL_KEYWORDS = ["amethyst", "coral", "dripstone", "mushroom", "sculk", "vines"];

const FUNCTIONAL_TAGS = new Set([
  "minecraft:beds",
  "minecraft:campfires",
  "minecraft:candles",
  "minecraft:flower_pots",
  "minecraft:signs",
  "minecraft:all_signs",
  "minecraft:shulker_boxes",
  "minecraft:anvil",
  "minecraft:banners",
  "minecraft:cauldrons",
]);

const FUNCTIONAL_SUFFIXES = [
  "_bed",
  "_banner",
  "_sign",
  "_candle",
  "_campfire",
  "_lantern",
  "_torch",
  "_chest",
  "_furnace",
  "_table",
  "_shelf",
  "shulker_box",
  "_pot",
  "_frame",
];

const FUNCTIONAL_PREFIXES = ["potted_"];

const FUNCTIONAL_IDS = new Set([
  "barrel",
  "beacon",
  "bee_nest",
  "beehive",
  "bell",
  "blast_furnace",
  "bookshelf",
  "brewing_stand",
  "cake",
  "candle_cake",
  "cartography_table",
  "cauldron",
  "chain",
  "chest",
  "chiseled_bookshelf",
  "composter",
  "conduit",
  "copper_bulb",
  "crafter",
  "crafting_table",
  "daylight_detector",
  "decorated_pot",
  "enchanting_table",
  "end_rod",
  "ender_chest",
  "fletching_table",
  "flower_pot",
  "furnace",
  "glow_item_frame",
  "grindstone",
  "honey_block",
  "item_frame",
  "jack_o_lantern",
  "jukebox",
  "ladder",
  "lantern",
  "lava_cauldron",
  "lectern",
  "lightning_rod",
  "lodestone",
  "loom",
  "note_block",
  "powder_snow_cauldron",
  "respawn_anchor",
  "scaffolding",
  "sea_lantern",
  "sculk_sensor",
  "calibrated_sculk_sensor",
  "shulker_box",
  "slime_block",
  "smithing_table",
  "smoker",
  "soul_lantern",
  "sponge",
  "stonecutter",
  "target",
  "tnt",
  "torch",
  "trapped_chest",
  "water_cauldron",
  "wet_sponge",
]);

const REDSTONE_TAGS = new Set([
  "minecraft:buttons",
  "minecraft:pressure_plates",
  "minecraft:rails",
  "minecraft:stone_buttons",
  "minecraft:stone_pressure_plates",
  "minecraft:wooden_buttons",
  "minecraft:wooden_pressure_plates",
]);

const REDSTONE_SUFFIXES = ["_button", "_pressure_plate", "_rail"];

const REDSTONE_IDS = new Set([
  "comparator",
  "copper_bulb",
  "crafter",
  "daylight_detector",
  "dispenser",
  "dropper",
  "hopper",
  "lever",
  "lightning_rod",
  "moving_piston",
  "note_block",
  "observer",
  "piston",
  "piston_head",
  "repeater",
  "sculk_sensor",
  "calibrated_sculk_sensor",
  "sticky_piston",
  "target",
  "tnt",
  "trapped_chest",
  "tripwire",
  "tripwire_hook",
]);

const TECHNICAL_TAGS = new Set(["minecraft:portals", "minecraft:fire", "minecraft:air"]);

const TECHNICAL_IDS = new Set([
  "air",
  "barrier",
  "bedrock",
  "bubble_column",
  "cave_air",
  "chain_command_block",
  "command_block",
  "dragon_egg",
  "end_gateway",
  "end_portal",
  "end_portal_frame",
  "fire",
  "jigsaw",
  "lava",
  "light",
  "moving_piston",
  "nether_portal",
  "piston_head",
  "repeating_command_block",
  "soul_fire",
  "spawner",
  "structure_block",
  "structure_void",
  "trial_spawner",
  "vault",
  "void_air",
  "water",
]);

function hasAnyTag(tags: ReadonlySet<string>, names: ReadonlySet<string>): boolean {
  for (const name of names) {
    if (tags.has(name)) return true;
  }
  return false;
}

function hasAnySuffix(path: string, suffixes: readonly string[]): boolean {
  return suffixes.some((suffix) => path.endsWith(suffix));
}

function hasAnyPrefix(path: string, prefixes: readonly string[]): boolean {
  return prefixes.some((prefix) => path.startsWith(prefix));
}

function hasAnyKeyword(path: string, keywords: readonly string[]): boolean {
  return keywords.some((keyword) => path.includes(keyword));
}

/**
 * Returns the basic groups for one block id and its direct block tags. Every block has at
 * least one group: unmatched blocks (including unknown modded entries) fall into `other`.
 */
export function categorizeBlock(id: string, tags: readonly string[]): PaletteGroupId[] {
  if (!id.startsWith("minecraft:")) return ["other"];
  const path = id.slice("minecraft:".length);
  const tagSet = new Set(tags);
  const groups: PaletteGroupId[] = [];

  if (
    hasAnyTag(tagSet, BUILDING_TAGS) ||
    hasAnySuffix(path, BUILDING_SUFFIXES) ||
    hasAnyPrefix(path, BUILDING_PREFIXES) ||
    (hasAnyKeyword(path, BUILDING_KEYWORDS) && !path.endsWith("_ore"))
  ) {
    groups.push("building");
  }

  if (
    hasAnyTag(tagSet, NATURAL_TAGS) ||
    hasAnySuffix(path, NATURAL_SUFFIXES) ||
    hasAnyKeyword(path, NATURAL_KEYWORDS) ||
    NATURAL_IDS.has(path) ||
    path === "grass_block"
  ) {
    groups.push("natural");
  }

  if (
    hasAnyTag(tagSet, FUNCTIONAL_TAGS) ||
    hasAnySuffix(path, FUNCTIONAL_SUFFIXES) ||
    hasAnyPrefix(path, FUNCTIONAL_PREFIXES) ||
    FUNCTIONAL_IDS.has(path) ||
    path.includes("candle")
  ) {
    groups.push("functional");
  }

  if (
    hasAnyTag(tagSet, REDSTONE_TAGS) ||
    hasAnySuffix(path, REDSTONE_SUFFIXES) ||
    REDSTONE_IDS.has(path) ||
    path.startsWith("redstone_")
  ) {
    groups.push("redstone");
  }

  if (
    hasAnyTag(tagSet, TECHNICAL_TAGS) ||
    TECHNICAL_IDS.has(path) ||
    path.startsWith("infested_")
  ) {
    groups.push("technical");
  }

  if (hasAnyPrefix(path, COLOR_PREFIXES) && hasAnySuffix(path, COLORED_SUFFIXES)) {
    groups.push("colored");
  }

  return groups.length > 0 ? groups : ["other"];
}

/** Case-insensitive palette search over id, display name and direct tags. */
export function matchesPaletteSearch(
  entry: { name: string; displayName: string | null; tags?: readonly string[] },
  query: string,
): boolean {
  const needle = query.trim().toLowerCase();
  if (!needle) return true;
  return (
    entry.name.toLowerCase().includes(needle) ||
    (entry.displayName?.toLowerCase().includes(needle) ?? false) ||
    (entry.tags?.some((tag) => tag.toLowerCase().includes(needle)) ?? false)
  );
}
