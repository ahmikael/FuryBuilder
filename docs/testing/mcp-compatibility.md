# MCP compatibility

Status: implemented and verified against a real external MCP client (OpenCode CLI 2.3.2),
including a complete AI explore/build/screenshot/fix/undo loop with real confirmation
dialogs. Setup, access modes and revocation: `docs/mcp-setup.md`.

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
the UI. Write methods require an explicit access grant and per-request confirmation; they are
described below.

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

## Safe write tools

| Tool                     | Maps to                      | Behavior                                               |
| ------------------------ | ---------------------------- | ------------------------------------------------------ |
| `preview_operation`      | endpoint `preview_operation` | validates and counts changes; never mutates            |
| `apply_operation`        | endpoint `apply_operation`   | one typed operation; requires user grant + idempotency |
| `apply_operations_batch` | endpoint `apply_operation`   | atomic `batch` of up to 8192 changes                   |
| `undo_operation`         | endpoint `undo_operation`    | latest/named undo; requires user grant                 |
| `get_operation_status`   | endpoint `operation_status`  | audit entry or recent history; queue and linked job    |

MCP starts **read-only**. The bridge cannot change the policy: only the Tauri UI can call
`set_mcp_access`. Three modes exist:

| Mode                | Effect                                                                 |
| ------------------- | ---------------------------------------------------------------------- |
| `read_only`         | preview/status work; apply and undo return `permission_denied`         |
| `edit_in_selection` | writes and undo must fit one dimension and one inclusive box           |
| `full_project`      | writes are not box-restricted, but hard block/batch limits still apply |

The default policy allows 50,000 blocks per operation and 4,096 changes per batch; the
hard ceilings are 1,000,000 blocks and 8,192 changes. Limits are enforced on the operation
itself, not only on the request schema. A selection with an overflowing volume is rejected
through the saturating `BlockBox::checked_volume` path instead of panicking.

Every apply and undo requires an explicit `world_revision`. Undo is checked against the
exact extent recorded for the operation, and it is executed with that resolved
`operation_id`, so a newer operation cannot be silently undone after approval. With
`require_confirmation` enabled (the default), the endpoint emits
`furybuilder-mcp-confirmation-request` to the editor, waits up to 90 seconds, and writes
only after the user calls `submit_mcp_confirmation` with `approved: true`. Denial, timeout
or a missing UI fails closed with `permission_denied`. The policy is re-checked after the
approval in case the user changed it while the dialog was open.

Accepted writes are recorded with `actor: mcp-bridge`, `source: mcp` in the authoritative
history and audit log. `preview_operation` remains read-only and now reports
`access.allowed`, the denial code and details so a client knows whether an apply would be
permitted without guessing. Preview and status remain available in every mode.

## MCP activity journal

Every authenticated request is appended to a bounded in-memory journal (1,000 entries,
oldest evicted first). The record contains the request id, the bridge `clientId`, method,
status (`ok`/`denied`/`error`), structured error code, project, requested revision,
`operationId`, changed blocks, confirmation result, timestamp and duration. `ping` and the
journal query itself are excluded so health checks and UI polling cannot flush useful
records.

Successful apply/undo responses are linked through the same `operationId` that appears in
the authoritative history and audit log. `get_operation_status` returns
`mcpActivity` for that operation; without an operation id it returns the latest operations
plus recent MCP calls. The endpoint method `mcp_activity` exposes the same metadata to the
editor and to MCP clients, bounded by `limit` (1–100) and scoped to the explicitly named
open project. Denied and failed calls are recorded too, which makes a refused write
auditable instead of invisible.

The editor's History panel shows the journal alongside transactions and polls it every five
seconds for the open project. The journal never duplicates full operation payloads, and a
journal failure cannot turn a completed world write into a failed tool call.

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
and cancellation, and `start_job` requires an `idempotency_key` plus `world_revision` for
write kinds.

## Long-running job queue

The Core owns a bounded single-worker queue (`world-application::jobs`) with at most 16
active jobs and 256 retained records. An MCP job may execute `apply_operation` or
`undo_operation`; any other kind is refused before queueing. A write job passes the same
project, revision, access-mode, selection, limit and confirmation checks as the synchronous
`apply_operation`/`undo_operation`, then returns a snapshot with `state: "queued"` or
`running`. The confirmation is deferred: the approval summary says so and the job carries
the approved `operationId`/extent.

