# Performance runs

Environment: local Windows development machine; each run identifies its own
build and scenario. These are internal reference numbers, not release promises.
Re-run after hot-path changes.

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

## 2026-11-18 — M0 WebView2 + Three.js hardware smoke

Machine: Windows 11 Pro build 26300, Intel i5-9600KF (6 cores),
NVIDIA GeForce GTX 1060 3GB, driver `32.0.15.6094`; WebView2 Runtime
`157.0.4341.46`. The WebGL renderer identified itself as NVIDIA through ANGLE
and Direct3D 11, not a software renderer. Node was `26.13.1`.

The test starts a separate Tauri process with a fresh WebView2 profile and a
local-only DevTools endpoint. It navigates that WebView2 to this checkout's
Vite server and instantiates the actual `ViewportRenderer`, `SnapshotBuilder`
and mesher modules; it does **not** substitute a browser, mock WebGL or
replace the renderer. The original placeholder atlas is used, not Minecraft
textures. The test creates a synthetic 17×17-chunk snapshot (289 loaded
chunks, 616,752 non-air blocks), installs it in the viewport, points a fixed
camera at it, and uses `ViewportRenderer.capture()` to produce a real PNG.
The result is checked for a valid PNG, nonblank pixels, no missing requested
chunks and no WebGL error. The screenshot was also visually inspected.

| Result                             |                  Observed |
| ---------------------------------- | ------------------------: |
| WebGL version / error              |             WebGL 2.0 / 0 |
| Loaded / missing chunks            |                   289 / 0 |
| Meshes / reported sections         |                 289 / 578 |
| Draw calls / triangles             |             283 / 354,464 |
| Canvas / PNG                       | 1,600×900 / 911,350 bytes |
| Snapshot mesh setup                |                    465 ms |
| Renderer FPS / smoothed frame time |               138 / 16 ms |

The snapshot setup time measures `setSnapshot`, including synchronous mesh
construction, but excludes creating the input and IPC. FPS and frame time are
the renderer's short-window on-screen metrics; they are **not** a controlled
benchmark, percentile, guarantee, or measurement of interactive editing. The
camera frustum and fog limit what is visible. Results vary with GPU, drivers,
window scaling, workload and system activity.

Evidence: [metrics.json](evidence/2026-11-18-webview2/metrics.json) and
[viewport.png](evidence/2026-11-18-webview2/viewport.png).
PNG SHA-256:
`4782eb2130cb7c74347045dedf8c8ce42cf5ed946b9285b3b09a9020bc4023f1`.
The executable SHA-256 is recorded in the metrics. An earlier run on the same
machine produced the same PNG hash. The checked-in image contains only this
synthetic scene.

### Reproduce on Windows

Install the development prerequisites in the root README, then from the
repository root build a debug Tauri binary and start the Vite server in a
separate terminal:

```powershell
cargo build -p furybuilder-desktop
pnpm dev
```

With that server running, in another terminal:

```powershell
$out = Join-Path $env:TEMP ("fury-webview-evidence-" + [guid]::NewGuid().ToString("N"))
node scripts/webview2-smoke.mjs $out
python -m unittest discover -s scripts -p "test_webview2_evidence.py" -v
```

The script requires a **new** output directory, checks that Vite serves the
local checkout, and starts/stops only its own Tauri process. The temporary
WebView2 profile is left in the system temporary directory. The Python test
checks the committed evidence, not a newly generated output directory. Actual
resolution may differ with display scaling.

This smoke test does not verify the normal desktop UI workflow, a real world,
Rust IPC throughput, interactive edits, GPU memory, context-loss recovery or
performance on other machines. It does not establish vanilla worldgen parity.

## 2026-11-19 — M9.23 large-map stress and streaming

Command:

```powershell
# build the debug Tauri binary once, then keep `pnpm dev` running
cargo build -p furybuilder-desktop
node scripts/webview2-stress.mjs docs/testing/evidence/2026-11-19-webview2-stress "8,12,16,20,24,28,32"
python -m unittest discover -s scripts -p "test_webview2_stress_evidence.py" -v
```

Environment: Intel Core i5-9600KF (6 cores), 16 GB RAM, NVIDIA GeForce GTX
1060 3 GB, WebView2 157 / ANGLE D3D11, 1280x720 canvas, Windows development
machine. The script starts its own Tauri process with a fresh WebView2 profile
and uses the real `ViewportRenderer`, `SnapshotBuilder`, mesher and
`updateChunks` path; it does not substitute a browser or mock WebGL.

Scenario: a synthetic 65A-65-chunk square (4,225 chunks, 9,022,512 non-air
blocks) with a 7-layer stone base, a grass surface and bounded oak-plank
towers. This is a rendering and streaming load, not a Minecraft worldgen
claim. Each radius uses `setSnapshot`; the overview camera looks straight down
at the whole tested square, which is the worst case for draw calls and
fill rate. The normal camera FPS is not used for the limit table because it
depends on the view frustum.

