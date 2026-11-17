import { describe, expect, it } from "vitest";
import { SnapshotBuilder, snapshotFromBlocks } from "@/renderer/builders";
import { buildChunkMesh } from "@/renderer/mesher";
import { RenderWorld } from "@/renderer/world";

function worldWith(blocks: Array<{ x: number; y: number; z: number; name: string }>): RenderWorld {
  const world = new RenderWorld();
  world.setSnapshot(snapshotFromBlocks(blocks, 1));
  return world;
}

describe("chunk mesher", () => {
  it("emits six faces for an isolated block", () => {
    const world = worldWith([{ x: 0, y: 64, z: 0, name: "minecraft:stone" }]);
    const mesh = buildChunkMesh(world, { x: 0, z: 0 });
    expect(mesh.faceCount).toBe(6);
    expect(mesh.opaque.positions.length).toBe(6 * 4 * 3);
    expect(mesh.opaque.normals.length).toBe(6 * 4 * 3);
    expect(mesh.opaque.uvs.length).toBe(6 * 4 * 2);
    expect(mesh.opaque.colors.length).toBe(6 * 4 * 3);
    expect(mesh.opaque.indices.length).toBe(6 * 6);
    expect(mesh.cutout.positions.length).toBe(0);
    expect(mesh.translucent.positions.length).toBe(0);
  });

  it("culls the shared face of two adjacent opaque blocks", () => {
    const world = worldWith([
      { x: 0, y: 64, z: 0, name: "minecraft:stone" },
      { x: 1, y: 64, z: 0, name: "minecraft:stone" },
    ]);
    const mesh = buildChunkMesh(world, { x: 0, z: 0 });
    expect(mesh.faceCount).toBe(10);
  });

  it("culls faces across a loaded chunk border", () => {
    const world = worldWith([
      { x: 0, y: 64, z: 0, name: "minecraft:stone" },
      { x: -1, y: 64, z: 0, name: "minecraft:stone" },
    ]);
    const east = buildChunkMesh(world, { x: 0, z: 0 });
    const west = buildChunkMesh(world, { x: -1, z: 0 });
    expect(east.faceCount).toBe(5);
    expect(west.faceCount).toBe(5);
  });

  it("keeps boundary faces visible when the neighbor chunk is unloaded", () => {
    const world = worldWith([{ x: 0, y: 64, z: 0, name: "minecraft:stone" }]);
    const mesh = buildChunkMesh(world, { x: 0, z: 0 });
    expect(mesh.faceCount).toBe(6);
  });

  it("puts translucent blocks in a separate buffer and culls same-material faces", () => {
    const world = worldWith([
      { x: 0, y: 64, z: 0, name: "minecraft:glass" },
      { x: 1, y: 64, z: 0, name: "minecraft:glass" },
      { x: 3, y: 64, z: 0, name: "minecraft:water" },
    ]);
    const mesh = buildChunkMesh(world, { x: 0, z: 0 });
    expect(mesh.opaque.positions.length).toBe(0);
    expect(mesh.translucent.positions.length).toBeGreaterThan(0);
    // two adjacent glass blocks: 10 faces + 6 water faces
    expect(mesh.faceCount).toBe(16);
  });

  it("handles cutout leaves without merging faces between identical leaves", () => {
    const world = worldWith([
      { x: 0, y: 64, z: 0, name: "minecraft:oak_leaves" },
      { x: 1, y: 64, z: 0, name: "minecraft:oak_leaves" },
    ]);
    const mesh = buildChunkMesh(world, { x: 0, z: 0 });
    expect(mesh.cutout.positions.length).toBeGreaterThan(0);
    expect(mesh.faceCount).toBe(10);
  });

  it("renders stone faces next to glass but not glass faces next to stone", () => {
    const world = worldWith([
      { x: 0, y: 64, z: 0, name: "minecraft:stone" },
      { x: 1, y: 64, z: 0, name: "minecraft:glass" },
    ]);
    const mesh = buildChunkMesh(world, { x: 0, z: 0 });
    expect(mesh.faceCount).toBe(11);
    expect(mesh.opaque.positions.length).toBe(6 * 4 * 3);
    expect(mesh.translucent.positions.length).toBe(5 * 4 * 3);
  });

  it("returns empty buffers for missing chunks", () => {
    const world = new RenderWorld();
    const mesh = buildChunkMesh(world, { x: 42, z: -7 });
    expect(mesh.faceCount).toBe(0);
    expect(mesh.opaque.positions.length).toBe(0);
  });

  it("recomputes a chunk after a neighbour is unloaded", () => {
    const world = worldWith([
      { x: 15, y: 64, z: 0, name: "minecraft:stone" },
      { x: 16, y: 64, z: 0, name: "minecraft:stone" },
    ]);
    expect(buildChunkMesh(world, { x: 0, z: 0 }).faceCount).toBe(5);
    expect(buildChunkMesh(world, { x: 1, z: 0 }).faceCount).toBe(5);

    world.removeChunk({ x: 1, z: 0 });
    const rebuilt = buildChunkMesh(world, { x: 0, z: 0 });
    expect(rebuilt.faceCount).toBe(6);
    expect(rebuilt.opaque.indices.length).toBe(36);
  });

  it("builds multiple sections in one chunk", () => {
    const world = worldWith([
      { x: 0, y: 0, z: 0, name: "minecraft:stone" },
      { x: 0, y: 20, z: 0, name: "minecraft:stone" },
    ]);
    const mesh = buildChunkMesh(world, { x: 0, z: 0 });
    expect(mesh.faceCount).toBe(12);
  });

  it("emits half-height geometry for slabs", () => {
    const world = worldWith([{ x: 0, y: 64, z: 0, name: "minecraft:oak_slab" }]);
    const mesh = buildChunkMesh(world, { x: 0, z: 0 });
    expect(mesh.faceCount).toBe(6);
    const positions = Array.from(mesh.opaque.positions);
    const ys = positions.filter((_, index) => index % 3 === 1);
    expect(Math.min(...ys)).toBe(64);
    expect(Math.max(...ys)).toBe(64.5);
  });

  it("emits two boxes for stairs", () => {
    const world = worldWith([{ x: 0, y: 64, z: 0, name: "minecraft:oak_stairs" }]);
    const mesh = buildChunkMesh(world, { x: 0, z: 0 });
    expect(mesh.faceCount).toBe(12);
    expect(mesh.opaque.indices.length).toBe(12 * 6);
  });

  it("emits small geometry for torches", () => {
    const world = worldWith([{ x: 0, y: 64, z: 0, name: "minecraft:torch" }]);
    const mesh = buildChunkMesh(world, { x: 0, z: 0 });
    expect(mesh.faceCount).toBe(6);
    expect(mesh.cutout.positions.length).toBeGreaterThan(0);
    const xs = Array.from(mesh.cutout.positions).filter((_, index) => index % 3 === 0);
    expect(Math.min(...xs)).toBeCloseTo(0.4375);
    expect(Math.max(...xs)).toBeCloseTo(0.5625);
  });

  it("produces a stable face count for a built snapshot", () => {
    const builder = new SnapshotBuilder();
    for (let x = 0; x < 4; x += 1) {
      for (let z = 0; z < 4; z += 1) {
        builder.setBlock(x, 64, z, "minecraft:stone");
      }
    }
    const world = new RenderWorld();
    world.setSnapshot(builder.build());
    const mesh = buildChunkMesh(world, { x: 0, z: 0 });
    // 16 blocks: top + bottom = 32 faces; perimeter sides = 16 faces.
    expect(mesh.faceCount).toBe(48);
  });
});
