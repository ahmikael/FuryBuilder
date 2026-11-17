# IPC and MCP contract

Status: v0 (M1). Versioned alongside the app; breaking changes require a migration note.

## Error envelope

Every failing command returns:

```json
{
  "code": "out_of_bounds | invalid_argument | not_found | conflict | permission_denied | revision_mismatch | unsupported_version | unsupported_format | backend_unavailable | internal",
  "message": "Human-readable explanation",
  "details": { "optional": "structured context" }
}
```

- `code` is stable and machine-readable; UI and MCP branch on it.
- `message` is safe to show to a user; it never contains secrets or private system paths.
- Panics are bugs; commands must return structured errors.

## Commands in v0

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

## Conventions

- Serde DTOs use `#[serde(rename_all = "camelCase")]`.
- TypeScript types live in `apps/desktop/src/lib/contracts.ts` until generated bindings
  from `packages/contracts` are introduced.
- Commands do not expose filesystem paths, shell execution or network access.
- Errors from the world core are mapped, never stringified ad hoc.
- Contract tests exist on both sides (`cargo test`, Vitest).
