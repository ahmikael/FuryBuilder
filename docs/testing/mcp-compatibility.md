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

## Automated evidence

Command:

```powershell
pnpm --filter @furybuilder/mcp-bridge test
pnpm --filter @furybuilder/mcp-bridge typecheck
```

Observed: 3 tests passed, typecheck clean. The tests spawn the real bridge over
stdio with `tsx src/main.ts` and use `@modelcontextprotocol/client` 2.4.0:

- a client pinned to `2026-07-28` connects in the modern era, lists `bridge_info`
  and receives an explicit project/revision response;
- a default client connects through the legacy lifecycle and calls the same tool;
- a call without `project_id` is rejected, and consecutive calls with different
  project IDs do not inherit one another's context.

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
- Tasks/Apps negotiation is deliberately absent until those extensions are
  implemented and tested; clients must use ordinary tools.
- This evidence does not establish end-to-end AI editing or real-world
  compatibility.
