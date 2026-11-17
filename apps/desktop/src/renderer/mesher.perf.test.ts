import { describe, expect, it } from "vitest";
import { createDemoSnapshot } from "@/renderer/demo";
import { buildChunkMesh } from "@/renderer/mesher";
import { RenderWorld } from "@/renderer/world";

const enabled = process.env.FURY_PERF === "1";

/**
 * Manual renderer profiling. Run with:
 * `$env:FURY_PERF=1; pnpm exec vitest run src/renderer/mesher.perf.test.ts`
 * Results are recorded in docs/testing/perf-runs.md.
 */
describe.skipIf(!enabled)("renderer meshing profile", () => {
  it("measures the centre demo chunk", () => {
    const world = new RenderWorld();
    world.setSnapshot(createDemoSnapshot());

    for (let index = 0; index < 10; index += 1) {
      buildChunkMesh(world, { x: 0, z: 0 });
    }
    const iterations = 50;
    const started = performance.now();
    let faces = 0;
    for (let index = 0; index < iterations; index += 1) {
      faces = buildChunkMesh(world, { x: 0, z: 0 }).faceCount;
    }
    const elapsed = performance.now() - started;
    const perChunk = elapsed / iterations;
    console.log(
      `MESH_PERF iterations=${iterations} totalMs=${elapsed.toFixed(2)} perChunkMs=${perChunk.toFixed(2)} faces=${faces}`,
    );
    expect(faces).toBeGreaterThan(0);
  });
});
