# Reference (Windows beta 0.2.0)

Target: Minecraft Java **1.18–1.21.11** classic Anvil, with **1.21.4** / `DataVersion` 4189 as
the full baseline. Last reviewed: 2026-11-26.

The version picker reads the full official Mojang manifest. Every one of the 27 classic Anvil
releases from 1.18 through 1.21.11 has a profile that verifies import, block/biome editing,
project snapshots and same-version export, and enables generation with that release's exact
official server JAR. Snapshots, split 26.1+ releases and legacy/special ids are visible and are
refused at project creation rather than guessed.

The other 26 releases use the `read + write subset` support level; **1.21.4** remains the only
`full` profile. The difference is deliberate: structure `.nbt` interchange and the remaining
full-only surfaces are verified only for 1.21.4, while every modern classic release supports
block/biome editing, project snapshots, same-version export and generation with its exact
server JAR.

This page combines the supported baseline, the tested reference machine, feature
dependencies and the maintained limitation list. Format details are in
[formats.md](formats.md); content catalog rules are in [catalog.md](catalog.md);
architecture and security boundaries are in [architecture.md](architecture.md).

## Supported baseline

| Area             | Requirement                                                                                                                                        | Verification status                                                                                                                           |
| ---------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- |
| Operating system | 64-bit Windows 10 or Windows 11                                                                                                                    | Windows 11 Pro `10.0.26300` tested; Windows 10 is not tested                                                                                  |
| WebView2         | Microsoft Edge WebView2 Runtime                                                                                                                    | Preinstalled on current Windows 10/11; tested `157.0.4341.60`. The installer bootstrapper needs network only when the runtime is missing      |
| CPU              | 64-bit x86-64 CPU supported by the installed Windows version                                                                                       | Intel Core i5-9600KF (6 cores / 6 threads) tested; older/low-core CPUs and ARM64 are not tested                                               |
| Memory           | 8 GB is the practical baseline target; 16 GB is the tested reference                                                                               | 16 GB measured; 8 GB has **not** been validated                                                                                               |
| Graphics         | WebGL 2.0-capable GPU with a working Direct3D 11 path; software WARP/SwiftShader can run but is much slower                                        | One physical NVIDIA GeForce GTX 1060 3 GB (driver `32.0.15.6094`) tested; integrated GPUs, switching and other vendors are not tested          |
| Disk             | Keep at least 1 GB free for projects, checkpoints and component caches; managed Java and per-version client/server JARs add well over 100 MB | The 0.1.0 NSIS installer was 7.0 MB; Vanilla World dependencies took about 270 MB in the server test. The 0.2.0 package was not re-measured |
| Display/input    | A normal desktop session with a visible window; keyboard and mouse                                                                                 | MCP screenshots require a visible viewport; headless and minimized capture are not supported                                                  |
| Network          | Not required for `Empty Project`, `Flat base`, `Import World`, structure exchange or local MCP. The updater contacts GitHub Releases after startup | Core offline work is the design; optional downloads, update checks and the WebView2 bootstrapper need network when used                       |
| Permissions      | The NSIS package is per-user and does not require administrator rights                                                                             | NSIS install/update/uninstall tested without elevation                                                                                         |
| User data        | `%APPDATA%\com.furybuilder.desktop` for projects, settings, logs and optional runtime/server caches                                                | Installer and uninstall preserve this directory                                                                                               |

### Feature dependencies

| Feature                           | Minecraft install | Client JAR                    | Java                                | Official `server.jar` | Network                                           |
| --------------------------------- | ----------------- | ----------------------------- | ----------------------------------- | --------------------- | ------------------------------------------------- |
| `Empty Project` / `Flat base`     | No                | Optional; `Placeholder` works | No                                  | No                    | Only for managed downloads                        |
| `Import World` (1.18–1.21.11)     | No                | Optional; `Placeholder` works | No                                  | No                    | Only for managed downloads                        |
| Structure `.nbt` import/export    | No                | No                            | No                                  | No                    | No                                                |
| Real textures / exact catalog     | No                | Yes                           | No                                  | No                    | First download only; cached afterwards            |
| `Vanilla World` generation        | No                | Optional; `Placeholder` works | Yes, profile major (17 or 21)       | Yes                   | Downloads; generation can run offline after cache |
| MCP bridge (external AI client)   | No                | No                            | No                                  | No                    | No (local named pipe)                             |
| Installing a newer beta           | No                | No                            | No                                  | No                    | Yes (manual installer download)                   |

