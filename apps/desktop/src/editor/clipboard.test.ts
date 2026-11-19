import { describe, expect, it } from "vitest";

import { clipboardPasteBounds, MAX_REGION_READ_VOLUME, planRegionTiles } from "@/editor/clipboard";
import { boundsVolume } from "@/editor/session";
import type { BoxBounds, ClipboardPayload } from "@/editor/types";

const clipboard: ClipboardPayload = {
  dimension: "minecraft:overworld",
  size: [3, 2, 4],
  blocks: [],
};

function contains(outer: BoxBounds, inner: BoxBounds): boolean {
  return (
    outer.min.x <= inner.min.x &&
    outer.min.y <= inner.min.y &&
    outer.min.z <= inner.min.z &&
    outer.max.x >= inner.max.x &&
    outer.max.y >= inner.max.y &&
    outer.max.z >= inner.max.z
  );
}

function overlaps(a: BoxBounds, b: BoxBounds): boolean {
  return (
    a.min.x <= b.max.x &&
    a.max.x >= b.min.x &&
    a.min.y <= b.max.y &&
    a.max.y >= b.min.y &&
    a.min.z <= b.max.z &&
    a.max.z >= b.min.z
  );
}

describe("region read tiling", () => {
  it("keeps a region inside the core limit as one request", () => {
    const bounds = { min: { x: -5, y: 60, z: 7 }, max: { x: 10, y: 75, z: 22 } };
    expect(planRegionTiles(bounds)).toEqual([bounds]);
  });

  it("covers a large region exactly once with bounded tiles", () => {
    const bounds = { min: { x: -17, y: -32, z: 41 }, max: { x: 94, y: 95, z: 140 } };
    const tiles = planRegionTiles(bounds, 4_096);
    expect(tiles.length).toBeGreaterThan(1);

    for (const tile of tiles) {
      expect(boundsVolume(tile)).toBeLessThanOrEqual(4_096);
      expect(contains(bounds, tile)).toBe(true);
    }
    expect(tiles.reduce((sum, tile) => sum + boundsVolume(tile), 0)).toBe(boundsVolume(bounds));
    for (let index = 0; index < tiles.length; index += 1) {
      for (let other = index + 1; other < tiles.length; other += 1) {
        expect(overlaps(tiles[index]!, tiles[other]!)).toBe(false);
      }
    }
  });

  it("splits the full editor selection into a bounded number of tiles", () => {
    const bounds = { min: { x: 0, y: 0, z: 0 }, max: { x: 99, y: 99, z: 99 } };
    const tiles = planRegionTiles(bounds);
    expect(boundsVolume(bounds)).toBe(1_000_000);
    expect(tiles.length).toBeLessThanOrEqual(2 * Math.ceil(1_000_000 / MAX_REGION_READ_VOLUME));
    expect(tiles.every((tile) => boundsVolume(tile) <= MAX_REGION_READ_VOLUME)).toBe(true);
    expect(tiles.reduce((sum, tile) => sum + boundsVolume(tile), 0)).toBe(1_000_000);
  });

  it("is deterministic and preserves the minimum corner", () => {
    const bounds = { min: { x: 4, y: 5, z: 6 }, max: { x: 83, y: 84, z: 85 } };
    const first = planRegionTiles(bounds, 1_024);
    const second = planRegionTiles(bounds, 1_024);
    expect(second).toEqual(first);
    expect(first[0]!.min).toEqual(bounds.min);
    expect(first.at(-1)!.max).toEqual(bounds.max);
  });

  it("rejects invalid bounds instead of inventing a request", () => {
    expect(planRegionTiles({ min: { x: 2, y: 2, z: 2 }, max: { x: 1, y: 2, z: 2 } })).toEqual([]);
  });
});

describe("paste bounds", () => {
  it("offsets the copied size from the target corner", () => {
    expect(clipboardPasteBounds(clipboard, { x: -10, y: 64, z: 3 })).toEqual({
      min: { x: -10, y: 64, z: 3 },
      max: { x: -8, y: 65, z: 6 },
    });
  });
});
