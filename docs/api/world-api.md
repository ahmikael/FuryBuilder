# World command/query contract (M2)

The Rust application facade (`world_application::WorldService`) is the only authority for
world mutations. UI and MCP are clients of the same commands and queries.

## Commands

```rust
OperationRequest {
  projectId: string,
  operation: Operation,
  expectedRevision?: number,
  idempotencyKey?: string,
  actor: { id: string, source: "ui" | "mcp" | "api" | "system" },
  linkedObjectId?: string,
  layerId?: string,
}

Operation =
  | { type: "set_block", dimension, pos: {x,y,z}, state: BlockState }
  | { type: "batch", dimension, changes: [{pos, state}] }
  | { type: "fill_cuboid", dimension, bounds: {min,max}, state }
  | { type: "replace", dimension, bounds, from, to }
  | {
      type: "terrain_brush", dimension, bounds, kind, material:
        BlockState, amount: 1..16, seed: u64, step: 2..16, targetY?
    }
  | {
      type: "generate_heightmap", dimension, bounds, seed: u64,
        scale: 1..64, octaves: 1..6, amplitude: 1..256,
        baseY: i32, surfaceDepth: 1..32,
        surface: BlockState, filler: BlockState
    }
  | { type: "set_biome", dimension, bounds, biome: BiomeId }
  | {
      type: "generate_road", dimension, bounds, from: Pos, to: Pos,
        width: 1..16, meander: 0..64, seed: u64,
        clearance: 0..8, surface: BlockState
    }
  | {
      type: "generate_river", dimension, bounds, from: Pos, to: Pos,
        width: 1..16, meander: 0..64, seed: u64,
        depth: 1..16, water: BlockState, bed: BlockState
    }
  | {
      type: "generate_vegetation", dimension, bounds, seed: u64,
        groundCover: 0..100, treeChance: 0..100, treeSpacing: 1..32,
        plant: BlockState, trunk: BlockState, leaves: BlockState
    }

OperationResult {
  operationId: string,
  worldRevision: number,
  changedBlocks: number,
  dirtyChunks: [{x,z}],
  warnings: string[],
  replayed: boolean,
}
```

Order of evaluation: validate request → idempotency lookup → expected revision → state
registry validation → bounds/limits → apply atomically → bump revision → history/audit.

- `expectedRevision` mismatch returns `revision_mismatch`; nothing is written.
- Replaying the same `idempotencyKey` with the identical payload returns the original
  result with `replayed: true` and does not bump the revision.
- Reusing a key with a different payload returns `conflict`.
- A no-op operation returns `changedBlocks: 0` and does not bump the revision or history.
- Operation limits: 1,000,000 affected blocks per operation.
- `layerId` records which editor layer produced the edit. Omitting it uses the active
  layer. A missing, locked or policy-blocked layer is rejected with `permission_denied`
  before any write; undo/redo restore snapshots and bypass this policy check.

## Undo / redo

History stores section-level before/after snapshots (not full world copies).

- `undo(projectId, actor, operationId?)` reverts the latest operation, or a specific
  operation only when it is the latest. Older entries return `conflict` because newer
  edits may depend on them.
- `redo(projectId, actor)` reapplies the last undone operation.
- A new applied operation clears the redo stack.
- History is bounded (default 100 entries); oldest entries are pruned first.

## Queries```ts

Query =
| { type: "world_info" }
| { type: "get_block", dimension, pos }
| { type: "get_biome", dimension, pos }
| { type: "region_summary", dimension, bounds }
| { type: "chunk_stats", dimension, chunk }
| {
type: "find_structure_sites", dimension, bounds,
width: 1..32, depth: 1..32, step: 1..64,
maxSlope: 0..32, clearance: 0..32, limit: 1..64
}
| { type: "workspace_policy" }
| { type: "scene_objects" }
| { type: "inspect_scene_object", objectId }
| { type: "parametric_objects" }
| { type: "inspect_parametric_object", objectId }

