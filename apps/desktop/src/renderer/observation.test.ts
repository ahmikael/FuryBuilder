import { describe, expect, it } from "vitest";
import { SnapshotBuilder, snapshotFromBlocks } from "@/renderer/builders";
import { buildCaptureMetadata, MAX_LEGEND_ENTRIES } from "@/renderer/observation";
import type { CameraPose } from "@/renderer/types";
import { RenderWorld } from "@/renderer/world";

const camera: CameraPose = {
  position: [10, 70, 20],
  target: [2, 66, -2],
  yaw: -0.4,
  pitch: -0.3,
  fov: 70,
};

function worldFrom(builder: SnapshotBuilder): RenderWorld {
  const world = new RenderWorld();
  world.setSnapshot(builder.build("minecraft:overworld", 7, 1));
  return world;
}

describe("observation metadata", () => {
  it("reports revision, dimension and camera pose", () => {
    const builder = new SnapshotBuilder();
    builder.setBlock(0, 64, 0, "minecraft:stone");
    const world = worldFrom(builder);
    const metadata = buildCaptureMetadata(
      world,
      camera,
      [1280, 720],
      [{ x: 0, z: 0 }],
      new Date("2026-11-17T12:00:00.000Z"),
    );

    expect(metadata.schemaVersion).toBe(1);
    expect(metadata.mode).toBe("viewport");
    expect(metadata.dimension).toBe("minecraft:overworld");
    expect(metadata.worldRevision).toBe(7);
    expect(metadata.camera).toEqual(camera);
    expect(metadata.resolution).toEqual([1280, 720]);
    expect(metadata.renderer).toBe("three-webgl2");
    expect(metadata.rendererSchema).toBe(1);
    expect(metadata.textureSource).toBe("placeholder");
    expect(metadata.capturedAt).toBe("2026-11-17T12:00:00.000Z");
    expect(metadata.partial).toBe(false);
    expect(metadata.notes.length).toBeGreaterThanOrEqual(3);
    expect(metadata.notes.join(" ")).toContain("original FuryBuilder placeholders");
  });

  it("labels a local Minecraft asset bundle without claiming that Mojang files are shipped", () => {
    const builder = new SnapshotBuilder();
    builder.setBlock(0, 64, 0, "minecraft:stone");
    const metadata = buildCaptureMetadata(
      worldFrom(builder),
      camera,
      [640, 480],
      [{ x: 0, z: 0 }],
      undefined,
      "minecraft_assets",
    );

    expect(metadata.textureSource).toBe("minecraft_assets");
    const notes = metadata.notes.join(" ");
    expect(notes).toContain("locally configured Minecraft asset bundle");
    expect(notes).toContain("not embedded in FuryBuilder");
    expect(notes).not.toContain("original FuryBuilder placeholders");
  });

  it("builds a sorted legend with approximate colours", () => {
    const builder = new SnapshotBuilder();
    builder.setBlock(0, 64, 0, "minecraft:stone");
    builder.setBlock(1, 64, 0, "minecraft:stone");
    builder.setBlock(2, 64, 0, "minecraft:grass_block");
    builder.setBlock(3, 64, 0, "minecraft:air");
    const metadata = buildCaptureMetadata(worldFrom(builder), camera, [640, 480], [{ x: 0, z: 0 }]);

    expect(metadata.legend[0]).toMatchObject({ state: "minecraft:stone", count: 2 });
    expect(metadata.legend[1]).toMatchObject({ state: "minecraft:grass_block", count: 1 });
    expect(metadata.legend.some((entry) => entry.state === "minecraft:air")).toBe(false);
    expect(metadata.legend[0]?.color).toMatch(/^#[0-9a-f]{6}$/i);
    expect(metadata.legendTruncated).toBe(false);
  });

  it("marks missing requested chunks as partial with an explicit list", () => {
    const builder = new SnapshotBuilder();
    builder.setBlock(0, 64, 0, "minecraft:stone");
    const world = worldFrom(builder);
    const metadata = buildCaptureMetadata(
      world,
      camera,
      [800, 600],
      [
        { x: 0, z: 0 },
        { x: 5, z: -3 },
      ],
    );

    expect(metadata.partial).toBe(true);
    expect(metadata.missingChunks).toEqual([{ x: 5, z: -3 }]);
    expect(metadata.loadedChunks).toEqual([{ x: 0, z: 0 }]);
  });

  it("truncates very large legends but reports the truncation", () => {
    const builder = new SnapshotBuilder();
    const names = [
      "minecraft:stone",
      "minecraft:dirt",
      "minecraft:grass_block",
      "minecraft:sand",
      "minecraft:oak_planks",
      "minecraft:oak_log",
      "minecraft:oak_leaves",
      "minecraft:glass",
      "minecraft:water",
      "minecraft:bricks",
      "minecraft:stone_bricks",
      "minecraft:deepslate",
      "minecraft:netherrack",
      "minecraft:snow_block",
      "minecraft:obsidian",
      "minecraft:gold_block",
      "minecraft:sea_lantern",
      "minecraft:red_wool",
      "minecraft:white_wool",
      "minecraft:torch",
    ];
    names.forEach((name, index) => builder.setBlock(index, 64, 0, name));
    const metadata = buildCaptureMetadata(worldFrom(builder), camera, [320, 240], [{ x: 0, z: 0 }]);

    expect(metadata.legend).toHaveLength(MAX_LEGEND_ENTRIES);
    expect(metadata.legendTruncated).toBe(true);
  });

  it("counts non-air palette entries across sections", () => {
    const world = new RenderWorld();
    world.setSnapshot(
      snapshotFromBlocks([
        { x: 0, y: 0, z: 0, name: "minecraft:stone" },
        { x: 0, y: 20, z: 0, name: "minecraft:stone" },
        { x: 1, y: 20, z: 0, name: "minecraft:glass" },
      ]),
    );
    const counts = world.blockCountsByState();
    expect(counts.get("minecraft:stone")).toBe(2);
    expect(counts.get("minecraft:glass")).toBe(1);
    expect(counts.has("minecraft:air")).toBe(false);
  });
});
