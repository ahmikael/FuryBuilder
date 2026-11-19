# Reference overlays: 2D images and schematic ghosts

Status: research and design only (M9.22). No reference-overlay UI or command is enabled in
this build. The separate backlog item "insert drawings/images into 3D as construction
underlays" remains future work.

## Question

Can FuryBuilder let a builder place a 2D drawing (floor plan, elevation, pixel map) or an
existing schematic in the viewport as a **non-destructive reference**, without turning it
into world data, changing the export, or weakening the existing permission and revision
model?

## Recommendation

Yes, in two separately shippable slices:

1. **2D image reference (PNG first).** Import a user-selected PNG through the native dialog,
   copy and normalize it into the project, and render it as one textured plane aligned to a
   world plane. Store only reference metadata; never write blocks.
2. **Schematic ghost reference (Java `.nbt` first).** Reuse the existing verified structure
   reader, render a bounded translucent ghost, and keep the write path separate
   (`insert_template` remains the only way to turn a template into blocks).

Do **not** start with a general image pipeline (SVG/WebP/GIF/JPEG/remote URLs), a full
texture editor, or automatic tracing of drawings into blocks. Those add attack surface and
product scope without proving the core workflow.

## What already exists (verified in this repository)

| Capability                                                        | Evidence                                                                                            | Consequence for references                                                                                            |
| ----------------------------------------------------------------- | --------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------- |
| Three.js renderer with per-chunk meshes and GPU resource disposal | `apps/desktop/src/renderer/viewport-renderer.ts`                                                    | Add reference planes as a separate scene group; dispose textures/geometry on project close.                           |
| Canvas/atlas textures and `SRGBColorSpace` handling               | `viewport-renderer.ts` (`CanvasTexture`, `configureAtlas`)                                          | A reference can use `CanvasTexture`/`DataTexture` and the same colour conventions.                                    |
| Native file/folder picker only                                    | `apps/desktop/src-tauri/capabilities/default.json` (`dialog:allow-open`; no `fs`, `shell`, network) | The import command must receive a chosen path and copy the bytes in Rust; the WebView must not read arbitrary files.  |
| Project folder layout with `templates/`, `source/`, `cache/`      | `crates/project-store/src/store.rs` (`TEMPLATES_DIR`, `SOURCE_DIR`, `CACHE_DIR`)                    | A new `references/` directory fits the existing pattern.                                                              |
| Atomic JSON writes and versioned metadata                         | `crates/project-store/src/store.rs` (`write_json`, `write_atomic`)                                  | Reference metadata can be published atomically and recovered independently.                                           |
| Verified Java structure import                                    | `crates/world-io/src/java_structure.rs`, `crates/structure-library/src/lib.rs`                      | `.nbt` files are already parsed fail-closed with size, checksum and DataVersion checks.                               |
| Structure payload limit                                           | `structure-library` `MAX_TEMPLATE_BYTES = 32 MiB`                                                   | Use the same bound for schematic references initially.                                                                |
| PNG encoder/decoder crate already in the dependency graph         | `Cargo.lock`: `png 0.17.16`; used by `asset-source` and `observation`                               | No new image codec is required for PNG. `png::Decoder::new_with_limits` and `Limits { bytes }` exist in this version. |
| Project manifest tolerates unknown fields                         | `crates/project-store/src/manifest.rs` has no `deny_unknown_fields` on `ProjectManifest`            | Adding fields is format-compatible, but a separate index is still preferable for references.                          |

## User scenarios

1. **Floor plan.** Import a top-down plan, anchor it at the selection minimum, set
   `1 px = 1 block`, rotate in quarter turns, lower opacity and build walls over it.
2. **Elevation.** Place the same image on a vertical plane (`xy` or `zy`) to check window
   heights and roof lines.
3. **Schematic trace.** Load a verified `.nbt` as a translucent ghost, align its origin,
   inspect its footprint and build a variant by hand; nothing is applied automatically.
4. **Revision-safe review.** Hide or delete a reference at any time. Toggling references
   never changes `world_revision`, history, export bytes or MCP world data.

## Why not a block-writing import?

The existing template path already answers "place this structure". A reference is an
editor-only guide:

- it must not enter undo/redo or the operation history;
- it must not be exported to `.nbt`, `.schem` or a Java world;
- it must not be visible to MCP as world content;
- deleting it must not require a world revision;
- a stale or wrong reference must be harmless.

