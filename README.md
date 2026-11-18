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
now keeps block edits in memory, persists them as a versioned document and
materialises a patched chunk NBT while preserving unknown tags, biomes, block
entities and entities; the source stays byte-identical. The `Import World` wizard
finally analyzes a selected world read-only, reports blockers and limitations, and
creates a project with a byte-for-byte immutable source copy plus an editable block
snapshot. A verified writer now publishes a new region file for classic 1.21.4
when only `sections[].block_states` changes; untouched chunk records stay
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
and must pass the same internal checks.
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
- Projects live in the app-data directory. `Empty Project`, a flat base and `Import World`
  (classic Java 1.21.4) are available. Automatic vanilla world generation
  (`Vanilla World`) is a later M9 task and is not enabled.
- Imported worlds are analyzed read-only and copied byte for byte into the project source
  layer. Only block states are editable yet; biomes, block entities, entities, ticks,
  heightmaps, lighting and POI are preserved as raw source data by default and are not
  recalculated after an edit. The opt-in `VanillaRebuild` writer policy instead
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
  provides verified server JAR provisioning. EULA confirmation and the isolated server
  process are not implemented yet.
- Import of full Java world saves is read-only. M8 implements version/layout
  validation, region/NBT reading, block states and biomes, a byte-preserving copy
  of unchanged worlds, an in-memory block-edit overlay with a versioned JSON
  document and verified chunk NBT materialization, the `Import World` wizard
  with an immutable source copy, and a verified writer for a new region file
  limited to classic 1.21.4 `sections[].block_states`. M8.8 adds a read-only
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
