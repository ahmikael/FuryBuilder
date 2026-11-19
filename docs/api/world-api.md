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

QueryResult =
| { type: "world_info", data: WorldInfo }
| { type: "block", data: { dimension, pos, state } }
| { type: "biome", data: { dimension, pos, biome: BiomeId | null } }
| { type: "region_summary", data: RegionSummary }
| { type: "chunk_stats", data: ChunkStats }
| { type: "structure_sites", data: StructureSites }

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
