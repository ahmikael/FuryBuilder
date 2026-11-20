# Changelog

All notable user-visible changes to FuryBuilder are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the application follows
[Semantic Versioning](https://semver.org/). The application version is independent from the
project schema, the world snapshot format and the MCP tool schema; see
[docs/release/versioning.md](docs/release/versioning.md).

## [Unreleased]

_Nothing yet._

## [0.1.0] - 2026-11-20

First public beta: a local Windows desktop editor for Minecraft Java Edition 1.21.4
(`DataVersion` 4189), built on a Rust world core with verified Java world import/export,
optional vanilla generation through the official game engine, and an MCP bridge for external
AI clients. The supported boundary and current limitations are documented in
[docs/known-limitations.md](docs/known-limitations.md).

### Added

- Windows desktop editor built on Tauri 2, React, TypeScript and Three.js, with a Rust core as
  the single source of truth for blocks, chunks and project state.
- Empty and flat projects, read-only analysis and safe copy-on-write import of classic 1.21.4
  worlds, and Vanilla World generation through the official local Java server engine.
- Sparse chunk editing, selection tools, terrain brushes, biome tools, layers, masks, protected
  regions, templates, semantic objects, a typed Build DSL, deterministic observation maps and
  bounded validation findings.
- Undo/redo, autosave, checkpoints, crash recovery and an eight-snapshot project history with
  the previous manifest retained on every save.
- Java structure `.nbt` round-trip, complete world-folder export with byte-preserving
  passthrough, and import/export verification against the official 1.21.4 server.
- Local Minecraft asset discovery, verified client-version matching, ordered resource packs
  and a documented placeholder fallback that never changes block data.
- Managed Java runtime and official `server.jar` provisioning with explicit consent, checksum
  verification, proxy support, offline cache and isolated generation processes.
- MCP stdio bridge over the same application API as the UI: read-only access by default,
  scoped writes, previews, confirmations, jobs, screenshots and observation tools.
- Built-in Tauri updater: installed builds check GitHub Releases after startup, show a
  non-blocking banner and Settings controls, verify the Tauri updater signature and preserve
  projects and settings.
- Bundled self-authored demo project, real WebView2 screenshots, a deterministic observation
  map and a recorded read-only MCP probe that open without Minecraft or Java.
- Reproducible Windows release tooling: per-user NSIS install/update/uninstall, a single
  application version in `version.json` checked with `pnpm version:check` and updated with
  `pnpm version:set`, and a tag-driven GitHub Release with signed updater artifacts,
  `latest.json` and SHA-256 checksums.
- Beginner and advanced workflow guides, Windows graphics-path stress evidence, cache budgets
  and diagnostics.

### Changed

- Replaced the Node.js/TypeScript MCP bridge with a Rust `rmcp` sidecar bundled in the
  installer. Tool names, JSON payloads, prompts, both protocol eras and the MCP Apps fallback
  are unchanged.
- Viewport capture metadata now names its texture source (`placeholder` or `minecraft_assets`)
  instead of always claiming placeholder textures.
- Raised the minimum supported Rust version to 1.90 and the Node.js runtime to 26 for building
  and releasing the desktop app.

### Removed

- The Windows beta ships only the per-user NSIS installer; the MSI package was removed before
  the first release, leaving one elevation-free installation path.

### Fixed

- A failed migration or interrupted save can no longer leave an unbounded pile of manifest
  backups or a non-verbatim migration backup.
- Block counts in operation labels and editor previews use deterministic `en-US` grouping
  instead of the host locale, keeping history text and CI expectations stable.

### Security

- Imported files, resource packs, worlds and MCP calls are bounded and fail closed; destructive
  operations require previews, limits, revisions and confirmation.
- Removed six known `zip` extraction advisories by upgrading to `zip 9.7.3`; the runtime
  extractor keeps its own path, symlink, entry-count and size checks.
- Added a repeatable dependency and license audit, a generated third-party inventory and an
  Authenticode verification gate. Beta installers remain unsigned until an external
  code-signing certificate and key exist.
- FuryBuilder distributes no Mojang JARs, assets or game binaries; downloads happen only after
  user action and are verified against official metadata.