`Vanilla World` also requires the user to accept the Minecraft EULA for the exact server
JAR SHA-1 and Minecraft version, then confirm each local run separately. FuryBuilder never
accepts it automatically and never bundles Java or the server JAR.

Managed downloads honor `FURYBUILDER_PROXY` / `FURYBUILDER_HTTPS_PROXY` and the standard
proxy environment. `FURYBUILDER_OFFLINE=1` disables network access; cached version metadata
remains available, while uncached component downloads fail with an `offline` error.

### Tested reference machine

| Item              | Recorded value                                                                                                                 |
| ----------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| OS                | Windows 11 Pro, version `10.0.26300`, 64-bit                                                                                   |
| CPU               | Intel Core i5-9600KF, 6 cores / 6 threads                                                                                      |
| Memory            | 16 GB                                                                                                                          |
| GPU               | NVIDIA GeForce GTX 1060 3 GB, driver `32.0.15.6094`; only one physical display adapter                                         |
| WebView2          | `157.0.4341.60`                                                                                                                |
| Application       | FuryBuilder `0.2.0`                                                                                                            |
| Java (optional)   | Eclipse Temurin `21.0.13.1+2` for Vanilla World and server tests                                                               |
| Server (optional) | Official Minecraft Java `1.21.4` server JAR, SHA-1 `4707d00eb834b446575d89a61a11b5d548d8c001`                                  |
| MCP client        | OpenCode CLI `2.3.2` is the verified client; other clients are not guaranteed                                                  |

Only one physical GPU and one Windows 11 machine were used. Performance and rendering
results are a reference point, not a cross-vendor or minimum-hardware guarantee.

### Not tested / not supported

- Windows 10, Windows on ARM, Windows Server and non-Windows hosts.
- 8 GB RAM systems, lower-core CPUs and very old drivers.
- Integrated/discrete switching, multi-GPU laptops, other GPU vendors and remote-desktop
  display capture.
- Modded servers, data packs, other server versions, Bedrock worlds, snapshots, split 26.1+
  saves, legacy worlds and Minecraft versions outside 1.18–1.21.11. Other official versions
  are listed but cannot be created or edited. Java majors other than the profile's required
  major are not validated in this beta.
- Generation was exercised end to end on the 1.21.4 reference machine. The other 1.18–1.21.11
  profiles are covered by exact `level-type`, Java major, data pack format and
  `server.properties` tests, not by downloading every official server JAR in this beta.
- Code-signed installers and a stable (non-beta) update channel. Authenticode signing is
  still pending; the updater itself uses signed Tauri packages.
- Streamable HTTP or remote MCP; the bridge is local Windows stdio only.
- External beta-user field data. Recorded checks use internal harnesses and one machine.

## Known limitations

A limitation stays here until behaviour and tests actually change. Do not work around a
data-safety limitation by editing project files or the source world manually; create a
checkpoint and keep the original world untouched.

