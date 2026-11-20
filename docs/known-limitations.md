# Known limitations

Status: **Windows beta 0.1.0**, target Minecraft Java **1.21.4 / DataVersion 4189**. Last reviewed:
2026-11-19.

This is the maintained, example-backed list of things FuryBuilder does not do, does only
partially, or does only on specific hardware. It is not a roadmap. A limitation stays here
until the behaviour, tests and documentation actually change. The supported baseline and
tested reference machine are in [`system-requirements.md`](system-requirements.md).

How to read an entry:

- **Status** — `by design`, `not implemented`, `environment-specific` or `pending M11`.
- **Example** — a concrete, user-visible scenario.
- **Workaround** — the safe path that exists today, or “none” when none exists.
- **Evidence** — the code or recorded run that supports the statement.

Never work around a data-safety limitation by editing project files or the source world
manually. Create a checkpoint first and keep the original world untouched.

## Summary

| ID    | Area             | Limitation                                                                             | Status               |
| ----- | ---------------- | -------------------------------------------------------------------------------------- | -------------------- |
| KL-01 | Platform         | Windows-first; macOS/Linux are not tested                                              | by design            |
| KL-02 | Platform         | Browser preview cannot edit, save, export or serve MCP                                 | by design            |
| KL-03 | Platform         | Single-user, single-instance desktop; no collaboration or cloud                        | by design            |
| KL-04 | Distribution     | Beta installers are not Authenticode-signed; signed GitHub auto-updates are configured | pending M11          |
| KL-05 | Version support  | Only Java 1.21.4 / DV 4189; other versions and Bedrock are refused                     | by design            |
| KL-06 | Import           | A bare `.mca` or `region/` is not a complete world; split/legacy refused               | by design            |
| KL-07 | Import           | Only block states and 4x4x4 biomes are editable; other data is raw                     | by design            |
| KL-08 | Derived data     | Default preserve keeps stale heightmaps/light; rebuild is not a UI toggle              | not implemented      |
| KL-09 | Block entities   | Chests/signs/barrels are not semantically created, moved or updated                    | not implemented      |
| KL-10 | World export     | Complete world-folder export has no one-click UI action                                | not implemented      |
| KL-11 | Structures       | Interchange is Java structure `.nbt` only; entities are refused                        | by design            |
| KL-12 | Structures       | `.nbt` is not a full world and cannot carry FuryBuilder metadata                       | by design            |
| KL-13 | Vanilla World    | Requires Java 21, official server JAR, EULA and possibly a download                    | by design            |
| KL-14 | Vanilla World    | Generation is bounded; only completed runs can become projects                         | by design            |
| KL-15 | Vanilla parity   | Parity check is untouched-revision block states only, not general parity               | by design            |
| KL-16 | Vanilla settings | Only `generate-structures` is exposed; data-pack behaviour is not tested               | not implemented      |
| KL-17 | Editor           | Box selection is two clicks; no drag selection or multi-selection                      | by design            |
| KL-18 | Landscape        | Terrain/heightmap/generator tools are local and do not rebuild light                   | by design            |
| KL-19 | Brush            | Brush replaces every state in its sphere; no “only air” mode                           | not implemented      |
| KL-20 | Clipboard        | Clipboard is in-memory and is lost on restart                                          | by design            |
| KL-21 | History          | Undo history is bounded to 100; checkpoints are manual and capped at 5                 | by design            |
| KL-22 | Shortcuts        | Shortcuts are fixed; no user keybinding editor                                         | by design            |
| KL-23 | Build DSL        | Recipes are transient, house/repeat only, and budget-limited                           | not implemented      |
| KL-24 | Parametric       | Only house v1; manual edits block transform unless Override is chosen                  | by design            |
| KL-25 | Semantic objects | Metadata only; no MCP write path and no automatic recognition                          | by design            |
| KL-26 | Policy           | One active mask; no multi-select layers/masks                                          | by design            |
| KL-27 | References       | PNG/schematic reference overlays are researched but not enabled                        | not implemented      |
| KL-28 | Rendering        | Nether/End are listed but only the overworld is streamed                               | not implemented      |
| KL-29 | Assets           | Mojang assets are not bundled; placeholders and approximate shapes                     | by design            |
| KL-30 | Assets           | Model subset: no entity/block-entity models, fluid flow or animations                  | not implemented      |
| KL-31 | Performance      | Large-scene envelope is machine-specific; radius 32 is stress-only                     | environment-specific |
| KL-32 | Performance      | Only one physical GPU tested; WARP/SwiftShader are software                            | environment-specific |
| KL-33 | MCP              | Read-only by default; access resets on restart                                         | by design            |
| KL-34 | MCP              | Windows stdio named pipe only; no HTTP or remote transport                             | by design            |
| KL-35 | MCP              | Only OpenCode 2.3.2 is a verified real client; Tasks is fallback-only                  | environment-specific |
| KL-36 | MCP screenshots  | A visible viewport is required; no headless or minimized capture                       | not implemented      |
| KL-37 | MCP audit        | Activity journal is in-memory and bounded; no durable audit trail                      | by design            |
| KL-38 | Security         | Local OS account and client stdin memory are outside the threat model                  | by design            |
| KL-39 | Recovery         | Autosave runs every 30 s; a hard crash can lose up to 30 s of edits                    | by design            |
| KL-40 | Recovery         | A damaged autosave is not recovered; the last saved revision opens                     | by design            |
| KL-41 | Diagnostics      | Logs are local, bounded and redacted; no telemetry or crash upload                     | by design            |
| KL-42 | Project storage  | No project deletion or folder relocation in the UI; rename is display-only             | not implemented      |
| KL-43 | Release evidence | No external beta-user field data yet; checks use internal harnesses and one machine    | pending M11          |

