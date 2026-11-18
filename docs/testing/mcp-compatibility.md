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

Initial methods: `ping`, `world_info`, `query`, `project_status`. World methods require an
explicit `project_id` matching the project currently open in the editor; `query` reuses the
same bounded `Query` enum as the UI. Write methods are deliberately not exposed yet.

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

Observed: 15 tests passed, typecheck and build clean. The tests spawn the real bridge
over stdio with `tsx src/main.ts` and use `@modelcontextprotocol/client` 2.4.0:

- a client pinned to `2026-07-28` connects in the modern era, lists `bridge_info`
  and receives an explicit project/revision response;
- a default client connects through the legacy lifecycle and calls the same tool;
- a call without `project_id` is rejected, and consecutive calls with different
  project IDs do not inherit one another's context;
- the three fallback job tools are listed and return `job_api_unavailable` instead
  of a fabricated job;
- the Tasks policy enables the extension only when client, bridge and Job API all
  support it;
- `bridge_info` reports `runtime_file_missing` when the desktop app is absent and a
  live project/revision when a local endpoint answers;
- the desktop client maps a wrong token to `authentication_failed`, a stale endpoint
  to `app_not_running`, malformed discovery files to `invalid_runtime_file`, and a
  silent endpoint to `timeout`.

### Real Tauri process

`scripts/probe-local-bridge.mjs` was run against a real `tauri dev` process:

```text
discovery: schemaVersion 1, local_socket, protocol 1, named pipe, tokenLength 64
ping: ok, projectId "local-project", worldRevision 0, app 0.1.0
world_info: ok, target Minecraft 1.21.4, revision 0, 3 dimensions
wrong token: unauthorized, responseContainsToken false
LOCAL_BRIDGE_PROBE_OK
```

The probe writes nothing and does not print the token. The app was force-stopped after the
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

- `bridge_info` is a handshake diagnostic, not an editor tool. Project/world tools
  arrive in the following M7 tasks.
- Only `ping`, `world_info`, `query` and `project_status` are exposed; writes and the
  Job API are still absent, so an AI cannot edit through MCP yet.
- Unix domain socket support exists only as unexercised code; Windows is the tested target.
- The discovery token relies on the per-user application data directory ACL; stronger
  named-pipe ACLs are still a security review item.
- No real external MCP client (OpenCode/Claude Desktop) has connected yet.
- This evidence does not establish end-to-end AI editing or real-world compatibility.