`get_job_status` returns the live state (`queued`, `running`, `succeeded`, `failed`,
`cancelled`), progress 0–100, phase message, cancellation flag, world revision, error or
completed result. `cancel_job` cancels a queued job before it starts and requests
cooperative cancellation for a running one; cancellation during planning writes nothing,
and cancellation during the write phase restores every touched section. Jobs are scoped to
the explicitly named open project.

`start_job` is idempotent: the queue fingerprints the project, kind, parameters and expected
revision under `(project_id, idempotency_key)`. A retry returns the existing job with
`replayed: true` without queueing again or asking for confirmation twice; reusing the key
for different work is `conflict`. `operation_status` reports `job.available: true`, the queue
summary, recent jobs and the job linked to a specific `operationId`. The editor can read the
same data through `list_jobs`; changing the MCP access policy cancels every active MCP job so
an approval cannot outlive the policy that granted it. Opening or closing a project and
restoring a checkpoint cancel every active job before the authoritative world is replaced,
so work authorized against the previous world cannot commit into the next one.

If the desktop app is absent, the job tools return the stable `job_api_unavailable` error
instead of inventing a job, progress or result.

## Write propagation into the viewport

MCP writes bypass the UI command path, so the Rust process emits
`furybuilder-world-changed` after every allowed synchronous apply/undo and after a job
reaches `succeeded`. The payload carries `source: "mcp"`, the action, project, operation,
world revision, changed blocks and dirty chunks. The mounted editor's
`McpWorldChangeHost` refreshes the world summary, history and session marker and rebuilds
the named chunks; a job snapshot without dirty chunks falls back to a bounded full reload.
Without this event the authoritative world advanced while the Three.js viewport stayed on
the revision it had loaded last, and a following capture reported `stale: true`. That is
exactly what the first end-to-end run exposed, and the reason this event exists.

`capture_viewport` keeps its image block and now also publishes `image.bytes` and
`image.sha256` in the text metadata block, so a text-only client can verify that a
non-empty PNG arrived even when it cannot inspect pixels.

## Structured observations

The minimum observation set the plan requires is summary, block inspection and a bounded
screenshot. Each answers with an explicit revision and never claims more than it knows:

- `world_info` reports the authoritative `revision`, dimensions and block/chunk counts.
- `get_block` returns `type: "block"`, the canonical state (name plus properties) and the
  `worldRevision` the read was taken at.
- `inspect_region` returns `type: "region_summary"` with the echoed dimension and bounds,
  the inclusive volume (`blocks`), `nonAirBlocks`, the palette and the same
  `worldRevision`. The palette counts always cover the volume.
- `capture_viewport` returns a real PNG image plus a metadata block: schema version, mode,
  dimension, world revision, camera pose, resolution, renderer schema, a legend capped at
  16 states with `legendTruncated`, requested/loaded/missing chunks and `partial`. The Rust
  endpoint adds `verification` with `renderRevision`, `authoritativeRevision`, `stale` and
  `partial`; the bridge adds the PNG byte count and SHA-256.

The revision is read under the same lock as the data, so a block or region observation is
attributable to an exact world state. A caller that supplies a stale `world_revision`
receives a structured `revision_mismatch` with `expectedRevision` and `actualRevision`
instead of silently reading newer data. A capture never upgrades its claim: an empty
project with no loaded chunks reports `partial: true`, names the nine missing chunks and an
empty legend, even though the PNG itself is valid.

## Tasks option and fallback

Tasks remains an explicit option, never an assumption. The bridge evaluates three
independent conditions for each request:

1. the client's advertised capabilities include `tasks`;
2. the bridge implements the extension (currently `false`);
3. the authoritative Core Job API is connected (the desktop answers and its queue reports
   `available: true`).

Only when all three hold may the extension be advertised. In every other case the
response reports `mode: "fallback"` with a specific reason
(`client_did_not_declare_tasks`, `tasks_not_implemented` or
`job_backend_unavailable`), and callers use the ordinary `start_job`/`get_job_status`/
`cancel_job` tools, which now reach the authoritative queue.

## Automated evidence

Command:

```powershell
pnpm --filter @furybuilder/mcp-bridge test
pnpm --filter @furybuilder/mcp-bridge typecheck
pnpm --filter @furybuilder/mcp-bridge build
```

