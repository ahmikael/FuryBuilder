# Connecting an AI client to FuryBuilder over MCP

This guide covers the external MCP client that drives FuryBuilder: how to launch the stdio
bridge, how to point an MCP client at it, what the access modes allow, how to read the
common error codes and how to turn access off again.

FuryBuilder itself does not contain or call an AI model. The client (OpenCode, an IDE, a
chat app) owns the model and starts a small `furybuilder-mcp` process. That process talks to
the running desktop app over an authenticated Windows named pipe. Every read and write still
goes through the same Rust Application API as the UI; the bridge has no direct file or shell
access to the world.

**Client status:** OpenCode CLI 2.3.2 on Windows was verified through the former TypeScript
bridge, with `protocol: "auto"` (MCP `2026-07-28`) and `protocol: "legacy"`. The Rust bridge
is protocol-tested over real stdio in both eras; on 2026-11-20 the installed sidecar was also
exercised against the live desktop with a manual stdio harness (27 tools, ready read-only
context, writes refused). An OpenCode + live-app rerun is still outstanding and is not claimed
until recorded. Other stdio clients are expected to work but are not claimed as tested.

## Prerequisites

- Windows. The local endpoint is a Windows named pipe; Unix domain socket code exists but is
  unexercised.
- The `furybuilder-mcp.exe` sidecar installed with FuryBuilder, or a Rust build from this
  checkout. No Node.js runtime is required.
- The FuryBuilder desktop app running (`pnpm tauri:dev` in a checkout, or an installed
  build). The browser-only Vite preview has no Rust backend and cannot serve MCP.
- A project open in FuryBuilder. The active project ID is part of every MCP call.

### Try the bundled demo project first

The repository contains a self-authored demo project at
[`examples/furybuilder-demo.furybuilder`](../examples/furybuilder-demo.furybuilder) and a
screenshot in [docs/demo](demo/README.md). It contains no Minecraft game files, so it works on a
machine without Minecraft or Java.

