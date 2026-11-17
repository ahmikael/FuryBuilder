import { describe, expect, it } from "vitest";
import { boxShapeFor } from "@/renderer/shape";
import type { BlockState } from "@/renderer/types";

function state(name: string, properties: Record<string, string> = {}): BlockState {
  return { name, properties };
}

describe("block shapes", () => {
  it("returns null for full cubes", () => {
    expect(boxShapeFor(state("minecraft:stone"))).toBeNull();
    expect(boxShapeFor(state("minecraft:glass"))).toBeNull();
    expect(boxShapeFor(state("minecraft:deepslate", { axis: "y" }))).toBeNull();
  });

  it("models bottom, top and double slabs", () => {
    expect(boxShapeFor(state("minecraft:oak_slab", { type: "bottom" }))).toEqual([
      { min: [0, 0, 0], max: [1, 0.5, 1] },
    ]);
    expect(boxShapeFor(state("minecraft:oak_slab", { type: "top" }))).toEqual([
      { min: [0, 0.5, 0], max: [1, 1, 1] },
    ]);
    expect(boxShapeFor(state("minecraft:oak_slab", { type: "double" }))).toBeNull();
  });

  it("orients stairs by facing", () => {
    const north = boxShapeFor(state("minecraft:oak_stairs", { facing: "north" }));
    expect(north).toHaveLength(2);
    expect(north?.[1]).toEqual({ min: [0, 0.5, 0], max: [1, 1, 0.5] });

    const east = boxShapeFor(state("minecraft:oak_stairs", { facing: "east" }));
    expect(east?.[1]).toEqual({ min: [0.5, 0.5, 0], max: [1, 1, 1] });

    const west = boxShapeFor(state("minecraft:oak_stairs", { facing: "west" }));
    expect(west?.[1]).toEqual({ min: [0, 0.5, 0], max: [0.5, 1, 1] });

    const south = boxShapeFor(state("minecraft:oak_stairs", { facing: "south" }));
    expect(south?.[1]).toEqual({ min: [0, 0.5, 0.5], max: [1, 1, 1] });
  });

  it("scales snow by layers and keeps torches small", () => {
    expect(boxShapeFor(state("minecraft:snow", { layers: "3" }))?.[0]?.max[1]).toBe(0.375);
    expect(boxShapeFor(state("minecraft:snow", { layers: "99" }))?.[0]?.max[1]).toBe(1);
    expect(boxShapeFor(state("minecraft:snow", { layers: "0" }))?.[0]?.max[1]).toBe(0.125);

    const torch = boxShapeFor(state("minecraft:torch"));
    expect(torch).toHaveLength(1);
    expect(torch?.[0]?.min).toEqual([0.4375, 0, 0.4375]);
    expect(torch?.[0]?.max).toEqual([0.5625, 0.625, 0.5625]);
  });
});