Keeping references outside the world model also keeps the Core simple: no new operation
type, no new undo baseline, no new revision semantics.

## 2D image design

### Supported formats

Phase 1 accepts **PNG only**:

- Lossless, alpha-capable and appropriate for pixel-art plans.
- The `png` crate is already audited and version-locked in this workspace.
- The decoder exposes a byte budget before allocation (`new_with_limits`), which is the
  property we need for untrusted input.

Rejected initially, with an explicit message:

| Format     | Reason                                                                                                                                 |
| ---------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| JPEG       | No decoder in the current graph; lossy artifacts are poor for block-precise plans. Add later only with a bounded decoder and fixtures. |
| WebP       | No Rust decoder; WebView decoding would bypass the Rust byte budget.                                                                   |
| GIF/APNG   | Animation adds frames, timing and memory risk for no reference value.                                                                  |
| SVG        | Executable document surface, external references, font/CSS dependencies.                                                               |
| HTML/PDF   | Not an image format; would require a renderer and a new sandbox.                                                                       |
| Remote URL | Network access, privacy and non-reproducible projects. Import must be local.                                                           |

### Import pipeline (proposed)

```text
dialog path
  → Rust: metadata check (regular file, size ≤ 16 MiB, no symlink escape)
  → read bytes
  → PNG signature + IHDR check
  → reject APNG (frame_control present)
  → reject width/height over 4096 or pixels over 8,388,608
  → decode with png::Limits { bytes: 64 MiB }
  → normalize to RGBA8, no metadata
  → re-encode a canonical PNG (strips ancillary chunks)
  → SHA-256 of the original bytes and of the normalized bytes
  → atomic write references/<uuid>/texture.png
  → atomic update references/index.json
```

Why re-encode: it strips text/EXIF/ICC/time chunks we do not need, gives a deterministic
stored artifact, and proves the decoder accepted every pixel. The original SHA-256 remains
for provenance. The normalized file is what the renderer loads.

A 16 MiB compressed PNG can still decode to a much larger buffer; the pixel cap and
decoder byte limit are both required. Never trust the IHDR dimensions before checking them
against the pixel budget.

### Storage model (proposed)

Use a separate index, not `project.json`:

```text
<project>/
  references/
    index.json
    <uuid>/
      texture.png       # normalized RGBA8, metadata stripped
```

```json
{
  "schemaVersion": 1,
  "references": [
    {
      "id": "uuid",
      "kind": "image",
      "name": "harbour-plan",
      "file": "references/<uuid>/texture.png",
      "sourceSha256": "…",
      "textureSha256": "…",
      "bytes": 123456,
      "pixelWidth": 512,
      "pixelHeight": 384,
      "importedAtMs": 0,
      "transform": {
        "dimension": "minecraft:overworld",
        "plane": "xz",
        "anchor": [0, 64, 0],
        "rotation": 0,
        "widthBlocks": 32,
        "heightBlocks": 24,
        "opacity": 40,
        "visible": true,
        "alwaysOnTop": false
      }
    }
  ]
}
```

- `references/index.json` has its own schema version, size limit and validation. A broken
  index must produce a warning and disable references, not block project open or world
  load.
- Absolute source paths are never stored. The original file is not referenced after
  import.
- Deleting a reference removes its directory and index entry atomically; the world is
  untouched.
- The index is bounded (for example 32 references, 128 MiB normalized bytes and 64 MiB
  decoded GPU budget). Exact limits belong in the implementation ADR.

### Transform semantics

An image is one plane with:

- **Plane:** `xz` (horizontal), `xy` (vertical, normal +z) or `zy` (vertical, normal +x).
- **Anchor:** the minimum corner in world coordinates, snapped to integers.
- **Rotation:** `0`, `90`, `180`, `270` degrees around the plane normal, in quarter turns.
- **Size:** `widthBlocks × heightBlocks`, independent of pixels; the UI defaults to the
  pixel dimensions and offers "lock aspect ratio".
- **Orientation:** texture top-left maps to the anchor; for `xz`, image +x maps to world
  +x and image +y maps to world +z. For vertical planes, image +y maps to world +y.
- **Offset:** a small epsilon (`0.01`) along the plane normal avoids z-fighting with block
  faces. `alwaysOnTop` toggles `depthTest: false` for a true tracing overlay; the default
  keeps depth testing so the reference is occluded by terrain.
- **Opacity:** 0–100%, default around 40%; the renderer uses a transparent
  `MeshBasicMaterial`.
