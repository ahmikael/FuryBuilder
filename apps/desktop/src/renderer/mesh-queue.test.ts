import { describe, expect, it } from "vitest";

import {
  ChunkMeshQueue,
  chunkDistanceSq,
  DEFAULT_MESH_BUDGET,
  normalizeMeshBudget,
  selectChunksToEvict,
} from "@/renderer/mesh-queue";

describe("chunk mesh queue", () => {
  it("de-duplicates entries and skips cancelled work", () => {
    const queue = new ChunkMeshQueue();
    expect(queue.enqueue({ x: 1, z: 2 })).toBe(true);
    expect(queue.enqueue({ x: 1, z: 2 })).toBe(false);
    expect(queue.enqueue({ x: 0, z: 0 })).toBe(true);
    expect(queue.size).toBe(2);

    queue.remove({ x: 1, z: 2 });
    expect(queue.size).toBe(1);
    expect(queue.take({ maxJobs: 10, maxMs: 100, now: () => 0 })).toEqual([{ x: 0, z: 0 }]);
    expect(queue.size).toBe(0);
  });

  it("orders snapshot work by distance and honours the queue limit", () => {
    const queue = new ChunkMeshQueue();
    const added = queue.enqueueAll(
      [
        { x: 5, z: 0 },
        { x: 0, z: 0 },
        { x: 2, z: 0 },
      ],
      { x: 0, z: 0 },
      2,
    );
    expect(added).toBe(2);
    expect(queue.take({ maxJobs: 10, maxMs: 100, now: () => 0 })).toEqual([
      { x: 0, z: 0 },
      { x: 2, z: 0 },
    ]);
  });

  it("caps jobs per frame and always takes at least one job", () => {
    const queue = new ChunkMeshQueue();
    queue.enqueueAll([
      { x: 1, z: 0 },
      { x: 2, z: 0 },
      { x: 3, z: 0 },
    ]);
    expect(queue.take({ maxJobs: 2, maxMs: 100, now: () => 0 })).toHaveLength(2);
    expect(queue.size).toBe(1);

    const second = new ChunkMeshQueue();
    second.enqueueAll([
      { x: 1, z: 0 },
      { x: 2, z: 0 },
    ]);
    let now = 0;
    const clock = () => {
      const value = now;
      now += 5;
      return value;
    };
    expect(second.take({ maxJobs: 10, maxMs: 1, now: clock })).toHaveLength(1);
    expect(second.size).toBe(1);
  });

  it("clears all pending work", () => {
    const queue = new ChunkMeshQueue();
    queue.enqueueAll([
      { x: 1, z: 0 },
      { x: 2, z: 0 },
    ]);
    queue.clear();
    expect(queue.size).toBe(0);
    expect(queue.take({ maxJobs: 10, maxMs: 100, now: () => 0 })).toEqual([]);
  });
});

describe("chunk residency budget", () => {
  it("selects deterministic farthest chunks first", () => {
    const chunks = [
      { x: 0, z: 0 },
      { x: 5, z: 0 },
      { x: -5, z: 0 },
      { x: 0, z: 5 },
      { x: 0, z: -5 },
    ];
    expect(selectChunksToEvict(chunks, { x: 0, z: 0 }, 2)).toEqual([
      { x: -5, z: 0 },
      { x: 0, z: -5 },
    ]);
  });

  it("returns nothing when there is no excess", () => {
    const chunks = [{ x: 1, z: 1 }];
    expect(selectChunksToEvict(chunks, { x: 0, z: 0 }, 0)).toEqual([]);
    expect(selectChunksToEvict(chunks, { x: 0, z: 0 }, -1)).toEqual([]);
    expect(selectChunksToEvict([], { x: 0, z: 0 }, 4)).toEqual([]);
  });

  it("clamps an excess larger than the loaded set", () => {
    const chunks = [
      { x: 1, z: 0 },
      { x: 2, z: 0 },
    ];
    expect(selectChunksToEvict(chunks, { x: 0, z: 0 }, 10)).toHaveLength(2);
  });

  it("computes squared chunk distance", () => {
    expect(chunkDistanceSq({ x: 0, z: 0 }, { x: 3, z: 4 })).toBe(25);
    expect(chunkDistanceSq({ x: -2, z: -3 }, { x: 0, z: 0 })).toBe(13);
  });

  it("clamps invalid budget values instead of disabling a bound", () => {
    const budget = normalizeMeshBudget({
      maxResidentChunks: 0,
      maxJobsPerFrame: Number.NaN,
      maxFrameMs: -4,
      maxPendingJobs: 3,
    });
    expect(budget.maxResidentChunks).toBe(1);
    expect(budget.maxJobsPerFrame).toBe(DEFAULT_MESH_BUDGET.maxJobsPerFrame);
    expect(budget.maxFrameMs).toBe(1);
    expect(budget.maxPendingJobs).toBe(3);
    expect(DEFAULT_MESH_BUDGET.maxResidentChunks).toBeGreaterThan(0);
    expect(DEFAULT_MESH_BUDGET.maxPendingJobs).toBeGreaterThanOrEqual(
      DEFAULT_MESH_BUDGET.maxResidentChunks,
    );
  });
});
