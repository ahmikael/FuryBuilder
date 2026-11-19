# FuryBuilder

FuryBuilder is an offline-first 3D studio for creating, inspecting, editing and exporting
Minecraft Java Edition worlds and structures. It is a desktop editor, not a game client:
human users work through the UI, and external AI agents work through the same
Application API over MCP.

- **Platform:** Windows first (Tauri 2 + WebView2), macOS/Linux later.
- **Target game version:** Minecraft Java **1.21.4** (`DataVersion` 4189, Java 21).
- **Project modes:** `Empty Project` and `Import World` (classic Java 1.21.4) are available; `Vanilla World` is a later phase.
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

M0–M8 are complete. M9 is in progress: the read-only 1.21.4 classic reader
validates `level.dat` and decodes region `.mca`/chunk NBT, including negative
coordinates and proto-chunks, then decodes block-state palettes, sections and
biomes without writing to the source world. A byte-preserving, fail-closed copy
of an unchanged world - including unknown files and region containers - is
implemented and verified against a real 841-chunk world. A copy-on-write overlay
now keeps block and biome edits in memory, persists them as a versioned document and
materialises a patched chunk NBT while preserving unknown tags, untouched biome
sections, block entities and entities; the source stays byte-identical. The `Import World` wizard
finally analyzes a selected world read-only, reports blockers and limitations, and
creates a project with a byte-for-byte immutable source copy plus an editable block
snapshot. A verified writer now publishes a new region file for classic 1.21.4
when only `sections[].block_states` or `sections[].biomes` changes; untouched chunk records stay
byte-identical and the source hash is rechecked. A read-only derived-data
inventory and two official 1.21.4 server round-trips document the boundary:
the default `preserve` policy copies stale heightmaps/light raw, while the
opt-in `vanillaRebuild` policy invalidates them and the vanilla engine rebuilt
all four heightmaps, both light arrays, `isLightOn` and `PostProcessing` on the
next load. A complete new world-folder export now copies `level.dat`, dimensions,
sidecars and unknown files byte for byte, rebuilds only dirty regions and
publishes the staged folder after reopening and re-validating every chunk. The
import/export boundary now classifies the selection first: a bare `.mca` file and
a `region/` directory are explicitly not complete saves, while split, mixed and
legacy layouts are detected and refused. An automated official-server fixture
now generates a fresh 1.21.4 world, exports an edited complete folder, reloads it
in the same server, verifies the edited and neighbor blocks and decodes the saved
result again. The Import World wizard now shows a machine-readable
read/edit/write/passthrough support matrix by version and data category.
Placing block-entity blocks and POI updates are still not semantically supported.
Client rendering is no longer only a manual placeholder check. M9.1 adds read-only
Minecraft asset discovery: conventional launcher roots and launcher profiles are scanned
for installed versions, `settings → Minecraft assets` accepts a manual `.minecraft`
folder, client JAR or resource pack, and a 1.21.4 client is accepted only after its
declared SHA-1, internal `version.json` and `DataVersion` 4189 agree. M9.2 resolves
blockstates, parent models and PNG textures from that verified source, applies an ordered
resource-pack stack, builds a cached atlas and renders real reference blocks; a missing
asset falls back to a clearly marked checker without changing block data. M9.3 adds
`runtime-manager`: installed Java candidates are executed and accepted only when they
report Java 21, a user can select `java.exe` manually, and an explicit opt-in can download
the official Eclipse Temurin JRE from Adoptium with size/SHA-256 verification into a
managed, clearable FuryBuilder cache. No global installation or environment change happens.
M9.4 adds the official `server.jar` path: the exact 1.21.4 download is resolved through
Mojang's version manifest, SHA-1/size verified, checked for `world_version` 4189 and a server
entry point, and stored in a separate clearable cache; a local JAR can be selected manually
and must pass the same internal checks. M9.5 adds the explicit EULA step and a supervised,
isolated generation process: after the user confirms the official EULA for that exact JAR,
the original server generates a requested area in a temporary run directory. It listens on
loopback only, has memory/time limits, is contained in a kill-on-close Windows Job Object,
reports real region-header progress and always terminates the whole Java tree on
cancel/crash. M9.6 adds the `Vanilla World` project wizard: it shows the verified Java 21,
server JAR and EULA state, lets the user choose a seed, preset, supported worldgen settings
and validated local `.zip` data packs, and requires a separate explicit consent before
starting the official generator. M9.7 imports a completed run as a `Vanilla World` project:
the generated folder is copied byte-for-byte as an immutable source, decoded into the Rust
World Core, and its seed, preset, data packs, Java/server identity and requested area are
recorded in `project.json`. M9.8 verifies the saved generated base against the original
generator's reference chunks: settings and provenance must agree exactly, only an untouched
revision-1 base is checked, and block differences are reported with coordinates. The check is
bounded and refuses unsupported or incomplete references instead of claiming parity. M9.9
adds deterministic terrain brushes - raise, lower, smooth, flatten, noise, erode and terrace -
over the box selection. They adjust the topmost non-air block of each column in one reversible
operation, read heights before writing, use a stable SplitMix64 noise seed, and are previewed
by the Rust core. They are local surface tools, not a whole-world generator; heightmaps are
still preserved raw and are not recalculated after a brush. M9.10 adds a bounded deterministic
local heightmap generator: an explicit seed, scale, octaves, amplitude, base Y and surface
depth produce an integer fBm patch that is previewed by the Rust core and applied as one
reversible operation. The surface and filler materials are chosen explicitly, above-surface
blocks inside the bounds are cleared, and repeating the same parameters reproduces the same
blocks. It is a construction tool, not a vanilla world generator, and it makes no worldgen
parity claim. M9.11 adds biome tools for the confirmed Java 1.21.4 layout. A box selection
paints the 4x4x4 biome cells it covers as one reversible operation, with a 64-entry
per-section palette; the same path is exposed in the UI and through MCP. Biome edits are
stored in the world domain, snapshot format 2 and the overlay document, written into
`sections[].biomes`, preserved through region and world-folder export, and covered by the
standard undo/redo history. The tool refuses to invent biome data for a section that has
none instead of silently assuming a default biome. Terrain brushes and the heightmap
generator still change block states only and do not recalculate biomes or lighting.
M9.12 adds deterministic local road, river and vegetation generators plus a bounded
structure-site placement query. Roads follow the existing surface and clear headroom,
rivers carve a bed and water column without deepening on reapply, and vegetation only
fills free air with configurable ground cover, tree chance and spacing. The panel previews
and applies each edit through the same reversible history as every other operation, and
the `find_structure_sites` query is read-only, capped at 2,000,000 checks and 64 sites.
None of these tools claims vanilla worldgen or vanilla structure placement parity.
M9.13 adds editor-only workspace layers, an active edit mask, protected regions and a
project-local structure-template library. Layers, visibility and lock state are stored in
`project.json`; every preview and apply checks the locked layer, requires the operation to
fit fully inside the active mask of its dimension, and rejects any intersection with a
protected region. Policy changes are metadata: they do not bump the world revision and are
not part of block history, while undo/redo still restores earlier snapshots. Templates are
verified Java `.nbt` payloads stored per project with SHA-256 and bounded size; a selection
can be saved as a template, an `.nbt` file can be imported, and either can be inserted as
one reversible Batch operation through the same layer/mask/protection checks. MCP exposes
the policy and library read-only plus a confirmed `insert_template` write. M9.14 adds
editor-only semantic objects: districts, buildings, roads, entrances and landmarks stored
in `project.json` with a name, dimension, inclusive bounds, optional parent and bounded
tags. The `Objects` tab creates an object from the current selection, shows its hierarchy
and inspects the current block palette and non-air count at the authoritative revision.
Scene edits never place blocks, bump the world revision or enter block history. MCP exposes
`list_scene_objects` and `inspect_scene_object` as read-only tools.
M9.15 adds a minimal parametric scene graph: a versioned house recipe (generator, version,
dimension, origin, rotation and bounded materials) compiles deterministically into one
reversible voxel overlay. The Rust Core captures the blocks that existed before the first
apply, so rollback restores terrain instead of assuming air. Recipes persist in
`project.json`; transform and rebake are blocked when manual edits no longer match the
generated overlay unless the user explicitly chooses override. Bake detaches the recipe
and keeps the blocks, while rollback restores the captured baseline. MCP exposes
`list_parametric_objects` and `inspect_parametric_object` as read-only tools with conflict
details; the desktop panel owns the mutating actions.
M9.16 adds a typed, data-only Build DSL. A versioned JSON recipe of houses and repeat nodes
is validated and compiled deterministically into one ordinary reversible Batch, with hard
budgets (64 houses, 16,384 blocks, depth 4), rotated block states and overlap detection.
The `Build DSL` tab compiles and previews the exact operation before applying it; MCP
exposes `preview_build_dsl`, which returns the preview plus the same typed operation for the
standard confirmed `apply_operation` path. Recipes cannot execute JavaScript, shell or file
access.
M9.17 adds deterministic observation maps: top-down, heightmap, material/biome maps,
cross-sections and revision diffs are rendered by the Core at an exact world revision.
Every observation returns a PNG data URL plus bounds, scale, legend, completeness and
warnings; revision diff reconstructs prior state from retained section history and fails
closed when that history is no longer available. The `Observation` tab previews the same
maps, and MCP exposes the read-only `observe` tool. Exact block queries remain the
authority; colour maps are summaries.
The first interchange format,
Minecraft Java 1.21.4 (`DataVersion` 4189) structure `.nbt`, is implemented and
verified against real game-saved fixtures, a real Tauri IPC flow and an official
1.21.4 server load. Block states and orientations are preserved; entities and
block entities are refused rather than silently dropped. Sponge `.schem` is the
next format and is not enabled in the UI. External AI clients can read, build,
screenshot, correct and undo through MCP, with read-only access by default and a
real confirmation dialog for writes. Milestone progress is tracked in
`FURYBUILDER_MASTER_PLAN.md`; a checked implementation item is not by itself a
guarantee that its whole milestone is complete.

