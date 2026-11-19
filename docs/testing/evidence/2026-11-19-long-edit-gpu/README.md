# M10.8 — long editing on Windows graphics paths

Recorded: 2026-11-19. Application: debug `target/debug/furybuilder-desktop.exe`,
SHA-256 `209d4f0c849ad952494cbd4a7947f880cb2c1ac61402bf848b504014eae7eea9`.

## Environment

- Windows 11 Pro 10.0.26300, Intel Core i5-9600KF (6 cores / 6 threads), 16 GB RAM.
- NVIDIA GeForce GTX 1060 3GB, driver 32.0.15.6094.
- Microsoft Edge WebView2 Runtime 157.0.4341.60.
- **Only one physical display adapter is present** and the i5-9600KF has no
  integrated graphics. The matrix therefore covers one hardware adapter path and
  two software rasterizers. It does not claim integrated/discrete or
  cross-vendor hardware coverage.

## Method

`scripts/long-edit-webview2.mjs` starts its own Tauri process with isolated
app-data and WebView2 profiles. It creates a flat project through the real
frontend store and applies every edit through the real `applyEditorOperation`
path: server preview → Rust apply → dirty-chunk refresh. It does not feed
synthetic snapshots directly to the renderer.

The schedule mixes `fill_cuboid`, `batch`, `terrain_brush` and `replace`
operations every 250 ms. After each operation the harness reads a sampled block
from the renderer and requires the applied revision. A fully emptied chunk is
accepted as air when the renderer has unloaded it. Every minute a gold/diamond
probe verifies target → previous → target through the real undo and redo
commands. At the end the last diamond marker is saved, the project is closed
without saving, reopened, and revision, block count, chunk count and the marker
are compared.

Frame metrics come from the renderer's real `onMetrics` callback and
`renderer.info`; heap samples come from Chromium `performance.memory` after
explicit DevTools garbage collection. GPU timer queries were not used. Browser
flags disabled background/occlusion throttling and enabled precise memory
information; backend-specific flags are recorded in each `metrics.json`.

## Results

| Path                   | Renderer                                          | Duration | Operations | Changed blocks | Median FPS (min–p95) | Frame p95 | Heap growth (max)   | Undo/redo | Context loss | Save/reopen |
| ---------------------- | ------------------------------------------------- | -------- | ---------: | -------------: | -------------------- | --------- | ------------------- | --------- | ------------ | ----------- |
| `hardware-d3d11`       | ANGLE (NVIDIA GeForce GTX 1060 3GB, Direct3D11)   | 609.0 s  |      1,165 |        120,174 | 116 (51–158)         | 20.3 ms   | +10.7 MB (107.4 MB) | 9/9       | 0            | pass        |
| `software-warp`        | ANGLE (Microsoft Basic Render Driver, Direct3D11) | 310.7 s  |        424 |         46,334 | 31 (19–37)           | 41.0 ms   | +10.6 MB (101.3 MB) | 4/4       | 0            | pass        |
| `software-swiftshader` | ANGLE (Google SwiftShader, Vulkan 1.3.0)          | 313.0 s  |        297 |         32,583 | 15 (11–21)           | 70.2 ms   | +8.1 MB (110.1 MB)  | 4/4       | 0            | pass        |

All three runs recorded:

- 0 failed operations, 0 renderer verification failures, 0 revision mismatches;
- 0 WebGL context losses and no renderer replacement;
- 0 uncaught exceptions and 0 console errors;
- equal revision, block count and chunk count after save/close/reopen, with the
  final `minecraft:diamond_block` marker present;
- a 64-character snapshot checksum and non-zero snapshot size;
- viewport and application screenshots with recorded SHA-256 hashes.

`probes/` contains the preflight WebGL probes. `smoke/` is a 30-second
functional run and `smoke-probe/` is a 75-second run that exercises undo/redo
before the long sessions. They are preflight evidence, not part of the long-run
table.

## Interpretation

- The hardware D3D11 run is the representative performance result for this
  machine. WARP and SwiftShader are software rasterizers; their lower FPS is
  expected and is not a hardware performance claim.
- The runs prove long-session stability, edit/render consistency, undo/redo and
  save/reopen integrity on the tested paths. They do not prove behavior on a
  second physical GPU, an integrated GPU, another vendor or another driver.
- The 250 ms interval is deliberately faster than human editing. It stresses the
  command and refresh path but is not a replay of a real user's pacing.
- Hardware device loss was not injected. The harness monitors spontaneous
  `webglcontextlost` events and fails on any loss.
- No GPU timer queries were used, so frame timings are renderer/JS timings, not
  pure GPU time. This matches the M10.1 limitation.
- The session lengths are 10 minutes on hardware and 5 minutes on software, not
  an hours-long thermal or memory soak.

## Reproduce

```powershell
pnpm --filter @furybuilder/desktop dev
cargo build -p furybuilder-desktop

node scripts/long-edit-webview2.mjs <new-output-dir> hardware-d3d11 600000 250
node scripts/long-edit-webview2.mjs <new-output-dir> software-warp 300000 250
node scripts/long-edit-webview2.mjs <new-output-dir> software-swiftshader 300000 250

python -m unittest scripts/test_long_edit_evidence.py
```

The output directory must not exist; the harness creates it exclusively. Keep
the Vite server at `http://127.0.0.1:1420` running for the whole session. The
harness never attaches to an existing user process and removes its isolated
profiles afterwards.

## Files

- `runs/<path>/metrics.json` — full samples, operations, assertions and console
  state.
- `runs/<path>/viewport.png`, `runs/<path>/app.png` — final screenshots.
- `probes/<path>/metrics.json` — preflight renderer strings.
- `../scripts/long-edit-webview2.mjs` — the harness.
- `scripts/test_long_edit_evidence.py` — automated evidence checks.
