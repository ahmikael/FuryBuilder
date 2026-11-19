# Editor (M4)

The editor is a thin React layer over the same revisioned `Operation` API used by MCP and
future automation. The Rust world core remains the only source of truth; the renderer and
palette are derived views.

## Tools

| Tool             | Key | Click behaviour                                                            |
| ---------------- | --- | -------------------------------------------------------------------------- |
| Select / inspect | 1   | Picks a block, updates the inspector and hover outline.                    |
| Place            | 2   | Places the palette state on the picked face.                               |
| Remove           | 3   | Replaces the picked block with `minecraft:air`.                            |
| Brush            | 4   | Places a radius-1…4 sphere on the picked face (19 blocks at radius 1).     |
| Box select       | 5   | First click sets a corner, second click completes the box.                 |
| Fill             | 6   | Fills the current box selection with the palette state.                    |
| Line             | 7   | First click sets a start, second click places a connected 3D line.         |
| Replace          | 8   | Replaces one source state inside the box selection with the palette state. |

Alt+click picks a world block as the brush state. Right-click without dragging opens the
context menu (place, remove, pick, replace source, box corner, copy/paste, fill, delete,
focus). Right-drag stays reserved for the fly camera.

## Terrain brushes (M9.9)

The `Terrain` tab edits the topmost non-air block of every column in the current box
selection. It requires a selection; the footprint keeps the selected X/Z extent and adds a
bounded 16-block vertical air margin so raise and lower can write above or below the
surface without escaping the operation bounds.

| Brush   | Behaviour                                                                    |
| ------- | ---------------------------------------------------------------------------- |
| Raise   | Adds `amount` layers of the palette material above each surface.             |
| Lower   | Removes up to `amount` blocks above each surface.                            |
| Smooth  | Moves each surface toward the rounded 8-neighbour average, at most `amount`. |
| Flatten | Moves each surface to an explicit target Y or the median surface height.     |
| Noise   | Adds stable SplitMix64 noise in `-amount..=amount` from the entered seed.    |
| Erode   | Lowers peaks toward the rounded 4-neighbour average, at most `amount`.       |
| Terrace | Snaps each surface to the nearest multiple of `step` (2..16).                |

- Amount is 1..16, terrace steps are 2..16, material must not be air, and `targetY` is only
  valid for flatten and must be inside the selection.
- Heights are read before any write, so smoothing and erosion do not depend on iteration
  order.
- Empty columns are skipped and reported in operation warnings; a brush never builds a
  pillar from the selection floor.
- The panel previews the exact Rust operation and applies it through the same revisioned
  pipeline as other edits, so it is one undoable history entry.
- Brushes adjust block states only. They do not recalculate heightmaps, light or biomes;
  biome cells are painted explicitly in the `Biomes` tab.

## Heightmap generator (M9.10)

The `Heightmap` tab builds a bounded local surface patch from explicit parameters instead
of editing the existing surface. It requires a box selection and writes one reversible
operation; it is not a whole-world generator and makes no vanilla worldgen parity claim.

| Parameter     | Behaviour                                                                                |
| ------------- | ---------------------------------------------------------------------------------------- |
| Seed          | Unsigned integer seed; the same seed and parameters reproduce the same patch.            |
| Scale         | Noise cell size in blocks, 1..64.                                                        |
| Octaves       | Fractal octave count, 1..6; each octave contributes half the weight of the previous one. |
| Amplitude     | Maximum height offset above `baseY`, 1..256.                                             |
| Base Y        | Starting surface height; empty uses the selection's minimum Y.                           |
| Surface depth | Number of top layers written with the surface material, 1..32.                           |
| Surface       | Palette material placed on the top layer(s).                                             |
| Filler        | Material placed between `baseY` and the surface layers.                                  |

- The footprint is the selected X/Z extent. The vertical bounds expand to include `baseY`
  and `baseY + amplitude`, so the whole patch stays inside the recorded operation extent.