QueryResult =
| { type: "world_info", data: WorldInfo }
| { type: "block", data: { dimension, pos, state } }
| { type: "biome", data: { dimension, pos, biome: BiomeId | null } }
| { type: "region_summary", data: RegionSummary }
| { type: "chunk_stats", data: ChunkStats }
| { type: "structure_sites", data: StructureSites }
| { type: "workspace_policy", data: WorkspacePolicy }
| { type: "scene_objects", data: SemanticObject[] }
| { type: "scene_object", data: SceneObjectInspection }
| { type: "parametric_objects", data: ParametricObjectSummary[] }
| { type: "parametric_object", data: ParametricInspection }

````

`region_summary` is bounded to 1,000,000 blocks and returns a palette sorted by count.
`get_biome` reports the biome covering the block from the 4x4x4 editable cell grid, or
`null` when the source has no biome data. `find_structure_sites` is a read-only, bounded
heuristic search: it checks at most 2,000,000 block positions and returns at most 64
non-overlapping flat, dry, clear footprints. Queries never mutate the world or bump
revisions.

`generate_road` follows the topmost non-air block in each column, writes the surface at
the next Y and clears a bounded headroom; `generate_river` carves a channel and fills it
with water over a bed. Repeating the same parameters is idempotent. `generate_vegetation`
places ground cover and small trees only into existing air, with minimum tree spacing.
All three are deterministic local construction tools; they make no vanilla worldgen
parity claim and are recorded as ordinary reversible operations.

## Build DSL

Build DSL is a versioned, declarative recipe format compiled by the Core into the same
`Operation::Batch` used by manual editing and MCP. It is data only: there is no script,
expression evaluator, shell or file access.

```ts
BuildRecipe {
  schemaVersion: 1,
  dimension: string,
  nodes: (
    | { type: "house", at: [x, y, z], rotation?: "none" | "cw90" | "cw180" | "cw270",
        params: HouseParams }
    | { type: "repeat", axis: "x" | "y" | "z", count: 1..16, spacing: 1..64,
        child: BuildNode }
  )[]
}
```

- Budgets: 64 top-level nodes, 64 expanded houses, 16,384 generated blocks, repeat depth 4.
  Duplicate positions are rejected as `conflict`; invalid materials, unknown fields and
  unsupported schema versions are refused.
- `build_dsl_preview(recipe)` compiles, runs the normal preview and returns
  `{ summary, operation, preview }` without writing. The returned operation is then passed
  to `apply_operation`, so revision checks, confirmation, workspace policy, history and
  undo are identical to manual edits.
- MCP exposes `preview_build_dsl` as read-only and returns the same operation. Applying it
  still requires explicit write access and the normal confirmation dialog.
- Compilation is deterministic for a given schema and generator version. Repetition is
  expanded before writing, and rotation also turns `facing`, `axis` and `rotation` block
  properties instead of only moving coordinates.

## Semantic objects

Semantic objects are project metadata, separate from voxel data and from the workspace
edit policy. They are persisted in `project.json` and never change blocks by themselves.

```ts
SemanticObject {
  id, name, kind: "district" | "building" | "road" | "entrance" | "landmark" | "other",
  dimension, bounds, parentId?, tags?: Record<string, string>, note?
}
```

- Ids are lowercase slugs derived from the name and uniquified; names never become file
  paths. Limits: 1,024 objects, 64-character names, 16 tags per object, 8 hierarchy levels
  and 1,024-character notes. A child must share its parent's dimension.
- `scene_objects` returns the full catalog. `inspect_scene_object` returns the object,
  its direct children and bounded `region_summary` statistics (volume, non-air count,
  sorted palette) for its current bounds. The region summary limit of 1,000,000 blocks
  applies, so an oversized object is refused instead of scanning without a bound.
- Creating or deleting an object is metadata-only: no revision bump and no block-history
  entry. Deleting a parent that still has children returns `conflict`.
- Unknown objects and unknown dimensions return `not_found`; invalid kinds, names, tags
  or an unsupported scene schema return `invalid_argument` or `unsupported_version`.

## Parametric objects

A parametric object is a persisted recipe plus a captured rollback baseline. It is not a
second world format: applying a recipe compiles a deterministic block overlay and submits
it as one ordinary reversible `Batch` operation with `linked_object_id` set to the object
id.

