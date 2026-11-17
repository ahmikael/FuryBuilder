# Performance runs

Environment: Windows, Rust release (`cargo bench`), local development machine.
These are internal reference numbers, not release promises. Re-run after hot-path changes.

## 2026-11-17 — M2 core smoke benchmark

Command:

```powershell
cargo bench -p furybuilder-world-application --bench world_ops
```

Scenario: one `fill_cuboid` of 100×32×100 (320,000) stone blocks, then 100,000 block
reads, undo and redo using section-diff snapshots.

```text
fill: 320000 blocks in 149.59 ms (2139223 blocks/s), revision 1
reads: 100000 blocks in 13.88 ms (7206532 reads/s), checksum 1500000
undo: 320000 blocks in 0.18 ms, revision 2
redo: 320000 blocks in 0.09 ms, revision 3
```

Notes:

- The fill time includes change collection, section snapshots and command overhead.
- Undo/redo restore whole touched sections, which is why they are far faster than the
  edit that produced them.
- Region summaries and IPC payloads are bounded separately; these numbers do not measure
  renderer meshing or IPC throughput.

## 2026-11-17 — M3 renderer meshing smoke benchmark

Command (from `apps/desktop`):

```powershell
$env:FURY_PERF='1'; pnpm exec vitest run src/renderer/mesher.perf.test.ts --reporter=verbose
```

Scenario: centre 16×16 column of the offline demo scene (terrain, house, stairs, leaves,
water), 50 iterations after 10 warm-up iterations. Node/Vitest, CPU only.

```text
MESH_PERF iterations=50 totalMs=228.33 perChunkMs=4.57 faces=875
```

Interpretation:

- ~4.6 ms per dirty chunk on the development machine, with 875 emitted faces after
  hidden-face culling and approximate stairs/torch shapes.
- Initial 3×3 load is 9 chunk builds; later edits rebuild one chunk plus neighbours only.
- Greedy meshing is not adopted (see `docs/design/renderer.md`). It would need UV tiling
  in a custom shader and does not address the first bottleneck at this scene size.

