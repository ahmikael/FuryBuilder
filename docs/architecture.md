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
| ADR-001 | Classic Anvil **1.18–1.21.11** is admitted for import, block/biome editing, project snapshots, same-version export, generation with the exact official server JAR and copy-only same-family upgrade; **1.21.4**, `DataVersion` 4189, remains the full-support baseline. Split 26.x and legacy versions are detected and refused. |
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
| ADR-015 | Java, client assets and the official server JAR are discovered first and provisioned only after explicit consent through one component facade; the desktop shell exposes one `components_*` command set with progress, change and error events, and verifies checksums before publishing. |
| ADR-016 | Rust core crates stay independent of UI/desktop dependencies; UI and MCP produce identical operations for the same command.                |
| ADR-017 | The MCP bridge is a Rust `rmcp` sidecar bundled next to the desktop executable; no Node.js runtime is required.                           |

## Project format

A project folder contains `project.json`, `source/` (immutable imported source), `data/`
(versioned world snapshots), `changes/` (journal and checkpoints), `cache/` (regenerable)
and optional `assets/` and `previews/`. Projects live in the user's `Documents/FuryBuilder`
library, falling back to the app data directory when the documents folder is unavailable.
`project.json` is the only eagerly parsed document;
it stores schema version, project id, name, creation mode, target version profile
(version/DataVersion/Java/pack formats), component references, world revision and provenance.

Saves are atomic at the project level: stage, verify, publish, keep the previous manifest.
The MVP keeps one deterministic whole-world snapshot per committed revision, the newest
eight, plus autosave and rolling checkpoints. Project schema and snapshot format versions are
exact: FuryBuilder reads only the current versions and refuses older or newer files without
rewriting them.

A version upgrade never edits the original project in place. The project store first takes a
mandatory checkpoint of the saved revision, then copies the project into a hidden staging
sibling and rebuilds only the copied snapshot for the target modern classic profile. Empty,
Flat and Import projects are eligible; an imported source tree remains immutable and keeps its
original version. Vanilla projects are refused because their seed, server JAR and DataVersion
provenance cannot be carried to another release without regenerating the world. The target
block catalog and biome registry must both be exact; any unknown used state blocks publication
instead of being substituted. The staged copy is reopened, semantically verified and published
with one atomic rename, and the report proves the checkpoint, the new revision and that the
source manifest and snapshot did not change.

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

Rendering uses the exact client JAR pinned by the open project and that project's ordered
resource-pack snapshots. Project Settings changes the stack without rewriting the world.
The resolver checks each cached pack's SHA-1 and its metadata against the exact target
profile. Missing, changed, invalid or incompatible packs are skipped with a warning, not
removed from the manifest; the remaining packs and the base JAR still render. A missing
base asset falls back to a clearly marked placeholder and never changes block data.
Mojang JARs, textures and assets are not redistributed.

## Components

The desktop manages Java, client assets and the official server JAR through one
`component-manager` facade. The Tauri shell owns exactly one instance and exposes:

- `components_status`, `components_resolve`, `components_inventory`;
- `components_java_release`, `components_assets_release`, `components_server_release`;
- `components_install`, `components_cancel`, `components_relink`, `components_remove`,
  `components_verify`;
- `components_verify_inventory` and `components_open_folder` for the Settings inventory.

`components_verify` re-inspects the component currently selected for a project target and
refuses a stale id. `components_verify_inventory` re-hashes or re-probes one managed artifact
from the shared cache without a project target, while `components_open_folder` only opens one
of the three known FuryBuilder cache roots; the frontend cannot ask it to open an arbitrary
path.

The `versions_catalog` command reads the official manifest through the same
`DownloadClient` and cache as provisioning and returns every release/snapshot entry with the
verified profile this build can actually offer; it never downloads a JAR. The wizard version
step is its only consumer.

Every mutation validates the project target and the reviewed version/hash before touching
the network. Installs stream `furybuilder-component-progress`; successful mutations emit
`furybuilder-component-changed`; failures emit `furybuilder-component-error` with the same
stable code the command rejects with. The kind-specific legacy events are emitted alongside
the unified change event while the remaining panels migrate. Manual Java, client-asset and
server selections refuse symlinks, junctions and other reparse points, verify the exact
version/DataVersion and never copy or edit the user's file.