- Heights are sampled with integer value noise/fBm anchored to world coordinates. There is
  no floating-point state, so the same inputs match on every platform.
- Blocks from `baseY` to the generated height are filled; blocks above the generated
  surface inside the bounds are cleared. Blocks below `baseY` are untouched.
- The panel previews the exact Rust operation and applies it through the same revisioned
  pipeline, so it is one undoable history entry.
- This tool changes block states only. World heightmaps, light, biomes and other derived
  data are not recalculated; biome cells are painted explicitly in the `Biomes` tab.

## Biome painter (M9.11)

The `Biomes` tab paints Java 1.21.4 biome data for the box selection. Biomes in the
editable world are a grid of 4x4x4 block cells per section, stored as a palette plus 64
YZX cells. A cell touched anywhere by the selection is painted, so the operation reports
the covered block volume while changing whole cells.

| Input     | Behaviour                                                                                                              |
| --------- | ---------------------------------------------------------------------------------------------------------------------- |
| Selection | Required box; every 4x4x4 cell it touches is painted.                                                                  |
| Biome id  | Namespaced lowercase id such as `minecraft:forest`; the panel reads the current biome at the selection minimum corner. |

- The operation is `set_biome`; the Rust core previews it, applies it through the
  revisioned pipeline and records it as one reversible history entry. Undo restores the
  previous palette and cells because section diffs include biome data.
- The core refuses `InvalidBiome` when any touched section has no source biome data. It
  never invents a default biome. A section palette is limited to 64 entries.
- Block states are untouched by biome painting. Biome cells are written into
  `sections[].biomes` by the overlay, verified region writer and complete world export;
  unknown section tags and block entities/entities are preserved.
- This tool edits existing biome data; it does not run vanilla biome generation or claim
  biome parity with a seed.

## Local generators and structure sites (M9.12)

The `Generators` tab adds deterministic local construction tools for roads, river
channels, vegetation and a read-only placement-rule query. Every edit requires a box
selection and is previewed and applied as one reversible operation. These are not
whole-world generators and make no vanilla worldgen or vanilla structure-placement claim.

| Tool       | Behaviour                                                                                                                                                                                          |
| ---------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Road       | Runs between the selection edges on the chosen X/Z axis, follows the topmost non-air surface, writes the road surface and clears up to 8 blocks of headroom. Width 1..16, meander 0..64, seed u64. |
| River      | Same route rasterization; carves 1..16 blocks below the surface, places a bed below the water and fills the channel.                                                                               |
| Vegetation | Scatters ground cover (0..100%) and small trees (0..100% chance, spacing 1..32) only into existing air; trees need a free trunk and canopy volume.                                                 |
| Sites      | Bounded query for flat, dry, clear, non-overlapping structure footprints. Width/depth 1..32, step 1..64, max slope 0..32, clearance 0..32, limit 1..64.                                            |

- Route endpoints are derived from the selection edges; the operation box adds the same
  bounded 16-block vertical margin as terrain brushes so headroom and canopies stay inside
  the recorded extent.
- Route columns outside the selection, without a surface, or resting on a fluid are
  skipped with warnings instead of bridging or building a pillar. Repeating a road does
  not stack a second layer; repeating a river does not deepen it.
- The placement query inspects at most 2,000,000 block positions and returns at most 64
  sites. It never mutates the world, bumps a revision or creates an operation.
- The selected materials come from the curated 1.21.4 registry. Block states only are
  changed; heightmaps, light and biomes are not recalculated.

## Layers, masks, protection and templates (M9.13)

The `Layers` tab manages editor-only policy stored in `project.json`; it is not Minecraft
world data and never changes block states by itself. The `Templates` tab stores reusable
Java `.nbt` payloads inside the same project.

