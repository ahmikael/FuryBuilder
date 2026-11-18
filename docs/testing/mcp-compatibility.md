# MCP compatibility

Status: protocol negotiation implemented and tested locally; real external client
connection is still outstanding.

## Bridge baseline

| Item                | Value                                                             |
| ------------------- | ----------------------------------------------------------------- |
| Bridge package      | `mcp/bridge`, `@furybuilder/mcp-bridge` 0.1.0                     |
| SDK                 | `@modelcontextprotocol/server` 2.4.0 (pinned)                     |
| Modern revision     | MCP `2026-07-28`                                                  |
| Legacy fallback     | 2025-era clients served from the same factory                     |
| Transport           | stdio only; no network listener                                   |
| Context policy      | explicit per request; no project/selection/job in transport state |
| Optional extensions | Tasks and Apps are **not** advertised yet                         |
| Job tools           | `start_job`, `get_job_status`, `cancel_job` always registered     |

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

Observed: 8 tests passed, typecheck and build clean. The tests spawn the real bridge
over stdio with `tsx src/main.ts` and use `@modelcontextprotocol/client` 2.4.0:

- a client pinned to `2026-07-28` connects in the modern era, lists `bridge_info`
  and receives an explicit project/revision response;
- a default client connects through the legacy lifecycle and calls the same tool;
- a call without `project_id` is rejected, and consecutive calls with different
  project IDs do not inherit one another's context;
- the three fallback job tools are listed and return `job_api_unavailable` instead
  of a fabricated job;
- the Tasks policy enables the extension only when client, bridge and Job API all
  support it.

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
- The bridge does not yet connect to the Tauri Application API.
- Tasks is not advertised; the fallback job tools currently refuse with
  `job_api_unavailable` until the Core Job API is connected.
- This evidence does not establish end-to-end AI editing or real-world
  compatibility.