## Project lifecycle

The desktop project store is the single source of project state. It exposes
`closed → preparing → ready → saving/error`:

- `closed` — no project is open; the welcome screen owns the shell.
- `preparing` — create, open, import or checkpoint restore is running; progress is visible
  and no viewport is mounted.
- `ready` — an opened project may mount the editor and renderer.
- `saving` — a manual save or save-before-close is in flight; the open world stays mounted.
- `error` — the last lifecycle operation failed. The failure carries a stable code, message
  and hint; retry returns to `preparing` or `ready` without opening a project halfway.

Only the store transitions these phases; components read them instead of inferring state
from ad-hoc booleans. Autosave stays a background operation and does not leave `ready`.

The shell uses a single predicate, `projectAllowsEditor(phase, session)`. The viewport
mounts its renderer only when it returns true; before that no renderer, asset listener,
`world_info`, `render_snapshot`, `render_chunks`, `renderer_ready` or world write is
issued. Editor commands re-check the predicate as well, so a global shortcut cannot edit
while the welcome screen is visible. `resume` runs from startup and from an explicit
retry, never from the viewport.

Closing a project first invalidates autosave and waits for an autosave that is already
writing, then closes the backend session (which cancels world jobs). Afterwards the store
clears the job, selection, clipboard, world summary, camera, metrics, layers, scene,
parametric state and undo history. A late autosave result is discarded by generation, so
it cannot touch the closed project. The editor shell unmounts and the viewport cleanup
disposes the renderer, removes its bus and asset listeners, and returns the world store to
idle.

## Project wizard

Creating a project is a full-screen wizard, never a dialog over an empty scene. The shell owns
the ordered steps, progress, validation and navigation. Empty, Flat and Import use
`Basics → Version → Source → Components → Review`; Import swaps Version and Source because the
source world owns its version. Vanilla uses `Basics → Version → Components → Source`, because it
creates from its generation panel instead of a review step.

`Basics` names the project, shows the library folder and lets the user override the
project folder name. The backend owns the library root (`Documents/FuryBuilder`, with an
app-data fallback) and the `.furybuilder` suffix; the wizard never invents either. The
folder field is optional: a blank value uses the project name, an existing folder is never
overwritten, and path separators or reserved characters are refused before `Next`. The
same step chooses Empty, Flat, Import or Vanilla and lists that mode's requirements.

`Version` loads the official version manifest through the shared component network policy and
the same `<app-data>/version-manifest.json` cache as provisioning; only manifest metadata is
read. Releases are grouped by release series, the latest release and the recommended version
are marked, and snapshots stay behind an explicit toggle. Each row carries the support badge of
its verified profile. The card repeats the profile's Java major, world height, world layout,
resource/datapack formats and catalog formats, plus honest notes. Versions without a verified
profile remain visible for inspection but block `Next`; a snapshot without a profile is
read-only/convertible by policy, never presented as writable. Import locks the selection to the
version detected in `level.dat` and asks the user to confirm it.

`Source` shows the starting world. Empty states the selected version and its overworld height.
Flat reads backend-owned defaults, limits and quick choices, then lets the user edit the layer
stack (block, thickness, order), bottom Y and biome. The wizard validates the same limits before
`Next`, and `project_create` carries the verified `profileId` plus the flat preset so the store
re-validates every block, the biome and the profile's Y range before it writes a single block.
Import accepts a world folder or a `.zip` archive. The backend extracts archives into a
caller-owned staging folder with bounded entry counts and sizes, refuses traversal, absolute
paths, symlinks, reserved names and duplicates, then analyzes the discovered world root
read-only; preview removes the staging folder, while import keeps it only until the
byte-preserving source copy is complete. The step shows the source kind, dimensions,
region/chunk counts, blockers and warnings, and requires the `Import as-is` strategy; conversion
is an explicit disabled choice because no verified converter ships in this build. Reanalyzing
the source sends the user through the locked Version step again. `project_import_world` requires
the confirmed Minecraft version and DataVersion; both must match the fresh analysis, the profile
of the decoded world, and the copied `level.dat` before the project can open. A changed source
must be analyzed and confirmed again; import never retargets it. Vanilla keeps
the existing generation panel but receives the selected version and the verified component
summary from Components; the Source step never asks for Java or the server JAR again. The
generation target is built from the exact selected profile, so the preset list, `level-type`
syntax, Java major, data pack format and server JAR facts cannot drift from the project version.
The step owns the seed,
preset, radius/center, structures toggle, time limit and validated local data packs, and requires
an explicit EULA acceptance for the selected JAR plus a separate per-run consent. It shows the
backend-owned generation limits, expected chunk count, a conservative world-size estimate and the
free space on the run volume; `server_generation_estimate` is a read-only preflight, and
`server_start_generation` repeats the disk check before it creates a run directory. A run stops
at the requested time limit even when the area is incomplete.

