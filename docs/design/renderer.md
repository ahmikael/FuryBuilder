# Viewport renderer (M3)

The viewport is an imperative Three.js scene owned by `ViewportRenderer`. React owns only
the surrounding chrome (overlay, dialogs, inspector); it never re-renders the scene tree.

## Module map

| File | Responsibility |
|---|---|
| `renderer/viewport-renderer.ts` | Scene, camera modes, materials, chunk meshes, metrics, capture |
| `renderer/world.ts` | CPU-side decoded snapshot mirror and block reads |
| `renderer/mesher.ts` | Merged chunk geometry with hidden-face culling |
| `renderer/shape.ts` | Approximate box shapes for slabs, stairs, snow and torches |
| `renderer/assets.ts` | Original placeholder atlas and state → tile resolution |
| `renderer/camera-controller.ts` | Fly camera (WASD, right-drag look, wheel speed) |
| `renderer/raycast.ts` | Voxel DDA picking against authoritative block data |
| `renderer/observation.ts` | Capture metadata contract |
| `renderer/demo.ts` | Offline preview scene (browser mode only) |

## Data flow

```text
Rust WorldService ──render_snapshot──▶ RenderSnapshot (palette + packed indices)
                                             │
                                      RenderWorld (decode, CPU mirror)
                                             │
                                 buildChunkMesh (per chunk, 3 buffers)
                                             │
                                  Three.js BufferGeometry + atlas
```

The renderer never mutates world data. Snapshots carry the authoritative revision.

## Meshing

- One merged geometry per render category (opaque / cutout / translucent) per chunk.
  There is never one Mesh per block.
- Hidden faces between opaque blocks are culled, including across loaded chunk borders.
  Border faces stay visible toward unloaded chunks, so the edge of the loaded area is
  honest instead of showing fake solid walls.
- Indexed quads (two triangles per face) are mandatory; four vertices without indices
  produce stretched spikes.
- Leaves, torches and other cutout blocks use an alpha-tested material. Water and glass
  use a translucent material with `depthWrite: false`.
- `renderer/shape.ts` emits approximate boxes for slabs, stairs, snow layers and torches.
  Sharing/merging those boxes is intentionally conservative: a partial shape is not
  culled against a neighbour unless the full face is covered. Unknown blocks render as
  full cubes with the placeholder tile, preserving their exact data.
- Shapes are approximate (ADR-009), not a full vanilla model engine.

### Greedy meshing decision (M3)

Measured with `FURY_PERF=1 pnpm exec vitest run src/renderer/mesher.perf.test.ts`:

```text
iterations=50 totalMs=228.33 perChunkMs=4.57 faces=875
```

The demo chunk (terrain + house + stairs + leaves) produces 875 faces and meshes in
~4.6 ms on the development machine. Only dirty chunks rebuild, so a normal edit costs one
chunk rebuild. Greedy merging would reduce faces on large flat surfaces, but per-block
atlas UVs would need a texture-repeat shader or per-quad UV expansion, plus handling of
per-state shapes. It is **not adopted now**; revisit only if profiling shows meshing or
draw calls as the bottleneck at larger view distances.

## Materials and assets

- `createPlaceholderAtlasCanvas()` paints an original 8×8 atlas. No Mojang textures are
  bundled or downloaded.
- `CanvasTexture.flipY = false` because tile UVs use a top-left origin. The WGSL/WebGL
  convention otherwise mirrors rows into unpainted tiles.
- Block state identity and asset appearance are separate: a missing texture changes the
  visual fallback only, never the block state.
- Resource packs and real Minecraft assets are a later milestone (ADR-004, ADR-009).

## Cameras and picking

- **Fly:** WASD + Space/Ctrl, Shift boost, right-drag look, wheel changes speed.
- **Orbit:** enabled on demand; damping on, polar angle bounded.
- **Go to coordinates** moves the fly camera near the target and points orbit at it.
- Picking uses a voxel DDA over CPU block data, not mesh intersection. It returns the
  exact block position, entered face and state, including negative coordinates. A ray that
  enters unloaded space returns `null` instead of inventing air.

## Chunk lifecycle

- `setSnapshot` replaces the loaded set (used for initial load and view-distance changes).
- `updateChunks` upserts dirty chunks and rebuilds their 8 neighbours so seams heal.
- `removeChunks` unloads chunks and rebuilds surviving neighbours.
- `refreshViewportChunks(chunks)` is the typed bridge that M4 edits will call after a
  committed operation. The Rust side returns a bounded `ChunkSnapshotBatch`.
- Every geometry is disposed when a chunk mesh is replaced, removed or the viewport
  unmounts. Materials and the atlas are disposed once.
- A lost WebGL context stops the animation loop and surfaces a recoverable error state.

## Metrics

The performance panel reports FPS, frame time, draw calls, triangles, meshes, geometries,
textures, loaded chunks and rendered sections. Values come from `WebGLRenderer.info` and
the CPU world mirror; they are diagnostic, not a benchmark promise.

## Capture

`capture()` renders one frame and returns a PNG data URL plus Observation metadata:
dimension, world revision, camera pose, resolution, renderer schema, loaded/missing chunks,
`partial`, legend and timestamp. See `docs/api/observation.md`.