| Concept          | Behaviour                                                                                                                                                       |
| ---------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Layer            | Named edit layer with visibility and lock state. Edits record the layer id; the active layer is used when a request omits it.                                   |
| Active mask      | At most one mask is active. Preview and apply must fit entirely inside it, and only operations in the mask's own dimension are constrained.                     |
| Protected region | Any intersection with a protected region is refused for preview and apply, regardless of the active layer or mask.                                              |
| Template library | Project-local `templates/index.json` plus `templates/objects/<uuid>.nbt`. Save a selection, import a verified `.nbt`, preview at the selection minimum, insert. |

- The Core re-validates the whole policy: 64 layers, 64 masks, 64 protected regions,
  64-character names, unique slug ids and a 16 MiB volume limit per region. The last layer
  and the active layer cannot be deleted.
- A locked layer blocks writes, including an implicit active layer. Removing a mask clears
  the active mask; the UI never sends a half-applied policy.
- Policy updates are metadata. They do not bump the world revision and are not recorded in
  block history. Undo/redo still restore the previous block snapshots, so a policy change
  cannot make earlier work unrecoverable.
- Template ids are UUIDs and never become file paths. Every read re-verifies the SHA-256
  recorded in the index; tampered or truncated payloads, traversal names and duplicate ids
  are refused. Limits: 128 templates, 32 MiB per file, 256 MiB total.
- Insertion is one reversible `Batch` operation on the chosen or active layer, so it uses
  the same preview, revision, audit, mask and protected-region path as any other write.
  Templates are construction data, not vanilla structure-placement parity.

MCP exposes `get_workspace_policy` and `list_templates` as read-only tools and
`insert_template` as a confirmed write. The template is re-read and re-verified by the
desktop before the confirmation dialog is shown.

## Semantic objects (M9.14)

The `Objects` tab adds a project-local catalog of districts, buildings, roads, entrances,
landmarks and other named areas. It is metadata layered over the voxel world: creating,
renaming or deleting an object never places or removes blocks.

| Concept    | Behaviour                                                                                                                                      |
| ---------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| Object     | Name, kind, dimension, inclusive bounds, optional parent, bounded tags and an optional note. Stored in `project.json`.                         |
| Creation   | Uses the current box selection by default; the note and tags are optional. Ids are unique lowercase slugs, not file paths.                     |
| Hierarchy  | A child must share its parent's dimension. Depth is capped at 8; deleting a parent with children is refused until they are removed/reparented. |
| Inspection | Shows volume, non-air blocks, current palette (top 8 states) and direct children. Palette statistics are read from the authoritative world.    |

- The Core validates the whole scene when opening, saving or replacing it: 1,024 objects,
  64-character names, 16 tags per object, 32-character tag keys, 128-character values and
  1,024-character notes. Cycles, duplicate ids, unknown parents and cross-dimension parents
  are refused.
- Scene changes are metadata: they do not bump the world revision and are not added to
  block history. Undo/redo therefore continue to apply only to voxel edits.
- `Use bounds as selection` reproduces an object's box in the viewport when its dimension
  is the one currently displayed; it does not switch dimensions silently.
- The `inspect_scene_object` path reuses the bounded region-summary query, so an object
  larger than 1,000,000 blocks is reported as `limit_exceeded` instead of scanning
  unbounded data.
- MCP exposes `list_scene_objects` and `inspect_scene_object` as read-only tools. There is
  no MCP scene-write tool in M9.14; the desktop UI is the authority for scene creation.

## Parametric scene graph (M9.15)

The `Parametric` tab adds versioned recipe objects whose voxels are compiled by the Core.
A house recipe stores its generator id/version, dimension, origin, rotation, materials and
size; applying it is one reversible `Batch` operation. The tab can create a house from the
current selection, transform the recipe in place, bake/detach it or roll it back.