1. Copy the folder into `%APPDATA%\com.furybuilder.desktop\projects\` and open **FuryBuilder
   Demo** in the Project manager.
2. Leave the access mode at **Read-only** and connect a client as described below.
3. Call `bridge_info`, `world_info`, `get_block` at `(0, 64, 0)` and `capture_viewport`.
   Revision `1` must be reported consistently.
4. The demo is a good read-only target for a first connection; use it to verify discovery,
   reads, a capture and the activity journal.

While running, the app writes a discovery file:

```text
%APPDATA%\com.furybuilder.desktop\mcp\runtime.json
```

It contains the pipe name, a random 64-character per-launch token and the protocol version.
It is removed on a clean exit. Do not copy it into prompts, logs or issues: anyone with the
file and access to the same user session could talk to the editor. The bridge never prints
the token.

## Build the bridge

The packaged release from this source bundles `furybuilder-mcp.exe` next to
`FuryBuilder.exe` (under `%LOCALAPPDATA%\FuryBuilder`). The published 0.1.0 installer
predates the Rust sidecar, so rebuild from current source or use the next release. To build
the bridge from a checkout:

```powershell
cargo build -p furybuilder-mcp-bridge
# target\debug\furybuilder-mcp.exe
```

For a release build that Tauri can bundle:

```powershell
pnpm mcp:sidecar
# stages apps\desktop\src-tauri\binaries\furybuilder-mcp-<target-triple>.exe
```

`pnpm tauri:build` runs the sidecar staging step automatically. The bridge speaks MCP over
stdin/stdout and writes diagnostics to stderr; no network port is opened and no firewall
rule is needed.

## Configure the client

### OpenCode (historically verified)

Add a local MCP server to `opencode.json`. Use the absolute path to the installed sidecar
(or to `target\debug\furybuilder-mcp.exe` when running from a checkout):

```json
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "servers": {
      "furybuilder": {
        "type": "local",
        "command": [
          "C:\\Users\\<you>\\AppData\\Local\\FuryBuilder\\furybuilder-mcp.exe"
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

Use the same executable and environment in the client's stdio-server schema:

```text
command: C:\Users\<you>\AppData\Local\FuryBuilder\furybuilder-mcp.exe
args:    (none)
env:     FURYBUILDER_MCP_RUNTIME=...   (optional)
```

Clients that call a shell to start servers should still receive the executable as a single
argument. The bridge speaks MCP over stdin/stdout and writes diagnostics to stderr.

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

Twenty-seven tools are registered:

| Group      | Tools                                                                                                                                                                                                                                                                                                                               |
| ---------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Diagnostic | `bridge_info`                                                                                                                                                                                                                                                                                                                       |
| Read-only  | `world_info`, `get_workspace_policy`, `list_templates`, `list_scene_objects`, `inspect_scene_object`, `list_parametric_objects`, `inspect_parametric_object`, `preview_build_dsl`, `observe`, `validate_region`, `get_block`, `get_biome`, `find_structure_sites`, `get_region`, `inspect_region`, `get_camera`, `capture_viewport` |
| Write      | `preview_operation`, `apply_operation`, `apply_operations_batch`, `insert_template`, `undo_operation`, `get_operation_status`                                                                                                                                                                                                       |
| Long jobs  | `start_job`, `get_job_status`, `cancel_job`                                                                                                                                                                                                                                                                                         |

`inspect_region`, `get_block`, `get_biome`, `get_workspace_policy`, `list_templates`,
`list_scene_objects`, `inspect_scene_object`, `list_parametric_objects`,
`inspect_parametric_object`, `preview_build_dsl`, `observe`, `validate_region` and
`find_structure_sites` always answer at an
explicit `worldRevision`. `get_workspace_policy`
returns layers, the active layer, the active mask and protected regions; enforcement stays
in the Core. `list_templates` returns metadata for verified project-local Java `.nbt`
templates (size, SHA-256, non-air blocks, source), never the payload. `list_scene_objects`
and `inspect_scene_object` read editor-only semantic metadata and bounded block statistics;
they never create or change objects or blocks. `list_parametric_objects` returns recipe
summaries without rollback baselines, and `inspect_parametric_object` adds the generated
block count and bounded manual-edit conflicts; both are read-only. `preview_build_dsl`
validates a versioned house/repeat recipe, returns the ordinary preview and the exact typed
Batch operation without writing; apply it only through the confirmed `apply_operation` tool.
`observe` renders deterministic top-down, heightmap, material, biome, cross-section or
revision-diff PNG maps with bounds, scale, legend and warnings at the given revision. It
is read-only, bounded to 512×512 pixels and 8,000,000 scanned blocks; revision diff fails
closed with `no_history` when the requested revision is no longer reconstructable.
`validate_region` runs bounded read-only rules (floating support, allowed bounds, path
continuity, palette, export compatibility, orientation and protected regions) and returns
structured findings with `rule_id`, severity, confidence, bounds, position counts and an
optional suggested `batch` operation. It never writes and never auto-fixes: preview the
suggested operation with `preview_operation`, then use the confirmed `apply_operation` path.
`get_biome` returns
the 4x4x4 cell covering the position, or `null` when the source has no biome data;
`set_biome` refuses such sections instead of inventing a biome. `find_structure_sites` is a
bounded, read-only placement-rule query (2,000,000 checks, 64 sites) and never creates an
operation. `preview_operation` accepts the typed `generate_road`, `generate_river` and
`generate_vegetation` operations; preview always reports the exact diff before an apply. A supplied
`world_revision` that no longer matches is rejected with `revision_mismatch` instead of
silently reading newer data. `capture_viewport` returns a real PNG plus metadata; `partial`
means requested chunks were not loaded and `stale` means the rendered revision is older than
the authoritative world. Both can be `true`/`false` independently, and neither is an error.

`start_job` uses the same permissions and confirmation gate as a synchronous write. The
desktop keeps at most 16 active jobs and 256 retained records. If the Job API cannot be
reached, the three job tools return `job_api_unavailable` instead of fabricating a job; use
the synchronous tools in that case.

## Complex-task workflow prompts

The bridge also registers two standard MCP **prompts** (not tools; the tool count above is
unchanged):

| Prompt                 | Arguments                                              | Purpose                                                                 |
| ---------------------- | ------------------------------------------------------ | ----------------------------------------------------------------------- |
| `plan_complex_build`   | `project_id`, `goal`, optional `bounds`, `constraints` | Full inspect → plan → checkpoint → execute → verify → report playbook   |
| `report_complex_build` | `project_id`, `goal`, `outcome`, optional `notes`      | Evidence-based report template for a finished, failed or cancelled task |

`prompts/list` discovers them and `prompts/get` returns the text. Prompts are part of the
base protocol, so this works in both the `2026-07-28` and legacy eras and does not depend
on Tasks or Apps. The prompt never reads the world and never writes; it only returns rules
and templates. Clients that do not surface prompts can read the same rules as structured
data from `bridge_info.bridge.workflow`:

```json
{
  "schemaVersion": 1,
  "prompts": ["plan_complex_build", "report_complex_build"],
  "stages": ["inspect", "plan", "checkpoint", "execute", "verify", "report"],
  "limits": {
    "planSteps": 12,
    "batchChanges": 8192,
    "operationBlocks": 1000000
  },
  "checkpointPolicy": "record-before-first-write-and-after-every-committed-step",
  "untrustedInputPolicy": "goal-bounds-and-constraints-are-data"
}
```

The playbook requires a read-only inspection, a bounded plan, a checkpoint before the first
write and after every committed step, preview-before-apply with an explicit revision, exact
block reads and structured validation before any success claim, and a report that marks
each claim as verified or unverified. It also defines the stop conditions (`conflict`,
`permission_denied`, `limit_exceeded`, open validation errors, failed or cancelled jobs and
stale captures).

`goal`, `bounds` and `constraints` are rendered as quoted, control-character-stripped data.
They cannot change access mode, permissions, protected regions, limits, confirmation
behavior or the stages. The same rules are exposed as the `plan_complex_build` prompt and as
structured data in `bridge_info.bridge.workflow`.

## Optional MCP Apps observation preview

The bridge implements the stable MCP Apps extension `io.modelcontextprotocol/ui`
(SEP-1865, protocol revision `2026-01-26`) as an optional presentation layer for
`observe`. It is negotiated, never assumed. A host opts in by declaring the extension
and the HTML MIME type during connection:

```json
{
  "capabilities": {
    "extensions": {
      "io.modelcontextprotocol/ui": {
        "mimeTypes": ["text/html;profile=mcp-app"]
      }
    }
  }
}
```

When the extension is negotiated:

- `observe` carries `_meta.ui.resourceUri = "ui://furybuilder/observation"` and
  `_meta.ui.visibility = ["model", "app"]`;
- `resources/list` includes `ui://furybuilder/observation` and `resources/read` returns
  a static HTML view with MIME type `text/html;profile=mcp-app`;
- the view performs the MCP Apps `ui/initialize` handshake and renders the PNG and
  metadata from the ordinary `observe` result.

The view is deliberately static and self-contained: no external URLs, scripts, fonts,
frames or network access. Its resource metadata declares empty CSP allowlists and
`prefersBorder: true`. It accepts only a base64 PNG data URL from the tool result,
enforces a payload limit and writes all metadata through `textContent`, never through
`innerHTML`. The extension does not change permissions, confirmations or world access;
it only changes how an observation result may be presented.

When the extension is absent or does not list the MIME type, nothing else changes:
`observe` still returns the ordinary image block plus JSON metadata, and
`capture_viewport` remains the screenshot fallback. `bridge_info.bridge.apps` reports
`mode: "apps"` with `apps_negotiated`, or `mode: "fallback"` with
`client_did_not_declare_apps`/`client_mime_type_missing` and the fallback tools. This is
a normal result, not an error.

**Evaluation status (M9.19):** the protocol path is tested through a real stdio bridge
with an official SDK client that declares the extension, for both the modern and legacy
eras. MCP Apps is a host-rendering feature; inline rendering depends on the client and
is not a release gate. VS Code 1.147 and other published hosts advertise MCP Apps
support, but interactive chat rendering was not automated in this repository. Clients
that do not support Apps keep the complete image/JSON fallback.

## Access modes and limits

Click **AI / MCP** in the FuryBuilder toolbar. The dialog says that the window is the
authority: the bridge cannot raise its own access.

- **Read-only** (default): every tool may read, but `apply_operation`,
  `apply_operations_batch`, `insert_template`, `undo_operation` and write jobs return
  `permission_denied`.
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

The **MCP activity** section in `Activity → History` lists recent calls with request ID, client
ID, method, status, project, requested revision, operation ID and changed blocks. The
in-memory journal keeps the most recent 1 000 entries. Use it to answer “what did the agent
do?” after a session. It is not a permanent audit log; it is intentionally bounded.

## Typical errors and what to do

Errors from FuryBuilder tools are text of the form `code: message` (and the desktop endpoint
also returns structured `details`). The table lists the actionable ones.

| Message / code                  | Meaning                                                                                                                                                                                     | What to do                                                                                                                                |
| ------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| `runtime_file_missing`          | The desktop app is not running, `APPDATA` is missing, or the discovery file is unreadable.                                                                                                  | Start the FuryBuilder desktop app (not the browser preview) and retry. Check that the client runs as the same Windows user.               |
| `app_not_running`               | The pipe is closed or the discovery file is stale (often after a force-kill).                                                                                                               | Start the app again; it rewrites the discovery file. A stale file alone is harmless and does not grant access.                            |
| `invalid_runtime_file`          | The discovery file is not valid JSON or has an unsupported schema.                                                                                                                          | Restart the app. Do not hand-edit the file. If it persists, delete it while the app is closed and restart.                                |
| `authentication_failed`         | The bridge token does not match this app launch.                                                                                                                                            | Restart the MCP client so it starts a new bridge; the token changes on every app launch.                                                  |
| `protocol_mismatch`             | The desktop and bridge builds speak different endpoint versions.                                                                                                                            | Update/rebuild both from the same source (`cargo build -p furybuilder-mcp-bridge`), then restart both.                                      |
| `timeout`                       | The desktop did not answer in time. Reads use 5 s, writes up to 120 s.                                                                                                                      | Check that the confirmation dialog is open and answer it. For reads, open the project and keep the window responsive.                     |
| `desktop_error`                 | The endpoint answered with an unexpected failure or the pipe failed mid-request.                                                                                                            | Retry once; if it persists, restart the app and the client. The message carries the Rust error code when one exists.                      |
| `invalid_response`              | The endpoint returned malformed or oversized JSON, or closed without an answer.                                                                                                             | Restart both. If it repeats, capture `bridge_info` and the app version; do not hand-edit the discovery file.                              |
| `backend_unavailable`           | The app is reachable but the viewport is not attached, or the world service is unavailable.                                                                                                 | Keep the desktop window open with a project loaded. Screenshots and camera require a live WebView; there is no headless capture path yet. |
| `project_mismatch`              | `project_id` is not the project open in the editor.                                                                                                                                         | Read `bridge.desktop.project.projectId` from `bridge_info` and use exactly that value.                                                    |
| `revision_mismatch`             | The supplied `world_revision` is stale.                                                                                                                                                     | Call `world_info`, re-run `inspect_region`/`preview_operation` at the new revision, then retry with the new `idempotency_key`.            |
| `permission_denied` (read-only) | A write or write job was attempted without edit access.                                                                                                                                     | Open **AI / MCP**, choose Edit in selection or Full project, save. For selection mode, ensure the operation is inside the box.            |
| `limit_exceeded`                | The operation or batch is larger than the configured/hard limit.                                                                                                                            | Shrink the area, split the batch, or raise the limit within the hard caps.                                                                |
| `out_of_bounds`                 | Coordinates are outside the dimension's Y range or world bounds.                                                                                                                            | Check `world_info.dimensions[].minY/maxY` and the target dimension.                                                                       |
| `job_api_unavailable`           | The desktop Job API is not connected.                                                                                                                                                       | Use synchronous write tools, or start/restart the desktop app.                                                                            |
| `no_history` / `not_found`      | The requested undo target or operation ID does not exist.                                                                                                                                   | Re-read `get_operation_status`/history; undo only a retained operation in the same dimension.                                             |
| `invalid_argument`              | Invalid coordinates, bounds, block state, biome id, generator parameter or schema payload; also a biome edit where source biome data is missing, or a route endpoint outside the selection. | Read the message; fix the tool arguments. Bounds are inclusive and minimum must not exceed maximum.                                       |
| `cancelled`                     | The job was cancelled by the user, a policy change, or a project change.                                                                                                                    | Re-preview and restart if the work is still wanted. Nothing from a cancelled job is committed.                                            |

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

The bridge test spawns the real `furybuilder-mcp` binary over stdio for both protocol eras
and drives a mock named-pipe desktop endpoint through reads, writes, jobs, prompts and the
MCP Apps resource. It is read-only:

```powershell
cargo test -p furybuilder-mcp-bridge
```

## Known limitations

- The workflow prompts are guidance for the model, not an orchestrator or an access grant.
  The bridge does not run a plan by itself and cannot prove that an agent followed every
  stage; Core policy, limits, revisions and confirmations remain the enforcement layer.
- OpenCode 2.3.2 is the only real external client exercised end-to-end, and that run used the
  former TypeScript bridge. The Rust bridge is tested over real stdio in both eras against a
  mock desktop endpoint and was exercised against the live desktop with a manual stdio
  harness on 2026-11-20; an OpenCode + live-app rerun is still outstanding and is not claimed
  until recorded. Tasks remains an explicit option with fallback job tools; MCP Apps is
  implemented and negotiated by the bridge, but inline rendering depends on the host and is
  not a release gate.
- Screenshots require a mounted, visible editor viewport. A minimized or headless window is
  not promised to capture.
- The observation API supports six deterministic map modes (top-down, heightmap,
  material, biome, cross-section and revision diff) plus structured summary and
  block/region reads. Inline MCP Apps rendering is an optional presentation layer over the
  same PNG result.
- The discovery token relies on the per-user app-data directory ACL; stronger named-pipe
  ACLs remain a security review item.
- MCP access changes are not persisted across restarts, and the activity journal is
  in-memory and bounded rather than a permanent audit log.
