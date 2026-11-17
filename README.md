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

M0–M4 are implemented: application shell and IPC, the Rust world core with revisions,
transactions and section-diff undo/redo, a live Three.js viewport fed by revisioned
render snapshots, and a manual editor with a searchable block palette, block-state
properties, place/remove/brush/box/fill/line/replace tools, copy/paste, ghost previews,
undo/redo, history and cancellable preview/read jobs. Project persistence (M5) is next.
Milestone progress is tracked in `FURYBUILDER_MASTER_PLAN.md`; only tasks with passing
verification are checked there.

## Known limitations (early development)

- Minecraft game assets are not bundled. The renderer ships original placeholder textures
  and approximate shapes for slabs/stairs/torches.
- The viewport starts with a deterministic demo scene. Projects, saving and exports
  arrive in M5/M6; edits live in the running Rust session.
- Box selection uses two clicks (corner A, corner B), not drag-rubber-band selection.
- `Vanilla World` requires a user-provided or explicitly downloaded Java runtime and
  official server JAR; FuryBuilder never accepts the Minecraft EULA for the user.
- Import of full Java world saves is read-only until the version/format matrix marks a
  combination as write-verified.