| Radius | Chunks |   Non-air | Setup ms | Overview FPS | Draw calls | Triangles | Observed heap MB |
| -----: | -----: | --------: | -------: | -----------: | ---------: | --------: | ---------------: |
|      8 |    289 |   616,752 |      456 |          172 |        289 |   363,104 |              260 |
|     12 |    625 | 1,334,192 |      904 |          172 |        500 |   609,600 |              346 |
|     16 |  1,089 | 2,325,040 |    1,579 |          144 |        858 | 1,040,992 |              489 |
|     20 |  1,681 | 3,589,296 |    2,454 |          165 |      1,312 | 1,589,760 |              533 |
|     24 |  2,401 | 5,126,960 |    3,498 |          106 |      1,862 | 2,254,240 |              688 |
|     28 |  3,249 | 6,938,032 |    5,035 |           90 |      2,394 | 2,891,104 |              862 |
|     32 |  4,225 | 9,022,512 |    6,404 |           71 |      3,120 | 3,767,040 |            1,046 |

All snapshots loaded every requested chunk, captured a 1600A-900 PNG with
`missingChunks = 0` and `partial = false`, and reported no WebGL error.
`setupMs` is synchronous `setSnapshot` time (mesh construction included, input
and IPC excluded). Heap is `performance.memory.usedJSHeapSize` sampled after
settling; it includes prior snapshots that were not yet collected, so it is an
upper-bound trend, not a per-scene budget.

Streaming through `updateChunks` from one centre chunk to the full 4,225
chunks, with deterministic Chebyshev rings:

| Batch chunks | Updates | Total ms | Max single batch ms | FPS after stream |
| -----------: | ------: | -------: | ------------------: | ---------------: |
|           16 |     272 |   19,129 |                 171 |               63 |
|           32 |     144 |   16,533 |                 249 |               62 |
|           64 |      80 |   14,946 |                 340 |               65 |

Every streaming run reached all 4,225 chunks with no failure. Larger batches
finish sooner but pause the main thread longer; 16-32 chunks per update keeps
the worst observed pause under 250 ms on this machine. The FPS column is a
short-window reading taken immediately after the stream, not a sustained
interactive-editing measurement.

### Practical limits from this run

- **Tested ceiling:** 4,225 chunks / 9.0 million non-air blocks at radius 32.
  It renders and captures correctly, but the overview drops to 71 FPS, the
  synchronous snapshot setup takes 6.4 s and observed heap exceeds 1 GB. Treat
  this as a stress bound, not a recommended working set.
- **Comfortable interactive envelope on this GPU class:** radius 16
  (1,089 chunks / 2.3 million non-air blocks), 1.6 s setup, 144 FPS overview.
- **Upper playable bound:** radius 24 (2,401 chunks / 5.1 million non-air
  blocks), 3.5 s setup, 106 FPS overview. Above this, setup pauses and memory
  grow faster than the visible benefit.
- **Streaming budget:** add 16-32 chunks per `updateChunks` call and yield
  between calls; 64-chunk batches caused a 340 ms pause. The current renderer
  still holds all requested chunks, so these are meshing/upload limits, not a
  full LRU streaming implementation.

### Limits of this evidence

This was one run on one GPU and one WebView2 build. It did not test
interactive edits during streaming, 30-60 minutes of camera movement, GPU
memory, context loss, other GPUs, or a real imported/Vanilla world. It does
not close UAT-09 or establish vanilla worldgen parity. Re-run after renderer
or meshing changes.

Evidence: [metrics.json](evidence/2026-11-19-webview2-stress/metrics.json) and
[viewport.png](evidence/2026-11-19-webview2-stress/viewport.png).
PNG SHA-256:
`91a2db32aabd4574c3b927cc328ce4e62503d4d3a658167bfd82436879756d27`.
The executable SHA-256 is recorded in the metrics.

## 2026-11-19 — M10.1 CPU, GPU and RAM profile

Command:

```powershell
# Vite server and debug Tauri binary already running/built
$env:FURY_PROFILE='1'
node scripts/webview2-stress.mjs docs/testing/evidence/2026-11-19-webview2-profile "8,16,24"
Remove-Item Env:FURY_PROFILE

cargo bench -p furybuilder-world-application --bench world_ops

cd apps/desktop
$env:FURY_PERF='1'
pnpm exec vitest run src/renderer/mesher.perf.test.ts --reporter=verbose --disableConsoleIntercept
Remove-Item Env:FURY_PERF
python -m unittest discover -s ../../scripts -p "test_webview2_profile_evidence.py" -v
```

The profiled WebView2 run uses the same real Tauri/WebView2/Three.js path as the
M9.23 stress test. Chrome `Profiler` sampled at 250 µs across three snapshot
sizes and three `updateChunks` streaming runs (60.4 s, 100,668 samples). The
profile includes synthetic waits, so absolute times carry profiling overhead;
the ranking and module shares are the useful result. Evidence:
[metrics.json](evidence/2026-11-19-webview2-profile/metrics.json),
[cpu-profile-summary.json](evidence/2026-11-19-webview2-profile/cpu-profile-summary.json)
and [viewport.png](evidence/2026-11-19-webview2-profile/viewport.png).