- **Colour:** `SRGBColorSpace`; `NearestFilter` magnification to preserve pixel edges;
  mipmaps only for minification, measured against shimmering.

Snapping, numeric clamping and validation happen in Rust/the command layer, not only in
React. A transform that leaves the project's supported bounds is rejected with a structured
error.

### Rendering

- A dedicated `referenceGroup` added to the scene, separate from `chunkGroup`.
- One `PlaneGeometry` per reference plus one `CanvasTexture`/`DataTexture`; no per-pixel
  meshes.
- `renderOrder` above terrain, `depthWrite: false`, `transparent: true`,
  `side: DoubleSide` (so a vertical plan can be viewed from both sides).
- Query `MAX_TEXTURE_SIZE` at startup and clamp the texture; never assume 8192.
- Dispose geometry, material and texture when the reference is deleted, hidden and
  re-shown, the project changes, or the window closes.
- A reference plane is not a chunk and never invalidates chunk meshes. Hiding it must not
  trigger a render-chunk request.

### UI

Add a `References` section to the existing workspace panel rather than a new top-level
area:

- Import button (native dialog), empty state with accepted format/size.
- List with name, dimensions, plane, size in blocks and visibility toggle.
- Inspector fields: plane, anchor (use current selection or camera target), rotation, width,
  height, lock aspect, opacity, always-on-top, rename, delete.
- Error states: unsupported format, too many pixels, decode failure, missing file, broken
  index. Errors never modify the world.
- A clear "reference only · not part of the world or export" label.

## Schematic ghost design

### Data path

Reuse the existing verified reader instead of a second parser:

```text
.nbt bytes (≤ 32 MiB)
  → world_io::read_verified_import
  → Structure { size, palette, blocks }
  → reference-only summary (size, palette, non-air count, warnings)
  → ghost renderer (no operation, no history)
```

The reader already rejects entities, block entities and unknown tags for the supported
format. A schematic reference cannot become an "apply" path by accident.

### Rendering options

| Option                                        | Cost       | Use                                  |
| --------------------------------------------- | ---------- | ------------------------------------ |
| Bounding box + origin axes                    | O(1)       | Always shown; proves size and anchor |
| Instanced translucent cubes for stored blocks | O(non-air) | Exact shape; hard instance cap       |
| Occupancy point cloud / sampled shell         | O(sample)  | Large structures; approximate        |

Recommendation: start with bounding box + instanced cubes capped at 25,000 instances. Above
the cap, sample deterministically (for example every Nth stored block) and label the ghost
`sampled` with the exact ratio. Never silently show a partial shape as complete. A later
slice can add shell extraction or per-layer slices.

- Ghost material: low-opacity, `depthWrite: false`, `depthTest: true` by default.
- `alwaysOnTop` is optional but defaults off; otherwise a large ghost hides the terrain the
  user is editing.
- The ghost origin snaps to integers and supports the same quarter-turn rotations. Block
  state rotation is **not** applied to a reference; the ghost shows positions, not a
  promised future operation. If rotation is added later, it must use the same block-state
  rotation rules as template insertion and say so.
- No collision, no raycast interception: picking must ignore reference objects so clicks
  still target world blocks.
- A schematic ghost is not `insert_template`; the UI must offer "insert as template" as a
  separate, confirmed action.

### Limits

- Source: same 32 MiB payload limit as the verified structure importer.
- Stored non-air blocks: bounded by the parser and by `MAX_OPERATION_BLOCKS` (1,000,000).
- Ghost instances: 25,000 hard cap; deterministic sampling above it.
- Active schematic references per project: 8 (proposal).

## MCP position

The master plan lists `read_reference_image` as an extended MCP tool. It is **not**
implemented here. If added, it should:

- accept `project_id` and `reference_id`, never a filesystem path;
- return only a normalized image at a bounded resolution (for example ≤ 1024 px) plus
  metadata: dimensions, SHA-256, transform, bounds and index revision;
- mark references as editor-only data with no `world_revision`;
- never accept an upload, write a reference, or turn a reference into blocks;
- keep the existing rule that MCP cannot read arbitrary local files.

A text-only summary (`name`, size, plane, bounds, opacity) is enough for most agents and
avoids sending large images through the model context.

## Security and robustness budget (proposal)

