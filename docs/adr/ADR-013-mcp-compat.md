# ADR-013: MCP 2026-07-28 compatibility strategy

- **Status:** Accepted
- **Date:** 2026-11-17

## Decision

- Target the **stateless core** of MCP revision **2026-07-28** through the official
  TypeScript SDK v2 (`@modelcontextprotocol/server`) in the stdio bridge.
- Do **not** bind project, selection or job state to a transport session. Every request
  carries explicit identifiers (`project_id`, `world_revision`, `selection_id`, `job_id`,
  `snapshot_id`).
- Negotiate capabilities on connect. Optional extensions are used only after negotiation:
  - `io.modelcontextprotocol/tasks` for long jobs, with a fallback of ordinary
    `start_job` / `get_job_status` / `cancel_job` tools over the internal Job API.
  - MCP Apps (`ui://`) only for clients that advertise support; screenshots and
    structured resources remain the mandatory fallback.
- Legacy clients that use the older initialization lifecycle still work; the bridge
  reports the negotiated protocol version and never assumes Tasks/Apps.
- Multi round-trip approval is expressed through tool results that request approval and
  the Core policy layer — an AI's textual "yes" is never treated as authorization.

## Implementation note (2026-11-18)

Verified `@modelcontextprotocol/server` **2.4.0** and its `serveStdio(factory)` entry.
The package serves both protocol eras, but modern `2026-07-28` behaviour is an explicit
choice: a hand-built server keeps the legacy `initialize` lifecycle, while modern peers
open with `server/discover` and per-request `_meta`. The bridge must therefore select and
record the negotiated era. The Tasks extension is no longer part of the core protocol;
its 2025 wire vocabulary is deprecated, and the extension uses `tasks/get`,
`tasks/update` and `tasks/cancel` with ordinary start/status/cancel tools as fallback.
The installed OpenCode CLI `2.3.0` supports local stdio servers via
`opencode mcp add <name> -- <command...>`. The real-client negotiation test is still
outstanding and must not be claimed before it runs.

## Consequences

- Tool handlers are pure request/response and safe to retry with idempotency keys.
- `get_operation_status` is authoritative for long-running work.
- Compatibility must be tested with at least one real client (OpenCode) and recorded in
  `docs/testing/mcp-compatibility.md`.