### CPU hot spots (self time, aggregated duplicates)

| Function                 | Self ms | Share | Module                 |
| ------------------------ | ------: | ----: | ---------------------- |
| `cachedBoxShapeFor`      | 5,059.5 |  8.4% | `shape.ts`             |
| `buildChunkMesh`         | 4,305.6 |  7.1% | `mesher.ts`            |
| `pushQuad`               | 3,579.7 |  5.9% | `mesher.ts`            |
| `readPackedIndex`        | 3,297.0 |  5.5% | `world.ts`             |
| `neighborName`           | 2,893.7 |  4.8% | `mesher.ts`            |
| `needsUpdate` (Three.js) | 1,852.0 |  3.1% | `three.module`         |
| `blockAt`                | 1,642.2 |  2.7% | `world.ts`             |
| `multiplyMatrices`       | 1,203.6 |  2.0% | `three.module`         |
| `rebuildChunk`           | 1,098.2 |  1.8% | `viewport-renderer.ts` |
| `finalize`               |   992.0 |  1.6% | `mesher.ts`            |
| `setup` (Three.js)       |   968.9 |  1.6% | `three.module`         |
| `isAirName`              |   804.7 |  1.3% | `assets.ts`            |
| `getBindingState`        |   745.5 |  1.2% | `three.module`         |
| `bindVertexArray`        |   698.2 |  1.2% | WebGL                  |
| `styleForBlock`          |   530.0 |  0.9% | `assets.ts`            |
| `Float32BufferAttribute` |   501.3 |  0.8% | `three.module`         |
| `bufferData`             |   415.3 |  0.7% | WebGL                  |
| `setProgram`             |   406.7 |  0.7% | `three.module`         |

Module shares from the top rows: `mesher.ts` 12.1 s (20.0%), Three.js 8.8 s
(14.6%), `shape.ts` 5.1 s (8.4%), `world.ts` 4.9 s (8.2%), `assets.ts` 1.3 s
(2.2%), `viewport-renderer.ts` 1.1 s (1.8%). Idle was 9.1 s (15.0%) and GC
1.48 s (2.4%).

The picture is consistent: per-block meshing and palette decoding dominate,
followed by Three.js buffer/VAO/program setup. The most actionable costs are:

1. **`cachedBoxShapeFor` builds and sorts a string key on every lookup.**
   Resolve shapes once per palette entry or use an interned state key instead of
   sorting properties inside the hot loop.
2. **`neighborName`/`blockAt` repeatedly build `${x},${z}` keys and decode
   coordinates.** Cache neighbour chunk references and local indices per
   section; keep the palette/style lookup out of the per-face path.
3. **`finalize` copies every array with `Float32Array.from`/`Uint32Array.from`.**
   Those copies add CPU and garbage on top of the buffers already built.
   Preallocate or hand over the typed arrays directly.
4. **`needsUpdate`, `setup`, `getBindingState`, `bindVertexArray`, `bufferData`
   and `setProgram` are the upload path.** Per-chunk geometry and one mesh per
   render category create many draw calls and buffer binds; M10.2 should bound
   meshing work per frame, and later work can reuse buffers or merge draws.
5. **`rebuildChunk` disposes and recreates geometry for every dirty chunk.**
   Reusing geometry buffers for small edits should reduce both CPU and GC.

### GPU and RAM

At radius 24 the profiled overview camera reported 99 FPS, 1,862 draw calls,
2,254,240 triangles, 2,052 geometries and one texture on the GTX 1060 3 GB.
`EXT_disjoint_timer_query_webgl2` is available, but this run did not use timer
queries; GPU attribution is therefore from frame time, draw calls and
triangles, not a vendor GPU profile. Draw calls scale with visible chunks, so
the per-chunk mesh strategy is the main GPU-side scaling limit.

After the three streaming runs the JS heap was 711 MB used, 760 MB total, with
a 4.4 GB limit; GC samples were 2.4% of the profile. This is not a leak proof:
each run replaces 2,401 chunks and their meshes, and old allocations may simply
not have been collected yet. It is enough to justify explicit cache budgets,
eviction and bounded meshing jobs in M10.2/M10.3.

### Rust core and mesher micro-benchmarks

`cargo bench` on the same machine: `fill_cuboid` of 320,000 blocks in 123.61 ms
(2,588,785 blocks/s), 100,000 reads in 9.44 ms (10,589,406 reads/s), undo in
0.28 ms and redo in 0.16 ms. The Rust core is not the bottleneck in this
renderer workload.

`FURY_PERF=1` mesher test: 50 centre-chunk builds, 201.76 ms total, 4.04 ms per
chunk, 875 faces. This is a single-chunk CPU measurement and excludes GPU
upload and draw calls.

### Caveats

One GPU, one WebView2 build, a synthetic scene and a short run. The profile
does not cover React UI work, real imported worlds, interactive brush edits,
long-running camera movement, GPU memory, context loss or other hardware. The
profiled run's absolute timings are inflated by sampling and are not used for
the M9.23 limit table.
