# Editor (M4)

The editor is a thin React layer over the same revisioned `Operation` API used by MCP and
future automation. The Rust world core remains the only source of truth; the renderer and
palette are derived views.

## Tools

| Tool | Key | Click behaviour |
|---|---|---|
| Select / inspect | 1 | Picks a block, updates the inspector and hover outline. |
| Place | 2 | Places the palette state on the picked face. |
| Remove | 3 | Replaces the picked block with `minecraft:air`. |
| Brush | 4 | Places a radius-1…4 sphere on the picked face (19 blocks at radius 1). |
| Box select | 5 | First click sets a corner, second click completes the box. |
| Fill | 6 | Fills the current box selection with the palette state. |
| Line | 7 | First click sets a start, second click places a connected 3D line. |
| Replace | 8 | Replaces one source state inside the box selection with the palette state. |

Alt+click picks a world block as the brush state. Right-click without dragging opens the
context menu (place, remove, pick, replace source, box corner, copy/paste, fill, delete,
focus). Right-drag stays reserved for the fly camera.

## Terrain brushes (M9.9)

The `Terrain` tab edits the topmost non-air block of every column in the current box
selection. It requires a selection; the footprint keeps the selected X/Z extent and adds a
bounded 16-block vertical air margin so raise and lower can write above or below the
surface without escaping the operation bounds.

| Brush | Behaviour |
|---|---|
| Raise | Adds `amount` layers of the palette material above each surface. |
| Lower | Removes up to `amount` blocks above each surface. |
| Smooth | Moves each surface toward the rounded 8-neighbour average, at most `amount`. |
| Flatten | Moves each surface to an explicit target Y or the median surface height. |
| Noise | Adds stable SplitMix64 noise in `-amount..=amount` from the entered seed. |
| Erode | Lowers peaks toward the rounded 4-neighbour average, at most `amount`. |
| Terrace | Snaps each surface to the nearest multiple of `step` (2..16). |

- Amount is 1..16, terrace steps are 2..16, material must not be air, and `targetY` is only
  valid for flatten and must be inside the selection.
- Heights are read before any write, so smoothing and erosion do not depend on iteration
  order.
- Empty columns are skipped and reported in operation warnings; a brush never builds a
  pillar from the selection floor.
- The panel previews the exact Rust operation and applies it through the same revisioned
  pipeline as other edits, so it is one undoable history entry.
- Brushes adjust block states only. Heightmaps, light, biomes and other derived data are
  not recalculated yet; that is a separate M9 task.

## Heightmap generator (M9.10)

The `Heightmap` tab builds a bounded local surface patch from explicit parameters instead
of editing the existing surface. It requires a box selection and writes one reversible
operation; it is not a whole-world generator and makes no vanilla worldgen parity claim.

| Parameter | Behaviour |
|---|---|
| Seed | Unsigned integer seed; the same seed and parameters reproduce the same patch. |
| Scale | Noise cell size in blocks, 1..64. |
| Octaves | Fractal octave count, 1..6; each octave contributes half the weight of the previous one. |
| Amplitude | Maximum height offset above `baseY`, 1..256. |
| Base Y | Starting surface height; empty uses the selection's minimum Y. |
| Surface depth | Number of top layers written with the surface material, 1..32. |
| Surface | Palette material placed on the top layer(s). |
| Filler | Material placed between `baseY` and the surface layers. |

- The footprint is the selected X/Z extent. The vertical bounds expand to include `baseY`
  and `baseY + amplitude`, so the whole patch stays inside the recorded operation extent.
- Heights are sampled with integer value noise/fBm anchored to world coordinates. There is
  no floating-point state, so the same inputs match on every platform.
- Blocks from `baseY` to the generated height are filled; blocks above the generated
  surface inside the bounds are cleared. Blocks below `baseY` are untouched.
- The panel previews the exact Rust operation and applies it through the same revisioned
  pipeline, so it is one undoable history entry.
- This tool changes block states only. World heightmaps, light, biomes and other derived
  data are not recalculated.

## Selection and geometry

