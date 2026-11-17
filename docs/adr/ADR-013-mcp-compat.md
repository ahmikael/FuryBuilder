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

## Consequences

- Tool handlers are pure request/response and safe to retry with idempotency keys.
- `get_operation_status` is authoritative for long-running work.
- Compatibility must be tested with at least one real client (OpenCode) and recorded in
  `docs/testing/mcp-compatibility.md`.