Observed: 25 bridge tests passed, typecheck and build clean; the Rust desktop test binary
runs 33 tests, `world-application` 45, and the frontend 142. The TypeScript tests spawn the
real bridge over stdio with `tsx src/main.ts` and use `@modelcontextprotocol/client` 2.4.0:

- a client pinned to `2026-07-28` connects in the modern era, lists `bridge_info`
  and receives an explicit project/revision response;
- a default client connects through the legacy lifecycle and calls the same tool;
- a call without `project_id` is rejected, and consecutive calls with different
  project IDs do not inherit one another's context;
- the context policy accepts a matching project/revision, accepts an omitted revision
  for discovery, and rejects a different project or a stale revision;
- the three fallback job tools are listed, publish `world_revision` and
  `idempotency_key`, and return `job_api_unavailable` against an absent desktop instead of
  a fabricated job;
- live `start_job`, `get_job_status` and `cancel_job` calls map to the desktop endpoint with
  camelCase `projectId`/`idempotencyKey`/`worldRevision`, return the authoritative snapshot,
  and surface `permission_denied` from the desktop as a structured tool error;
- `bridge_info` reports `jobs.backendConnected: true` only when the desktop is reachable
  and its queue answers `available: true`;
- the Tasks policy enables the extension only when client, bridge and Job API all
  support it;
- the six read-only tools are listed with read-only annotations and return
  `runtime_file_missing` instead of a fabricated result when the app is absent;
- `bridge_info` reports `runtime_file_missing` when the desktop app is absent and a
  live project/revision when a local endpoint answers;
- the same live endpoint yields `revision_mismatch` for a stale revision while keeping
  the active revision visible in the response;
- live reads cover `world_info`, `get_block`, `inspect_region`, `get_camera` and
  `capture_viewport`; block and region responses carry `worldRevision`, and the capture is
  returned as PNG image content plus metadata and a verification block;
- the five write tools are listed: preview is read-only, apply/undo declare destructive
  writes and require `idempotency_key`, and every call fails honestly when the desktop
  app is absent;
- live calls map `preview_operation`, `apply_operations_batch`, `operation_status` and
  `undo_operation` to the expected endpoint payloads;
- `bridge_info` exposes the live access mode, write flags, limits, renderer readiness and
  pending confirmation count from `ping`;
- every bridge request carries the same `clientId` that `bridge_info.connection.instanceId`
  publishes, so the desktop journal can attribute calls to one bridge process;
- the desktop client maps a wrong token to `authentication_failed`, a stale endpoint
  to `app_not_running`, malformed discovery files to `invalid_runtime_file`, and a
  silent endpoint to `timeout`.

The Rust tests cover camera/capture revision verification, dimension mismatch, invalid
image rejection, renderer timeout without a viewport, one-answer-per-request bridge
semantics, read-only preview/status, policy enforcement for both edit modes, block and
batch limits, missing revision rejection, approval/denial of apply and undo, undo extent
tracking, granted batch apply/undo with audit entries, the bounded activity journal, the
endpoint link between an authenticated request and its `operationId` (including denied
calls and one-entry-per-request ordering), and the job queue: progress phases, rollback of
a cancelled write, idempotent replay without duplicate confirmation, project scoping,
operation linking and the same policy gates as synchronous writes. The frontend tests cover
the policy command payloads, confirmation submission, dialog rendering, restoring a pending
confirmation after the UI reloads, and the activity query payload; the Rust bridge keeps the
request id and expiry so the dialog can be reconstructed.

### Real Tauri process

`scripts/probe-local-bridge.mjs` was run against a real `tauri dev` process:

```text
discovery: schemaVersion 1, local_socket, protocol 1, named pipe, tokenLength 64
ping: ok, projectId "local-project", worldRevision 0, app 0.1.0, rendererListening true
ping access: read_only, maxBlocks 50000, requireConfirmation true
renderer: listening true after the WebView mounted
world_info: ok, target Minecraft 1.21.4, revision 0, 3 dimensions
block observation: type block, worldRevision 0, state minecraft:air
region summary: type region_summary, worldRevision 0, blocks 27, nonAirBlocks 0,
                palette 1 entry covering all 27 blocks
stale observation: revision_mismatch, expectedRevision 1, actualRevision 0
camera: overworld, render revision 0 = authoritative revision 0, stale false
capture: image/png, PNG signature valid, 492714 bytes, partial true, stale false,
         render revision 0 = authoritative revision 0, 9 missing chunks, 0 loaded,
         0 legend entries
preview: affected 1, changed 1, world revision 0, access.allowed false (permission_denied)
apply in read-only: permission_denied, accessMode read_only, operation apply_operation
operation_status: revision 0, canUndo false, job.available true, queued 0, retained 0
job start in read-only: permission_denied, accessMode read_only; list_jobs empty, available true
activity: 11 entries, bounded true; denied apply -> status denied, error permission_denied,
          clientId "probe-local-bridge", projectId "local-project"; denied start_job is
          recorded as start_job/denied/permission_denied
activity limit 0: invalid_argument
wrong token: unauthorized, responseContainsToken false
LOCAL_BRIDGE_PROBE_OK
```