## Platform and distribution

### KL-01 — Windows-first, macOS/Linux untested

**Status:** by design. **Example:** the tested MCP endpoint is a Windows named pipe; the Unix
domain socket implementation exists but no macOS or Linux build, renderer, packaging or MCP
run has been validated. **Workaround:** use Windows 10/11 with WebView2. **Evidence:**
`docs/mcp-setup.md`, `docs/testing/mcp-compatibility.md`.

### KL-02 — Browser preview is not the editor

**Status:** by design. **Example:** `pnpm dev` opens the UI in a browser and shows
`Preview mode`; edits, project save/open, import/export and MCP are unavailable because the
Rust core is not attached. **Workaround:** run `pnpm tauri:dev` for the desktop app.
**Evidence:** `README.md`, `apps/desktop/src/features/project/ProjectManager.tsx`.

### KL-03 — Single-user desktop, no collaboration

**Status:** by design. **Example:** two FuryBuilder windows are not coordinated, and there is
no cloud sync, shared editing or server. The desktop also does not take an OS file lock to
detect a second live session. **Workaround:** use one instance per project; make a checkpoint
before copying a project folder. **Evidence:** `docs/design/projects.md` (M5 limitations).

### KL-04 — Beta installers are unsigned; signed auto-updates are configured

**Status:** pending M11 (external code-signing certificate). **Example:** the NSIS
package is built, installed and update-tested, but it is not Authenticode-signed and
Windows SmartScreen can warn about an unknown publisher. The Tauri updater is configured
against GitHub Releases: installed builds check after startup, verify the matching NSIS
package with the embedded updater public key, and preserve projects and settings.
Update signing is a
separate minisign key stored in repository secrets, not an Authenticode certificate.
**Workaround:** verify the published SHA-256 before a manual install, keep a project backup
before updating, and use **Settings → Updates** for the signed automatic update path.
**Evidence:** `docs/install-windows.md`, `docs/release/versioning.md`,
`docs/legal/security-audit.md`,
`docs/testing/evidence/2026-11-19-release-security/signature-evidence.json`.

## Minecraft versions, import and export

### KL-05 — Only Java 1.21.4 / DataVersion 4189

**Status:** by design. **Example:** analysing a 1.21.5 world reports
`detected_unsupported` / `version_refused`; Bedrock worlds, modded custom dimensions and
unknown `DataVersion`s are refused instead of guessed. **Workaround:** convert the world to
classic 1.21.4 with a trusted tool first; keep the original. **Evidence:**
`docs/formats/support-matrix.md`.

### KL-06 — A region is not a world

**Status:** by design. **Example:** selecting `r.0.0.mca` or a bare `region/` directory shows
an incomplete-selection error; split 26.1+ layouts, mixed layouts and legacy `.mcr` files are
detected and refused. **Workaround:** select the folder containing `level.dat` and the whole
classic world layout. **Evidence:** `docs/formats/java-world-selection.md`.

### KL-07 — Imported editing is block/biome only

