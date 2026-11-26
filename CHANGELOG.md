# Changelog

Notable user-visible changes to FuryBuilder.

## [Unreleased]

This release makes the Minecraft version a property of the project and moves first-run setup
into a full-screen wizard. It is a deliberate schema reset: projects written by older internal
formats are not migrated.

### Added

- Full-screen project wizard with Basics, Version, Source, Components, Review and Preparing
  steps, plus a welcome screen that acts as the project hub for recent projects and recovery.
- Official Minecraft version catalog with releases, snapshots and legacy/special manifest ids.
  A version needs a verified profile to advance; classic Anvil `1.18–1.21.11` is verified for
  import, editing, project snapshots and same-version export, and can generate with its exact
  official server JAR; `1.21.4` remains the full-support baseline.
- End-to-end import for all 27 classic Anvil releases from 1.18 through 1.21.11: exact
  `DataVersion`, profile dimensions and vertical bounds, editable block states and 4x4x4 biomes,
  byte-for-byte source copy and a version-bound project snapshot. Split 26.1+ and legacy layouts
  remain refused.
- Version capability contract across the wizard, welcome rows, project cards and Project
  Settings: exact support badges plus explicit `Read`, `Edit`, `Save`, `Export`, `Generate` and
  `Assets` states with visible reasons for unavailable actions. The `unverified` support level
  is now rendered honestly instead of appearing as a blank badge, and project summaries carry
  the verified capabilities of their target profile.
- Same-version world export for every modern classic release: staged sibling folder, dirty-region
  rebuild, full output revalidation, source-tree hashes and byte-for-byte preservation of
  untouched files.
- Copy-only same-family project upgrade for modern classic releases: a mandatory source
  checkpoint, exact target catalog/biome validation with mapped and unknown counts, staged
  reopen and verification, atomic publication and a report proving the original project stayed
  unchanged. Imported source copies keep their original version; Vanilla projects are refused
  in favour of regeneration.
- Vanilla World generation for every modern classic release: the exact `level-type` spelling,
  Java major, data pack format and bounded `server.properties` come from the project profile,
  and each run records its profile id, Minecraft version and `DataVersion`.
- Managed client JAR download and an exact-version block/item catalog built from that JAR
  (1,097 blocks and 1,385 items for 1.21.4), plus manual `.minecraft`/client JAR selection and
  an explicit placeholder mode.
- Unified component manager for Java, client assets and official servers with progress,
  cancel, verify, relink, remove and open-folder actions. Settings shows a read-only inventory.
- Manual Java selection that probes the major version, architecture and vendor; newer runtimes
  are allowed with a warning, while older, 32-bit or unreadable runtimes are blocked.
- Project schema v2, snapshot format v3 and checkpoint metadata v2, with the target version,
  component hashes and catalog identity pinned in the project.
- Five-tab Settings (Appearance, Components, Updates, Diagnostics, Advanced) and a grouped
  bottom panel with `Blocks`, `Analysis` (Observation/Validation) and `Activity`
  (Console/History).
- Versioned editor layout: a layout saved by an older panel format is discarded once, while
  theme and UI scale remain user preferences.

### Changed

- Project creation and import moved to the full-screen wizard; Settings no longer downloads,
  selects or provisions components.
- Internal project formats are not migrated: a project from before manifest schema v2,
  snapshot format v3 or checkpoint metadata v2 must be recreated.
- The version manifest keeps Mojang's raw ids, so legacy and special versions no longer abort
  the catalog.
- Managed Java, client JAR and server downloads share one verified pipeline with host
  allowlisting, size and checksum checks, disk preflight, retry/offline handling and atomic
  publish.
- The top bar no longer shows an active block or a second core/save status; the project chip is
  the single save indicator.
- Notifications now use the shadcn Toast component and its Base UI toast manager, with
  consistent success, warning and error styling and automatic dismissal.
- Every select field now uses the shadcn Select component; the remaining native select
  wrapper was removed.
- Expandable detail areas now use the shadcn Collapsible component.

### Fixed

- The version catalog no longer fails on official pre-release and special ids such as
  `26.4-pre-1`, `1.14.2 Pre-Release 4` and `25w14craftmine`.
- A client JAR without launcher metadata is accepted with an explicit warning, while a
  declared checksum or version mismatch is still refused.

### Security

- Managed component downloads require an explicit action and verify official checksums and
  sizes. FuryBuilder bundles and redistributes no Mojang files.

## [0.2.0] - 2026-11-21

A focused interface release: the desktop UI is rebuilt on shadcn/ui with Base UI primitives,
the Neutral theme and the Geist font family, while world editing, projects and the MCP bridge
keep their existing behavior.

### Added

- New FuryBuilder application icon and a transparent logo in the toolbar.
- Bundled Geist and Geist Mono font licenses are recorded under `docs/legal/licenses/`.

### Changed

- Complete desktop interface redesign on shadcn/ui: consistent toolbar, navigation, panels,
  forms, cards, tables, dialogs, empty states and status surfaces.
- The UI uses the shadcn Neutral theme, standard component sizes and tokens, more generous
  spacing, Geist for interface text and Geist Mono for technical data.
- Native selects and checkboxes were replaced by the shadcn native select and checkbox
  components without changing their behavior.
- The project is now licensed under Apache-2.0.
- Windows releases remain per-user NSIS installers with signed updater artifacts.

### Fixed

- Generation cancellation is timed from the cancel request instead of the original start, so
  the cancellation window is no longer shortened by elapsed generation time.

### Security

- The dependency license policy now accepts the SIL Open Font License 1.1 used by the bundled
  fonts and requires their notices to be shipped with the project documentation.

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
