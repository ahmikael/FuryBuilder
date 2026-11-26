import { describe, expect, it } from "vitest";

import { matchesPaletteSearch } from "@/features/workspace/palette-categories";
import {
  PALETTE_SEARCH_INDEX_BUDGET_BYTES,
  buildPaletteSearchIndex,
  normalizePaletteSearch,
} from "@/features/workspace/palette-search-index";

const entries = Array.from({ length: 10_000 }, (_, index) => ({
  name: `example:block_${index}`,
  displayName: `Test Block ${index}`,
  tags: index % 17 === 0 ? ["example:generated", "minecraft:stone_family"] : ["example:generated"],
}));

describe("palette search index", () => {
  it("indexes 10k entries within the memory budget and matches the linear semantics", () => {
    const index = buildPaletteSearchIndex(entries);
    expect(index).not.toBeNull();
    expect(index?.entryCount).toBe(entries.length);
    expect(index?.estimatedBytes).toBeGreaterThan(0);
    expect(index?.estimatedBytes).toBeLessThan(PALETTE_SEARCH_INDEX_BUDGET_BYTES);

    for (const query of [
      "",
      "stone",
      "Block 9999",
      "generated",
      "block_1",
      "minecraft:stone_family",
      "zzz",
    ]) {
      const needle = normalizePaletteSearch(query);
      const matched = index?.search(needle) ?? new Set<string>();
      for (const entry of entries) {
        expect(matched.has(entry.name), `${query} / ${entry.name}`).toBe(
          matchesPaletteSearch(entry, query),
        );
      }
    }
  });

  it("returns no index when the caller's budget cannot hold it", () => {
    expect(buildPaletteSearchIndex(entries, 1_024)).toBeNull();
  });
});
