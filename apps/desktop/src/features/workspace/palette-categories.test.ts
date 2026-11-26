import { describe, expect, it } from "vitest";

import type { CatalogFlags } from "@/assets/types";
import {
  PALETTE_CATEGORIES,
  categorizeBlock,
  matchesPaletteFlagFilter,
  matchesPaletteSearch,
  paletteCategoryLabel,
} from "@/features/workspace/palette-categories";

const EMPTY_FLAGS: CatalogFlags = {
  placeable: null,
  hasItem: null,
  obtainable: null,
  creativeOnly: null,
  technical: null,
  fluid: null,
  airlike: null,
  blockEntity: null,
  transparent: null,
  fullCube: null,
  gravity: null,
  waterloggable: null,
  tinted: null,
  hasVariants: null,
};

describe("palette categories", () => {
  it("keeps a stable, unique filter list", () => {
    expect(PALETTE_CATEGORIES.map((category) => category.id)).toEqual([
      "all",
      "recent",
      "favorites",
      "building",
      "colored",
      "natural",
      "functional",
      "redstone",
      "technical",
      "other",
    ]);
    expect(new Set(PALETTE_CATEGORIES.map((category) => category.id)).size).toBe(
      PALETTE_CATEGORIES.length,
    );
    expect(paletteCategoryLabel("building")).toBe("Building Blocks");
  });

  it("groups building blocks from tags, suffixes and material names", () => {
    expect(categorizeBlock("minecraft:oak_stairs", ["minecraft:wooden_stairs"])).toContain(
      "building",
    );
    expect(categorizeBlock("minecraft:oak_planks", ["minecraft:planks"])).toContain("building");
    expect(categorizeBlock("minecraft:polished_deepslate", [])).toContain("building");
    expect(categorizeBlock("minecraft:white_concrete", [])).toContain("building");
    expect(categorizeBlock("minecraft:glass_pane", [])).toContain("building");
  });

  it("groups natural terrain, plants and ores", () => {
    expect(categorizeBlock("minecraft:oak_log", ["minecraft:oak_logs"])).toContain("natural");
    expect(categorizeBlock("minecraft:oak_leaves", ["minecraft:leaves"])).toContain("natural");
    expect(categorizeBlock("minecraft:dandelion", ["minecraft:small_flowers"])).toContain(
      "natural",
    );
    expect(categorizeBlock("minecraft:grass_block", ["minecraft:dirt"])).toContain("natural");
    expect(categorizeBlock("minecraft:gravel", [])).toContain("natural");
    expect(categorizeBlock("minecraft:deepslate_diamond_ore", [])).toContain("natural");
    expect(categorizeBlock("minecraft:dead_bubble_coral_fan", [])).toContain("natural");
    expect(categorizeBlock("minecraft:red_mushroom_block", [])).toContain("natural");
  });

  it("groups functional and redstone components", () => {
    expect(categorizeBlock("minecraft:crafting_table", [])).toContain("functional");
    expect(categorizeBlock("minecraft:chest", [])).toContain("functional");
    expect(categorizeBlock("minecraft:red_bed", ["minecraft:beds"])).toContain("functional");
    expect(categorizeBlock("minecraft:redstone_lamp", [])).toContain("redstone");
    expect(categorizeBlock("minecraft:oak_button", ["minecraft:wooden_buttons"])).toContain(
      "redstone",
    );
    expect(categorizeBlock("minecraft:powered_rail", ["minecraft:rails"])).toContain("redstone");
    expect(categorizeBlock("minecraft:copper_bulb", [])).toContain("redstone");
  });

  it("groups technical blocks and keeps unknown entries in other", () => {
    expect(categorizeBlock("minecraft:command_block", [])).toContain("technical");
    expect(categorizeBlock("minecraft:water", [])).toContain("technical");
    expect(categorizeBlock("minecraft:end_portal_frame", [])).toContain("technical");
    expect(categorizeBlock("minecraft:player_head", [])).toEqual(["other"]);
    expect(categorizeBlock("example:copper_machine", ["minecraft:planks"])).toEqual(["other"]);
  });

  it("does not present an ore as a building material just because its id contains a material", () => {
    const groups = categorizeBlock("minecraft:copper_ore", ["minecraft:copper_ores"]);
    expect(groups).toContain("natural");
    expect(groups).not.toContain("building");
  });

  it("adds the colored group only to color-family blocks", () => {
    expect(categorizeBlock("minecraft:white_concrete", [])).toEqual(
      expect.arrayContaining(["building", "colored"]),
    );
    expect(categorizeBlock("minecraft:white_wool", ["minecraft:wool"])).toEqual(
      expect.arrayContaining(["building", "colored"]),
    );
    expect(categorizeBlock("minecraft:white_stained_glass_pane", [])).toContain("colored");
    expect(categorizeBlock("minecraft:red_bed", ["minecraft:beds"])).toEqual(
      expect.arrayContaining(["functional", "colored"]),
    );
    expect(categorizeBlock("minecraft:red_sandstone", [])).not.toContain("colored");
    expect(categorizeBlock("minecraft:red_nether_bricks", [])).not.toContain("colored");
    expect(categorizeBlock("minecraft:brown_mushroom_block", [])).not.toContain("colored");
    expect(categorizeBlock("minecraft:redstone_wire", [])).not.toContain("colored");
  });

  it("searches by full id, display name or direct tag", () => {
    const entry = {
      name: "minecraft:oak_stairs",
      displayName: "Oak Stairs",
      tags: ["minecraft:wooden_stairs"],
    };
    expect(matchesPaletteSearch(entry, "")).toBe(true);
    expect(matchesPaletteSearch(entry, "OAK")).toBe(true);
    expect(matchesPaletteSearch(entry, "stairs")).toBe(true);
    expect(matchesPaletteSearch(entry, "wooden_stairs")).toBe(true);
    expect(matchesPaletteSearch(entry, "deepslate")).toBe(false);
  });

  it("filters by catalog flags without treating unknown as false", () => {
    const flags = {
      ...EMPTY_FLAGS,
      placeable: true,
      hasItem: true,
      technical: false,
      fluid: false,
    };
    expect(matchesPaletteFlagFilter(flags, "any")).toBe(true);
    expect(matchesPaletteFlagFilter(flags, "placeable")).toBe(true);
    expect(matchesPaletteFlagFilter(flags, "has_item")).toBe(true);
    expect(matchesPaletteFlagFilter(flags, "technical")).toBe(false);
    expect(matchesPaletteFlagFilter(EMPTY_FLAGS, "placeable")).toBe(false);
    expect(matchesPaletteFlagFilter(null, "any")).toBe(true);
    expect(matchesPaletteFlagFilter(null, "has_item")).toBe(false);
  });
});
