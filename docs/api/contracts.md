# IPC and MCP contract

Status: v2 (M4). Versioned alongside the app; breaking changes require a migration note.

## Error envelope

Every failing command returns:

```json
{
  "code": "out_of_bounds | invalid_argument | not_found | conflict | permission_denied | revision_mismatch | limit_exceeded | no_history | unsupported_version | unsupported_format | backend_unavailable | internal",
  "message": "Human-readable explanation",
  "details": { "optional": "structured context" }
}
```

- `code` is stable and machine-readable; UI and MCP branch on it.
- `message` is safe to show to a user; it never contains secrets or private system paths.
- Panics are bugs; commands must return structured errors.

## Commands in v1

### `app_info`

```ts
interface AppInfo {
  name: string;
  version: string;
  targetMinecraftVersion: string; // "1.21.4"
  targetDataVersion: number;      // 4189
}
```

### `ping`

Request: `{ name?: string }` (max 64 chars).

```ts
interface PingResponse {
  message: string;   // "pong, <name>"
  appVersion: string;
  timestampMs: number;
}
```

### `world_info`

Returns `WorldInfo` with target version, revision, dimensions and block/chunk/section
counts. See `docs/api/world-api.md`.

### `load_demo_world`

Seeds the deterministic M3 demo scene (terrain, house, pond, trees) and returns
`WorldInfo`. It is demo content, not a Minecraft world and not Vanilla World generation.
The scene is not added to user undo history.

### `render_snapshot`

Request: `{ dimension: string, center: {x,z}, radius: number }`, radius 0..6.

```ts
interface RenderSnapshot {
  dimension: string;
  revision: number;
  center: { x: number; z: number };
  radius: number;
  chunks: ChunkSnapshot[]; // requested square, loaded=false for missing chunks
}

interface ChunkSnapshot {
  chunk: { x: number; z: number };
  loaded: boolean;
  sections: Array<{
    sectionY: number;
    palette: BlockState[];       // one entry = implicit fill
    data: number[];              // u64 words as [lo32, hi32, ...]
    bitsPerIndex: number;
    nonAir: number;
  }>;
}
```

The payload is a derived render view. It never mutates the world and carries the
authoritative revision.

### `render_chunks`

Request: `{ dimension: string, chunks: ChunkPos[] }`, bounded to 256 unique chunks.
Returns `ChunkSnapshotBatch { dimension, revision, chunks }`. Used to refresh dirty chunks
after an edit.

### `get_block`

Request: `{ dimension: string, pos: {x,y,z} }`. Returns the exact `BlockState` from the
Rust core, including negative coordinates. Out-of-range Y and unknown dimensions return
structured errors.

## Editor commands (v2, M4)

### `block_catalog`

Returns `BlockDefinition[]` sorted by block id:

```ts
interface BlockDefinition {
  name: string;                       // "minecraft:oak_stairs"
  properties: Record<string, string>; // canonical defaults for 1.21.4
}
```

The catalog is curated data from the versioned Rust registry. It never contains textures
or asset paths.

### `preview_operation`

Request: `{ operation: Operation }`.

```ts
interface PreviewResult {
  affectedBlocks: number;
  changedBlocks: number;
  dirtyChunks: ChunkPos[];
  warnings: string[];
}
```

Validates bounds, limits and target states without mutating the world or the revision.
The editor uses it before every write; it is safe to cancel by discarding the result.

### `apply_operation`

Request:

```ts
{
  operation: Operation;
  expectedRevision?: number | null;
  idempotencyKey?: string | null;
}
```

Returns `OperationResult { operationId, worldRevision, changedBlocks, dirtyChunks,
warnings, replayed }`. The operation is atomic. If `expectedRevision` is set and does not
match the current core revision, the command fails with `revision_mismatch` and no write
occurs. Retrying with the same idempotency key returns the original result.

### `undo_operation` / `redo_operation`

`undo_operation` accepts `{ operationId?: string | null }`; `redo_operation` takes no
arguments. Both return `OperationResult` with the chunks that must be remeshed.

### `history_state`

Returns `{ canUndo, canRedo, revision, entries: HistorySummary[] }`; the UI uses it to
enable toolbar buttons. `operation_history` returns only the entry array for panels that
do not need the flags.

### `read_region`

Request: `{ dimension: string, bounds: { min: {x,y,z}, max: {x,y,z} } }`.

```ts
interface RegionBlocks {
  dimension: string;
  bounds: BoxBounds;
  volume: number;              // inclusive volume, bounded to 65,536
  blocks: Array<{ pos: BlockPos; state: BlockState }>; // non-air only
}
```

Used by copy/paste. Air is implied, so an all-air region returns an empty block list.

## Conventions

- Serde DTOs use `#[serde(rename_all = "camelCase")]`.
- TypeScript mirror types live in `apps/desktop/src/lib/contracts.ts` and
  `renderer/types.ts` until generated bindings from `packages/contracts` are introduced.
- Commands do not expose filesystem paths, shell execution or network access.
- Errors from the world core are mapped, never stringified ad hoc.
- Contract tests exist on both sides (`cargo test`, Vitest).
