# Connecting an AI client to FuryBuilder over MCP

This guide covers the external MCP client that drives FuryBuilder: how to launch the stdio
bridge, how to point an MCP client at it, what the access modes allow, how to read the
common error codes and how to turn access off again.

FuryBuilder itself does not contain or call an AI model. The client (OpenCode, an IDE, a
chat app) owns the model and starts a small `mcp/bridge` process. That process talks to the
running desktop app over an authenticated Windows named pipe. Every read and write still
goes through the same Rust Application API as the UI; the bridge has no direct file or shell
access to the world.

**Verified client:** OpenCode CLI 2.3.2 on Windows, `protocol: "auto"` (MCP `2026-07-28`)
and `protocol: "legacy"`. Other stdio clients are expected to work but are not claimed as
tested; see `docs/testing/mcp-compatibility.md` for the matrix.

## Prerequisites

- Windows. The local endpoint is a Windows named pipe; Unix domain socket code exists but is
  unexercised.
- Node.js 22 or newer for the bridge process.
- The FuryBuilder desktop app running (`pnpm tauri:dev` in a checkout, or an installed
  build). The browser-only Vite preview has no Rust backend and cannot serve MCP.
- A project open in FuryBuilder. The active project ID is part of every MCP call.

While running, the app writes a discovery file:

```text
%APPDATA%\com.furybuilder.desktop\mcp\runtime.json
```

It contains the pipe name, a random 64-character per-launch token and the protocol version.
It is removed on a clean exit. Do not copy it into prompts, logs or issues: anyone with the
file and access to the same user session could talk to the editor. The bridge never prints
the token.

## Build the bridge

From the repository root:

```powershell
pnpm install
pnpm --filter @furybuilder/mcp-bridge build
```

This produces `mcp\bridge\dist\main.js`. For development you can skip the build and let the
client run `node --import tsx src/main.ts` with `cwd` set to `mcp\bridge`; `tsx` is a dev
dependency, so the built `dist/main.js` is the portable choice.

No network port is opened and no firewall rule is needed.

## Configure the client

### OpenCode (tested)

Add a local MCP server to `opencode.json`. Use absolute paths for the Node executable and
the bridge entry point:

```json
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "servers": {
      "furybuilder": {
        "type": "local",
        "command": [
          "C:\\Program Files\\nodejs\\node.exe",
          "C:\\path\\to\\FuryBuilder\\mcp\\bridge\\dist\\main.js"
        ],
        "environment": {
          "FURYBUILDER_MCP_RUNTIME": "C:\\Users\\<you>\\AppData\\Roaming\\com.furybuilder.desktop\\mcp\\runtime.json"
        },
        "codemode": false,
        "protocol": "auto",
        "timeout": {
          "startup": 20000,
          "catalog": 20000,
          "execution": 120000
        }
      }
    }
  }
}
```

- `codemode` must be `false` so the client lists and calls the tools directly.
- `protocol: "auto"` negotiates the modern era and falls back to the legacy lifecycle.
  Set `"legacy"` only when a client cannot negotiate `2026-07-28`.
- `execution` must outlast a write waiting for the confirmation dialog. The bridge itself
  waits up to 120 s for a write and the dialog expires after 90 s.
- `FURYBUILDER_MCP_RUNTIME` is optional when the app uses the default app-data directory on
  Windows. Keep it when `APPDATA` is redirected or when testing a custom data directory.

Restart OpenCode after editing the config so it launches a fresh bridge process.

### Other stdio clients

Use the same command and environment in the client's stdio-server schema:

```text
command: C:\Program Files\nodejs\node.exe
args:    C:\path\to\FuryBuilder\mcp\bridge\dist\main.js
env:     FURYBUILDER_MCP_RUNTIME=...   (optional)
```

Clients that call a shell to start servers should still receive the two arguments as
separate values. The bridge speaks MCP over stdin/stdout and writes diagnostics to stderr.

## First connection checklist

1. Start the desktop app and open a project.
2. Start the client with the configuration above.
3. Ask the agent to call `bridge_info` with the project ID it should use. A convenient
   choice is the ID from a previous `world_info`, or `local-project` for the editor's local
   project. The response includes `bridge.desktop.project.projectId`.
4. Confirm in the response:
   - `bridge.desktop.connected` is `true`;
   - `bridge.context.state` is `"ready"`;
   - `bridge.jobs.backendConnected` is `true`;
   - `request.protocolEra` matches the negotiated era (`modern` or `legacy`).
5. Call `world_info` and note `revision`, the target Minecraft version and the dimensions.
6. Call `get_block` or `inspect_region` for a small area. Their responses carry the same
   `worldRevision`, so the agent can prove which state it observed.
7. In read-only mode, ask for an `apply_operation`; it must return `permission_denied`.