The probe waits for `renderer.listening` before asking for the camera, so a startup race is
reported instead of being mistaken for a broken tool. `partial=true` is expected here: the
default empty project has no loaded chunks, and the metadata names the missing chunks. The
probe writes nothing and does not print the token. The app was force-stopped after the
probe, so its stale discovery file was removed manually; a normal exit removes it. Raw
output: `docs/testing/evidence/2026-11-18-structured-observation.txt`.

## Live MCP observation probe

`mcp/bridge/scripts/probe-live-observation.mjs` connects to the running app through a real
stdio bridge and the modern MCP era. It calls `bridge_info`, `world_info`, `get_block`,
`inspect_region`, a stale read and `capture_viewport`, then asserts:

- block and region responses carry the exact `worldRevision` from `world_info`;
- the region palette counts add up to the 27-block volume and `nonAirBlocks` stays within it;
- a stale revision is rejected as `revision_mismatch`, not silently served;
- the capture image is a real PNG whose byte count and SHA-256 match the published metadata;
- `verification.partial` matches `metadata.partial`, `partial` matches the missing-chunk
  list, and `stale` matches the comparison of render and authoritative revisions.

Observed on 2026-11-18: modern era, revision 0, `minecraft:air` at `(0,64,0)`, region
volume 27 with one palette entry, stale request rejected, and a 492714-byte PNG
(`sha256 45b0044b…`) with `partial: true`, `stale: false`, 9 missing chunks and 0 loaded
chunks. Evidence: `docs/testing/evidence/2026-11-18-mcp-observation.json`.

## Real external MCP client (OpenCode)

On 2026-11-18 **OpenCode CLI 2.3.2** connected to the bridge over stdio and drove it
through a test agent with `--agent plan --auto`. The server was configured as a local MCP
server with the bridge's own working directory, `codemode: false`, and an explicit runtime
path:

```jsonc
{
  "mcp": {
    "servers": {
      "furybuilder": {
        "type": "local",
        "command": ["<node>", "--import", "tsx", "src/main.ts"],
        "cwd": "<repo>/mcp/bridge",
        "environment": {
          "FURYBUILDER_MCP_RUNTIME": "<appData>/com.furybuilder.desktop/mcp/runtime.json",
        },
        "codemode": false,
        "protocol": "auto",
      },
    },
  },
}
```

Two runs were made against a real `tauri dev` process:

- **Modern:** `protocol: "auto"` negotiated `protocolEra: "modern"` with
  `protocolVersion: "2026-07-28"`. The agent called `furybuilder_bridge_info` and
  `furybuilder_world_info`, reported `context.state: "ready"`, `desktop.connected: true`,
  `access.mode: "read_only"`, `jobs.backendConnected: true`, renderer listening, and world
  target Minecraft `1.21.4` / `DataVersion` `4189` at revision `0`.
- **Legacy:** `protocol: "legacy"` completed the classic `initialize` lifecycle, reported
  `protocolEra: "legacy"`, the same ready context and a live read-only desktop.

The agent was instructed to use only MCP tools and made no file or shell changes. Evidence:
`docs/testing/evidence/2026-11-18-opencode-mcp-client.json`.

## End-to-end AI edit loop

`scripts/e2e-mcp-agent.mjs` drives a real external client through the complete loop:
explore the empty scratch project, preview and apply a 13-block platform, capture the
viewport, replace the centre with glass, capture again, verify both block states, undo the
glass and then the platform, and inspect the final world. The script:

- refuses a non-empty world, so it is not an editing tool for a user's project;
- enables `full_project` with confirmation through the same `set_mcp_access` command the
  access dialog uses, then restores `read_only`;
- starts OpenCode with a temporary project configuration and instructs it to use only MCP
  tools;
