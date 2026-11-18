import { describe, expect, it } from "vitest";
import { AssetModels } from "@/assets/model-assets";
import type { AssetBundle, BundleBlock, BundleTexture } from "@/assets/render-types";
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

  describe("verified asset models", () => {
    const texture = (key: string, hasAlpha = false): BundleTexture => ({
      key,
      path: `assets/minecraft/textures/block/${key.split("/").at(-1)}.png`,
      x: 0,
      y: 0,
      width: 16,
      height: 16,
      sourceWidth: 16,
      sourceHeight: 16,
      missing: false,
      hasAlpha,
    });

    const missing: BundleTexture = {
      key: "__missing__",
      path: null,
      x: 16,
      y: 0,
      width: 16,
      height: 16,
      sourceWidth: 16,
      sourceHeight: 16,
      missing: true,
      hasAlpha: false,
    };

    function model(
      name: string,
      textureKey: string,
      options: { cullface?: boolean; tintIndex?: number | null } = {},
    ): BundleBlock {
      return {
        name,
        properties: {},
        missingModel: false,
        parts: [
          {
            model: `minecraft:block/${name.split(":").at(-1)}`,
            rotationX: 0,
            rotationY: 0,
            uvlock: false,
            elements: [
              {
                from: [0, 0, 0],
                to: [16, 16, 16],
                rotation: null,
                shade: true,
                faces: [
                  {
                    direction: "up",
                    texture: textureKey,
                    uv: [0, 0, 16, 16],
                    rotation: 0,
                    cullface: options.cullface === false ? null : "up",
                    tintIndex: options.tintIndex ?? null,
                  },
                ],
              },
            ],
          },
        ],
        warnings: [],
        unsupported: [],
      };
    }

    function assets(blocks: BundleBlock[], extra: BundleTexture[] = []): AssetModels {
      const bundle: AssetBundle = {
        schemaVersion: 1,
        cacheKey: "test",
        cacheHit: false,
        baseJar: null,
        baseJarSha1: null,
        packs: [],
        atlasWidth: 64,
        atlasHeight: 64,
        atlasPngBase64: "",
        textures: [texture("minecraft:block/stone"), missing, ...extra],
        blocks,
        warnings: [],
      };
      return new AssetModels(bundle);
    }

    it("emits model elements with atlas UVs instead of the placeholder cube", () => {
      const models = assets([model("minecraft:stone", "minecraft:block/stone")]);
      const world = worldWith([{ x: 0, y: 64, z: 0, name: "minecraft:stone" }]);
      const mesh = buildChunkMesh(world, { x: 0, z: 0 }, models);
      expect(mesh.faceCount).toBe(1);
      expect(mesh.opaque.positions.length).toBe(4 * 3);
      expect(mesh.opaque.indices.length).toBe(6);
      const ys = Array.from(mesh.opaque.positions).filter((_, index) => index % 3 === 1);
      expect(ys).toEqual([65, 65, 65, 65]);
      const uvs = Array.from(mesh.opaque.uvs);
      expect(uvs[0]).toBeCloseTo(0 / 64);
      expect(uvs[1]).toBeCloseTo(16 / 64);
    });

    it("honours cullface against an opaque neighbour and keeps non-cull faces", () => {
      const world = worldWith([
        { x: 0, y: 64, z: 0, name: "minecraft:stone" },
        { x: 0, y: 65, z: 0, name: "minecraft:stone" },
      ]);
      const culled = buildChunkMesh(
        world,
        { x: 0, z: 0 },
        assets([model("minecraft:stone", "minecraft:block/stone")]),
      );
      // Only the upper block's top face survives: the lower face is culled by the block above.
      expect(culled.faceCount).toBe(1);

      const unculled = buildChunkMesh(
        world,
        { x: 0, z: 0 },
        assets([model("minecraft:stone", "minecraft:block/stone", { cullface: false })]),
      );
      expect(unculled.faceCount).toBe(2);
    });

    it("routes alpha textures to cutout and uses the missing cube for unknown blocks", () => {
      const models = assets(
        [model("minecraft:oak_leaves", "minecraft:block/oak_leaves", { cullface: false })],
        [texture("minecraft:block/oak_leaves", true)],
      );
      const leavesWorld = worldWith([{ x: 0, y: 64, z: 0, name: "minecraft:oak_leaves" }]);
      const leavesMesh = buildChunkMesh(leavesWorld, { x: 0, z: 0 }, models);
      expect(leavesMesh.faceCount).toBe(1);
      expect(leavesMesh.opaque.positions.length).toBe(0);
      expect(leavesMesh.cutout.positions.length).toBe(4 * 3);

      const unknownWorld = worldWith([{ x: 0, y: 64, z: 0, name: "minecraft:not_in_bundle" }]);
      const unknownMesh = buildChunkMesh(unknownWorld, { x: 0, z: 0 }, models);
      expect(unknownMesh.faceCount).toBe(6);
      expect(unknownMesh.opaque.positions.length).toBe(6 * 4 * 3);
    });

    it("renders vanilla fluids with their real texture as a documented cube approximation", () => {
      const water: BundleBlock = {
        name: "minecraft:water",
        properties: {},
        missingModel: true,
        parts: [],
        warnings: ["vanilla fluid renderer not implemented"],
        unsupported: [],
      };
      const models = assets([water], [texture("minecraft:block/water_still", true)]);
      const world = worldWith([{ x: 0, y: 64, z: 0, name: "minecraft:water" }]);
      const mesh = buildChunkMesh(world, { x: 0, z: 0 }, models);
      expect(mesh.faceCount).toBe(6);
      expect(mesh.opaque.positions.length).toBe(0);
      expect(mesh.translucent.positions.length).toBe(6 * 4 * 3);
    });
  });
});