| ID    | Area             | Limitation                                                                             |
| ----- | ---------------- | -------------------------------------------------------------------------------------- |
| KL-01 | Platform         | Windows-first; macOS/Linux are not tested                                              |
| KL-02 | Platform         | Browser preview cannot edit, save, export or serve MCP                                 |
| KL-03 | Platform         | Single-user, single-instance desktop; no collaboration or cloud                        |
| KL-04 | Distribution     | Beta installers are not Authenticode-signed; signed GitHub auto-updates are configured |
| KL-05 | Version support  | Java 1.18–1.21.11 classic Anvil is import/edit/snapshot/export verified and can generate with its exact official server JAR. Split 26.x and legacy versions stop at the profile check |
| KL-06 | Import           | A bare `.mca` or `region/` is not a complete world; split saves get layout and header inspection only, and legacy import is refused |
| KL-07 | Import           | Only block states and 4x4x4 biomes are editable; other data is raw                     |
| KL-08 | Derived data     | Default preserve keeps stale heightmaps/light; rebuild is not a UI toggle              |
| KL-09 | Block entities   | Chests/signs/barrels are not semantically created, moved or updated                    |
| KL-10 | World export     | Complete world-folder export is verified for all modern classic releases and has no one-click UI action |
| KL-11 | Structures       | Interchange is Java structure `.nbt` only; entities are refused                        |
| KL-12 | Structures       | `.nbt` is not a full world and cannot carry FuryBuilder metadata                       |
| KL-13 | Vanilla World    | Requires the profile's Java major (17 for 1.18–1.19, 21 for newer releases), the exact official server JAR, EULA and possibly a download |
| KL-14 | Vanilla World    | Generation is bounded; only completed runs can become projects                         |
| KL-15 | Vanilla parity   | Parity check is untouched-revision block states only, not general parity               |
| KL-16 | Vanilla settings | Only `generate-structures` is exposed; data packs are validated against the exact data pack format, but their in-game effect is not verified |
| KL-17 | Editor           | Box selection is two clicks; no drag selection or multi-selection                      |
| KL-18 | Landscape        | Terrain/heightmap/generator tools are local and do not rebuild light                   |
| KL-19 | Brush            | Brush replaces every state in its sphere; no "only air" mode                           |
| KL-20 | Clipboard        | Clipboard is in-memory and is lost on restart                                          |
| KL-21 | History          | Undo history is bounded to 100; checkpoints are manual and capped at 5                 |
| KL-22 | Shortcuts        | Shortcuts are fixed; no user keybinding editor                                         |
| KL-23 | Build DSL        | Recipes are transient, house/repeat only, and budget-limited                           |
| KL-24 | Parametric       | Only house v1; manual edits block transform unless Override is chosen                  |
| KL-25 | Semantic objects | Metadata only; no MCP write path and no automatic recognition                          |
| KL-26 | Policy           | One active mask; no multi-select layers/masks                                          |
| KL-27 | References       | PNG/schematic reference overlays are researched but not enabled                        |
| KL-28 | Rendering        | Any reported dimension can be selected and streamed; only imported chunks are present |
| KL-29 | Assets           | Mojang assets are not bundled; managed client JAR, manual selection or explicit placeholder |
| KL-30 | Assets           | Model subset: no entity/block-entity models, fluid flow or animations                  |
| KL-31 | Performance      | Large-scene envelope is machine-specific; radius 32 is stress-only                     |
| KL-32 | Performance      | Only one physical GPU tested; WARP/SwiftShader are software                            |
| KL-33 | MCP              | Read-only by default; access resets on restart                                         |
| KL-34 | MCP              | Windows stdio named pipe only; no HTTP or remote transport                             |
| KL-35 | MCP              | Only OpenCode 2.3.2 is a verified real client; Tasks is fallback-only                  |
| KL-36 | MCP screenshots  | A visible viewport is required; no headless or minimized capture                       |
| KL-37 | MCP audit        | Activity journal is in-memory and bounded; no durable audit trail                      |
| KL-38 | Security         | Local OS account and client stdin memory are outside the threat model                  |
| KL-39 | Recovery         | Autosave runs every 30 s; a hard crash can lose up to 30 s of edits                    |
| KL-40 | Recovery         | A damaged autosave is not recovered; the last saved revision opens                     |
| KL-41 | Diagnostics      | Logs are local, bounded and redacted; no telemetry or crash upload                     |
| KL-42 | Project storage  | No project deletion or folder relocation in the UI; rename is display-only             |
| KL-43 | Release evidence | No external beta-user field data yet; checks use internal harnesses and one machine    |
| KL-44 | Catalog          | The exact-version catalog drives the virtualized Blocks palette and writes; complete blockstate sets use a constrained state picker, incomplete sets keep a warned text input, and imported undeclared values stay visible. Atlas icons load in visible-window batches and an isometric preview is shown; search uses a bounded trigram index. `not in this version` is limited to proven absence of an unknown selected state or a curated quick-pick id, not a full cross-version diff. Official creative tabs, the item browser, per-swatch color/shape/version filters and full model previews remain incomplete |
| KL-45 | Project formats  | Projects with older manifest, snapshot or checkpoint schemas are refused, not migrated |
