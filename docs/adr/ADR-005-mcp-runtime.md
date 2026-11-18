# ADR-005: MCP runtime and connection to the desktop app

- **Status:** Accepted
- **Date:** 2026-11-17

## Decision

The MCP server is a **local stdio adapter** over the same Application API used by the
Tauri UI.

- `crates/world-application` is the authority. It owns project state, policy, jobs,
  revisions and the operation journal.
- The desktop app runs the authority in the Tauri process and exposes a **local IPC
  endpoint** (Windows named pipe; Unix domain socket later) with a short-lived,
  per-launch token.
- `mcp/bridge` is a thin TypeScript stdio process using the official MCP SDK. It connects
  to the local endpoint, maps tool calls to Application API requests, and contains **no
  world logic**.
- If the app is not running, the bridge starts or requests a headless authority process
  for an explicitly selected project. It never opens a world file directly.
- No network port is opened by default. Streamable HTTP, if added, binds loopback only,
  validates Origin/Host, and requires explicit user enablement.

## Implementation note (2026-11-18)

The first transport slice is implemented and exercised against the real Tauri process:

- The desktop process creates a `local_socket` endpoint (Windows named pipe) and writes
  `mcp/runtime.json` under the application data directory. The file contains the endpoint,
  a 64-character per-launch token, the process id and protocol version 1. It is removed on
  a clean exit; a stale file is reported as `app_not_running`.
- The protocol is newline-delimited JSON with one authenticated request per connection.
  Messages are size-limited; the token is compared without an early exit; errors never echo
  it. The methods are `ping`, `world_info`, `query`, `project_status`, `get_camera` and
  `capture_viewport`.
- `query`/`world_info` require an explicit `project_id` matching the project currently open
  in the editor. The handler calls the same `WorldService` as the UI; write methods are not
  exposed yet.
- Camera and capture requests go from the Tauri process to the mounted viewport over a local
  event and come back through `submit_renderer_response`. `ping.renderer.listening` reports
  readiness. The endpoint validates the response size and PNG format, refuses a different
  visible dimension, and adds `verification.authoritativeRevision`/`stale` so an old frame is
  never claimed as current. `partial`/`missingChunks` stay visible.
- `mcp/bridge` reads the discovery file, connects over the local socket, and reports a
  non-failing `desktop` status in `bridge_info`. It never opens project or world files.
- Verified with a real `tauri dev` process: authorized `ping`, `world_info`, `get_camera` and
  `capture_viewport` succeeded (a valid 492714-byte PNG), and a wrong token returned
  `unauthorized` without leaking the secret.
- Unix domain sockets are not exercised yet; Windows is the first target. The discovery
  file relies on the per-user application data directory ACL, so multi-user pipe hardening
  remains a review item.

## Rationale

External AI clients start stdio servers easily. A sidecar on the official TS SDK avoids
coupling MCP protocol churn to the Rust core, while keeping every write behind the same
validation, transaction and audit path as the UI.

## Consequences

- The bridge is stateless with respect to project context: every call carries
  `project_id`, `world_revision` where relevant, and explicit selection/job IDs.
- Authentication failure and app-not-running states produce actionable errors.
- MCP cannot access arbitrary file paths or shell commands.