- approves each real `McpConfirmationHost` dialog by clicking `Approve and apply` in the
  live WebView (WebView2 CDP), never by calling `submit_mcp_confirmation` directly;
- verifies the outcome through the authenticated local endpoint and writes the evidence
  file.

Run on Windows after starting the app with remote debugging enabled:

```powershell
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=9229"
pnpm --filter @furybuilder/desktop tauri dev
# in a second terminal
$env:FURYBUILDER_E2E_MODEL = "opencode/deepseek-v4.1-flash-free"
node scripts/e2e-mcp-agent.mjs
```

On 2026-11-18 the run passed all 19 checks: 17 MCP calls, four approved dialogs (apply,
apply, undo, undo), revisions 0→1→2→3→4, two PNG captures at the authoritative revisions
(524841 and 524755 bytes, `stale: false`), undo restored 1 and then 13 blocks, the final
world had 0 blocks, and the activity journal recorded all four writes with approved
confirmations. The screenshots reported `partial: true` because five empty neighbour chunks
were not loaded; the edited chunk `(0,0)` was loaded and the legend changed from 13 stone to
12 stone plus 1 glass, so the frame reflects the build and the fix rather than a stale
revision. No job was queued: the synchronous tools were used. Evidence:
`docs/testing/evidence/2026-11-18-ai-edit-loop.json`.

The requested `openai/gpt-6-luna#max` run was refused by the provider with HTTP 403
`unsupported_country_region_territory`; the loop was executed with
`opencode/deepseek-v4.1-flash-free`. The model choice is therefore not evidence of a
particular provider; the bridge, confirmation UI, Core and verification are.

## Client matrix

| Client                       | Transport                                                 | Status                                                        |
| ---------------------------- | --------------------------------------------------------- | ------------------------------------------------------------- |
| OpenCode CLI 2.3.2           | local stdio via `mcp.servers` (`protocol: auto`/`legacy`) | real agent passed both modern 2026-07-28 and legacy lifecycle |
| MCP SDK test client 2.4.0    | stdio                                                     | modern and legacy tests passed                                |
| Claude Desktop / other hosts | stdio                                                     | documented target; untested                                   |

`bridge_info.request.protocolEra`/`protocolVersion` is the authoritative record of the
negotiated era; `opencode mcp list` does not show it.

## Limitations

- `bridge_info` is a handshake diagnostic, not an editor tool.
- Writes are disabled by default. Access modes, limits and per-request confirmation now
  exist. The end-to-end script clicks the real approval dialog in a live WebView; denial and
  timeout are covered by Rust and frontend tests.
- The policy and the MCP activity journal are process-wide, not per MCP connection. A future
  task should scope grants and audit records to a connection identity. The journal is
  in-memory and intentionally bounded, so it is not a durable audit trail across restarts;
  accepted operations still live in the authoritative history.
- The job queue is in-memory and process-wide: at most 16 active and 256 retained jobs, one
  worker, no persistence across restarts. Progress is phase-based (0–5 validating, 5–80
  planning, 80–99 applying, 100 succeeded), not a per-block estimate. A policy change
  cancels active MCP jobs, but a job that already committed is not rolled back by that
  policy change.
- Camera and capture require a mounted viewport in the running editor. A hidden or
  unloaded WebView reports `backend_unavailable`; there is no headless capture path yet.
- A capture reports `partial=true` when requested chunks are missing and `stale=true` when
  the render revision is behind the world. The end-to-end frame was partial because empty
  neighbour chunks were not loaded; the edited chunk and its legend were still correct. A
  capture is a viewport image, not a full-world render.
- The viewport refresh after an MCP write is asynchronous. The end-to-end agent's next tool
  call arrived after the refresh, so the captures were current; a capture issued in the same
  instant as the write can still report `stale=true` rather than pretending to be current.
- Unix domain socket support exists only as unexercised code; Windows is the tested target.
- The discovery token relies on the per-user application data directory ACL; stronger
  named-pipe ACLs are still a security review item.
- OpenCode 2.3.2 is the only real external client exercised. Claude Desktop and other hosts
  remain untested, and the modern run used Code Mode disabled so tools were listed directly.
  Only one end-to-end model/client combination was exercised; GPT-6 Luna Max was blocked by
  the provider's region policy, so the loop used a free DeepSeek model. This is not evidence
  of parity with every model or with real Minecraft gameplay.