**Status:** by design. **Example:** a chest is preserved byte-for-byte, but editing the block
under it does not move or update its block entity, and new POI/ticks are not synthesized.
Heightmaps, light, ticks, entities, block entities and structure references remain raw source
data. **Workaround:** let the target vanilla engine rebuild what it can, or edit those data in
a tool that supports them; keep the immutable source. **Evidence:**
`docs/formats/support-matrix.md`, `docs/formats/java-chunk-derived-data.md`.

### KL-08 — Derived-data policy has no UI toggle

**Status:** not implemented. **Example:** the default `Preserve` policy copies stale
heightmaps and light into a rebuilt region; a normal vanilla server load does not recalculate
them. `VanillaRebuild` invalidates heightmaps, light, `isLightOn` and `PostProcessing` for the
engine, but today it is a backend/test policy (`world_export` examples), not a checkbox in the
desktop UI. **Workaround:** no user-facing switch exists; do not assume derived data is
current after a block edit. **Evidence:** `docs/formats/java-world-export.md`,
`docs/testing/evidence/2026-11-18-derived-rebuild-1.21.4.json`.

### KL-09 — Block entities and POI are not semantic

**Status:** not implemented. **Example:** placing a chest block through the block-state writer
does not create a chest block entity; removing a barrel does not update its POI. The official
server fixture confirms it does not synthesize a missing block entity on load.
**Workaround:** avoid creating/removing block-entity-dependent blocks for a world you will use
in-game; edit them in-game or with a tool that supports block entities. **Evidence:**
`docs/formats/support-matrix.md`, `docs/testing/minecraft-server-fixture.md`.

### KL-10 — Complete world export is not exposed in the UI

**Status:** not implemented. **Example:** `Import / Export` in the Project dialog offers
structure `.nbt` import/export. The complete world-folder export exists and is verified in the
Rust/backend path, but there is no one-click “export world copy” action in this beta.
**Workaround:** none in the UI; use the verified backend/test command only if you are
developing and understand the output. **Evidence:** `docs/formats/java-world-export.md`,
`docs/testing/java-server-interop.md`,
`docs/testing/evidence/2026-11-19-java-server-interop/evidence.json` and
`apps/desktop/src-tauri/src/lib.rs` command list.

### KL-11 — Structure interchange is `.nbt` only

**Status:** by design. **Example:** choosing a `.schem`, `.litematic`, `.schematic` or
`.mcstructure` file is not supported; exporting a selection containing a chest, sign or other
block entity is refused rather than silently dropping it. Unknown tags are also refused.
**Workaround:** remove unsupported block entities from the export selection or use a tool
with a native adapter. **Evidence:** `docs/formats/java-structure-nbt.md`,
`docs/design/security.md`.

### KL-12 — A structure is not a world

**Status:** by design. **Example:** a `.nbt` export contains blocks/orientations in a bounded
box; it has no `level.dat`, dimensions, biomes, entities, history, layers, semantic objects,
Build DSL recipes or project provenance. **Workaround:** keep the FuryBuilder project as the
source of truth and treat `.nbt` as a one-way construction snippet. **Evidence:**
`docs/formats/java-structure-nbt.md`.

## Vanilla World

### KL-13 — Vanilla World has real prerequisites

**Status:** by design. **Example:** without Java 21, the official server JAR and an accepted
EULA, the readiness card lists blockers and generation cannot start. On a clean offline
machine the first run also needs the dependencies to be supplied locally or downloaded with
explicit consent. **Workaround:** use `Empty Project`/`Flat base` with no Java; configure
dependencies in `Settings → Java & World Generation` when you need vanilla generation.
**Evidence:** `docs/runtime/vanilla-generation.md`.

### KL-14 — Bounded generation, completed runs only

**Status:** by design. **Example:** a run that reaches its time limit or is cancelled stays
visible for inspection but cannot be opened as a project; generation produces the requested
area (plus spawn chunks), not an infinite lazy world. **Workaround:** request a larger area or
run again with higher limits, then open only a completed run. **Evidence:**
`docs/runtime/vanilla-generation.md` (Limits).

### KL-15 — Parity is deliberately narrow

**Status:** by design. **Example:** the parity panel refuses a saved base at revision 2 and
refuses a second run with a different seed or preset. A successful check compares block
states in the requested overworld area only; it does not compare biomes, heightmaps,
lighting, entities or POI. **Workaround:** compare an untouched revision-1 base, or use exact
block queries and an external reference for the rest. **Evidence:**
`docs/testing/worldgen-parity.md`.

### KL-16 — Vanilla settings are intentionally narrow

