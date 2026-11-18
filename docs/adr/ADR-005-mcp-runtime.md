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
  it. The methods are `ping`, `world_info`, `query`, `project_status`, `get_camera`,
  `capture_viewport`, `preview_operation`, `apply_operation`, `undo_operation`,
  `start_job`, `job_status`, `cancel_job`, `list_jobs`, `operation_status` and
  `mcp_activity`.
- `query`/`world_info` require an explicit `project_id` matching the project currently open
  in the editor. The handler calls the same `WorldService` as the UI; writes are gated (see
  below).
- Preview validates and counts blocks without mutating and reports whether current access
  would allow the write. Apply and undo enforce the process-wide access policy, which the
  bridge cannot change: only the Tauri-only `set_mcp_access` command can select
  `read_only`, `edit_in_selection` or `full_project` and set block/batch limits. The
  default is read-only with confirmation enabled. Undo is checked against the exact
  recorded operation extent and executed with that resolved operation id.
- With confirmation enabled, apply/undo emit `furybuilder-mcp-confirmation-request` and
  wait up to 90 seconds for the Tauri-only `submit_mcp_confirmation` command. Denial,
  timeout or a missing UI fails closed with `permission_denied`; the policy is re-checked
  after approval. Tool writes are recorded as `actor: mcp-bridge`, `source: mcp` in the
  same history and audit log as UI operations, and `operation_status` reads that audit
  trail.
- Long-running work goes through a bounded single-worker queue in
  `world-application::jobs`, never through bridge-owned state. `start_job` accepts only
  `apply_operation` and `undo_operation`, applies the same project, revision, access, limit
  and confirmation checks as the synchronous endpoints, and returns a queued/running
  snapshot. The operation observer reports validating/planning/applying phases as progress;
  cancellation before writing touches nothing, and cancellation during writing restores all
  touched sections. `start_job` is idempotent on `(project_id, idempotency_key)` with a
  payload fingerprint: a retry returns the existing job without a second confirmation, and a
  changed payload is a conflict. `job_status`/`cancel_job` are project-scoped, `list_jobs`
  exposes what the editor may show, and `operation_status` links a committed job back to its
  `operationId`. Changing the MCP access policy cancels active MCP jobs so an approval cannot
  outlive its policy; opening/closing a project or restoring a checkpoint cancels all active
  jobs before the world is replaced. The queue is in-memory (16 active, 256 retained) and is
  not a durable scheduler.
- Every authenticated bridge request also appends a bounded in-memory journal entry with
  its request id, bridge `clientId`, method, project, requested revision, status/error code,
  `operationId` and changed block count. `ping` and `mcp_activity` are excluded; denied and
  failed calls are retained. `operation_status` returns the matching journal entries, and
  the editor History panel reads `mcp_activity` for the open project. The journal is
  metadata only: it does not duplicate world payloads, and journaling failure cannot fail a
  completed write. `bridge_info.connection.instanceId` is the same value the bridge sends as
  `clientId`, so calls can be attributed to a specific stdio process even though the MCP
  layer does not expose the human-facing client name.
- Camera and capture requests go from the Tauri process to the mounted viewport over a local
  event and come back through `submit_renderer_response`. `ping.renderer.listening` reports
  readiness. The endpoint validates the response size and PNG format, refuses a different
  visible dimension, and adds `verification.authoritativeRevision`/`stale` so an old frame is
  never claimed as current. `partial`/`missingChunks` stay visible.
- `mcp/bridge` reads the discovery file, connects over the local socket, and reports a
  non-failing `desktop` status in `bridge_info`. It never opens project or world files.
- Verified with a real `tauri dev` process: authorized `ping`, `world_info`, `get_camera` and
  `capture_viewport` succeeded (a valid 492714-byte PNG); `ping` reported `read_only` access
  with confirmation enabled; preview reported `access.allowed=false`; apply returned
  `permission_denied` with `mcpAccessMode=read_only`; `operation_status` reported
  `job.available=true` with an empty queue; `start_job` returned the same `permission_denied`
  and `list_jobs` stayed empty; `mcp_activity` returned eight bounded records including both
  denied calls with `clientId=probe-local-bridge`, and an invalid limit returned
  `invalid_argument`; a wrong token returned `unauthorized` without leaking the secret. The
  approval dialog itself is covered by Rust and frontend tests rather than an automated click
  in the real window.
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
