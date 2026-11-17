# ADR-016: Independent Rust Core boundary

- **Status:** Accepted
- **Date:** 2026-11-17

## Decision

`world-domain`, `world-operations`, `world-io` and `world-generation` must not depend on
Tauri, React, Three.js or the operating system UI. They are ordinary Rust library crates
with deterministic tests and no global mutable state.

The dependency direction is:

```text
world-domain        <- world-operations <- world-application <- desktop-adapter (Tauri)
      ^                     ^                    ^
      |                     |                    +-- mcp-adapter / stdio bridge
      +---- world-io -------+---- world-generation
```

- `world-application` owns projects, policy, jobs, audit and the command/query facade.
- Tauri commands are thin adapters: parse/validate IPC input, call the application API,
  map structured errors to responses.
- The MCP bridge calls the same application API through the local endpoint; it never
  touches `.mca`/NBT files or renderer state directly.
- Renderer snapshots are derived data; the renderer cannot mutate the world.

## Enforcement

- Crate manifests contain no UI/desktop dependencies.
- CI runs `cargo test -p world-domain` etc. without Tauri features.
- Contract tests verify that UI/MCP operations produce identical results for the same
  command.