**Status:** not implemented. **Example:** the wizard exposes `generate-structures` but not the
full set of `generator-settings`; data packs must be local `.zip` files with a compatible
declared format, and their game behaviour/dependencies/worldgen effects are not validated.
**Workaround:** use supported defaults, or pre-validate data packs in a real server. **Evidence:**
`docs/runtime/vanilla-generation.md`.

## Editor workflows

### KL-17 — Two-click box selection

**Status:** by design. **Example:** press `5`, click corner A, click corner B; there is no
drag-rubber-band selection and no multi-selection. **Workaround:** use one box at a time and
keep checkpoints around large edits. **Evidence:** `docs/design/editor.md`.

### KL-18 — Landscape tools are local construction tools

**Status:** by design. **Example:** a heightmap patch with seed 42 is reproducible, but it is
not a Minecraft-generated biome or terrain; terrain brushes, roads, rivers and vegetation
change block states only and do not recalculate heightmaps, light or biomes. **Workaround:**
paint biomes explicitly in `Biomes`; let the target engine rebuild derived data where
supported; do not claim vanilla parity. **Evidence:** `docs/design/editor.md`,
`docs/advanced-workflow.md`.

### KL-19 — Brush has no “only air” mode

**Status:** not implemented. **Example:** a radius-2 brush overwrites stone, water and air
alike inside its sphere. **Workaround:** use `Replace` for state-specific changes or a
smaller radius; undo is available. **Evidence:** `docs/design/editor.md` (M4 limitations).

### KL-20 — Clipboard is session-only

**Status:** by design. **Example:** copying a build, closing FuryBuilder and reopening the
project leaves Paste empty. **Workaround:** save the selection as a template in `Templates`
for reuse across sessions. **Evidence:** `apps/desktop/src/state/tool-store.ts` (not
persisted), `docs/design/editor.md`.

### KL-21 — Bounded history and manual checkpoints

**Status:** by design. **Example:** after more than 100 operations, the oldest undo entry is
gone; only five checkpoints are kept per project, and there is no checkpoint browser for
projects that are not open. **Workaround:** create a named checkpoint before a large or risky
step. **Evidence:** `crates/world-application/src/service.rs`,
`docs/design/projects.md`.

### KL-22 — Fixed shortcuts and placeholder menu items

**Status:** by design. **Example:** tools `1`-`8`, `Ctrl+Z/Y`, `Ctrl+K`, `Ctrl+B`,
`Ctrl+Alt+B`, `Ctrl+J` are fixed; there is no keybinding editor. `Tools` and `Export` in the
main toolbar are disabled placeholders because those actions live elsewhere. **Workaround:**
use the command palette (`Ctrl+K`) and the viewport/Inspector controls. **Evidence:**
`docs/design/editor.md`, `apps/desktop/src/features/workspace/Toolbar.tsx`.

### KL-23 — Build DSL is transient and narrow

**Status:** not implemented (persistence/coverage). **Example:** a valid JSON house/repeat
recipe previews and applies, but closing the project loses the recipe; only house v1 and
repeat nodes exist, with at most 64 houses/16,384 blocks and depth 4. **Workaround:** keep the
recipe in an external file and paste it again; save the resulting build or a template.
**Evidence:** `docs/design/editor.md` (Build DSL).

### KL-24 — Parametric authoring is house-only

**Status:** by design. **Example:** only generator `house` version 1 is accepted. After a
manual edit inside the generated overlay, transform or rollback is refused with a conflict;
only an explicit `Override` accepts the loss. **Workaround:** bake/detach first, or resolve the
manual edits and rebake. **Evidence:** `docs/design/editor.md` (Parametric).

### KL-25 — Semantic objects are metadata

**Status:** by design. **Example:** `Objects` can name a district, building or road, but it
never places blocks, is not recognised automatically from an imported world, and has no MCP
write tool. **Workaround:** use it for planning/inspection; block edits still go through the
normal operation path. **Evidence:** `docs/design/editor.md` (Semantic objects).

### KL-26 — One active mask

**Status:** by design. **Example:** only one mask can constrain edits at a time; layers,
masks and protected regions are capped at 64 each. **Workaround:** switch masks deliberately
and keep protected regions for hard boundaries. **Evidence:** `docs/design/editor.md`
(Layers, masks, protection).

### KL-27 — Reference overlays are not enabled

**Status:** not implemented. **Example:** there is no PNG blueprint plane or `.nbt` schematic
ghost in the viewport; the work is research/design only in `docs/research/reference-overlays.md`.
**Workaround:** none in the editor; use external reference tools until the overlay ships.
**Evidence:** `docs/research/reference-overlays.md`.

