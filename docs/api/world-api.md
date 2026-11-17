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
  | { type: "region_summary", dimension, bounds }
  | { type: "chunk_stats", dimension, chunk }

QueryResult =
  | { type: "world_info", data: WorldInfo }
  | { type: "block", data: { dimension, pos, state } }
  | { type: "region_summary", data: RegionSummary }
  | { type: "chunk_stats", data: ChunkStats }
```

`region_summary` is bounded to 1,000,000 blocks and returns a palette sorted by count.
Queries never mutate the world or bump revisions.

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
```

The parameter summary is bounded (batch operations report counts, not every block) and is
safe to expose to AI clients. Secrets and filesystem paths are never included.

## Errors

See the error envelope above. M2 adds `out_of_bounds`, `limit_exceeded`, `no_history`,
`revision_mismatch` and `permission_denied`.

## Invariants covered by tests

- Block state property order is canonical; air variants are recognized.
- Negative coordinates use floor division for chunks, sections and regions.
- Y outside the dimension is rejected before any write.
- Batch edits are atomic; an invalid position leaves the world untouched.
- Undo restores the exact pre-operation section payloads, including mixed palettes.
- Idempotent retries never apply twice.
- Different dimensions never share coordinates.
