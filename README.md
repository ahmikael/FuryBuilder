# FuryBuilder

FuryBuilder is an offline-first 3D studio for creating, inspecting, editing and exporting
Minecraft Java Edition worlds and structures. It is a desktop editor, not a game client:
human users work through the UI, and external AI agents work through the same
Application API over MCP.

- **Platform:** Windows first (Tauri 2 + WebView2), macOS/Linux later.
- **Target game version:** Minecraft Java **1.21.4** (`DataVersion` 4189, Java 21).
- **Project modes:** `Empty Project` (MVP), `Vanilla World` and `Import World` phased in later.
- **License:** MIT for FuryBuilder code. Minecraft game assets are **not** distributed with
  this repository; see `docs/legal/third-party-notices.md`.

## Repository layout

```text
apps/desktop/          React + TypeScript + Vite UI and Tauri shell
crates/world-domain/   Block states, coordinates, chunks, sections, revisions
crates/world-operations/ Selection, fill/replace, transactions, undo/redo
crates/world-io/       Versioned NBT / structure / Anvil adapters
crates/world-generation/ Deterministic local terrain and object generators
crates/world-application/ Command/query API, projects, jobs, permissions
crates/project-store/  Versioned project folders, atomic snapshots, recovery
packages/contracts/    Shared TypeScript contracts for IPC and MCP
mcp/bridge/            Local stdio MCP adapter
docs/                  ADRs, format notes, testing matrix, UX notes
fixtures/              Small self-authored test structures (no Mojang assets)
```

## Requirements

- Node.js >= 22 and pnpm >= 10
- Rust stable (MSVC toolchain) and Visual Studio Build Tools
- WebView2 runtime (preinstalled on current Windows 10/11)

## Development

```powershell
pnpm install
pnpm dev                                      # Vite dev server (browser UI, preview mode)
pnpm tauri:dev                                # Full desktop app (requires Rust + MSVC)
pnpm test                                     # TypeScript unit tests
pnpm typecheck                                # TypeScript strict check
pnpm lint
cargo test --workspace                        # Rust core tests
cargo clippy --workspace --all-targets -- -D warnings
```

A browser tab intentionally shows preview mode and never fakes backend data. Run the
desktop app to exercise the typed Tauri IPC (`app_info`, `ping`).

## Current status

M0–M7 are complete. M8 is in progress: the read-only 1.21.4 classic reader
validates `level.dat` and decodes region `.mca`/chunk NBT, including negative
coordinates and proto-chunks, then decodes block-state palettes, sections and
biomes without writing to the source world. A byte-preserving, fail-closed copy
of an unchanged world — including unknown files and region containers — is
implemented and verified against a real 841-chunk world. Overlay and the
`Import World` wizard are next. The first interchange format, Minecraft Java
1.21.4 (`DataVersion` 4189) structure `.nbt`, is implemented and verified
against real game-saved fixtures, a real Tauri IPC flow and an official 1.21.4
server load. Block states and orientations are preserved; entities and block
entities are refused rather than silently dropped. Sponge `.schem` is the next
format and is not enabled in the UI. External AI clients can read, build,
screenshot, correct and undo through MCP, with read-only access by default and a
real confirmation dialog for writes. Milestone progress is tracked in
`FURYBUILDER_MASTER_PLAN.md`; a checked implementation item is not by itself a
guarantee that its whole milestone is complete.

## Documentation

- `docs/mcp-setup.md` — connect an MCP client, access modes, common errors and how to
  turn access off.
- `docs/formats/java-world-layouts.md` — observed Java world layouts, release
  `DataVersion`s and the supported-version matrix.
- `docs/testing/mcp-compatibility.md` — client/protocol matrix and live probe output.
- `docs/testing/test-matrix.md` — requirement-to-evidence coverage.
- `docs/adr/` — architecture decision records (format, MCP runtime, versions).

## Known limitations (early development)

- Minecraft game assets are not bundled. The renderer ships original placeholder textures
  and approximate shapes for slabs/stairs/torches.
- Projects live in the app-data directory and store an Empty or flat-base world. Automatic
  vanilla world generation (`Vanilla World`) and world imports (`Import World`) are shown
  as future modes until M9/M8.
- Structure import/export is currently Java structure `.nbt` only. It refuses entities,
  block entities and unknown tags; `.schem` support is not available yet.
- Box selection uses two clicks (corner A, corner B), not drag-rubber-band selection.
- Autosave runs every 30 seconds while dirty (plus visibility change and window close), so
  a hard crash can lose up to half a minute of edits.
- `Vanilla World` requires a user-provided or explicitly downloaded Java runtime and
  official server JAR; FuryBuilder never accepts the Minecraft EULA for the user.
- Import of full Java world saves is read-only. M8 implements version/layout
  validation, region/NBT reading, block states and biomes, and a byte-preserving copy
  of unchanged worlds. Overlay, editing and export of modified worlds are not available
  yet, and the source world is never written.
