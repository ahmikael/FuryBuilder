import { describe, expect, it } from "vitest";

import type { DimensionInfo } from "@/lib/contracts";
import { resolveDimension } from "@/state/world-store";

function dimension(
  id: string,
  kind: DimensionInfo["kind"] = "custom",
  chunkCount = 0,
): DimensionInfo {
  return {
    id,
    kind,
    minY: -64,
    maxY: 319,
    height: 384,
    chunkCount,
    sectionCount: chunkCount * 3,
    blockCount: chunkCount * 1_024,
  };
}

const dimensions = [
  dimension("minecraft:overworld", "overworld", 4),
  dimension("minecraft:the_nether", "nether", 2),
  dimension("custom:sky_islands"),
];

describe("resolveDimension", () => {
  it("keeps a reported selection", () => {
    expect(resolveDimension("minecraft:the_nether", dimensions)).toBe("minecraft:the_nether");
    expect(resolveDimension("custom:sky_islands", dimensions)).toBe("custom:sky_islands");
  });

  it("falls back to the overworld when the saved dimension no longer exists", () => {
    expect(resolveDimension("minecraft:removed", dimensions)).toBe("minecraft:overworld");
  });

  it("falls back to the first reported dimension without an overworld", () => {
    expect(
      resolveDimension("minecraft:removed", [
        dimension("custom:first"),
        dimension("minecraft:the_end", "end"),
      ]),
    ).toBe("custom:first");
  });

  it("keeps the request until the authoritative world reports dimensions", () => {
    expect(resolveDimension("minecraft:the_end", [])).toBe("minecraft:the_end");
  });
});