```ts
ParametricObjectSummary {
  id, name, generator: "house", generatorVersion: 1, dimension,
  bounds, origin, rotation: "none" | "cw90" | "cw180" | "cw270",
  params: { width, depth, height, wall, floor, roof, window? },
  layerId?, appliedRevision, lastOperationId?, generatedBlocks
}
```

- Only version 1 of the `house` generator is supported. Unknown generators or versions are
  rejected with `unsupported_version`; materials must be valid non-air block states.
- Limits: 64 parametric objects, 32,768 generated blocks per object, 262,144 baseline
  positions in total, footprint 3..32 and wall height 2..32. Ids are unique slug ids
  assigned by the Core, never by a client.
- The baseline stores the blocks that existed before the first apply in palette-compressed
  form. Rollback restores those exact states. Transform and rebake refuse to overwrite
  manual edits unless the caller passes `conflictPolicy: "override"`; the conflict response
  carries bounded position/expected/found details.
- `parametric_objects` returns summaries without the baseline. `inspect_parametric_object`
  returns one summary, its generated block count, the conflict count and up to 32 sample
  conflicts.
- `bake` is metadata-only: the block overlay stays and the recipe is forgotten, so the
  blocks become ordinary editable voxels. `rollback` writes the captured baseline and then
  removes the recipe. Undo/redo still operate on the resulting block operations.
- Scene recipe changes do not bump the revision by themselves; the overlay write does.
  `project_save` and `project_autosave` take the authoritative parametric catalog from the
  Core, so a UI client cannot omit or forge a baseline.

## Observation maps

`observe` is a read-only application endpoint shared by the desktop UI and MCP. It renders
deterministic PNG maps from the authoritative world at the current revision; it never
writes blocks or metadata. Exact `get_block`/`get_region` queries remain the authority for
positions.

```ts
ObservationRequest {
  dimension: string,
  bounds: { min, max },          // inclusive
  mode: "top_down" | "heightmap" | "material_map" | "biome_map"
      | "cross_section" | "revision_diff",
  slice?: { axis: "x" | "y" | "z", coordinate: number },  // cross_section only
  fromRevision?: number                                    // revision_diff only
}
```

`Observation` returns `schemaVersion: 1`, `mode`, `dimension`, `worldRevision`, `bounds`,
`width`, `height`, `blocksPerPixel`, `image` (`data:image/png;base64,...`), `imageBytes`,
a bounded `legend`, `legendTruncated`, `partial`, `warnings`, `notes`, `scannedBlocks`,
`sampledUnits`, `nonAirUnits`, optional `fromRevision`/`slice`, and block-level `counts`
for revision diffs.

- Surface modes (`top_down`, `heightmap`, `material_map`, `biome_map`) scan the Y range of
  `bounds` for the top non-air block per X/Z column. Each pixel is one column; the image
  width/height are the X/Z extents.
- `heightmap` uses five deterministic elevation bands. `biome_map` reports missing biome
  data explicitly instead of inventing a biome.
- `cross_section` requires `slice`. X/Z planes render upright with row 0 at the highest Y.
- `revision_diff` requires `fromRevision` and compares it with the current revision. It
  reconstructs the prior state from retained section history and returns `no_history`
  when an earlier revision can no longer be rebuilt. The image is a top-down projection;
  `counts.added/removed/changed` are exact for the bounds.
- Limits are 512×512 pixels, 262,144 pixels total, and 8,000,000 scanned/compared blocks.
  Violations return `limit_exceeded` before rendering. Legends are capped at 16 entries
  and say when they were truncated.
- `partial` is always false for these maps: they are computed from the authoritative world
  model, where absent chunks read as air. They do not claim game lighting or entity data.

## Validation

`validate_region` is a read-only application endpoint shared by the desktop UI and MCP. It
runs bounded rules over the authoritative world at the current revision and never changes
blocks, metadata, history or the revision.

```ts
ValidationQuery {
  dimension: string,
  bounds: { min, max },                                  // inclusive scan area
  rules?: ValidationRuleId[]                             // empty = default local set
  allowedBounds?: { min, max },                          // out_of_bounds only
  path?: { from, to, baseY, width, material },           // path_continuity only
  maxSurfaceMaterials?: number,                          // 1..=64, default 32
  floatingFix?: "none" | "remove" | "support"            // default support
}
```

