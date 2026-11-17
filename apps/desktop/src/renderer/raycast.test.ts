import { describe, expect, it } from "vitest";
import { snapshotFromBlocks } from "@/renderer/builders";
import { raycastBlocks } from "@/renderer/raycast";
import { RenderWorld } from "@/renderer/world";

function worldWith(blocks: Array<{ x: number; y: number; z: number; name: string }>): RenderWorld {
  const world = new RenderWorld();
  world.setSnapshot(snapshotFromBlocks(blocks));
  return world;
}

describe("voxel raycast", () => {
  it("hits a block and reports the entered face", () => {
    const world = worldWith([{ x: 0, y: 64, z: 0, name: "minecraft:stone" }]);
    const hit = raycastBlocks(world, [0.5, 64.5, 5], [0, 0, -1]);
    expect(hit?.pos).toEqual({ x: 0, y: 64, z: 0 });
    expect(hit?.face).toEqual([0, 0, 1]);
    expect(hit?.state.name).toBe("minecraft:stone");
  });

  it("hits a negative-coordinate block exactly", () => {
    const world = worldWith([
      { x: -1, y: 64, z: -1, name: "minecraft:gold_block" },
      // Load the intermediate chunk so the ray does not stop at unloaded space.
      { x: -16, y: 0, z: 0, name: "minecraft:stone" },
    ]);
    const hit = raycastBlocks(world, [-0.5, 64.5, 5], [0, 0, -1]);
    expect(hit?.pos).toEqual({ x: -1, y: 64, z: -1 });
    expect(hit?.face).toEqual([0, 0, 1]);
  });

  it("hits the top face when looking down", () => {
    const world = worldWith([{ x: 0, y: 64, z: 0, name: "minecraft:stone" }]);
    const hit = raycastBlocks(world, [0.5, 70, 0.5], [0, -1, 0]);
    expect(hit?.pos).toEqual({ x: 0, y: 64, z: 0 });
    expect(hit?.face).toEqual([0, 1, 0]);
  });

  it("returns null when the ray misses", () => {
    const world = worldWith([{ x: 0, y: 64, z: 0, name: "minecraft:stone" }]);
    expect(raycastBlocks(world, [5.5, 70, 5.5], [0, -1, 0])).toBeNull();
    expect(raycastBlocks(world, [0.5, 64.5, 5], [0, 0, 1])).toBeNull();
  });

  it("returns null when the ray enters unloaded space", () => {
    const world = worldWith([{ x: 0, y: 64, z: 0, name: "minecraft:stone" }]);
    expect(raycastBlocks(world, [0.5, 64.5, -5], [0, 0, -1])).toBeNull();
  });

  it("hits immediately when starting inside a block", () => {
    const world = worldWith([{ x: 0, y: 64, z: 0, name: "minecraft:stone" }]);
    const hit = raycastBlocks(world, [0.5, 64.5, 0.5], [1, 0, 0]);
    expect(hit?.pos).toEqual({ x: 0, y: 64, z: 0 });
    expect(hit?.face).toEqual([0, 1, 0]);
  });

  it("respects the maximum distance", () => {
    const world = worldWith([{ x: 0, y: 64, z: 0, name: "minecraft:stone" }]);
    expect(raycastBlocks(world, [0.5, 64.5, 100], [0, 0, -1], 10)).toBeNull();
  });
});