- Selections are inclusive `{min,max}` boxes in world coordinates. Negative coordinates
  are normalized by `min`/`max`, never by truncation toward zero.
- The box tool is a two-click state machine. Switching tools cancels a dangling corner;
  Escape cancels the pending click first, then the job, then the selection.
- Volumes above 1,000,000 blocks are rejected before an operation is built.
- The line uses rounded sampling on the dominant axis, so horizontal, vertical and
  negative diagonals stay connected.
- The locally computed ghost is a coloured translucent box (green place, red remove,
  amber brush/fill, cyan box/line, purple replace). It is a geometric preview, not a
  claim about the final diff; the server preview validates the real operation before it
  is committed.

## Commit pipeline

```text
click → planBlockTool (local operation) → preview_operation (server validation)
      → apply_operation (expectedRevision) → revision + dirty_chunks
      → render_chunks (only dirty chunks) → history_state refresh
```

- `expectedRevision` is the revision shown in the UI. A `revision_mismatch` response
  reloads the bounded snapshot and warns instead of silently overwriting newer work.
- Only `dirty_chunks` are re-fetched and remeshed after an edit. Neighbouring chunks heal
  through the renderer's border rebuild.
- Undo/redo use the same pipeline and return their own dirty chunks.

## Replace and conflicts

- The replace source comes from the palette ("Use as replace source"), Alt-click, the
  context menu or the current pick.
- Source and target must differ; this is rejected client-side with a clear toast and is
  revalidated by the Rust operation.
- Fill/replace require a selection. Copy/paste requires a selection and a target; paste
  uses the selection minimum or the hovered face.
- Clipboard blocks are stored relative to the copied minimum corner, so a structure can
  be pasted at negative coordinates or in another dimension. Air is implied, so a copy
  contains only non-air blocks.

## Undo, redo and history

- Toolbar and viewport buttons, Ctrl/Cmd+Z, Ctrl/Cmd+Shift+Z and Ctrl/Cmd+Y drive the
  Rust history. Buttons disable when the core reports no available action.
- The History tab lists operation id, actor source, changed blocks, revision range and
  time. History is bounded in the core (default 100 entries).

## Jobs and cancellation

- Preview validation and region reads are cancellable: the job is marked, its result is
  discarded, and no write is dispatched.
- A write that has already been dispatched is atomic and cannot be reported as cancelled
  mid-flight. The Cancel button is therefore hidden during apply. This is deliberate:
  pretending a partially applied write was cancelled would corrupt user trust.
- The job pill and bottom status bar show the active job and its cancel affordance.

## Save status

The toolbar and viewport report `No unsaved edits`, `Unsaved changes` or `Preview scene`.
Persistence is not part of M4; edits live in the running Rust session, and the Save button
is explicitly disabled until M5. The status never claims a project was written to disk.

## Keyboard protection

`editorShortcut` returns null when the event target is an input, textarea, select or
contenteditable element, so typing coordinates, block properties or search text never
moves the camera, changes tools or edits the world. Escape/Enter are also left to open
dialogs. Camera keys use the same guard.

## Testing

| Area | Evidence |
|---|---|
| Box selection, negative bounds, volume limits | `editor/session.test.ts`, `editor/commands.test.ts` |
| Faces, lines, brush, fill, replace | `editor/session.test.ts` |
| Conflicts (no selection/source, source = target, revision mismatch) | `editor/commands.test.ts` |
| Cancellation and stale job completion | `editor/commands.test.ts`, `state/tool-store.test.ts` |
| Hotkeys and text-field protection | `editor/use-editor-hotkeys.test.tsx`, `editor/session.test.ts` |
| Click → preview → apply → dirty chunks | `features/viewport/Viewport.test.tsx` |
| Palette search, canonical defaults, property editing | `features/workspace/BottomPanel.test.tsx` |

## Limitations (M4)

- Two-click box selection, not drag-rubber-band selection.
- Brush overwrites every state inside its sphere; there is no "only replace air" mode yet.
- Clipboard is session-scoped and does not survive a restart until M5/M6.
- Approximate block shapes (ADR-009) and placeholder textures still apply.
- Project persistence, autosave and crash recovery are M5.