| Item                                   | Limit                               |
| -------------------------------------- | ----------------------------------- |
| Source file bytes                      | 16 MiB                              |
| Decoded pixels per image               | 8,388,608 (for example 4096 × 2048) |
| Decoder allocation budget              | 64 MiB                              |
| Texture side                           | 4096, clamped to `MAX_TEXTURE_SIZE` |
| Active image references                | 32                                  |
| Normalized reference bytes per project | 128 MiB                             |
| Decoded GPU bytes per project          | 64 MiB                              |
| Schematic source bytes                 | 32 MiB                              |
| Schematic ghost instances              | 25,000                              |
| Active schematic references            | 8                                   |

Additional rules:

- Reject symlinks/reparse points and paths outside the dialog result before reading.
- Verify the PNG signature and IHDR before decoding; reject zero dimensions and APNG.
- Never execute image metadata, never render HTML/SVG, never call `innerHTML`.
- Treat names as display text, bound their length, and never use them as filesystem paths.
- Normalize to RGBA8 and strip metadata; record both hashes.
- Reject a reference index with duplicate IDs, paths outside `references/`, unsupported
  schema or over-budget entries; keep the project openable in a degraded state.
- No network access. No remote image URLs.

## Performance questions to measure

1. Cost of decoding a 8 MP PNG in Rust on the UI thread; should it run in a bounded worker
   or an async command?
2. Texture upload time for 4096² RGBA at project open and on first show.
3. Memory of 32 references at the cap versus the 64 MiB GPU budget.
4. Transparency sorting and overdraw for a full-screen ground plan at 40% opacity.
5. Ghost instance count versus frame time at 25,000 cubes, and the sampling threshold.
6. Whether `alwaysOnTop` planes force a full-scene redraw or interact badly with chunk
   culling.

These are implementation-time measurements, not claims made here.

## Test plan for the first implementation slice

- **Rust unit:** PNG signature/IHDR validation, APNG rejection, pixel/byte limits, decode
  failure, metadata stripping, SHA-256, transform validation, index schema, duplicate IDs,
  path traversal, symlink rejection, atomic delete/recovery.
- **Rust fixtures:** tiny valid PNG, grayscale/palette/alpha PNG, 16-bit PNG, interlaced
  PNG, truncated file, decompression bomb, oversized dimensions, APNG, `.nbt` fixture from
  the existing structure tests.
- **Renderer unit:** plane geometry/transform math for all three planes and four rotations,
  anchor snapping, aspect ratio, opacity, texture disposal on delete/project close, raycast
  ignores references.
- **UI:** import/cancel/error, list selection, rename, visibility, delete confirmation,
  missing-file state, broken-index warning, save/reopen round trip.
- **Integration:** a reference never changes `world_revision`, history, export bytes or MCP
  world data; opening a project with references on a machine without the source file still
  works from the copied asset.

## Legal and privacy

- The user supplies the image or schematic; FuryBuilder does not bundle copyrighted plans.
- The UI should state that the user is responsible for having the right to use the image.
- No original image path, personal metadata or thumbnail is written to logs.
- References are local project files; they are never uploaded.

## Open questions

1. Should the normalized texture be stored as PNG (smaller, deterministic) or raw RGBA
   (faster, larger)? Recommendation: PNG on disk, decode on project open.
2. Is `xz`/`xy`/`zy` enough, or do users need an arbitrary plane normal? Recommendation:
   three axis-aligned planes first; free planes add picking and snapping complexity.
3. Should a vertical plane anchor at the selection minimum or at the camera target?
   Recommendation: selection minimum when a box exists, otherwise the hovered face.
4. Do references need per-user visibility (editor state) or per-project visibility
   (reference metadata)? Recommendation: per-project transform, per-session visibility.
5. Is a separate `reference-store` crate warranted, or should `project-store` own the
   index? Recommendation: project-store owns paths/atomicity; a small reference module
   owns validation and image normalization.
6. When `.schem`/`.litematic` support arrives, should the reference importer accept it
   before template insertion does? Recommendation: no; one verified format at a time.

## Proposed slice order

1. **Reference index + PNG import + horizontal `xz` plane** with opacity, rotation, delete
   and tests. This proves the workflow.
2. **Vertical planes + anchor-from-selection + always-on-top** and renderer tests.
3. **Schematic ghost** for verified `.nbt` with bounding box and capped instances.
4. **MCP read-only summary** (`inspect_reference`) only after the UI and storage are stable.
5. **Optional `read_reference_image`** after measured payload and context budgets exist.

None of these slices are implemented by this research task.
