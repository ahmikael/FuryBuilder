import { describe, expect, it } from "vitest";
import { AssetModels, blockStateKey } from "@/assets/model-assets";
import type { AssetBundle, BundleBlock, BundleTexture } from "@/assets/render-types";

const stoneTexture: BundleTexture = {
  key: "minecraft:block/stone",
  path: "assets/minecraft/textures/block/stone.png",
  x: 1,
  y: 1,
  width: 16,
  height: 16,
  sourceWidth: 16,
  sourceHeight: 16,
  missing: false,
  hasAlpha: false,
};

const missingTexture: BundleTexture = {
  key: "__missing__",
  path: null,
  x: 20,
  y: 20,
  width: 16,
  height: 16,
  sourceWidth: 16,
  sourceHeight: 16,
  missing: true,
  hasAlpha: false,
};

const stoneBlock: BundleBlock = {
  name: "minecraft:stone",
  properties: {},
  missingModel: false,
  parts: [
    {
      model: "minecraft:block/stone",
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
              texture: "minecraft:block/stone",
              uv: [0, 0, 16, 16],
              rotation: 0,
              cullface: "up",
              tintIndex: null,
            },
          ],
        },
      ],
    },
  ],
  warnings: [],
  unsupported: [],
};

function bundle(): AssetBundle {
  return {
    schemaVersion: 1,
    cacheKey: "cache-key",
    cacheHit: false,
    baseJar: null,
    baseJarSha1: null,
    packs: [],
    atlasWidth: 64,
    atlasHeight: 64,
    atlasPngBase64: "",
    textures: [stoneTexture, missingTexture],
    blocks: [stoneBlock],
    warnings: ["probe warning"],
  };
}

describe("asset model bundle", () => {
  it("keys block states independently of property order", () => {
    expect(
      blockStateKey({
        name: "minecraft:oak_stairs",
        properties: { half: "bottom", facing: "north" },
      }),
    ).toBe(
      blockStateKey({
        name: "minecraft:oak_stairs",
        properties: { facing: "north", half: "bottom" },
      }),
    );
    expect(blockStateKey({ name: "minecraft:stone" })).toBe("minecraft:stone");
  });

  it("maps model UVs into the atlas and applies face rotation", () => {
    const assets = new AssetModels(bundle());
    const quad = assets.textureQuad("minecraft:block/stone", [0, 0, 16, 16], 0);
    expect(quad).toEqual([
      [1 / 64, 17 / 64],
      [1 / 64, 1 / 64],
      [17 / 64, 1 / 64],
      [17 / 64, 17 / 64],
    ]);
    expect(assets.textureQuad("minecraft:block/stone", [0, 0, 8, 8], 0)[2]).toEqual([
      (1 + 8) / 64,
      1 / 64,
    ]);
  });

  it("falls back to the missing checker without inventing a texture", () => {
    const assets = new AssetModels(bundle());
    expect(assets.texture("minecraft:block/not_there").missing).toBe(true);
    expect(assets.texture("minecraft:block/not_there").key).toBe("__missing__");
    expect(assets.textureHasAlpha("minecraft:block/not_there")).toBe(false);
    expect(assets.stats).toEqual({
      models: 1,
      missingModels: 0,
      textures: 1,
      missingTextures: 1,
      atlasWidth: 64,
      atlasHeight: 64,
      cacheKey: "cache-key",
      cacheHit: false,
    });
    expect(assets.warnings).toEqual(["probe warning"]);
  });

  it("applies fixed documented tints only when a face requests one", () => {
    const assets = new AssetModels(bundle());
    expect(assets.tint({ name: "minecraft:grass_block" }, 0)).toEqual([0.53, 0.78, 0.35]);
    expect(assets.tint({ name: "minecraft:grass_block" }, null)).toEqual([1, 1, 1]);
    expect(assets.tint({ name: "minecraft:stone" }, 0)).toEqual([1, 1, 1]);
  });
});
