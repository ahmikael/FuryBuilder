import type { BlockState } from "@/renderer/types";

export type Vec3 = readonly [number, number, number];

/** One axis-aligned box inside a block, in local 0..1 coordinates. */
export interface BlockBoxShape {
  min: Vec3;
  max: Vec3;
}

function slab(top: boolean): BlockBoxShape[] {
  return top ? [{ min: [0, 0.5, 0], max: [1, 1, 1] }] : [{ min: [0, 0, 0], max: [1, 0.5, 1] }];
}

/**
 * Approximate vanilla collision shapes for the curated MVP block set. These are visual
 * and picking approximations, not a full block-model engine (ADR-009). Unknown blocks
 * render as full cubes so their data is never hidden.
 *
 * Returns `null` for a full cube (the mesher can cull neighbours normally).
 */
export function boxShapeFor(state: BlockState): BlockBoxShape[] | null {
  const name = state.name;
  const properties = state.properties ?? {};
  if (name.endsWith("_slab")) {
    if (properties.type === "double") return null;
    return slab(properties.type === "top");
  }

  if (name.endsWith("_stairs")) {
    const lower: BlockBoxShape = { min: [0, 0, 0], max: [1, 0.5, 1] };
    const facing = properties.facing ?? "north";
    const upper: BlockBoxShape =
      facing === "east"
        ? { min: [0.5, 0.5, 0], max: [1, 1, 1] }
        : facing === "west"
          ? { min: [0, 0.5, 0], max: [0.5, 1, 1] }
          : facing === "south"
            ? { min: [0, 0.5, 0.5], max: [1, 1, 1] }
            : { min: [0, 0.5, 0], max: [1, 1, 0.5] };
    return [lower, upper];
  }

  if (name === "minecraft:snow") {
    const layers = Number.parseInt(properties.layers ?? "1", 10);
    const height = Math.min(8, Math.max(1, Number.isFinite(layers) ? layers : 1)) / 8;
    return [{ min: [0, 0, 0], max: [1, height, 1] }];
  }

  if (name === "minecraft:torch") {
    return [{ min: [0.4375, 0, 0.4375], max: [0.5625, 0.625, 0.5625] }];
  }

  if (name === "minecraft:wall_torch") {
    return [{ min: [0.4375, 0.2, 0.4375], max: [0.5625, 0.825, 0.5625] }];
  }

  if (name === "minecraft:lantern") {
    return [{ min: [0.3125, 0, 0.3125], max: [0.6875, 0.5625, 0.6875] }];
  }

  return null;
}

const shapeCache = new Map<string, BlockBoxShape[] | null>();

/** Cached shape lookup used by the mesher hot loop. */
export function cachedBoxShapeFor(state: BlockState): BlockBoxShape[] | null {
  const properties = state.properties ?? {};
  const key = `${state.name}|${Object.entries(properties)
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([name, value]) => `${name}=${value}`)
    .join(",")}`;
  if (shapeCache.has(key)) return shapeCache.get(key) ?? null;
  const shape = boxShapeFor(state);
  shapeCache.set(key, shape);
  return shape;
}
