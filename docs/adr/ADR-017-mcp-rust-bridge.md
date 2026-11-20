# ADR-017: Rust MCP bridge

- **Status:** Accepted
- **Date:** 2026-11-20
- **Supersedes:** the implementation-language decision in ADR-005 and ADR-013

## Context

ADR-005 chose a thin TypeScript stdio adapter on the official MCP SDK. That made the
first protocol slice quick to deliver, but it added a second runtime to the product:
the installer had to ship or require Node.js, the bridge duplicated validation already
enforced by Rust, and protocol churn was split across two languages.

The official Rust SDK, `rmcp`, now implements the targeted `2026-07-28` stateless
lifecycle (`server/discover`, per-request `_meta`), the legacy `initialize` handshake,
tool/resource/prompt handlers, MCP Apps extension capabilities and stdio transport.

## Decision

- The MCP server is a Rust crate, `crates/mcp-bridge`, built as the
  `furybuilder-mcp` binary with `rmcp` 3.11.
- The binary is bundled by Tauri as an `externalBin` and installed next to the desktop
  executable. Users and MCP clients need no Node.js runtime.
- The bridge remains a **thin adapter**: it validates and normalizes arguments, forwards
  requests to the authenticated local endpoint, and formats results. It owns no project,
  world, job or policy state.
- The existing local IPC contract, discovery file, token handling, timeouts, response
  size cap, tool names, JSON payloads, prompts and MCP Apps resource stay unchanged so
  existing clients keep working.
- Protocol behavior is tested by spawning the real binary over stdio in both the modern
  and legacy eras, including a mock named-pipe desktop endpoint.

## Consequences

- One language and one dependency policy cover the desktop, core and MCP bridge.
- The installer no longer depends on Node.js and can run the sidecar offline.
- The TypeScript bridge package, its lockfile entries and its Node license inventory are
  removed. Historical evidence produced by that bridge remains as recorded provenance.
- The sidecar must be built for the same target triple as the desktop app. Release
  tooling stages it at `apps/desktop/src-tauri/binaries/furybuilder-mcp-<triple>` before
  the Tauri bundle step.
- A protocol change now requires a Rust dependency update, not a parallel TypeScript
  implementation.
