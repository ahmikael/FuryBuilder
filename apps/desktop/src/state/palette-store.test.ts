import { beforeEach, describe, expect, it } from "vitest";

import {
  MAX_PALETTE_CATALOGS,
  MAX_RECENT_BLOCKS,
  PALETTE_STORAGE_KEY,
  usePaletteStore,
} from "@/state/palette-store";

const CATALOG = "java-1.21.4-0123456789abcdef";

function state() {
  return usePaletteStore.getState();
}

describe("palette recent and favorites", () => {
  beforeEach(() => {
    localStorage.clear();
    usePaletteStore.setState({ catalogs: {} });
  });

  it("keeps recent blocks newest-first, deduplicated and per catalog", () => {
    state().recordRecent(CATALOG, "minecraft:stone");
    state().recordRecent(CATALOG, "minecraft:oak_log");
    state().recordRecent(CATALOG, "minecraft:stone");
    state().recordRecent("curated", "minecraft:stone");

    expect(state().catalogs[CATALOG]?.recent).toEqual(["minecraft:stone", "minecraft:oak_log"]);
    expect(state().catalogs.curated?.recent).toEqual(["minecraft:stone"]);
  });

  it("toggles favorites without changing recent entries", () => {
    state().toggleFavorite(CATALOG, "minecraft:stone");
    state().toggleFavorite(CATALOG, "minecraft:oak_log");
    expect(state().catalogs[CATALOG]?.favorites).toEqual(["minecraft:oak_log", "minecraft:stone"]);

    state().toggleFavorite(CATALOG, "minecraft:stone");
    expect(state().catalogs[CATALOG]?.favorites).toEqual(["minecraft:oak_log"]);

    state().recordRecent(CATALOG, "minecraft:stone");
    expect(state().catalogs[CATALOG]).toEqual({
      recent: ["minecraft:stone"],
      favorites: ["minecraft:oak_log"],
    });
  });

  it("bounds recent history and the number of remembered catalogs", () => {
    for (let index = 0; index < MAX_RECENT_BLOCKS + 4; index += 1) {
      state().recordRecent(CATALOG, `minecraft:block_${index}`);
    }
    expect(state().catalogs[CATALOG]?.recent).toHaveLength(MAX_RECENT_BLOCKS);
    expect(state().catalogs[CATALOG]?.recent[0]).toBe(`minecraft:block_${MAX_RECENT_BLOCKS + 3}`);

    for (let index = 0; index < MAX_PALETTE_CATALOGS + 2; index += 1) {
      state().recordRecent(`catalog-${index}`, "minecraft:stone");
    }
    expect(Object.keys(state().catalogs)).toHaveLength(MAX_PALETTE_CATALOGS);
    expect(state().catalogs["catalog-0"]).toBeUndefined();
  });

  it("restores a valid persisted history", async () => {
    localStorage.setItem(
      PALETTE_STORAGE_KEY,
      JSON.stringify({
        state: {
          catalogs: {
            [CATALOG]: {
              recent: ["minecraft:stone"],
              favorites: ["minecraft:oak_log"],
            },
          },
        },
        version: 1,
      }),
    );

    await usePaletteStore.persist.rehydrate();

    expect(state().catalogs[CATALOG]).toEqual({
      recent: ["minecraft:stone"],
      favorites: ["minecraft:oak_log"],
    });
  });

  it("normalizes damaged persisted values instead of trusting them", async () => {
    localStorage.setItem(
      PALETTE_STORAGE_KEY,
      JSON.stringify({
        state: {
          catalogs: {
            [CATALOG]: {
              recent: ["minecraft:stone", 7, "minecraft:stone"],
              favorites: "not-an-array",
            },
          },
        },
        version: 1,
      }),
    );

    await usePaletteStore.persist.rehydrate();

    expect(state().catalogs[CATALOG]).toEqual({
      recent: ["minecraft:stone"],
      favorites: [],
    });
  });
});