`Components` resolves the exact runtime and client assets for the selected profile and mode
through the single `component-manager` facade. Client assets are required before the step can
pass; Java is required for Vanilla World and remains a recommended, warned-about addition for
editing-only projects. The server card appears only for Vanilla. The Vanilla Source step consumes
the verified component summary captured here and never repeats the Java or server choice; its
generation controls only start the already selected runtime and JAR. Every card offers the
managed download, an already-installed component from the inventory, or a manual file that is
validated in place.
Nothing is downloaded until the user presses a download button, and the managed preview only
reads release metadata. Each card shows the resolved version, size, source host, checksum and
status; installs stream progress with Cancel, and stable backend codes (offline, checksum
mismatch, no disk space, incompatible Java) go through the shared error presenter. Placeholder
textures are an explicit action: they satisfy the editing requirement but stay unverified, carry
a warning, and never claim a client JAR, checksum or block/item catalog. Once a verified client
JAR is ready, the step loads the exact-version catalog and shows its block/item counts and id.
The captured step state is keyed by profile and mode, so changing either discards the previous
result and requires a new check. `Review` repeats the name, folder, library location, mode,
version, flat layers, import source/strategy and source; it also shows the resolved Java,
client-asset and server modes and versions, the verified catalog counts, a conservative
expected size and an honest time expectation. It aggregates the profile, source and component
warnings (read-only/convert support, downgrade policy, preserved entities and placeholder
limitations) before creation. Every step returns its
own validation errors; `Next` refuses to advance, shows the reason, and the step buttons only
reach steps that were already unlocked.
`Back` keeps the entered values, and while editing, the progress bar plus `Step X of Y` label are
the only progress indicators.

Submitting enters `preparing`: the wizard lists the verified component facts captured earlier
(Java, client assets, catalog, server when required) and the project stages reported by
`furybuilder-project-prepare` (source analysis, editable blocks, metadata, byte-preserving source
copy, verify, ready). The copy reports bytes written against the analyzed source size; no stage
advances on a timer. `Cancel preparation` stops at the next safe checkpoint before publication,
removes the unpublished project folder, and keeps installed components. A failure keeps the
wizard open with the stable error message and hint. `Retry` repeats the same request; going back
returns to Review or, for Vanilla, to the completed run in Source. Retrying Vanilla imports the
same completed run without launching the server again. The wizard is built from stock shadcn
primitives (`Button`, `Card`, `Alert`, `Badge`,
`Checkbox`, `Field`, `Input`, `Progress`, `ScrollArea`, `Separator`, `ToggleGroup`) and uses
semantic theme variables only. Later steps are added to the shell as their backend data becomes
available, so no step shows a dead control.

During preparation, the desktop backend resolves component identities for the target profile
before writing `project.json`. It verifies that the catalog was built from the same client JAR
identified by the component manager and that a Vanilla server pin matches the completed run.
The project store writes these path-free records together with the first v2 manifest, not in a
follow-up save. No component download is started by project creation.

## Content catalog authority