### KL-28 — Only the overworld is streamed

**Status:** not implemented. **Example:** the World panel lists the Nether and End, but they
are marked not loaded and selecting them does not stream a dimension. **Workaround:** edit the
overworld; use an external viewer for other dimensions today. **Evidence:**
`apps/desktop/src/features/workspace/WorldTree.tsx`.

## Rendering, assets and performance

### KL-29 — No bundled Minecraft assets

**Status:** by design. **Example:** without a verified 1.21.4 client/resource pack, the
viewport shows original placeholder textures and approximate shapes (slabs, stairs, torches)
and a missing checker for unresolved models. Block data is unaffected. **Workaround:**
configure `Settings → Minecraft assets` or use placeholders for `Empty Project`. **Evidence:**
`docs/assets/minecraft-assets.md`.

### KL-30 — Asset renderer supports a subset

**Status:** not implemented. **Example:** `uvlock`/element `rescale`/`ambientocclusion=false`
are recorded but not applied; biome tint is a fixed palette; chests/signs and other builtin
entity models are not rendered; fluids use a cube approximation; animated textures show only
the first frame; mipmaps are disabled. **Workaround:** treat the viewport as an editor preview,
not pixel-perfect Minecraft. **Evidence:** `docs/assets/minecraft-assets.md` (renderer
limitations).

### KL-31 — Large-scene envelope is machine-specific

**Status:** environment-specific. **Example:** on the recorded GTX 1060 3 GB machine, radius 16
(1,089 chunks) is comfortable, radius 24 (2,401 chunks) is the upper playable bound and radius
32 (4,225 chunks) is a stress ceiling: 6.4 s snapshot setup, about 1 GB observed heap and
71 FPS overview. The renderer caps residency at 4,096 chunks with farthest-first eviction.
**Workaround:** keep normal editing around radius 16; raise it deliberately. **Evidence:**
`docs/testing/perf-runs.md` (M9.23/M10.2).

### KL-32 — One physical GPU, no GPU timers

**Status:** environment-specific. **Example:** the long-edit evidence covers one NVIDIA GTX
1060 through D3D11, plus Microsoft WARP and SwiftShader software paths (median 31 and 15 FPS).
It does not cover integrated/discrete switching or a second vendor; frame times are
renderer/JS timings, not GPU timer queries, and the longest hardware session was 10 minutes,
not an hours-long thermal soak. **Workaround:** use the hardware D3D11 numbers only as a
single-machine reference and validate on your own GPU. **Evidence:**
`docs/testing/evidence/2026-11-19-long-edit-gpu/`, `docs/testing/perf-runs.md`.

## AI and MCP

### KL-33 — Read-only by default and reset on restart

**Status:** by design. **Example:** `apply_operation` in the default policy returns
`permission_denied`; even after granting `Edit in selection`, restarting FuryBuilder returns to
read-only and a narrowed policy is re-checked after confirmation. **Workaround:** open
`AI / MCP`, choose the narrowest write mode and approve each write. **Evidence:**
`docs/mcp-setup.md`.

### KL-34 — Local Windows stdio transport only

**Status:** by design. **Example:** clients launch `furybuilder-mcp.exe` (installed next to
FuryBuilder or built from `crates/mcp-bridge`), which talks over a per-user Windows named
pipe; there is no TCP/HTTP listener, remote access or tested Unix socket path.
**Workaround:** run the client and FuryBuilder as the same Windows user on the same machine.
**Evidence:** `docs/mcp-setup.md`, `docs/testing/mcp-compatibility.md`.

### KL-35 — Client compatibility is not universal

**Status:** environment-specific. **Example:** OpenCode CLI 2.3.2 was the only real external
client exercised end-to-end through the former TypeScript bridge; the Rust bridge is
protocol-tested over stdio but a live-client rerun is still outstanding. Tasks is not
implemented as an extension, so the bridge exposes fallback
`start_job`/`get_job_status`/`cancel_job` tools; MCP Apps is optional and depends on
host rendering. **Workaround:** use the documented fallback tools/resources; do not assume
Tasks or Apps. **Evidence:** `docs/testing/mcp-compatibility.md`, `docs/mcp-setup.md`.

### KL-36 — Screenshots need a visible viewport

**Status:** not implemented. **Example:** `capture_viewport` fails with
`backend_unavailable` when no WebView viewport is attached; a minimized/headless window is not
promised to capture. A capture can also be `partial` or `stale` and says so. **Workaround:**
keep the project and desktop window open; use `observe` maps for deterministic read-only maps.
**Evidence:** `docs/mcp-setup.md`, `docs/testing/mcp-compatibility.md`.

