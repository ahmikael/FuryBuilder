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
- Exact profiles for Java `1.8-1.8.8` and `1.0-1.7.10`. Numeric
  `1.8-1.12.2` records `DataVersion` 47 for 1.8.9, 169-1343 for 1.9-1.12.2, `-1` for
  1.8-1.8.8 and resource pack formats 1-3; pre-1.8 splits McRegion `1.0-1.1` (128-block
  height) from Anvil `1.2.1-1.7.10` (256-block height). The `1.0-1.7.10` profiles are
  `convert-only` and cannot create editable projects; `1.8-1.8.8` remain `unverified`.
  Pre-1.8 generation is not enabled.
- Convert-only McRegion and early-Anvil analysis and passthrough for `1.0-1.7.10`. The
  selection classifier recognizes `.mcr` and pre-DataVersion `.mca` saves and inventories
  their dimensions, regions and chunks without guessing an exact release. With an exact
  profile, the adapter validates `level.dat` `Version` 19132/19133 and every flat
  `Level.Blocks`/`Data`/`Biomes` chunk through the shared numeric registry;
  `TerrainPopulated` maps to `full`/`partial`, and unknown ids, malformed arrays, mixed
  containers and `DataVersion` tags are rejected. `passthrough_legacy_world` copies the
  complete save byte for byte and re-validates the copy. Editing and editable project import
  remain disabled.
- Copy-only official-server conversion for `1.0-1.7.10`. A pre-1.8 save is validated and
  copied into an isolated run, the exact writable `1.13+` server JAR is started with
  `--forceUpgrade`, and the upgraded world is reopened and fully decoded before it is
  reported. The source is never started or written; downgrade, same-version and
  metadata-only targets are refused. The wizard Convert strategy is not wired yet.
- Import, block/biome editing, project snapshots and same-version export for the 15 numeric
  classic releases `1.8.9-1.12.2`. The writer patches only `Level.Sections[].Blocks`/`Data`/
  `Add` and `Level.Biomes`; unknown tags and untouched sections stay byte-for-byte. Column
  biomes are sampled at each 4x4 center on read and applied to the whole footprint on write.
  Unknown numeric block or biome ids are rejected instead of substituted, and 1.8.9 chunks
  without a root `DataVersion` use the exact level.dat profile.
- Exact built-in metadata for Java `1.13-1.17.1`: `DataVersion`, Java 8/16, resource and data
  pack formats 4–7, dimension folders and the 0–256 Overworld height (the extended height starts
  with 1.18), plus the column/section biome boundary.
- Import, block/biome editing, project snapshots and same-version export for all 19 flattened
  classic releases `1.13–1.17.1`. The writer patches only `Level.Sections[].Palette`/
  `BlockStates` and `Level.Biomes`; every other chunk tag is compared with the source and must
  stay unchanged. Column biomes apply an edited cell to every vertical section and refuse a
  request for different values by height.
- Vanilla World generation for all 19 flattened classic releases `1.13–1.17.1`, with
  release-specific presets, properties and Java 8 or 16. The 1.13.x server JARs without
  `version.json` are identified by official SHA-1. Minecraft 1.13 uses its natural spawn
  area; 1.13.1 and later also support `/forceload`.
- Official client JAR downloads and manual relinking for Minecraft 1.13–1.17.1. The verified
  JAR supplies the project catalog, palette atlas and viewport textures. Resource packs must
  declare the project's exact integer `pack_format` (4–7).
- Java structure `.nbt` interchange for all 19 flattened classic releases. A new project
  uses the file's exact `DataVersion`, insertion requires a matching project, and the
  default placement Y uses the selected dimension's build bounds.
- Flattened world import reads vanilla singleton block palettes with packed zeroes and
  empty proto-chunks without biomes. Same-version writes retain packed singleton indices.
- Internal chunk decoding for flattened Java `1.13–1.17.1`: pre-1.18 `Level.Sections` with
  direct `Palette`/`BlockStates`, continuous index packing through 1.15 and padded words from
  1.16, chunk-wide numeric biomes and light-only halo validation. Mixed 1.13–1.14 column
  footprints use center sampling and are counted; writing a changed footprint replaces all of
  its columns, while untouched footprints keep the original data.
- Exact-version block/item catalogs for Java `1.13–1.17.1` client JARs. The flattened asset
  layout (singular block/item textures, item models instead of item definitions, no atlases and
  plural block/item tags) is read alongside the modern layout. JARs without a root
  `version.json` resolve their exact release from adjacent launcher metadata and the matching
  built-in profile, with a warning and checksum verification; version folders and managed cache
  folders are selected directly. Entity render blockstates, empty or template item models and
  wall-fan variants are filtered out of the catalog. Real 1.13.2, 1.14.4, 1.15.2, 1.16.5 and
  1.17.1 JARs reproduce the vanilla registries exactly.
- End-to-end import for all 27 classic Anvil releases from 1.18 through 1.21.11: exact
  `DataVersion`, profile dimensions and vertical bounds, editable block states and 4x4x4 biomes,
  byte-for-byte source copy and a version-bound project snapshot. Split 26.1+ and legacy layouts
  remain refused.
