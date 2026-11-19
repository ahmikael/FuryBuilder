# Java world data support matrix (M8.13)

This is the consolidated answer to "what can FuryBuilder actually read, edit,
write and pass through?" for Java worlds. It is intentionally conservative: a
field may survive an export as raw bytes without being understood or
recalculated.

Machine-readable source: `crates/world-io/src/support_matrix.rs`. The same data
is returned as `supportMatrix` by `analyze_java_world` and rendered in the
`Import World` wizard. Tests in that module fail if the matrix claims a write
outside the verified field set or a supported write for an unsupported version.

## Legend

| Value                              | Meaning                                                                 |
| ---------------------------------- | ----------------------------------------------------------------------- |
| `supported`                        | Semantically decoded or exposed by a verified path                      |
| `supported_subset`                 | A verified subset is decoded/edited; not the whole save                 |
| `block_states_and_biomes`          | `sections[].block_states` and 4x4x4 `sections[].biomes` can be edited   |
| `verified`                         | Re-read, re-decoded and compared after writing                          |
| `verified_block_states_and_biomes` | A new region/folder is written and verified for block states and biomes |
| `preserved_raw`                    | Copied unchanged; never interpreted or recalculated                     |
| `byte_for_byte`                    | File or untouched record is copied as exact bytes                       |
| `preserve_or_invalidate`           | Policy-dependent: raw preserve, or explicit engine rebuild hand-off     |
| `inventoried`                      | Counted/inspected read-only; not editable                               |
| `not_exposed`                      | Present and preserved if copied, but no public API/UI for it            |
| `none` / `refused` / `skipped`     | Not available, explicitly refused, or deliberately omitted              |

## By version

| Version family      | DataVersion | Layout                            | Status                 | Read                        | Edit                      | Write                              | Passthrough     | Note                                                                                                              |
| ------------------- | ----------- | --------------------------------- | ---------------------- | --------------------------- | ------------------------- | ---------------------------------- | --------------- | ----------------------------------------------------------------------------------------------------------------- |
| Java 1.21.4         | 4189        | classic Anvil                     | **supported**          | `supported_subset`          | `block_states_and_biomes` | `verified_block_states_and_biomes` | `byte_for_byte` | Only target with verified decode, block/biome edit, region writer, full-folder export and official-server fixture |
| Java 1.21.5–1.21.11 | 4325–4671   | classic Anvil                     | `detected_unsupported` | `version_refused`           | `none`                    | `none`                             | `none`          | Layout is recognisable, version is refused                                                                        |
| Java 26.1–26.3      | 4786–5023   | split (`dimensions/`, `players/`) | `detected_unsupported` | `layout_and_region_headers` | `none`                    | `none`                             | `none`          | Layout and region headers are inventoried read-only; no split chunk decoding                                      |
| Java 26.4 snapshots | 5119–5130+  | split                             | `out_of_scope`         | `none`                      | `none`                    | `none`                             | `none`          | Moving target                                                                                                     |
| Pre-1.13            | varies      | McRegion (`region/*.mcr`)         | `detected_unsupported` | `legacy_marker`             | `none`                    | `none`                             | `none`          | Legacy marker is detected and refused                                                                             |
| Bedrock Edition     | n/a         | Bedrock                           | `out_of_scope`         | `none`                      | `none`                    | `none`                             | `none`          | Different format family                                                                                           |

A complete 26.1+ split save is structurally complete, but it is not readable,
editable or comparable to a supported save. A bare `.mca` file or a `region/`
directory is not a complete save at all; see
`docs/formats/java-world-selection.md`.

## By data category (Java 1.21.4 classic)

