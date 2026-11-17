# ADR-006: Renderer baseline

- **Status:** Accepted
- **Date:** 2026-11-17

## Decision

Use **Three.js `WebGLRenderer` (WebGL 2)** as the baseline in an imperative scene module,
not React Three Fiber as the core voxel renderer. WebGPU remains an evaluated future path
behind a feature check and fallback.

- One geometry per section (merged visible faces), not one mesh per block.
- Separate opaque/cutout/translucent material groups.
- Local origin rebasing around the camera; global coordinates stay 64-bit integers in Rust.
- Explicit GPU resource lifecycle (`dispose()` on geometry/material/texture), context-loss
  handling and a recoverable error state.
- Adaptive DPR and quality profiles (Low/Medium/High).

## Consequences

- The renderer is a derived view: it never owns block data.
- Profiling counters (FPS, draw calls, meshes, loaded chunks, GPU memory estimates) are
  first-class UI/debug data.