The client-JAR catalog is not a preview-only artifact. Opening a project rebuilds it and checks
the catalog id, catalog hash, client JAR SHA-1, version and `DataVersion` against the pinned
manifest; a different JAR is a conflict. The rebuilt catalog becomes the write-admission
registry of the open `WorldService`, while the curated registry remains the quick-pick list
exposed by `block_catalog` and the explicit fallback when a project uses placeholder assets.
Validation uses the same write registry, so the palette, command writes and `Validation`
panel agree on which block names and property keys exist, and values are checked whenever
the blockstate file enumerates them. A separate icon atlas packs the catalog's referenced
textures for presentation; it is cached by exact version, client JAR SHA-1, ordered pack-stack
hash and catalog schema, so a resource-pack change cannot silently alter the pinned registry.
The Blocks palette derives basic Building/Natural/Functional/Redstone/Technical/Other groups
from direct tags and ids and keeps recent/favorite selections as local UI preferences keyed by
catalog id. Multipart defaults, exact per-block value registries, full creative categories and
item browsing remain later catalog stages.

Import loads source states into the initial snapshot without using the live write path.
On open, the service checks used section-palette entries against its write registry and
reports unsupported states without changing them. An unsupported state can be matched
by a replace predicate or restored by undo, but live writes (including template or
structure insertion) use the same admission check regardless of the caller.

## Welcome screen

The top bar and status bar are always present; the content area switches between the
welcome screen and the editor. The editor mounts only when a session exists and the phase
is `ready`, `saving`, or `error` with an already-open project. While `closed` or
`preparing`, no viewport or renderer exists.

The welcome screen shows the brand, up to six recent projects sorted by last save, and the
three entry points: `New project`, `Open project`, and `Import world`. Recent rows show the
creation mode, project version/DataVersion, size, relative save time, recoverability, and
honest schema warnings. `New project` and `Import world` open the full-screen project wizard on
the matching creation mode; `Open project` uses the desktop folder picker, and a browser preview
explains that local folders need the desktop core instead of opening a dialog. A desktop-offline
alert explains why project actions cannot work, and an unreadable library shows the backend
error with a retry action.

The Projects dialog exists only while a project is open. It switches projects, shows the
current session, and exposes checkpoints and import/export; closing the project also closes the
dialog. Without a project, the welcome screen is the project hub and the full-screen wizard owns
creation and import. The toolbar's Project action is disabled in that state with the reason, and
the command palette does not offer the project manager command.

Recoverable projects carry a `Recoverable` badge, a short explanation, and a `Recover`
action instead of `Open`; the card also summarises how many recent projects have a verified
autosave newer than the saved revision. Recovering opens the verified autosave revision and
the editor banner offers `Save now`, `Discard recovered changes`, or `Later`. When the
autosave cannot be verified, opening falls back to the saved revision and the banner says
so instead of pretending the recovery succeeded.

Startup probes the stored session once before any project UI mounts. A failure during
preparation stays on the welcome screen with the stable code, message, hint, Retry, and
Dismiss; the viewport is never opened halfway. The screen is built from the stock shadcn
`Button`, `Card`, `Alert`, `Badge`, `Empty`, `ScrollArea`, and `Separator` components with
semantic theme variables only.

## Settings

Settings is a five-tab dialog: `Appearance` (theme and UI scale), `Components` (an inventory
of FuryBuilder-owned Java runtimes, client JARs and server JARs with versions, sizes and paths,
where each artifact can be verified against its cache manifest, removed from the cache, or
shown in the platform file manager), `Updates`, `Diagnostics`, and `Advanced` (backend status).
Project creation owns first-time provisioning: downloads, manual paths, placeholder textures,
EULA acceptance and Vanilla generation stay in the full-screen wizard. Settings never starts a
component download, selects a manual or placeholder source, accepts an EULA or generates a
world. Removal only deletes FuryBuilder-owned cache data; a user file is never touched, and a
project that still needs the artifact can download or link it again through the wizard. The
Components tab and the project wizard render through the same `features/components` primitives
(`ComponentCard`, `ComponentPicker`, `ComponentStatus`); only their meta and actions differ, so
there is one status vocabulary and one card layout across both surfaces. The dialog is composed
from the stock shadcn `Dialog`, `Tabs`, `Card`, `Slider`, `ToggleGroup`, `Button`, `Badge`,
`Alert`, and confirmation `Dialog` primitives with semantic theme variables only.

## World codec boundary