## Documentation

- `docs/mcp-setup.md` — connect an MCP client, access modes, common errors and how to
  turn access off.
- `docs/formats/java-world-layouts.md` — observed Java world layouts, release
  `DataVersion`s and the supported-version matrix.
- `docs/formats/java-chunk-derived-data.md` — 1.21.4 heightmaps, light, ticks,
  block entities, entity/POI sidecars and the preserve/vanilla-rebuild server
  round-trips.
- `docs/formats/java-world-export.md` — complete new world-folder export, staged
  validation, preserved files and the real 841-chunk evidence.
- `docs/formats/java-world-selection.md` — save vs region file vs region
  directory, layout matrix and passthrough refusal boundaries.
- `docs/formats/support-matrix.md` — read/edit/write/passthrough/unsupported
  status by version and data category, also shown in the Import World wizard.
- `docs/assets/minecraft-assets.md` - launcher discovery, JAR checksum/version
  verification, manual selection, resource-pack validation, blockstate/model/texture
  render bundles, atlas caching and renderer limits.
- `docs/runtime/java-runtime.md` - Java 21 probing, manual selection, opt-in Temurin
  download/checksum verification, managed cache cleanup and its evidence.
- `docs/runtime/minecraft-server-jar.md` - official server JAR resolution through Mojang
  metadata, SHA-1/version verification, manual selection, cache cleanup and its evidence.
