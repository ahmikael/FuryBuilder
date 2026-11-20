# Changelog

Notable user-visible changes to FuryBuilder.

## [Unreleased]

_Nothing yet._

## [0.1.0] - 2026-11-20

First public beta: a local Windows desktop editor for Minecraft Java Edition 1.21.4
(`DataVersion` 4189), built on a Rust world core with verified Java world import/export,
optional vanilla generation through the official game engine, and an MCP bridge for external
AI clients. The supported boundary and current limitations are documented in
[docs/reference.md](docs/reference.md).

### Added

- Windows desktop editor built on Tauri 2, React, TypeScript and Three.js, with a Rust core as
  the single source of truth for blocks, chunks and project state.
- Project manager with project creation, listing, opening and renaming; Empty and flat
  projects, read-only analysis and safe copy-on-write import of classic 1.21.4 worlds, and
  Vanilla World generation through the official local Java server engine.
- Sparse chunk editing with place/remove brushes, box selection, fill, replace, line and
  copy/paste, plus terrain brushes, biome tools, layers, masks, protected regions and reusable
  templates.
- Semantic objects and a parametric Scene Graph, a typed Build DSL with deterministic previews,
  observation maps and bounded validation findings.
- Undo/redo, autosave, checkpoints, crash recovery and an eight-snapshot project history with
  the previous manifest retained on every save.
- Java structure `.nbt` round-trip, complete world-folder export with byte-preserving
  passthrough, and import/export verification against the official 1.21.4 server.
- Local Minecraft asset discovery, verified client-version matching, ordered resource packs
  and a documented placeholder fallback that never changes block data.
- Managed Java runtime and official `server.jar` provisioning with explicit consent, checksum
  verification, proxy support, offline cache and isolated generation processes.
- MCP stdio bridge over the same application API as the UI: read-only access by default,
  scoped writes, previews, confirmations, jobs, screenshots and observation tools. It is a
  bundled Rust sidecar and needs no Node.js runtime.
- Built-in Tauri updater: installed builds check GitHub Releases after startup, show a
  non-blocking banner and Settings controls, verify the Tauri updater signature and preserve
  projects and settings.
- Bundled self-authored demo project, real WebView2 screenshots, a deterministic observation
  map and a recorded read-only MCP probe that open without Minecraft or Java.
- Windows beta packaging as a per-user NSIS installer with signed updater artifacts and
  `latest.json` published with the release.
- Command palette, dark/light themes, persistent panel layout, searchable block palette and
  beginner/advanced workflow guides.

### Security

- Imported files, resource packs, worlds and MCP calls are bounded and fail closed; destructive
  operations require previews, limits, revisions and confirmation.
- FuryBuilder distributes no Mojang JARs, assets or game binaries; downloads happen only after
  user action and are verified against official metadata.
- Dependency and license audits run in CI, known RustSec and pnpm advisories are resolved, and
  updater artifacts are signed. The beta installer is not Authenticode-signed, so Windows can
  show a SmartScreen warning for an unknown publisher.
