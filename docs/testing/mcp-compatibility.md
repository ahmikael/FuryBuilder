# MCP compatibility

Status: protocol negotiation and the authenticated local desktop endpoint are implemented
and tested locally; a real external MCP client connection is still outstanding.

## Bridge baseline

| Item                | Value                                                             |
| ------------------- | ----------------------------------------------------------------- |
| Bridge package      | `mcp/bridge`, `@furybuilder/mcp-bridge` 0.1.0                     |
| SDK                 | `@modelcontextprotocol/server` 2.4.0 (pinned)                     |
| Modern revision     | MCP `2026-07-28`                                                  |
| Legacy fallback     | 2025-era clients served from the same factory                     |
| MCP transport       | stdio only; no network listener                                   |
| Desktop transport   | authenticated local socket (Windows named pipe), protocol 1       |
| Discovery           | `%APPDATA%\com.furybuilder.desktop\mcp\runtime.json`              |
| Context policy      | explicit per request; no project/selection/job in transport state |
| Optional extensions | Tasks and Apps are **not** advertised yet                         |
| Job tools           | `start_job`, `get_job_status`, `cancel_job` always registered     |

## Desktop local endpoint

The Tauri process owns the authoritative `WorldService` and exposes it over a local socket.
The discovery file contains the endpoint, a 64-character per-launch token, the process id
and protocol version 1; it is removed on a clean exit. A stale file is reported as
`app_not_running`, never as a live connection. Requests and responses are newline-delimited
JSON with one authenticated request per connection, bounded message sizes and a token
comparison that does not exit early. Errors never echo the token.

Initial methods: `ping`, `world_info`, `query`, `project_status`, `get_camera`,
`capture_viewport`. World methods require an explicit `project_id` matching the project
currently open in the editor and accept an optional `world_revision`. A supplied revision
must equal the authoritative revision; otherwise the request fails with a structured
`revision_mismatch` error carrying `expectedRevision` and `actualRevision`. A stale caller
therefore never reads newer data silently. `query` reuses the same bounded `Query` enum as
the UI. Write methods are deliberately not exposed yet.

`get_camera` and `capture_viewport` are forwarded from the Tauri process to the mounted
WebView over a local event and answered through the `submit_renderer_response` command.
`ping.renderer.listening` reports whether that listener is attached, and the UI reports
readiness with `renderer_ready`. A capture carries the renderer's real revision and
`partial`/`missingChunks`; the endpoint adds `verification.authoritativeRevision` and
`stale` so an old frame is never presented as current. A requested dimension that differs
from the visible one is refused.

## Read-only MCP tools

| Tool               | Maps to                     | Notes                                            |
| ------------------ | --------------------------- | ------------------------------------------------ |
| `world_info`       | endpoint `world_info`       | version, dimensions, bounds, revision            |
| `get_block`        | `query` → `get_block`       | exact block state at one position                |
| `inspect_region`   | `query` → `region_summary`  | bounded palette, volume and non-air count        |
| `get_region`       | `query` → `region_blocks`   | sparse non-air blocks up to the Core read limit  |
| `get_camera`       | endpoint `get_camera`       | current pose, visible dimension, render revision |
| `capture_viewport` | endpoint `capture_viewport` | real PNG image block plus observation metadata   |

All six are registered with read-only annotations. Screenshots are returned as an MCP
`image` content block plus a text metadata block; the bridge never fabricates an image or
claims that a partial frame is complete.

## Explicit request context and connection lifecycle

Every MCP tool call names its project. The bridge compares it with the live desktop project
and, when the caller supplies `world_revision`, with the authoritative world revision. The
possible states are `ready`, `desktop_unavailable`, `project_mismatch` and
`revision_mismatch`; `bridge_info` reports the state together with the requested and active
context. Nothing is inferred from the stdio session.

The connection is explicitly ephemeral: `bridge_info.connection` publishes a random
per-process `instanceId`, start time, `transport: "stdio"`,
`lifecycle: "ephemeral-stdio"`, `expiresWithProcess: true` and the context state. A bridge
process serves exactly one client and dies with it. Job calls carry a `job_id` for status
and cancellation, and `start_job` requires an `idempotency_key` plus an optional
`world_revision`.

## Tasks option and fallback

Tasks remains an explicit option, never an assumption. The bridge evaluates three
independent conditions for each request:

1. the client's advertised capabilities include `tasks`;
2. the bridge implements the extension (currently `false`);
3. the authoritative Core Job API is connected (currently `false`).

