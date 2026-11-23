# Architecture

FuryBuilder is a Windows desktop editor with an independent Rust core. The desktop UI and
external MCP clients are both clients of the same Rust application API; the renderer is a
derived view and never owns world data.

```text
world-domain <- world-operations <- world-application <- desktop-adapter (Tauri)
      ^                ^                    ^
      |                |                    +-- mcp-bridge (stdio sidecar)
      +-- world-io ----+---- world-generation
```

`world-domain`, `world-operations`, `world-io` and `world-generation` must not depend on
Tauri, React, Three.js or OS UI. `world-application` owns projects, policy, jobs, audit and
the command/query facade. Tauri commands are thin adapters. The MCP bridge calls the same
application API over an authenticated local endpoint and never opens world files directly.

## Decisions

| ID      | Decision                                                                                                                                  |
| ------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| ADR-001 | First and only write-verified target is Minecraft Java **1.21.4**, `DataVersion` 4189. Other versions are detected and refused.           |
| ADR-002 | A project is a folder `*.furybuilder/` with `project.json`, `source/`, `data/`, `changes/`, `cache/`; atomic saves and exact schema versions. |
| ADR-003 | Meshing runs in the TypeScript renderer; Rust produces compact section snapshots and dirty-chunk events.                                  |
| ADR-004 | Assets resolve from launcher discovery, manual selection or built-in placeholders; resource packs form an ordered validated stack.        |
| ADR-005 | MCP is a local stdio adapter over the same Application API; no world logic in the bridge, no network port by default.                     |
| ADR-006 | Renderer baseline is Three.js `WebGLRenderer` (WebGL 2) in an imperative scene module; one geometry per section.                           |
| ADR-007 | First interchange format is Java structure `.nbt`; Sponge `.schem` v3 is second; the native project format is primary.                    |
| ADR-008 | Undo/redo stores reversible diffs of modified sections; checkpoints are periodic full snapshots of touched sections.                      |
| ADR-009 | MVP renders a curated vanilla-compatible model set with placeholder textures; unknown models fall back visually without changing data.    |
| ADR-010 | Ship Windows first: Tauri 2, per-user NSIS installer, WebView2 check. macOS/Linux come after Windows stability.                            |
| ADR-011 | Block states in `world-domain` are authoritative; parametric objects are an optional overlay that compiles into voxel operations.          |
| ADR-012 | Build DSL is a typed JSON/AST schema, not an interpreter. Observation and validation outputs are revisioned and completeness-aware.       |
| ADR-013 | MCP targets the stateless `2026-07-28` core and keeps the legacy initialization lifecycle; state is never bound to a transport session.   |
| ADR-014 | Three creation modes only: `Empty Project`, `Import World` and `Vanilla World` (official local server, explicit EULA).                    |
| ADR-015 | Java, client assets and the official server JAR are discovered first and provisioned only after explicit consent through one component facade, with checksum verification. |
| ADR-016 | Rust core crates stay independent of UI/desktop dependencies; UI and MCP produce identical operations for the same command.                |
| ADR-017 | The MCP bridge is a Rust `rmcp` sidecar bundled next to the desktop executable; no Node.js runtime is required.                           |

## Project format

A project folder contains `project.json`, `source/` (immutable imported source), `data/`
(versioned world snapshots), `changes/` (journal and checkpoints), `cache/` (regenerable)
and optional `assets/` and `previews/`. `project.json` is the only eagerly parsed document;
it stores schema version, project id, name, creation mode, target version profile
(version/DataVersion/Java/pack formats), component references, world revision and provenance.

Saves are atomic at the project level: stage, verify, publish, keep the previous manifest.
The MVP keeps one deterministic whole-world snapshot per committed revision, the newest
eight, plus autosave and rolling checkpoints. Project schema and snapshot format versions are
exact: FuryBuilder reads only the current versions and refuses older or newer files without
rewriting them.

## MCP boundary

- The desktop process creates a per-launch named pipe and a user-private discovery file
  with a 64-character token. The bridge reads the file, connects, and reports desktop
  status; it never opens project or world files.
- Every request carries `project_id` and, where relevant, `world_revision`, selection or
  job ids. There is no session-scoped project state.
- Preview validates and counts changes without mutating. Apply and undo enforce the
  process-wide access policy, which only the Tauri UI can change. The default is
  `read_only` with confirmation enabled.
- Long work goes through a bounded single-worker job queue. Jobs are idempotent on
  `(project_id, idempotency_key)` plus a payload fingerprint.
- MCP writes are recorded in the same history and audit trail as UI operations, with actor
  and source attribution. The activity journal is metadata-only and bounded.
- Tools expose no filesystem, shell or network access. Camera captures require a visible
  viewport and report revision, completeness and staleness.

## Renderer and assets

The renderer owns no block data. It receives palette plus packed indices, builds one merged
geometry per section, separates opaque/cutout/translucent materials, rebases local origins
around the camera and disposes GPU resources explicitly. React owns only the surrounding
chrome.

Assets come from the user's installed 1.21.4 client JAR/assets or a selected resource pack.
A missing asset falls back to a clearly marked placeholder and never changes block data.
Mojang JARs, textures and assets are not redistributed. Atlas caches are keyed by version,
asset fingerprint, pack-stack hash and renderer schema.

## Security boundaries

Imported files and MCP clients are untrusted input. The desktop process is authoritative.

- World import accepts classic Anvil 1.21.4 only: bounded `level.dat`, region files and
  chunk counts; external `.mcc`, LZ4 and custom compression are refused; offsets and
  sector lengths are validated.
- Passthrough and export refuse symlinks, junctions and reparse points; the destination
  must be new and outside the source; copied bytes are hashed and re-read.
- NBT has hard limits on compressed/decompressed size, depth, collection length and name
  length; lengths are checked against remaining bytes before allocation.
- Archives (JAR/ZIP/resource packs/datapacks) are parsed in memory, never extracted, and
  reject traversal, absolute paths, NUL bytes, symlinks and oversized entries.
- MCP writes default to read-only, enforce block/batch limits, require a desktop
  confirmation unless disabled, and re-check policy and revision after approval.
- The local OS account and the MCP client's own stdin memory are outside the threat model.

## Dependency policy

Dependencies are pinned by `Cargo.lock` and `pnpm-lock.yaml`. No crate enters a domain crate
if it pulls in Tauri, an async runtime or filesystem policy that belongs to an adapter.
Every new dependency is checked for license compatibility and security advisories.
NBT parsing is implemented and verified against round-trip fixtures first; a third-party
parser is adopted only if it preserves unknown tags or is used strictly read-only.
Generated contracts must be versioned; no duplicated hand-maintained Rust/TypeScript
contracts.
