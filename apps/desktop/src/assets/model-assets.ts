import type { AssetBundle, BundleBlock, BundleTexture } from "@/assets/render-types";
import type { BlockState } from "@/renderer/types";

const MISSING_TEXTURE = "__missing__";

/** Canonical key used by both bundle requests and the renderer. Property order never matters. */
export function blockStateKey(state: BlockState): string {
  const properties = state.properties ?? {};
  const keys = Object.keys(properties).sort();
  if (keys.length === 0) return state.name;
  return `${state.name}[${keys.map((key) => `${key}=${properties[key]}`).join(",")}]`;
}

export interface BlockTint {
  /** Fixed approximation of a biome tint; documented as not biome-accurate. */
  color: [number, number, number];
}

const TINTS: Record<string, [number, number, number]> = {
  "minecraft:grass_block": [0.53, 0.78, 0.35],
  "minecraft:grass": [0.53, 0.78, 0.35],
  "minecraft:short_grass": [0.53, 0.78, 0.35],
  "minecraft:tall_grass": [0.53, 0.78, 0.35],
  "minecraft:fern": [0.53, 0.78, 0.35],
  "minecraft:large_fern": [0.53, 0.78, 0.35],
  "minecraft:oak_leaves": [0.45, 0.68, 0.31],
  "minecraft:jungle_leaves": [0.45, 0.68, 0.31],
  "minecraft:acacia_leaves": [0.55, 0.7, 0.28],
  "minecraft:dark_oak_leaves": [0.4, 0.6, 0.28],
  "minecraft:vine": [0.45, 0.68, 0.31],
  "minecraft:water": [0.25, 0.46, 0.89],
  "minecraft:flowing_water": [0.25, 0.46, 0.89],
};

export interface AssetStats {
  models: number;
  missingModels: number;
  textures: number;
  missingTextures: number;
  atlasWidth: number;
  atlasHeight: number;
  cacheKey: string;
  cacheHit: boolean;
}

/**
 * Resolved render assets for one bundle. A block state without a model is rendered as the
 * clearly marked missing checker, never as an invented texture. Missing assets never change
 * block data.
 */
export class AssetModels {
  readonly blocks = new Map<string, BundleBlock>();
  readonly textures = new Map<string, BundleTexture>();
  readonly atlasWidth: number;
  readonly atlasHeight: number;
  readonly warnings: string[];
  readonly stats: AssetStats;

  constructor(readonly bundle: AssetBundle) {
    this.atlasWidth = Math.max(1, bundle.atlasWidth);
    this.atlasHeight = Math.max(1, bundle.atlasHeight);
    this.warnings = [...bundle.warnings];
    for (const texture of bundle.textures) {
      this.textures.set(texture.key, texture);
    }
    for (const block of bundle.blocks) {
      this.blocks.set(blockStateKey(block), block);
    }
    this.stats = {
      models: bundle.blocks.filter((block) => !block.missingModel).length,
      missingModels: bundle.blocks.filter((block) => block.missingModel).length,
      textures: bundle.textures.filter((texture) => !texture.missing).length,
      missingTextures: bundle.textures.filter((texture) => texture.missing).length,
      atlasWidth: this.atlasWidth,
      atlasHeight: this.atlasHeight,
      cacheKey: bundle.cacheKey,
      cacheHit: bundle.cacheHit,
    };
  }

  block(state: BlockState): BundleBlock | undefined {
    return this.blocks.get(blockStateKey(state));
  }

  texture(key: string): BundleTexture {
    return this.textures.get(key) ?? this.textures.get(MISSING_TEXTURE) ?? fallbackTexture();
  }

  textureHasAlpha(key: string): boolean {
    return this.texture(key).hasAlpha;
  }

  /**
   * Maps a model UV rectangle (0..16 texture units) to four atlas UV pairs in the same
   * corner order as the mesher's face definitions. Face rotation is applied cyclically.
   */
  textureQuad(
    key: string,
    uv: readonly [number, number, number, number],
    rotation: number,
  ): Array<[number, number]> {
    const texture = this.texture(key);
    const [u0, v0, u1, v1] = uv;
    let corners: Array<[number, number]> = [
      [u0, v1],
      [u0, v0],
      [u1, v0],
      [u1, v1],
    ];
    const steps = (((Math.round(rotation / 90) % 4) + 4) % 4) | 0;
    for (let step = 0; step < steps; step += 1) {
      const last = corners[3]!;
      corners = [last, corners[0]!, corners[1]!, corners[2]!];
    }
    return corners.map(([u, v]) => [
      (texture.x + (u / 16) * texture.width) / this.atlasWidth,
      (texture.y + (v / 16) * texture.height) / this.atlasHeight,
    ]);
  }

  tint(state: BlockState, tintIndex: number | null): [number, number, number] {
    if (tintIndex === null) return [1, 1, 1];
    return TINTS[state.name] ?? [1, 1, 1];
  }

  atlasDataUrl(): string {
    return `data:image/png;base64,${this.bundle.atlasPngBase64}`;
  }
}

function fallbackTexture(): BundleTexture {
  return {
    key: MISSING_TEXTURE,
    path: null,
    x: 0,
    y: 0,
    width: 16,
    height: 16,
    sourceWidth: 16,
    sourceHeight: 16,
    missing: true,
    hasAlpha: false,
  };
}
