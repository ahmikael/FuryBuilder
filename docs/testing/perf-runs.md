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

| Result | Observed |
|---|---:|
| WebGL version / error | WebGL 2.0 / 0 |
| Loaded / missing chunks | 289 / 0 |
| Meshes / reported sections | 289 / 578 |
| Draw calls / triangles | 283 / 354,464 |
| Canvas / PNG | 1,600×900 / 911,350 bytes |
| Snapshot mesh setup | 465 ms |
| Renderer FPS / smoothed frame time | 138 / 16 ms |

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
