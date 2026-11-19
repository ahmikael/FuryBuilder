import type { ChunkPos } from "@/renderer/types";

/**
 * Bounded meshing and chunk-residency policy for the viewport.
 *
 * The renderer keeps decoded chunks and GPU meshes in memory. Without a budget a large
 * snapshot can freeze the UI while every chunk is meshed, and a long camera tour can grow
 * GPU memory without limit. These helpers make the policy explicit and testable without a
 * WebGL context.
 */

export interface MeshBudget {
  /** Maximum decoded chunks kept in the renderer; farthest chunks are evicted first. */
  maxResidentChunks: number;
  /** Snapshots at or below this size are meshed synchronously for immediate feedback. */
  maxSynchronousSnapshotChunks: number;
  /** Small edits are rebuilt synchronously; larger dirty sets go through the queue. */
  maxSynchronousUpdateChunks: number;
  /** Upper bound on chunk meshes built in one animation frame. */
  maxJobsPerFrame: number;
  /** Soft CPU budget for one animation frame; one job always runs to avoid starvation. */
  maxFrameMs: number;
  /** Queue cap; prevents a broken caller from growing the pending list without bound. */
  maxPendingJobs: number;
}

export const DEFAULT_MESH_BUDGET: MeshBudget = {
  maxResidentChunks: 4096,
  maxSynchronousSnapshotChunks: 256,
  maxSynchronousUpdateChunks: 64,
  maxJobsPerFrame: 16,
  maxFrameMs: 12,
  maxPendingJobs: 8192,
};

function positiveInt(value: number, fallback: number): number {
  if (!Number.isFinite(value)) return fallback;
  return Math.max(1, Math.floor(value));
}

/** Clamps a partial policy so no caller can disable a bound with a bad value. */
export function normalizeMeshBudget(partial: Partial<MeshBudget> = {}): MeshBudget {
  const merged = { ...DEFAULT_MESH_BUDGET, ...partial };
  return {
    maxResidentChunks: positiveInt(merged.maxResidentChunks, DEFAULT_MESH_BUDGET.maxResidentChunks),
    maxSynchronousSnapshotChunks: positiveInt(
      merged.maxSynchronousSnapshotChunks,
      DEFAULT_MESH_BUDGET.maxSynchronousSnapshotChunks,
    ),
    maxSynchronousUpdateChunks: positiveInt(
      merged.maxSynchronousUpdateChunks,
      DEFAULT_MESH_BUDGET.maxSynchronousUpdateChunks,
    ),
    maxJobsPerFrame: positiveInt(merged.maxJobsPerFrame, DEFAULT_MESH_BUDGET.maxJobsPerFrame),
    maxFrameMs: positiveInt(merged.maxFrameMs, DEFAULT_MESH_BUDGET.maxFrameMs),
    maxPendingJobs: positiveInt(merged.maxPendingJobs, DEFAULT_MESH_BUDGET.maxPendingJobs),
  };
}

export function chunkDistanceSq(a: ChunkPos, b: ChunkPos): number {
  const dx = a.x - b.x;
  const dz = a.z - b.z;
  return dx * dx + dz * dz;
}

/** Deterministic farthest-first eviction; ties prefer smaller x, then smaller z. */
export function selectChunksToEvict(
  chunks: readonly ChunkPos[],
  cameraChunk: ChunkPos,
  excess: number,
): ChunkPos[] {
  if (excess <= 0 || chunks.length === 0) return [];
  const count = Math.min(Math.floor(excess), chunks.length);
  return chunks
    .map((pos) => ({ pos, distance: chunkDistanceSq(pos, cameraChunk) }))
    .sort(
      (left, right) =>
        right.distance - left.distance || left.pos.x - right.pos.x || left.pos.z - right.pos.z,
    )
    .slice(0, count)
    .map((entry) => entry.pos);
}

function keyOf(pos: ChunkPos): string {
  return `${pos.x},${pos.z}`;
}

/**
 * FIFO queue with de-duplication. A queue entry can be cancelled by remove(); take()
 * skips cancelled entries. `enqueueAll` can order work by distance to a snapshot centre.
 */
export class ChunkMeshQueue {
  private order: ChunkPos[] = [];
  private head = 0;
  private readonly keys = new Set<string>();

  get size(): number {
    return this.keys.size;
  }

  enqueue(pos: ChunkPos): boolean {
    const key = keyOf(pos);
    if (this.keys.has(key)) return false;
    this.keys.add(key);
    this.order.push({ x: pos.x, z: pos.z });
    return true;
  }

  enqueueAll(chunks: readonly ChunkPos[], center?: ChunkPos, limit = Infinity): number {
    const ordered = center
      ? [...chunks].sort(
          (left, right) =>
            chunkDistanceSq(left, center) - chunkDistanceSq(right, center) ||
            left.x - right.x ||
            left.z - right.z,
        )
      : chunks;
    const max = Math.max(0, Math.floor(limit));
    let added = 0;
    for (const pos of ordered) {
      if (added >= max) break;
      if (this.enqueue(pos)) added += 1;
    }
    return added;
  }

  remove(pos: ChunkPos): void {
    this.keys.delete(keyOf(pos));
  }

  clear(): void {
    this.order = [];
    this.head = 0;
    this.keys.clear();
  }

  /**
   * Removes up to `maxJobs` entries, stopping near `maxMs`. The first available entry is
   * always returned so a zero/small time budget cannot starve the queue.
   */
  take(options: { maxJobs: number; maxMs: number; now?: () => number }): ChunkPos[] {
    const now = options.now ?? (() => performance.now());
    const startedAt = now();
    const maxJobs = Math.max(1, Math.floor(options.maxJobs));
    const taken: ChunkPos[] = [];
    while (this.head < this.order.length && taken.length < maxJobs) {
      if (taken.length > 0 && now() - startedAt >= options.maxMs) break;
      const pos = this.order[this.head]!;
      this.head += 1;
      if (!this.keys.delete(keyOf(pos))) continue;
      taken.push(pos);
    }
    this.compact();
    return taken;
  }

  private compact(): void {
    if (this.head === 0) return;
    if (this.head >= this.order.length) {
      this.order = [];
      this.head = 0;
      return;
    }
    if (this.head >= 1024 && this.head * 2 >= this.order.length) {
      this.order = this.order.slice(this.head);
      this.head = 0;
    }
  }
}