| Action        | Behaviour                                                                                                                                                      |
| ------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Create        | Compiles the recipe, captures the pre-existing blocks at every generated position and applies the overlay as one operation.                                    |
| Transform     | Restores the old footprint to its captured baseline and writes the new footprint in one operation. Rotation also turns `facing`, `axis` and `rotation` values. |
| Bake/detach   | Forgets the recipe and baseline but keeps the blocks in the world. The voxels become ordinary history content.                                                 |
| Rollback      | Writes the captured baseline back and removes the recipe. Traffic back to the pre-object terrain.                                                              |
| Conflict view | Compares the generator output with the authoritative blocks and lists bounded samples of manual edits.                                                         |

- The default conflict policy is fail-closed. Manual edits inside the overlay produce a
  `conflict` with bounded details, and the transform/rollback is refused. The user can
  explicitly choose `Override` in the panel to accept the loss.
- The baseline is palette-compressed and stored with the recipe in `project.json`. The UI
  never round-trips it: `project_save` and `project_autosave` take the Core's catalog.
- The panel requires an existing JSON-valid recipe and never executes arbitrary code. The
  only generator is `house` version 1; unknown generator versions are rejected.
- Bake and recipe-only changes do not bump the revision. Create, transform and rollback
  write blocks and therefore appear in history and undo/redo.
- MCP exposes `list_parametric_objects` and `inspect_parametric_object` as read-only tools.
  Creation, transform, bake and rollback stay in the desktop UI in M9.15.

## Build DSL (M9.16)

The `Build DSL` tab edits a versioned JSON recipe of houses and repeat groups. It is a
controlled construction format, not a scripting sandbox: the frontend only parses JSON,
the Core validates and compiles, and the result is an ordinary `Batch` operation sent
through the normal preview/apply and undo path.

| Concept    | Behaviour                                                                                                                          |
| ---------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| House node | Absolute origin, optional rotation, and bounded house parameters; version 1 is the only generator.                                 |
| Repeat     | Expands a child 1–16 times along X, Y or Z with 1–64 block spacing. Nesting is capped at depth 4.                                  |
| Budget     | At most 64 top-level nodes, 64 expanded houses and 16,384 blocks. Overlaps and malformed recipes are rejected before any write.    |
| Preview    | Compiling runs the authoritative preview and shows bounds, house/block counts, affected/changed blocks, dirty chunks and warnings. |
| Apply      | Applying sends the exact compiled operation through the shared history path; it is never written just because it was previewed.    |

- The editor rejects invalid JSON locally, but all semantic validation and budgets stay in
  Rust. There is no `eval`, dynamic import, filesystem or shell access.
- `preview_build_dsl` in MCP returns the same summary, preview and typed operation as the
  tab. An agent must still call the confirmed `apply_operation` tool to commit it.
- Recipes are transient in M9.16. They are not written to `project.json`, and export formats
  do not claim to preserve DSL metadata.

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

| Area                                                                | Evidence                                                       |
| ------------------------------------------------------------------- | -------------------------------------------------------------- |
| Box selection, negative bounds, volume limits                       | `editor/session.test.ts`, `editor/commands.test.ts`            |
| Faces, lines, brush, fill, replace                                  | `editor/session.test.ts`                                       |
| Conflicts (no selection/source, source = target, revision mismatch) | `editor/commands.test.ts`                                      |
| Cancellation and stale job completion                               | `editor/commands.test.ts`, `state/tool-store.test.ts`          |
| Hotkeys and text-field protection                                   | `editor/use-editor-hotkeys.test.tsx`, `editor/session.test.ts` |
| Click → preview → apply → dirty chunks                              | `features/viewport/Viewport.test.tsx`                          |
| Palette search, canonical defaults, property editing                | `features/workspace/BottomPanel.test.tsx`                      |

## Limitations (M4)

- Two-click box selection, not drag-rubber-band selection.
- Brush overwrites every state inside its sphere; there is no "only replace air" mode yet.
- Clipboard is session-scoped and does not survive a restart until M5/M6.
- Approximate block shapes (ADR-009) and placeholder textures still apply.
- Project persistence, autosave and crash recovery are M5.
