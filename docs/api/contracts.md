# IPC and MCP contract

Status: v1 (M3). Versioned alongside the app; breaking changes require a migration note.

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

## Conventions

- Serde DTOs use `#[serde(rename_all = "camelCase")]`.
- TypeScript mirror types live in `apps/desktop/src/lib/contracts.ts` and
  `renderer/types.ts` until generated bindings from `packages/contracts` are introduced.
- Commands do not expose filesystem paths, shell execution or network access.
- Errors from the world core are mapped, never stringified ad hoc.
- Contract tests exist on both sides (`cargo test`, Vitest).