- `docs/runtime/vanilla-generation.md` - explicit EULA confirmation, the Vanilla World
  wizard, validated data packs, the isolated official server process,
  loopback/resource/process-tree guarantees, progress, cancellation, generated-base
  verification and real evidence.
- `docs/testing/worldgen-parity.md` — the independent JSON parity oracle, the implemented
  M9.8 reference-chunk check, its real evidence and remaining limitations (parity
  compares block states only; biome cells are modelled and editable but are not part of
  the parity comparison).
- `docs/testing/minecraft-server-fixture.md` — the automated official 1.21.4
  server fixture, its markers, evidence and limits.
- `docs/testing/mcp-compatibility.md` — client/protocol matrix and live probe output.
- `docs/testing/test-matrix.md` — requirement-to-evidence coverage.
- `docs/adr/` — architecture decision records (format, MCP runtime, versions).

## Known limitations (early development)

- Minecraft game assets are not bundled. Without a verified 1.21.4 source the renderer
  ships original placeholder textures and approximate shapes for slabs/stairs/torches.
  With a verified source, blockstates/models/textures and up to 16 ordered resource packs
  are used; missing assets show the missing checker. Fluid flow, animation timing,
  `uvlock`, element `rescale`, builtin entity models and biome-accurate tints are not
  implemented yet. See `docs/assets/minecraft-assets.md`.
