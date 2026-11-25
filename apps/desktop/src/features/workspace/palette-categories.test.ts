import { describe, expect, it } from "vitest";

import {
  PALETTE_CATEGORIES,
  categorizeBlock,
  matchesPaletteSearch,
  paletteCategoryLabel,
} from "@/features/workspace/palette-categories";

describe("palette categories", () => {
  it("keeps a stable, unique filter list", () => {
    expect(PALETTE_CATEGORIES.map((category) => category.id)).toEqual([
      "all",
      "recent",
      "favorites",
      "building",
      "natural",
      "functional",
      "redstone",
      "technical",
      "other",
    ]);
    expect(new Set(PALETTE_CATEGORIES.map((category) => category.id)).size).toBe(
      PALETTE_CATEGORIES.length,
    );
    expect(paletteCategoryLabel("building")).toBe("Building");
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

  it("searches by full id or display name without changing the case of the query", () => {
    const entry = { name: "minecraft:oak_stairs", displayName: "Oak Stairs" };
    expect(matchesPaletteSearch(entry, "")).toBe(true);
    expect(matchesPaletteSearch(entry, "OAK")).toBe(true);
    expect(matchesPaletteSearch(entry, "stairs")).toBe(true);
    expect(matchesPaletteSearch(entry, "deepslate")).toBe(false);
  });
});
