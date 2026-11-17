import { isAirName } from "@/renderer/assets";
import type { BlockPosition, BlockState } from "@/renderer/types";
import type { RenderWorld } from "@/renderer/world";

export interface RayHit {
  pos: BlockPosition;
  state: BlockState;
  /** Outward face normal of the block that was entered. */
  face: [number, number, number];
}

type ScanAxis = "x" | "y" | "z";

/**
 * Voxel DDA (Amanatides & Woo) against authoritative CPU block data. Mesh intersection
 * is intentionally not used: it can disagree with the world after a rebuild and does not
 * return exact block coordinates on negative coordinates.
 */
export function raycastBlocks(
  world: RenderWorld,
  origin: [number, number, number],
  direction: [number, number, number],
  maxDistance = 256,
): RayHit | null {
  const length = Math.hypot(direction[0], direction[1], direction[2]);
  if (length === 0 || Number.isNaN(length)) return null;
  const dx = direction[0] / length;
  const dy = direction[1] / length;
  const dz = direction[2] / length;

  let x = Math.floor(origin[0]);
  let y = Math.floor(origin[1]);
  let z = Math.floor(origin[2]);

  const stepX = dx > 0 ? 1 : dx < 0 ? -1 : 0;
  const stepY = dy > 0 ? 1 : dy < 0 ? -1 : 0;
  const stepZ = dz > 0 ? 1 : dz < 0 ? -1 : 0;

  const tDeltaX = dx === 0 ? Infinity : Math.abs(1 / dx);
  const tDeltaY = dy === 0 ? Infinity : Math.abs(1 / dy);
  const tDeltaZ = dz === 0 ? Infinity : Math.abs(1 / dz);

  const boundary = (value: number, step: number, coordinate: number): number => {
    if (step === 0) return Infinity;
    return step > 0 ? coordinate + 1 - value : value - coordinate;
  };

  let tMaxX = boundary(origin[0], stepX, x) * tDeltaX;
  let tMaxY = boundary(origin[1], stepY, y) * tDeltaY;
  let tMaxZ = boundary(origin[2], stepZ, z) * tDeltaZ;

  const startBlock = world.blockAt({ x, y, z });
  if (startBlock && !isAirName(startBlock.name)) {
    return { pos: { x, y, z }, state: startBlock, face: [0, 1, 0] };
  }

  let traveled = 0;
  let lastAxis: ScanAxis | null = null;
  for (let step = 0; step < maxDistance * 3 + 8; step += 1) {
    if (tMaxX <= tMaxY && tMaxX <= tMaxZ) {
      x += stepX;
      traveled = tMaxX;
      tMaxX += tDeltaX;
      lastAxis = "x";
    } else if (tMaxY <= tMaxZ) {
      y += stepY;
      traveled = tMaxY;
      tMaxY += tDeltaY;
      lastAxis = "y";
    } else {
      z += stepZ;
      traveled = tMaxZ;
      tMaxZ += tDeltaZ;
      lastAxis = "z";
    }
    if (traveled > maxDistance) return null;

    const block = world.blockAt({ x, y, z });
    if (block === null) return null; // ray reached unloaded space
    if (isAirName(block.name)) continue;

    const face: [number, number, number] =
      lastAxis === "x" ? [-stepX, 0, 0] : lastAxis === "y" ? [0, -stepY, 0] : [0, 0, -stepZ];
    return { pos: { x, y, z }, state: block, face };
  }
  return null;
}