If step 3 cannot connect, see the error table below before changing the client config.

## Tool catalog

Fifteen tools are registered:

| Group      | Tools                                                                                                      |
| ---------- | ---------------------------------------------------------------------------------------------------------- |
| Diagnostic | `bridge_info`                                                                                              |
| Read-only  | `world_info`, `get_block`, `get_region`, `inspect_region`, `get_camera`, `capture_viewport`                |
| Write      | `preview_operation`, `apply_operation`, `apply_operations_batch`, `undo_operation`, `get_operation_status` |
| Long jobs  | `start_job`, `get_job_status`, `cancel_job`                                                                |

`inspect_region` and `get_block` always answer at an explicit `worldRevision`. A supplied
`world_revision` that no longer matches is rejected with `revision_mismatch` instead of
silently reading newer data. `capture_viewport` returns a real PNG plus metadata; `partial`
means requested chunks were not loaded and `stale` means the rendered revision is older than
the authoritative world. Both can be `true`/`false` independently, and neither is an error.

`start_job` uses the same permissions and confirmation gate as a synchronous write. The
desktop keeps at most 16 active jobs and 256 retained records. If the Job API cannot be
reached, the three job tools return `job_api_unavailable` instead of fabricating a job; use
the synchronous tools in that case.

## Access modes and limits

Click **AI / MCP** in the FuryBuilder toolbar. The dialog says that the window is the
authority: the bridge cannot raise its own access.

- **Read-only** (default): every tool may read, but `apply_operation`,
  `apply_operations_batch`, `undo_operation` and write jobs return `permission_denied`.
- **Edit in selection**: writes and undos must stay inside one dimension and one inclusive
  box. The box volume must fit the current max-blocks limit when the policy is saved.
- **Full project**: the box restriction is removed, but the same per-operation and batch
  limits remain.

Defaults and hard caps:

| Setting                     | Default            | Hard cap  |
| --------------------------- | ------------------ | --------- |
| Max blocks per operation    | 50 000             | 1 000 000 |
| Max changes per batch       | 4 096              | 8 192     |
| Batch changes vs max blocks | batch ≤ max blocks | enforced  |

The policy lives in memory for the running desktop. Restarting FuryBuilder returns it to
read-only. Closing or opening a project does not preserve pending edits; it cancels active
MCP jobs. Saving a new policy also cancels active MCP jobs, because their approval belonged
to the previous policy. A write that was already committed is not rolled back by a later
policy change.

### Confirmation dialog

**Ask me to approve every apply and undo** is enabled by default. Before a write, the
editor shows **Approve an AI edit?** with the action (apply/undo), project, requested
revision, access mode, operation and bounds/blocks. Nothing is written until the user
clicks **Approve and apply**.

- **Deny** and a 90-second timeout both fail closed with `permission_denied`; the world is
  unchanged.
- If the editor UI is not running, the write cannot be approved and fails; the bridge never
  assumes consent.
- After approval the endpoint re-checks the policy and revision, so narrowing access while
  the dialog is open still blocks the write.

## Activity and audit

The **MCP activity** section in the bottom panel lists recent calls with request ID, client
ID, method, status, project, requested revision, operation ID and changed blocks. The
in-memory journal keeps the most recent 1 000 entries. Use it to answer “what did the agent
do?” after a session. It is not a permanent audit log; it is intentionally bounded.

## Typical errors and what to do

Errors from FuryBuilder tools are text of the form `code: message` (and the desktop endpoint
also returns structured `details`). The table lists the actionable ones.