| Category                             | Read            | Edit                 | Write                       | Passthrough     | Note                                                                                                                         |
| ------------------------------------ | --------------- | -------------------- | --------------------------- | --------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `level.dat` metadata                 | `supported`     | `not_exposed`        | `preserved_raw`             | `byte_for_byte` | `DataVersion`, `Version.Id`, `LevelName`, `WorldGenSettings` are validated; `level.dat` is not rewritten by the block writer |
| Region containers                    | `supported`     | `chunk_records_only` | `verified_replaced_regions` | `byte_for_byte` | Untouched region files/records are exact bytes; only dirty regions are rebuilt                                               |
| Block states and palettes            | `supported`     | `supported`          | `verified`                  | `byte_for_byte` | Only `sections[].block_states` in fully generated `minecraft:full` chunks                                                    |
| Biomes                               | `supported`     | `supported`          | `verified`                  | `byte_for_byte` | Whole 4x4x4 cells are editable; palette <= 64; sections without source biome data are refused; untouched cells are preserved |
| Block entities                       | `inventoried`   | `not_exposed`        | `preserved_raw`             | `byte_for_byte` | Counted and preserved; missing block entities are not synthesized                                                            |
| Chunk entities                       | `inventoried`   | `not_exposed`        | `preserved_raw`             | `byte_for_byte` | Legacy chunk entity lists are counted and preserved                                                                          |
| `entities/` sidecars                 | `inventoried`   | `not_exposed`        | `preserved_raw`             | `byte_for_byte` | Read schema-neutrally and copied exactly                                                                                     |
| Block/fluid ticks                    | `inventoried`   | `not_exposed`        | `preserved_raw`             | `byte_for_byte` | Stale ticks are preserved; new blocks get no scheduled ticks                                                                 |
| Heightmaps                           | `inventoried`   | `not_exposed`        | `preserve_or_invalidate`    | `byte_for_byte` | `Preserve` keeps raw values; `VanillaRebuild` removes them for the engine                                                    |
| SkyLight / BlockLight / `isLightOn`  | `inventoried`   | `not_exposed`        | `preserve_or_invalidate`    | `byte_for_byte` | `VanillaRebuild` removes section light and sets `isLightOn = 0`                                                              |
| `PostProcessing`                     | `inventoried`   | `not_exposed`        | `preserve_or_invalidate`    | `byte_for_byte` | `VanillaRebuild` removes pre-edit queued positions                                                                           |
| `poi/` sidecars                      | `inventoried`   | `not_exposed`        | `preserved_raw`             | `byte_for_byte` | FuryBuilder never rewrites POI; one server-added barrel POI is not a guarantee                                               |
| Structures and references            | `not_exposed`   | `not_exposed`        | `preserved_raw`             | `byte_for_byte` | Raw starts/references survive; block edits do not update them                                                                |
| Unknown chunk tags                   | `preserved_raw` | `not_exposed`        | `preserved_raw`             | `byte_for_byte` | Compared with the source and must stay unchanged                                                                             |
| Unknown files/directories            | `preserved_raw` | `not_exposed`        | `preserved_raw`             | `byte_for_byte` | Copied, not interpreted or dropped                                                                                           |
| `playerdata/`, `data/`, `datapacks/` | `preserved_raw` | `not_exposed`        | `preserved_raw`             | `byte_for_byte` | Copied as whole files/folders; internals are not semantically validated                                                      |
| Custom dimensions                    | `refused`       | `not_supported`      | `not_supported`             | `refused`       | Classic worlds declaring custom dimensions are refused                                                                       |
| `session.lock`                       | `detected`      | `not_supported`      | `skipped`                   | `skipped`       | Transient game lock; the only deliberately skipped file                                                                      |

## What "write" does not mean

- It does not mean the field is recalculated. A `preserved_raw` write copies the
  source representation.
- `heightmaps`, `lighting` and `PostProcessing` under `VanillaRebuild` are not
  written with new values. They are removed/disabled so the target vanilla engine
  rebuilds them. This was verified with an official 1.21.4 server:
  `docs/testing/evidence/2026-11-18-derived-rebuild-1.21.4.json`.
- Block entities, entities, POI and ticks are not synthesized, moved or cleared.
  Placing or removing a chest/sign/barrel through the block-state writer is not
  semantically supported.
- Biome edits write only the selected 4x4x4 cells into `sections[].biomes`. FuryBuilder
  does not run vanilla biome generation or derive biomes from block states; a world
  without biome data is refused instead of receiving a default biome.
- The full-folder export must never be confused with a single `.mca` file. The
  destination is a new complete save folder, verified by reopening it and by an
  official-server load/save fixture:
  `docs/testing/evidence/2026-11-18-server-fixture-1.21.4.json`.

## Evidence

| Claim                                   | Evidence                                                                                                                       |
| --------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| Real 1.21.4 read/decode                 | `2026-11-18-anvil-read-1.21.4.json`, `2026-11-18-anvil-decode-1.21.4.json`, `2026-11-18-world-layout-1.21.4.json`              |
| Real 1.21.4 passthrough and overlay     | `2026-11-18-anvil-passthrough-1.21.4.json`, `2026-11-18-anvil-overlay-1.21.4.json`                                             |
| Real 1.21.4 import                      | `2026-11-18-world-import-1.21.4.json`                                                                                          |
| Verified region write                   | `2026-11-18-region-write-1.21.4.json`                                                                                          |
| Derived data inventory and policies     | `2026-11-18-derived-inventory-1.21.4.json`, `2026-11-18-derived-data-1.21.4.json`, `2026-11-18-derived-rebuild-1.21.4.json`    |
| Complete world-folder export            | `2026-11-18-world-export-1.21.4.json`                                                                                          |
| Save vs region file vs region directory | `2026-11-18-world-selection-1.21.4.json`                                                                                       |
| Official-server load/save fixture       | `2026-11-18-server-fixture-1.21.4.json`                                                                                        |
| M11.5 app import/edit/export round trip | `2026-11-19-java-server-interop/evidence.json` — app import, edit, backend export, official-server verification, app re-import |
| Real biome paint UI                     | `2026-11-19-biome-ui.json`, `.png`, `-applied.png`                                                                             |

## Reproduce

```powershell
cargo test -p furybuilder-world-io --lib support_matrix
cargo test -p furybuilder-world-io --test world_selection --test world_export --test overlay --test region_write
cargo test -p furybuilder-project-store snapshot::tests
cargo run -p furybuilder-world-io --example world_selection_probe -- <path>
pwsh -File scripts/verify_java_world_export.ps1 -Java <java.exe> -ServerJar <server.jar> -ExpectedSha1 4707d00eb834b446575d89a61a11b5d548d8c001 -AcceptMinecraftEula
node scripts/java-server-interop-webview2.mjs <new-output-directory> <java.exe> <server.jar> 4707d00eb834b446575d89a61a11b5d548d8c001 --accept-minecraft-eula
```