- Projects live in the app-data directory. `Empty Project`, a flat base, `Import World`
  and `Vanilla World` (classic Java 1.21.4) are available. Vanilla World generation
  requires a verified Java 21 runtime and official server JAR plus explicit EULA consent.
- Imported worlds are analyzed read-only and copied byte for byte into the project source
  layer. Block states and 4x4x4 biome cells are editable; untouched biomes, block
  entities, entities, ticks, heightmaps, lighting and POI are preserved as raw source data
  by default and are not recalculated after an edit. The opt-in `VanillaRebuild` writer policy instead
  invalidates heightmaps, light, `isLightOn` and `PostProcessing` so the target
  vanilla engine rebuilds them; this was verified with an official 1.21.4 server.
  Placing or removing block-entity blocks and updating POI are still not semantically
  supported: the server did not synthesize a missing block entity on load.
- A single region `.mca` file or a bare `region/` directory is reported as an incomplete
  selection, not as a world. Import, copy and export require `level.dat` plus the world
  folder. Split 26.1+, mixed and legacy `.mcr` layouts are detected but not supported.
- Minecraft asset discovery targets conventional launcher roots plus launcher profile
  `gameDir` entries. Custom launchers are handled through manual selection. Modded
  client profiles are reported but never used as vanilla assets. Resource packs are
  validated as untrusted archives and applied only in the saved priority order.
- Structure import/export is currently Java structure `.nbt` only. It refuses entities,
  block entities and unknown tags; `.schem` support is not available yet.
- Box selection uses two clicks (corner A, corner B), not drag-rubber-band selection.
- Autosave runs every 30 seconds while dirty (plus visibility change and window close), so
  a hard crash can lose up to half a minute of edits.
- `Vanilla World` requires a user-provided or explicitly downloaded Java runtime and
  official server JAR; FuryBuilder never accepts the Minecraft EULA for the user. M9.3
  provides Java 21 discovery, manual selection and the managed Temurin download; M9.4
  provides verified server JAR provisioning; M9.5-M9.7 provide the explicit EULA step,
  the isolated loopback-only server process and import of a completed run as an immutable
  vanilla project.
- Import of full Java world saves is read-only. M8 implements version/layout
  validation, region/NBT reading, block states and biomes, a byte-preserving copy
  of unchanged worlds, an in-memory block-edit overlay with a versioned JSON
  document and verified chunk NBT materialization, the `Import World` wizard
  with an immutable source copy, and a verified writer for a new region file
  limited to classic 1.21.4 `sections[].block_states` and `sections[].biomes` (biome
  cells became editable in M9.11). M8.8 adds a read-only
  inventory of heightmaps, light, ticks, block entities, POI and entity sidecars,
  plus an official-server round-trip showing that a normal load/save does not
  rebuild stale heightmaps or light and does not create missing block entities.
  M8.9 adds the explicit `vanillaRebuild` policy and verifies with the official
  server that invalidated heightmaps, light, `isLightOn` and `PostProcessing` are
  rebuilt on the next load/save without changing the edited block states.
  M8.10 adds a complete world-folder export: every source file is copied byte for
  byte, only dirty regions are rebuilt, the staged folder is reopened and
  re-validated, and the source tree is hashed before and after. The source world
  is never written. M8.11 classifies the selection before import or copy: a
  `.mca` region file and a bare `region/` directory are not complete saves, and
  split, mixed and legacy layouts are detected and refused before any destination
  is created. M8.12 adds an automated official-server fixture: the server
  generates a fresh 1.21.4 world, FuryBuilder exports an edited copy, the same
  server loads it, verifies the edited and neighbor blocks and saves, and
  FuryBuilder decodes the saved block states again. A licensed client was not
  launched. M8.13 adds a machine-readable support matrix by version and data
  category; the same matrix is shown in the Import World wizard.