| Message / code                  | Meaning                                                                                     | What to do                                                                                                                                |
| ------------------------------- | ------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| `runtime_file_missing`          | The desktop app is not running, `APPDATA` is missing, or the discovery file is unreadable.  | Start the FuryBuilder desktop app (not the browser preview) and retry. Check that the client runs as the same Windows user.               |
| `app_not_running`               | The pipe is closed or the discovery file is stale (often after a force-kill).               | Start the app again; it rewrites the discovery file. A stale file alone is harmless and does not grant access.                            |
| `invalid_runtime_file`          | The discovery file is not valid JSON or has an unsupported schema.                          | Restart the app. Do not hand-edit the file. If it persists, delete it while the app is closed and restart.                                |
| `authentication_failed`         | The bridge token does not match this app launch.                                            | Restart the MCP client so it starts a new bridge; the token changes on every app launch.                                                  |
| `protocol_mismatch`             | The desktop and bridge builds speak different endpoint versions.                            | Update/rebuild both from the same source (`pnpm --filter @furybuilder/mcp-bridge build`), then restart both.                              |
| `timeout`                       | The desktop did not answer in time. Reads use 5 s, writes up to 120 s.                      | Check that the confirmation dialog is open and answer it. For reads, open the project and keep the window responsive.                     |
| `desktop_error`                 | The endpoint answered with an unexpected failure or the pipe failed mid-request.            | Retry once; if it persists, restart the app and the client. The message carries the Rust error code when one exists.                      |
| `invalid_response`              | The endpoint returned malformed or oversized JSON, or closed without an answer.             | Restart both. If it repeats, capture `bridge_info` and the app version; do not hand-edit the discovery file.                              |
| `backend_unavailable`           | The app is reachable but the viewport is not attached, or the world service is unavailable. | Keep the desktop window open with a project loaded. Screenshots and camera require a live WebView; there is no headless capture path yet. |
| `project_mismatch`              | `project_id` is not the project open in the editor.                                         | Read `bridge.desktop.project.projectId` from `bridge_info` and use exactly that value.                                                    |
| `revision_mismatch`             | The supplied `world_revision` is stale.                                                     | Call `world_info`, re-run `inspect_region`/`preview_operation` at the new revision, then retry with the new `idempotency_key`.            |
| `permission_denied` (read-only) | A write or write job was attempted without edit access.                                     | Open **AI / MCP**, choose Edit in selection or Full project, save. For selection mode, ensure the operation is inside the box.            |
| `limit_exceeded`                | The operation or batch is larger than the configured/hard limit.                            | Shrink the area, split the batch, or raise the limit within the hard caps.                                                                |
| `out_of_bounds`                 | Coordinates are outside the dimension's Y range or world bounds.                            | Check `world_info.dimensions[].minY/maxY` and the target dimension.                                                                       |
| `job_api_unavailable`           | The desktop Job API is not connected.                                                       | Use synchronous write tools, or start/restart the desktop app.                                                                            |
| `no_history` / `not_found`      | The requested undo target or operation ID does not exist.                                   | Re-read `get_operation_status`/history; undo only a retained operation in the same dimension.                                             |
| `invalid_argument`              | Invalid coordinates, bounds, block state or schema payload.                                 | Read the message; fix the tool arguments. Bounds are inclusive and minimum must not exceed maximum.                                       |
| `cancelled`                     | The job was cancelled by the user, a policy change, or a project change.                    | Re-preview and restart if the work is still wanted. Nothing from a cancelled job is committed.                                            |

`partial: true` on a screenshot is not an error: it names the chunks that were not loaded
when the frame was rendered. `stale: true` means the viewport had not yet applied the latest
world revision; wait for the editor to refresh and capture again. If an AI provider itself
returns a quota or region error (for example HTTP 403), that error comes from the model
provider, not from FuryBuilder.

## Disabling access

1. Open **AI / MCP** and set the mode to **Read-only**. Save the policy. New writes are
   rejected immediately and active MCP jobs are cancelled; an open confirmation is denied by
   the post-approval re-check.
2. Close the project or quit FuryBuilder. The local endpoint disappears with the process,
   and every bridge call returns `app_not_running`/`runtime_file_missing`. A normal quit
   removes the discovery file; after a crash, starting the app again rewrites it.
3. Remove the `furybuilder` server entry from the MCP client configuration if you do not
   want the client to start the bridge at all. If a client keeps a bridge process alive,
   terminate that client or the process; bridges are ephemeral children of the client.
4. No firewall or network change is needed: access is a per-user named pipe, not a TCP
   listener. Revoking file access to `runtime.json` also prevents discovery, but the
   supported switch is the read-only policy plus closing the app.

There is no persistent “remember edit access” setting in this version. A restart always
returns to read-only, and the UI footer states that policy changes apply to the running
desktop only.

## Verification without a model

Two scripts exercise the path without asking a model to plan anything:

```powershell
# while the desktop app is running
node scripts/probe-local-bridge.mjs
node mcp/bridge/scripts/probe-live-observation.mjs
```

The first checks discovery, reads, a PNG capture, read-only refusal and the activity
journal. The second starts a real stdio bridge and verifies structured observations through
MCP: revision attribution, region palette totals, stale-revision rejection and screenshot
metadata/digest consistency. Both are read-only.

## Known limitations

- OpenCode 2.3.2 is the only real external client exercised. Tasks and Apps extensions are
  reported as options but not implemented; the fallback job tools are the supported path.
- Screenshots require a mounted, visible editor viewport. A minimized or headless window is
  not promised to capture.
- The observation API currently has one image mode (`viewport`) plus structured summary and
  block/region reads. Heightmap, top-down, material/biome maps, cross-sections and revision
  diffs are later M9 work.
- The discovery token relies on the per-user app-data directory ACL; stronger named-pipe
  ACLs remain a security review item.
- MCP access changes are not persisted across restarts, and the activity journal is
  in-memory and bounded rather than a permanent audit log.
