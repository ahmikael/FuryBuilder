/**
 * TypeScript mirror of the Rust `asset-source` render bundle (M9.2), serialized as camelCase.
 * These types describe derived rendering data only; world block data never comes from here.
 */

export type FaceDirection = "down" | "up" | "north" | "south" | "west" | "east";

export interface BlockStateRequest {
  name: string;
  properties: Record<string, string>;
}

export interface BundleTexture {
  key: string;
  path: string | null;
  x: number;
  y: number;
  width: number;
  height: number;
  sourceWidth: number;
  sourceHeight: number;
  missing: boolean;
  hasAlpha: boolean;
}

export interface BundleElementRotation {
  origin: [number, number, number];
  axis: "x" | "y" | "z";
  angle: number;
  rescale: boolean;
}

export interface BundleFace {
  direction: FaceDirection;
  texture: string;
  uv: [number, number, number, number];
  rotation: number;
  cullface: FaceDirection | null;
  tintIndex: number | null;
}

export interface BundleElement {
  from: [number, number, number];
  to: [number, number, number];
  rotation: BundleElementRotation | null;
  shade: boolean;
  faces: BundleFace[];
}

export interface BundlePart {
  model: string;
  rotationX: number;
  rotationY: number;
  uvlock: boolean;
  elements: BundleElement[];
}

export interface BundleBlock {
  name: string;
  properties: Record<string, string>;
  missingModel: boolean;
  parts: BundlePart[];
  warnings: string[];
  unsupported: string[];
}

export interface AssetBundlePack {
  priority: number;
  path: string;
  name: string;
  packFormat: number | null;
}

export interface AssetBundle {
  schemaVersion: number;
  cacheKey: string;
  cacheHit: boolean;
  baseJar: string | null;
  baseJarSha1: string | null;
  packs: AssetBundlePack[];
  packIssues: string[];
  atlasWidth: number;
  atlasHeight: number;
  atlasPngBase64: string;
  textures: BundleTexture[];
  blocks: BundleBlock[];
  warnings: string[];
}
