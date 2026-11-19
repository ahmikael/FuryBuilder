# IPC and MCP contract

Status: v3 (M5). Versioned alongside the app; breaking changes require a migration note.

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
  targetDataVersion: number; // 4189
}
```

### `ping`

Request: `{ name?: string }` (max 64 chars).

```ts
interface PingResponse {
  message: string; // "pong, <name>"
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
    palette: BlockState[]; // one entry = implicit fill
    data: number[]; // u64 words as [lo32, hi32, ...]
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
  name: string; // "minecraft:oak_stairs"
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
  volume: number; // inclusive volume, bounded to 65,536
  blocks: Array<{ pos: BlockPos; state: BlockState }>; // non-air only
}
```

Used by copy/paste. Air is implied, so an all-air region returns an empty block list.

## Project commands (v3, M5)

The desktop shell owns a `ProjectStore` rooted at the app-data `projects/` directory and a
single open project session. All project commands return the same structured `AppError`
envelope; filesystem paths are never accepted from the UI (only validated folder names).

### `project_list`

Returns `{ projects: ProjectSummary[], warnings: string[] }`, newest first. Corrupt
folders become warnings instead of breaking the list.

```ts
interface ProjectSummary {
  folder: string; // validated single path component, ends in .furybuilder
  projectId: string;
  name: string;
  creationMode: "empty" | "flat" | "vanilla" | "import";
  targetMinecraftVersion: string;
  targetDataVersion: number;
  worldRevision: number;
  updatedAtMs: number;
  lastSavedAtMs: number;
  lastSaveKind: "manual" | "autosave" | "checkpoint";
  bytes: number;
  schemaVersion: number;
  recoverable: boolean; // session marker or a newer verified autosave
}
```

### `project_status`

Returns the current `SessionStatus` (`folder`, `name`, `savedRevision`,
`autosavedRevision`, `revision`, `dirty`, `editorState`, `availableCheckpoints`) or null
fields when nothing is open.

### `project_create`

Request `{ name, mode, editorState }`. Available modes are `empty` and `flat`; `vanilla`
and `import` are rejected until their milestones. Creates the folder, an initial verified
snapshot (revision 1) and `project.json`, starts a session and replaces the authoritative
world service.

### `project_open`

Request `{ folder }`. Verifies the manifest and SHA-256, decodes and re-encodes the
snapshot, adopts a newer verified autosave if a crash marker exists, starts a session and
replaces the world service. Returns `{ detail, info, recovery, session }`; `recovery` is
`null` or:

```ts
interface RecoveryInfo {
  reason: string;
  recoveredRevision: number; // revision actually opened
  recoveredAtMs: number;
  fromBackup: boolean;
  available: boolean; // false when the newer autosave was rejected
  autosaveRevision: number | null; // revision recorded by the artifacts
}
```

`available: false` means the last saved snapshot was opened; the autosave was preserved and
never claimed as restored.

### Diagnostics commands

| Command                       | Request / result                                                                         |
| ----------------------------- | ---------------------------------------------------------------------------------------- |
| `diagnostics_info`            | Returns `{ appVersion, logDir, fileName, files, totalBytes, maxBytesPerFile, maxFiles }` |
| `diagnostics_clear`           | Removes only FuryBuilder-owned `furybuilder*.log` files and returns the empty info       |
| `diagnostics_open_folder`     | Opens the fixed application log directory in the OS file manager                         |
| `diagnostics_record_ui_error` | Request `{ message, source }`; appends a sanitized UI error without world data           |

Logs are local JSON Lines, bounded to 2 MiB per file and four files. They contain event
names, error codes and short messages; absolute path-like text and control characters are
redacted before writing.

### `project_save`

Request `{ editorState }`. Runs the atomic temp → verify → publish pipeline described in
`docs/design/projects.md` and returns:

```ts
interface SaveOutcome {
  project: ProjectDetail;
  checksum: string;
  snapshotFile: string;
  bytesWritten: number;
  backupCreated: boolean;
  prunedSnapshots: number;
}
```

### `project_autosave`

Request `{ editorState, reason?: string }`. Writes `data/autosave.fwl` + checksum metadata
without changing the manifest revision; a non-empty reason also writes a rolling
checkpoint. Returns the autosaved revision.

### `project_checkpoint` / `project_checkpoints` / `project_open_checkpoint`

`project_checkpoint` takes `{ reason }` and returns the bounded checkpoint list;
`project_checkpoints` takes `{ folder?: string }`; `project_open_checkpoint` takes
`{ folder?: string, revision }` and replaces the world service with the verified checkpoint
(leaving it unsaved).

### `project_close`

Request `{ save: boolean, editorState }`. With `save: true` it saves first; with `false` it
also removes autosave artifacts so discarded work cannot reappear. Removes the session
marker, clears only `cache/`, and resets the world service.

### `project_rename`

Request `{ folder, name }`. Changes the display name only; the folder never moves.

## Conventions

- Serde DTOs use `#[serde(rename_all = "camelCase")]`.
- TypeScript mirror types live in `apps/desktop/src/lib/contracts.ts` and
  `renderer/types.ts` until generated bindings from `packages/contracts` are introduced.
- Commands do not expose filesystem paths, shell execution or network access.
- Errors from the world core are mapped, never stringified ad hoc.
- Contract tests exist on both sides (`cargo test`, Vitest).