`world-io` routes inspection, bounded world and overlay loading, chunk decode and
materialization, verified region writes, and full-world export through its internal
`WorldCodec` interface. `ClassicAnvilCodec` owns the reader, chunk codec, overlay,
region writer, import, and export. It selects the on-disk section, palette,
heightmap and light layout from the version profile's format family and takes
vertical bounds from the selected dimension. The chunk decoder takes the expected
profile and rejects a different `DataVersion` even if the NBT layout is identical.
The 1.18–1.21.x profiles share the modern classic section layout. The adapter selects
the fully generated `Status` spelling from exact release facts; the raw value is kept.
The same release facts supply the vanilla biome registry for import diagnostics, not as
an allowlist for source biome palettes. Raw chunk decoding is separate from save
admission: every modern classic profile admits import, editing, snapshots, same-version export
and generation through its `capabilities`, while split/legacy profiles remain refused. The
generation target is derived from the same profile, so `level-type`, Java major, data pack
format and the required server JAR cannot be taken from another version.
A loaded overlay uses that same profile to patch the original NBT's palette lists
and packed indices. Section order, unrelated container tags and opaque block-entity
payloads are verified before and after the region write. NBT equality compares
serialized floating-point bits so NaNs and signed zero survive these checks.
Untouched chunk records never need to be re-encoded.
Export golden tests compare the full file tree against pre-extraction output for
both derived-data policies, masking only the edited chunk's write timestamp.
Light-only records one section below or above the terrain remain in raw NBT;
they never enter the editable block or biome model.

The editable domain is built from the profile's `dimensionProfiles`: `World::for_profile`
creates exactly those dimension IDs with their `minY`/`height`, and snapshot encoding rejects
a world whose dimension set or bounds differ from the profile. `WorldInfo` reports dimensions
in profile order (Overworld, Nether, End, then custom), not in map order. Classic import
discovers region folders from `dimensionProfiles[].folder` instead of fixed `DIM-1`/`DIM1`
names. Render snapshots and dirty-chunk batches carry the selected dimension's `minY`/`height`,
so chunk borders follow the real vertical extent. The desktop WorldTree reads
`WorldInfo.dimensions`, shows each dimension's stored chunk count or `Empty`, and switches the
streamed dimension; an unknown persisted selection falls back to the Overworld.

`world_selection` routes newer storage to `split_anvil` for metadata and header inspection.
The inspector uses the same bounded MCA header reader but does not call the classic chunk
decoder. It inventories namespaced terrain directories, the split world-generation sidecar
and optional player storage paths. Known `DataVersion` values identify incomplete split
saves too; mixed layouts are rejected before terrain inventory. The five exact split profiles
are unverified; they cannot open a `JavaWorld`, create an overlay, or admit a project. A
full split `WorldCodec` requires payload and write verification on real saves.

Biome conversion for 1.13–1.17 sits beside the codec in
`classic_anvil/flattened_biomes.rs`. It reads and patches only one chunk's `Level.Biomes`
array: 256 columns before 1.15 and 1024 YZX cells from 1.15, mapped through the exact
release's numeric registry. It produces the same normalized 4×4×4 sections as the modern
decoder and never rewrites other NBT. This is not an I/O admission path: the region reader,
wizard and exporter still refuse these releases until the P2 adapter verifies them.

## Security boundaries

Imported files and MCP clients are untrusted input. The desktop process is authoritative.

- World import accepts classic Anvil 1.18–1.21.11 only: bounded `level.dat`, region files and
  chunk counts; external `.mcc`, LZ4 and custom compression are refused; offsets and
  sector lengths are validated.
- World `.zip` archives are extracted into a unique temporary staging folder and removed
  after analysis or import. Entry counts and uncompressed sizes are bounded; traversal,
  absolute paths, NUL/control characters, Windows reserved names, symlinks, special files
  and duplicate entries are refused, and files are created with `create_new`.
- Passthrough and export refuse symlinks, junctions and reparse points; the destination
  must be new and outside the source; copied bytes are hashed and re-read.
- NBT has hard limits on compressed/decompressed size, depth, collection length and name
  length; lengths are checked against remaining bytes before allocation.
- JAR/resource-pack/datapack archives are parsed in memory, never extracted, and reject
  traversal, absolute paths, NUL bytes, symlinks and oversized entries.
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
