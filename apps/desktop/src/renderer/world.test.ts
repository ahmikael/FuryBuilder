import { describe, expect, it } from "vitest";
import { SnapshotBuilder, snapshotFromBlocks } from "@/renderer/builders";
import { RenderWorld, readPackedIndex } from "@/renderer/world";
import type { ChunkSnapshot, RenderSnapshot } from "@/renderer/types";

function loadedWorld(snapshot: RenderSnapshot): RenderWorld {
  const world = new RenderWorld();
  world.setSnapshot(snapshot);
  return world;
}

describe("RenderWorld", () => {
  it("decodes blocks from palette snapshots", () => {
    const world = loadedWorld(snapshotFromBlocks([{ x: 0, y: 64, z: 0, name: "minecraft:stone" }]));
    expect(world.blockAt({ x: 0, y: 64, z: 0 })?.name).toBe("minecraft:stone");
    expect(world.blockAt({ x: 1, y: 64, z: 0 })?.name).toBe("minecraft:air");
    expect(world.blockAt({ x: 0, y: 65, z: 0 })?.name).toBe("minecraft:air");
  });

  it("handles negative coordinates and chunk borders", () => {
    const world = loadedWorld(
      snapshotFromBlocks([{ x: -1, y: -1, z: -1, name: "minecraft:deepslate" }]),
    );
    expect(world.blockAt({ x: -1, y: -1, z: -1 })?.name).toBe("minecraft:deepslate");
    expect(world.blockAt({ x: 0, y: -1, z: -1 })).toBeNull();
    expect(world.loadedChunks()).toEqual([{ x: -1, z: -1 }]);
  });

  it("distinguishes unloaded chunks from air", () => {
    const world = loadedWorld(snapshotFromBlocks([{ x: 0, y: 0, z: 0, name: "minecraft:stone" }]));
    expect(world.blockAt({ x: 100, y: 0, z: 100 })).toBeNull();
    expect(world.blockAt({ x: 5, y: 5, z: 5 })?.name).toBe("minecraft:air");
  });

  it("decodes single-palette sections without packed data", () => {
    const section: ChunkSnapshot = {
      chunk: { x: 0, z: 0 },
      loaded: true,
      sections: [
        {
          sectionY: 0,
          palette: [{ name: "minecraft:stone", properties: {} }],
          data: [],
          bitsPerIndex: 0,
          nonAir: 4096,
        },
      ],
    };
    const world = new RenderWorld();
    world.upsertChunk(section);
    expect(world.blockAt({ x: 7, y: 7, z: 7 })?.name).toBe("minecraft:stone");
    expect(world.blockAt({ x: 15, y: 15, z: 15 })?.name).toBe("minecraft:stone");
  });

  it("reads packed indices across 32-bit boundaries", () => {
    const bits = 12;
    const data = new Uint32Array(Math.ceil((4096 * bits) / 32));
    // Write a synthetic value at an index whose 12-bit field crosses a u32 boundary.
    const index = 2; // bits 24..35
    const value = 0xabc;
    const bit = index * bits;
    const word = Math.floor(bit / 32);
    const offset = bit % 32;
    const lower = Math.min(bits, 32 - offset);
    data[word] =
      ((data[word] ?? 0) & ~(((1 << lower) - 1) << offset)) |
      ((value & ((1 << lower) - 1)) << offset);
    data[word + 1] = (data[word + 1] ?? 0) | (value >>> lower);
    expect(readPackedIndex(data, bits, index)).toBe(value);
  });

  it("reports missing requested chunks", () => {
    const builder = new SnapshotBuilder();
    builder.setBlock(0, 64, 0, "minecraft:stone");
    const world = loadedWorld(builder.build());
    expect(
      world.missingChunks([
        { x: 0, z: 0 },
        { x: 1, z: 1 },
      ]),
    ).toEqual([{ x: 1, z: 1 }]);
    expect(world.sectionCount()).toBeGreaterThan(0);
  });

  it("tracks the revision and dimension of the snapshot", () => {
    const snapshot = snapshotFromBlocks([{ x: 0, y: 64, z: 0, name: "minecraft:stone" }]);
    const world = loadedWorld(snapshot);
    expect(world.revision).toBe(1);
    expect(world.dimension).toBe("minecraft:overworld");
  });
});
