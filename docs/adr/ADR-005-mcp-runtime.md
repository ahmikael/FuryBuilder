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

## Rationale

External AI clients start stdio servers easily. A sidecar on the official TS SDK avoids
coupling MCP protocol churn to the Rust core, while keeping every write behind the same
validation, transaction and audit path as the UI.

## Consequences

- The bridge is stateless with respect to project context: every call carries
  `project_id`, `world_revision` where relevant, and explicit selection/job IDs.
- Authentication failure and app-not-running states produce actionable errors.
- MCP cannot access arbitrary file paths or shell commands.