Only when all three hold may the extension be advertised. In every other case the
response reports `mode: "fallback"` with a specific reason
(`client_did_not_declare_tasks`, `tasks_not_implemented` or
`job_backend_unavailable`), and callers use the ordinary job tools. Those tools
currently return the stable `job_api_unavailable` error instead of inventing a job,
progress or result. This is a deliberate refusal, not a simulated success.

## Automated evidence

Command:

```powershell
pnpm --filter @furybuilder/mcp-bridge test
pnpm --filter @furybuilder/mcp-bridge typecheck
pnpm --filter @furybuilder/mcp-bridge build
```

Observed: 22 tests passed, typecheck and build clean; the Rust local-bridge module runs
13 tests. The TypeScript tests spawn the real bridge over stdio with `tsx src/main.ts` and
use `@modelcontextprotocol/client` 2.4.0:

- a client pinned to `2026-07-28` connects in the modern era, lists `bridge_info`
  and receives an explicit project/revision response;
- a default client connects through the legacy lifecycle and calls the same tool;
- a call without `project_id` is rejected, and consecutive calls with different
  project IDs do not inherit one another's context;
- the context policy accepts a matching project/revision, accepts an omitted revision
  for discovery, and rejects a different project or a stale revision;
- the three fallback job tools are listed, publish `world_revision` and
  `idempotency_key`, and return `job_api_unavailable` instead of a fabricated job;
- the Tasks policy enables the extension only when client, bridge and Job API all
  support it;
- the six read-only tools are listed with read-only annotations and return
  `runtime_file_missing` instead of a fabricated result when the app is absent;
- `bridge_info` reports `runtime_file_missing` when the desktop app is absent and a
  live project/revision when a local endpoint answers;
- the same live endpoint yields `revision_mismatch` for a stale revision while keeping
  the active revision visible in the response;
- live reads cover `world_info`, `get_block`, `get_camera` and `capture_viewport`; the
  capture is returned as PNG image content plus metadata;
- the desktop client maps a wrong token to `authentication_failed`, a stale endpoint
  to `app_not_running`, malformed discovery files to `invalid_runtime_file`, and a
  silent endpoint to `timeout`.

The Rust tests cover camera/capture revision verification, dimension mismatch, invalid
image rejection, renderer timeout without a viewport, and one-answer-per-request bridge
semantics.

### Real Tauri process

`scripts/probe-local-bridge.mjs` was run against a real `tauri dev` process:

```text
discovery: schemaVersion 1, local_socket, protocol 1, named pipe, tokenLength 64
ping: ok, projectId "local-project", worldRevision 0, app 0.1.0, rendererListening false
renderer: listening true after the WebView mounted
world_info: ok, target Minecraft 1.21.4, revision 0, 3 dimensions
camera: overworld, render revision 0 = authoritative revision 0, stale false
capture: image/png, PNG signature valid, 492714 bytes, partial true, stale false
wrong token: unauthorized, responseContainsToken false
LOCAL_BRIDGE_PROBE_OK
```

The probe waits for `renderer.listening` before asking for the camera, so a startup race is
reported instead of being mistaken for a broken tool. `partial=true` is expected here: the
default empty project has no loaded chunks, and the metadata names the missing chunks. The
probe writes nothing and does not print the token. The app was force-stopped after the
probe, so its stale discovery file was removed manually; a normal exit removes it.

## Client matrix

| Client                       | Transport                                                 | Status                                    |
| ---------------------------- | --------------------------------------------------------- | ----------------------------------------- |
| OpenCode CLI 2.3.0           | local stdio via `opencode mcp add <name> -- <command...>` | configurable; real connection not yet run |
| MCP SDK test client 2.4.0    | stdio                                                     | modern and legacy tests passed            |
| Claude Desktop / other hosts | stdio                                                     | documented target; untested               |

The OpenCode CLI does not currently expose the negotiated era in its `mcp list`
output. The real-client check must inspect the bridge tool response rather than
assume a protocol revision.

## Limitations

- `bridge_info` is a handshake diagnostic, not an editor tool. The write tool surface
  arrives in the following M7 tasks.
- Reads are implemented; writes, preview/apply/undo and the Job API are still absent, so
  an AI cannot edit through MCP yet.
- Camera and capture require a mounted viewport in the running editor. A hidden or
  unloaded WebView reports `backend_unavailable`; there is no headless capture path yet.
- A capture reports `partial=true` when requested chunks are missing and `stale=true` when
  the render revision is behind the world. It is a viewport image, not a full-world render.
- Unix domain socket support exists only as unexercised code; Windows is the tested target.
- The discovery token relies on the per-user application data directory ACL; stronger
  named-pipe ACLs are still a security review item.
- No real external MCP client (OpenCode/Claude Desktop) has connected yet.
- This evidence does not establish end-to-end AI editing or real-world compatibility.
