import type { BoxBounds, ClipboardPayload } from "@/editor/types";
import { boundsVolume } from "@/editor/session";
import type { BlockPosition } from "@/renderer/types";

/**
 * Mirrors `world_application::MAX_REGION_READ_BLOCKS`. A copy request above this volume
 * is rejected by the core, so large selections are read as bounded tiles and merged.
 */
export const MAX_REGION_READ_VOLUME = 65_536;

/**
 * At or above this many pasted blocks the editor switches from the immediate apply path
 * to the cancellable Job API, which reports real progress from the Rust operation.
 */
export const LARGE_PASTE_THRESHOLD = 65_536;

type Axis = "x" | "y" | "z";

/** Bounds of a paste with the copied minimum corner moved to `target`. */
export function clipboardPasteBounds(
  clipboard: ClipboardPayload,
  target: BlockPosition,
): BoxBounds {
  return {
    min: { ...target },
    max: {
      x: target.x + clipboard.size[0] - 1,
      y: target.y + clipboard.size[1] - 1,
      z: target.z + clipboard.size[2] - 1,
    },
  };
}

function span(bounds: BoxBounds, axis: Axis): number {
  return bounds.max[axis] - bounds.min[axis] + 1;
}

function splitAxis(bounds: BoxBounds): Axis {
  const x = span(bounds, "x");
  const y = span(bounds, "y");
  const z = span(bounds, "z");
  if (x >= y && x >= z) return "x";
  if (z >= y) return "z";
  return "y";
}

function splitBounds(bounds: BoxBounds, axis: Axis): [BoxBounds, BoxBounds] {
  const middle = Math.floor((bounds.min[axis] + bounds.max[axis]) / 2);
  const lower: BoxBounds = {
    min: { ...bounds.min },
    max: { ...bounds.max, [axis]: middle },
  };
  const upper: BoxBounds = {
    min: { ...bounds.min, [axis]: middle + 1 },
    max: { ...bounds.max },
  };
  return [lower, upper];
}

/**
 * Splits an inclusive region into deterministic, non-overlapping tiles whose volume never
 * exceeds `maxVolume`. Splitting the longest axis keeps the number of requests close to
 * `2 * ceil(volume / maxVolume)` without hard-coding chunk or world-height assumptions.
 * Tiles are returned in x/z/y depth-first order so the merged clipboard is deterministic.
 */
export function planRegionTiles(
  bounds: BoxBounds,
  maxVolume: number = MAX_REGION_READ_VOLUME,
): BoxBounds[] {
  const limit = Math.max(1, Math.floor(maxVolume));
  const total = boundsVolume(bounds);
  if (!Number.isFinite(total) || total <= 0) return [];
  if (total <= limit) return [bounds];

  const tiles: BoxBounds[] = [];
  const pending: BoxBounds[] = [bounds];
  while (pending.length > 0) {
    const next = pending.pop()!;
    if (boundsVolume(next) <= limit) {
      tiles.push(next);
      continue;
    }
    const axis = splitAxis(next);
    if (span(next, axis) < 2) {
      // A single block can never be split; this is unreachable for a valid limit >= 1.
      tiles.push(next);
      continue;
    }
    const [lower, upper] = splitBounds(next, axis);
    pending.push(upper);
    pending.push(lower);
  }
  return tiles;
}