- Version capability contract across the wizard, welcome rows, project cards and Project
  Settings: exact support badges plus explicit `Read`, `Edit`, `Save`, `Export`, `Generate` and
  `Assets` states with visible reasons for unavailable actions. The `unverified` support level
  is now rendered honestly instead of appearing as a blank badge, and project summaries carry
  the verified capabilities of their target profile.
- Same-version world export for every modern and flattened classic release: staged sibling
  folder, dirty-region rebuild, full output revalidation, source-tree hashes and byte-for-byte
  preservation of untouched files.
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
  an explicit placeholder mode. Catalog snapshots are cached by exact version, JAR SHA-1 and
  catalog schema, so moving or copying the same JAR reuses the same pinned facts.
- Cross-family catalog fixtures and an end-to-end catalog path: a synthetic exact-version JAR
  for representative `1.18.2`, `1.19.4`, `1.20.6` and `1.21.4` projects is used to create,
  edit, save, reopen, rebuild the pinned catalog, export and re-import the world while checking
  the edit and preserved unknown states.
- Flattened catalog fixtures for all 19 releases `1.13`–`1.17.1`: exact synthetic client JARs
  model 1.13 launcher metadata, 1.14–1.14.2 build-suffixed ids, numeric 1.14–1.16 pack
  versions and split 1.17 formats. Each fixture checks block, item, state and tag facts, then pins the
  catalog in a project and verifies create, edit, save, reopen, catalog rebuild, export and
  re-import with the unknown imported state preserved.
- Numeric catalogs for Java `1.8–1.12.2` client JARs. A shared `1.12.2` registry attaches
  block id+metadata and item id+damage to the catalog entries, explicit state variants and item
  forms through their own `legacyIds` arrays. Legacy asset names are bridged by explicit renames,
  and the JAR still decides which blocks and items exist. Official 1.8.9 and 1.12.2 JARs resolve
  every catalog block state and all discovered item entries except the two smooth-sandstone items
  that have no legacy item form; item-only entries that the legacy JAR layout cannot expose
  remain a later legacy admission item. Pre-1.8 catalogs remain out of scope.
- One catalog entry per block/item id, with numeric ids/metadata, display name, category,
  tags, icon, aliases, nested block states, item form and typed placement flags. Facts the JAR
  cannot prove stay `null` rather than being guessed; the snapshot no longer splits blocks and
  items into parallel arrays.
- Full blockstate variants: every explicit `variants` key plus its weighted model choices,
  rotation, `uvlock`, the single unambiguous default state and an honest completeness flag.
  Multipart blocks expose their conditions without inventing a default state.
- Item entries cover block items and item-only entries with exact names, icons, direct item
  tags and `placeable`/`hasItem` facts. Imported item stacks and their data components remain
  opaque NBT and survive editing, saving and export unchanged.
- Version-aware placement flags: technical, creative-only, fluid, airlike, block-entity and
  obtainable facts are explicit for the supported vanilla range, while unprovable geometry
  facts and non-vanilla namespaces stay unknown. Flags label entries but never remove them
  from the write registry.
- Creative-style Blocks palette: Building, Colored, Natural, Functional and Redstone groups,
  plus Technical and Other, sit alongside All, Recent and Favorites. Search matches full ids,
  display names and direct tags, and placement-flag filters narrow the same exact-version
  catalog without hiding technical or creative-only entries.
- Exact-version catalog diffs: added and removed entries, verified or conservatively inferred
  rename hints with evidence, and per-entry changes to display names, categories, tags, states,
  item forms, flags and aliases. Removed ids stay explicit unknown entries for the target
  version instead of being silently substituted.
- Virtualized Blocks palette with catalog atlas icons, an isometric selected-entry preview and
  a constrained state picker. Complete blockstate sets offer only declared values plus
  `Game default`; incomplete sets keep a warned text input; imported values outside the
  declared set stay visible and are never substituted. Selected states missing from the exact
  catalog show `Not in this version` with a disabled `Game default` action and an explicit
  replace-source exception. Curated quick-pick ids proven absent from the exact version are
  listed separately and disabled; this is not presented as a complete cross-version diff.
- Lazy palette icon atlases that decode only the visible window, overscan and selected entry.
  Requests are bounded to 512 catalog-declared textures, cached in 32-entry/16 MiB backend
  batches and merged into at most eight/16 MiB WebView batches. Search uses a bounded trigram
  index, and the canonical catalog payload has a 64 MiB hard cap.
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
- Classic Anvil multi-version support is complete for `1.13-1.21.11`: every release has an
  exact profile and client-JAR catalog, and import, editing, project snapshots, same-version
  export and generation are enabled. The 19 flattened releases also have managed/manual client
  JARs, resource packs 4–7 and a catalog-pinned project round-trip for every release. `1.21.4`
  remains the only `full` support level; split 26.x and legacy versions stay profile-check only.
- Catalog construction from a directly selected client JAR uses that JAR's internal exact
  version and `DataVersion` instead of requiring the application target. A project-pinned
  rebuild still rejects any catalog id, hash, version, `DataVersion` or client JAR mismatch.
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
- Flattened client JARs no longer produce phantom entries: an empty item model such as
  `item/air` does not create an item, item models that are only templates do not create items,
  entity render blockstates are not listed as blocks, and wall fans do not inherit an item form
  from their base fan.

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