Rules: `floating_blocks`, `out_of_bounds`, `path_continuity`, `palette`,
`export_compatibility`, `block_state_orientation`, `protected_region`. In the default set,
`out_of_bounds` and `path_continuity` run only when their inputs are supplied; an explicitly
requested rule without its input fails with `invalid_argument` instead of silently passing.

`ValidationReport` returns `schemaVersion: 1`, `dimension`, `worldRevision`, `bounds`, the
executed `rules`, `findings`, severity `counts`, `truncated`, `scannedBlocks`, `warnings` and
`notes`. Each finding contains `ruleId`, `severity` (`info`/`warning`/`error`), `confidence`
(`certain`/`heuristic`), `message`, `bounds`, a bounded `positions` sample, the exact
`positionCount` and an optional `suggestedFix`.

- `suggestedFix.operation` is an ordinary reversible `Operation` (currently a `batch`). It is
  never applied by validation. Preview it with `preview_operation` and submit it through the
  confirmed `apply_operation`/UI transaction path; workspace masks, locked layers and
  protected regions still apply.
- `floating_blocks` is explicitly heuristic: Minecraft does not simulate structure stability.
  Blocks at the minimum scan Y are treated as supported and the report says so.
- `export_compatibility` checks names and properties against the verified 1.21.4 catalog.
  `block_state_orientation` checks cardinal `facing`, `half`, `hinge` and `axis` domains.
- `protected_region` is injected by the application from the persisted workspace policy;
  clients cannot supply or override protected regions through this request.
- Limits are 2,000,000 scanned blocks, 128 findings, 64 embedded positions per finding, 512
  path points and path width 1..=16. Violations return `limit_exceeded`/`invalid_argument`
  before any write is possible.

## Render snapshots

The renderer receives derived, revisioned payloads from the same core:

- `render_snapshot(dimension, center, radius)` returns a bounded square (radius ≤ 6) with
  `loaded: false` entries for missing chunks.
- `render_chunks(dimension, chunks)` returns at most 256 explicitly requested chunks and
  is used to refresh dirty chunks after an edit.
- Sections carry a palette and packed indices (`u64` words split into `[lo32, hi32, ...]`
  for JavaScript), not per-block JSON.

Queries and snapshots never increment the revision. They are safe to retry and contain no
filesystem or account data.

## Provenance and audit

Every applied operation, undo and redo appends an `AuditEntry`:

```ts
{
  projectId, operationId, actorId,
  source: "ui" | "mcp" | "api" | "system",
  action: "apply" | "undo" | "redo",
  revisionBefore, revisionAfter, changedBlocks, timestampMs,
  parameterSummary: object, linkedObjectId?: string
}
````

The parameter summary is bounded (batch operations report counts, not every block) and is
safe to expose to AI clients. Secrets and filesystem paths are never included.

## Errors

See the error envelope above. M2 adds `out_of_bounds`, `limit_exceeded`, `no_history`,
`revision_mismatch` and `permission_denied`. A biome operation on a section without
source biome data is an `invalid_argument` error; FuryBuilder never substitutes a
default biome.

## Invariants covered by tests

- Block state property order is canonical; air variants are recognized.
- Biome ids are namespaced lowercase; a biome operation touches only whole 4x4x4 cells,
  refuses sections without source biome data and caps each section palette at 64 entries.
- `set_biome` bumps the revision only when a cell actually changes; undo restores the
  previous cells and palette through the same section snapshot used for block edits.
- Negative coordinates use floor division for chunks, sections and regions.
- Y outside the dimension is rejected before any write.
- Batch edits are atomic; an invalid position leaves the world untouched.
- Undo restores the exact pre-operation section payloads, including mixed palettes.
- Road, river and vegetation generators are deterministic for the same seed and
  parameters, stay inside their operation bounds, skip fluid columns, and are reversible
  through the same section diffs. Reapplications do not stack roads or deepen rivers.
- Vegetation only fills existing air and enforces minimum tree spacing; a blocked trunk
  or canopy is skipped instead of clipped.
- `find_structure_sites` never mutates the world or bumps the revision, and its candidate
  and result counts are bounded (2,000,000 checks, 64 sites).
- Idempotent retries never apply twice.
- Different dimensions never share coordinates.