### KL-37 — No durable MCP audit trail

**Status:** by design. **Example:** the MCP activity journal holds 1,000 in-memory entries and
is lost on restart; policy is process-wide, not per connection. **Workaround:** record
important changes as checkpoints and review the History panel during the session. **Evidence:**
`docs/design/security.md`, `docs/mcp-setup.md`.

### KL-38 — Residual security boundaries

**Status:** by design. **Example:** the app cannot protect a project from the same OS account
that can edit its files; a hostile MCP client owns its own stdin memory; the discovery token
relies on the per-user app-data ACL; no third-party dependency audit or release signing has
been completed yet. **Workaround:** keep the app data ACL user-private, use read-only mode when
not editing, and do not run untrusted MCP clients. **Evidence:** `docs/design/security.md`.

## Recovery, diagnostics and storage

### KL-39 — Autosave can lose up to 30 seconds

**Status:** by design. **Example:** a hard crash after 29 seconds of unsaved edits can lose
that window; autosave is interval/visibility/close driven, not after every block. **Workaround:**
`Ctrl+S` before risky steps and create checkpoints. **Evidence:** `docs/design/projects.md`.

### KL-40 — A damaged autosave is not recovered

**Status:** by design. **Example:** if the autosave metadata/checksum/round-trip fails, the app
opens the last saved revision with an explicit warning and `available = false`; it does not
pretend the newer edits were restored. **Workaround:** keep the warning/artifacts for
diagnostics; recover from the saved revision or a checkpoint. **Evidence:**
`docs/design/projects.md`, `docs/design/diagnostics.md`.

### KL-41 — Diagnostics are local and bounded

**Status:** by design. **Example:** logs are JSON Lines, 2 MiB per file with four files, and
paths/control characters are redacted. There is no telemetry, crash upload or remote support;
clearing removes only `furybuilder*.log`. **Workaround:** use `Settings → Diagnostics` to open
the folder and attach the log manually to a report. **Evidence:** `docs/design/diagnostics.md`.

### KL-42 — Project storage is application-managed

**Status:** not implemented (management). **Example:** projects live in the FuryBuilder
app-data directory; the UI has no project deletion or folder relocation, and renaming changes
the display name only. A second live session is not detected with an OS lock. **Workaround:**
close the app before moving/copying project folders manually, and make a checkpoint/backup
first. **Evidence:** `docs/design/projects.md`.

### KL-43 — No external beta-user field data yet

**Status:** pending M11. **Example:** the release checks are automated harnesses and recorded
runs on one Windows 11 reference machine. No external user has yet completed the beta
scenarios on a clean Windows installation without Minecraft/Java, another launcher, another
GPU, offline mode or a corrupted cache. **Workaround:** treat 0.1.0 as a beta candidate, keep
the original world and checkpoints, and report reproducible steps; do not assume an untested
configuration is supported because the app opens. **Evidence:**
`docs/system-requirements.md`, `docs/testing/java-server-interop.md`,
`docs/testing/test-matrix.md`.

## Not limitations (intentional non-goals)

These are out of scope for the beta, not bugs to report:

- FuryBuilder is not a game client and does not simulate ticks, redstone, mob AI, fluid flow or
  gameplay. Only `Vanilla World` uses the original Minecraft generator.
- Bedrock Edition conversion, online multiplayer, server hosting, a marketplace, cloud storage
  and a web/WASM build are future work, not current features.
- Build DSL does not execute JavaScript, Python or shell code from an AI client.
- Mojang `client.jar`, textures and `server.jar` are never redistributed with FuryBuilder; the
  user supplies or explicitly downloads them.

## Reporting a limitation or a bug

1. Reproduce on the current beta with the smallest project you can share.
2. Note the exact mode (`Empty`, `Flat base`, `Import World`, `Vanilla World`), Minecraft
   target `1.21.4`/DV `4189`, Windows/WebView2 versions and whether assets/Java/server were
   configured.
3. Open `Settings → Diagnostics`, refresh, and attach the relevant `furybuilder*.log` after
   checking it for private paths.
4. State whether a checkpoint or saved revision still opens. Do not attach a world you do not
   have permission to share.

See also the [test matrix](testing/test-matrix.md) for what is actually verified and the
[advanced workflow guide](advanced-workflow.md) for the safe operating procedure.
