# Test matrix

Status: initial M0 matrix. Rows become green only with an automated or recorded manual run.

## Scenarios

| ID   | Scenario                          | Expected                                            | Automated layer            |
| ---- | --------------------------------- | --------------------------------------------------- | -------------------------- |
| T-01 | Empty world, no chunks            | 0 chunks, 0 blocks, revision 0                      | Rust unit                  |
| T-02 | Set/get one block                 | same state incl. properties                         | Rust unit + contract       |
| T-03 | Small house 9x5x7                 | exact block count, palette matches                  | Rust unit                  |
| T-04 | Chunk boundary X=15/16            | no gaps, both chunks dirty                          | Rust unit + renderer       |
| T-05 | Negative coordinates (-1,-17,-33) | floor division to correct chunk/section             | Rust unit                  |
| T-06 | Y outside dimension range         | structured `OutOfBounds` error                      | Rust unit                  |
| T-07 | Chest with NBT                    | block entity preserved through export/reimport      | Rust + golden              |
| T-08 | Biome palette                     | biome data round-trips                              | Rust + golden              |
| T-09 | Two dimensions                    | identical coordinates do not collide                | Rust unit                  |
| T-10 | Large region (>= 1 region file)   | bounded memory, explicit progress                   | Rust benchmark/integration |
| T-11 | Unknown NBT fields                | passthrough preserved or export refused             | Rust integration           |
| T-12 | Undo after 10k-block batch        | world equals pre-batch snapshot                     | Rust unit                  |
| T-13 | Idempotency key replay            | second call is a no-op                              | Rust unit                  |
| T-14 | Stale `expected_revision`         | conflict error, no write                            | Rust unit                  |
| T-15 | Read-only MCP write               | rejected by Core policy                             | Rust integration           |
| T-16 | Selection boundary edit           | rejected when outside allowed scope                 | Rust integration           |
| T-17 | Crash during save                 | previous checkpoint recoverable, no partial publish | Rust integration           |
| T-18 | `.nbt` structure round-trip       | semantic equality of states/positions               | Rust golden                |
| T-19 | Malformed NBT / zip bomb          | bounded error, no panic                             | Rust fuzz/unit             |
| T-20 | Mesh at chunk seam                | no holes/stale faces                                | Renderer test              |

## Format fixtures

Minimum fixtures (self-authored or legally redistributable):

1. Empty structure (0 blocks).
2. 1-block structure with a directional block (`oak_stairs[facing=east]`).
3. Mixed palette with air gaps.
4. Negative-position structure with offset.
5. Structure crossing chunk boundaries.
6. Structure with a chest block entity.
7. Two-dimension sample world (for import tests).
8. Corrupted/truncated NBT sample.
9. Region file with negative region coordinates.
10. Unknown-tag passthrough sample.

Fixtures stay under `fixtures/` and must not contain Mojang binaries or textures.

## Manual / external checks

- Open exported `.nbt` in Minecraft 1.21.4 structure block or a compatible editor.
- Open exported world copy in Minecraft 1.21.4 client/server.
- Verify MCP with at least one real external client (OpenCode).
- Long-session memory/FPS run recorded in `docs/testing/perf-runs.md` once available.

## M8 world import

| Requirement                                       | Automated / recorded evidence                                                                                                                                                                                                                                                                                     |
| ------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Distinguish classic and 26.1+ split world folders | `scripts/inspect_world_layout.py`; `scripts/test_inspect_world_layout.py` (classic, split, legacy `.mcr`, mixed markers, unknown folders)                                                                                                                                                                         |
| Both `level.dat` versions agree                   | Inspector reads `Data.DataVersion` and `Data.Version.Id`; a synthetic disagreement is flagged; malformed gzip/NBT, trailing bytes, duplicate tags and symlinked `level.dat` are refused                                                                                                                           |
| Real 1.21.4 classic world                         | `docs/testing/evidence/2026-11-18-world-layout-1.21.4.json` — official server world, revision 4189, 4 Overworld region files, `targetMatch: true`                                                                                                                                                                 |
| Real 26.3 split world                             | `docs/testing/evidence/2026-11-18-world-layout-26.3.json` — official server world, revision 5023, all dimensions under `dimensions/minecraft/`, `version_history: [5023]`, `targetMatch: false`                                                                                                                   |
| Current release metadata and DataVersion          | `docs/testing/evidence/2026-11-18-world-layout-26.3-server-version.txt` — official server SHA-1, `world_version: 5023`, Java 25, protocol 777; release/layout table in `docs/formats/java-world-layouts.md`                                                                                                       |
| Read classic `level.dat` + region `.mca`/NBT      | `crates/world-io/src/anvil.rs`; `crates/world-io/tests/anvil.rs` — target version, agreeing `DataVersion`/`Version.Id`, zlib/gzip/uncompressed chunks, negative region/chunk coordinates, empty `TAG_End` lists                                                                                                   |
| Region and chunk structural fail-closed checks    | `crates/world-io/tests/anvil.rs` — overlap, header offsets, short/unaligned files, bad names, oversized length, wrong position/version, trailing NBT, external `.mcc`, LZ4/custom compression, symlinks                                                                                                           |
| Real 1.21.4 region read                           | `docs/testing/evidence/2026-11-18-anvil-read-1.21.4.json` — 4 regions, 841 chunks all `DataVersion` 4189, 24 sections, `yPos` -4; status breakdown includes 49 `full` and 792 proto-chunks                                                                                                                        |
| Block states, palettes and biomes                 | `crates/world-io/src/java_chunk.rs`; `crates/world-io/tests/java_chunk.rs` — properties, non-spanning bit packing, one-entry palettes, 4/5/6-bit data, 64 biome cells, YZX order, negative coordinates                                                                                                            |
| Height and dimension validation                   | `crates/world-io/tests/java_chunk.rs` — `yPos` must match `min_y`, section Y must fit the dimension height, duplicate/oversized section lists are refused, missing sections read as air                                                                                                                           |
| Custom dimensions and dimension symlinks          | `crates/world-io/tests/anvil.rs` — a classic world with data under `dimensions/` is refused, an empty `dimensions/` is accepted, a symlinked `DIM-1` is refused                                                                                                                                                   |
| Corrupt palette fail-closed checks                | `crates/world-io/tests/java_chunk.rs` — wrong long counts, out-of-range indices, missing/extra data, invalid biome ids and missing biome palettes are refused                                                                                                                                                     |
| Real 1.21.4 chunk decoding                        | `docs/testing/evidence/2026-11-18-anvil-decode-1.21.4.json` — 841 chunks, 20,184 sections, 125 non-empty, 123,908 non-air blocks, 0 missing sections in present chunks, 5 block states, `minecraft:plains`                                                                                                        |
| Real multi-biome cross-check                      | `docs/testing/evidence/2026-11-18-anvil-biomes-1.21.4.json` — 5 biomes, 98 block states; 48 running-server biome queries matched the decoded cell grid at y=63/65/100 in chunk (-2, -3); empty-section caveat recorded                                                                                            |
| Passthrough of unchanged files and unknown data   | `crates/world-io/src/passthrough.rs`; `crates/world-io/tests/passthrough.rs` — byte equality for region/level/unknown files, unknown directories, source snapshot unchanged, only `session.lock` skipped                                                                                                          |
| Passthrough fail-closed checks                    | `crates/world-io/tests/passthrough.rs` — existing/nested destination, symlinked file/dir, corrupt chunk, file limit; no destination or temp directory is left after failure                                                                                                                                       |
| Real 1.21.4 passthrough copy                      | `docs/testing/evidence/2026-11-18-anvil-passthrough-1.21.4.json` — 20 files, 3,481,260 bytes, 4 regions, 841 chunks validated; independent SHA-256 comparison matched all files and source stayed unchanged                                                                                                       |
| Overlay block edits and source immutability       | `crates/world-io/src/overlay.rs`; `crates/world-io/tests/overlay.rs` — source reads through the overlay, edits are visible, reverting removes the dirty state, source byte snapshot is unchanged                                                                                                                  |
| Overlay batch and range validation                | `crates/world-io/tests/overlay.rs` — unknown dimension, out-of-range Y, missing chunk and missing section are refused; a failing batch leaves no changes; duplicate positions are refused                                                                                                                         |
| Overlay NBT materialization and unknown tags      | `crates/world-io/tests/overlay.rs`; `crates/world-io/src/overlay.rs` — only `block_states` changes, unknown root/section tags, block entities, entities and biomes are compared with the source; patched NBT is re-decoded                                                                                        |
| Overlay document round-trip and tamper refusal    | `crates/world-io/tests/overlay.rs` — schema/DataVersion, section Y, palette, bits, hex words, duplicate chunks and missing chunks are refused                                                                                                                                                                     |
| Wide palette non-spanning encoding                | `crates/world-io/tests/overlay.rs` — 40 block states, 6-bit non-spanning data, re-decoded blocks match and the long-array length is exact                                                                                                                                                                         |
| Real 1.21.4 overlay edit                          | `docs/testing/evidence/2026-11-18-anvil-overlay-1.21.4.json` — grass block became gold block in chunk (-1, -1), one section dirty, biome preserved, 16,671-byte patched NBT re-decoded, all 21 source files unchanged                                                                                             |
| Verified region write scope                       | `crates/world-io/src/region_write.rs`; `crates/world-io/tests/region_write.rs` — only 1.21.4 classic `sections[].block_states`; biomes, status, entities, DataVersion and identical replacements are refused; non-full chunks cannot be edited                                                                    |
| Verified region write safety                      | `crates/world-io/tests/region_write.rs` — existing destination, source path/source region directory and wrong region filename are refused; source SHA-256 is rechecked; output is reopened and every untouched record is compared byte for byte                                                                   |
| Real 1.21.4 verified region write                 | `docs/testing/evidence/2026-11-18-region-write-1.21.4.json` — 196 chunks, 1 replaced, 195 untouched records byte-identical with unchanged timestamps; output 811,008 bytes re-decoded to gold block with plains biome; all 21 source files unchanged                                                              |
| Derived-data inventory and sidecars               | `crates/world-io/src/derived.rs`; `crates/world-io/tests/derived.rs` — full/proto chunks, heightmaps, light, ticks, `PostProcessing`, block entities, POI compound shape, entity sidecars, missing folders; source tree unchanged                                                                                 |
| Real 1.21.4 derived-data inventory                | `docs/testing/evidence/2026-11-18-derived-inventory-1.21.4.json` — 841 chunks, 49 full, 20,184 sections, 49 chunks with `SkyLight`, no `BlockLight`, 24 `PostProcessing` entries per chunk, empty block entities/ticks, 0-byte sidecar placeholders                                                               |
| Official 1.21.4 server round-trip                 | `docs/testing/evidence/2026-11-18-derived-data-1.21.4.json` — five edits, server exit 0; only `sections` changed; heightmaps/light/BEs/ticks/`PostProcessing` unchanged; chest/sign/barrel survived without block entities; barrel got a `minecraft:fisherman` POI; source unchanged                              |
| Derived-data policy and rebuild hand-off          | `crates/world-io/tests/region_write.rs` — `VanillaRebuild` removes `Heightmaps`, `PostProcessing`, `SkyLight`/`BlockLight` and sets `isLightOn = 0` only for replaced chunks; unknown tags, block entities, entities and untouched chunks stay identical; source unchanged                                        |
| Real 1.21.4 vanilla rebuild                       | `docs/testing/evidence/2026-11-18-derived-rebuild-1.21.4.json` — two edits, server exit 0; after load/save all four heightmaps, sky/block light, `isLightOn` and 24 `PostProcessing` entries were rebuilt; edited block states preserved; block entities still 0; source unchanged                                |
| Complete world-folder export                      | `crates/world-io/src/world_export.rs`; `crates/world-io/tests/world_export.rs` — all three dimensions, sidecars, unknown files and `session.lock`; only dirty regions rebuilt; destination/source path refusal; non-full chunk fails without publishing; source tree verified                                     |
| Real 1.21.4 world-folder export                   | `docs/testing/evidence/2026-11-18-world-export-1.21.4.json` — 20 files / 3,481,260 bytes, 4 regions / 841 chunks, 1 region / 1 chunk replaced; all non-region files matched source SHA-256; source unchanged; exported block re-decoded to gold block with plains biome                                           |
| Selection: save vs region vs region directory     | `crates/world-io/src/world_selection.rs`; `crates/world-io/tests/world_selection.rs` — bare `.mca`, `region/` directory, complete classic save, split/mixed/legacy/`level.dat`-only layouts and unknown folders; passthrough refuses incomplete selections without a destination                                  |
| Real 1.21.4 / 26.3 selection classification       | `docs/testing/evidence/2026-11-18-world-selection-1.21.4.json` — classic save supported (841 chunks); its `r.0.0.mca` and `region/` not complete saves; real 26.3 split save complete but unsupported (529 chunks); both source trees unchanged; no refusal destination created                                   |
| Automated official-server fixture                 | `scripts/verify_java_world_export.ps1` — requires user-provided Java/server JAR, expected SHA-1 and explicit EULA acceptance; server generates a flat world, FuryBuilder exports an edit, the same server verifies blocks via `execute if block`, stops/saves, FuryBuilder decodes again; source hashes unchanged |
| Real 1.21.4 server load/save fixture              | `docs/testing/evidence/2026-11-18-server-fixture-1.21.4.json` — 4 regions / 841 chunks, 1 chunk replaced; verifier exit 0, `Done (0.302s)`, 0 chunk/command issues; gold edit marker present, stale grass absent, dirt neighbor intact; 10 files rewritten; post-save decode confirms gold and dirt               |
| Support matrix by version and data category       | `crates/world-io/src/support_matrix.rs` — one supported version (1.21.4/4189), no writes for unsupported versions, `block_states` is the only `verified` category, derived fields are `preserve_or_invalidate`, camelCase serialization                                                                           |
| Support matrix in Import World UI                 | `apps/desktop/src/features/project/ProjectManager.tsx`; `ProjectManager.test.tsx` — version chips, category table and the supported 1.21.4 row render from `analysis.supportMatrix`                                                                                                                               |
| Minecraft asset discovery and JAR validation      | `crates/asset-source/src/discovery.rs`, `jar.rs`; `crates/asset-source/tests/asset_source.rs` — target SHA-1/size, internal `version.json`/DV 4189, wrong version, modded `inheritsFrom`, checksum and world-version mismatch, missing metadata, version directories, missing roots                               |
| Resource pack archive safety                      | `crates/asset-source/src/archive.rs`, `resource_pack.rs`; `asset_source.rs` tests — in-memory zip, traversal/absolute/backslash names, deflate, `pack.mcmeta` formats 46/range/wrong/missing                                                                                                                      |
| Asset source settings and UI                      | `crates/asset-source/src/settings.rs`; settings round-trip test; `MinecraftAssetsSection.tsx` + test - saved source, invalid saved path, detect, empty automatic detection and manual fallback                                                                                                                    |
| Blockstate/model/texture bundle                   | `crates/asset-source/src/bundle/model.rs`, `texture.rs`; `tests/asset_bundle.rs` - variants/multipart, parent chains, elements/UV/rotation, missing model/texture, animated first frame, deterministic atlas, cache hit                                                                                           |
| Resource-pack stack priority and cache            | `bundle/stack.rs`, `settings.rs`; `asset_bundle.rs` - highest-priority override, swapped priority produces a different cache key, wrong format and malformed JSON fail closed, schema 1→2 upgrade, duplicates, 16-pack limit, unsafe archive entry and rejected stacks leave the saved stack unchanged            |
| Default variant and fluid fallback                | `bundle/model.rs` tests; `renderer/mesher.test.ts` - omitted properties pick a default-like variant, water/lava use a real-texture cube approximation instead of the missing checker                                                                                                                              |
| Renderer model integration                        | `renderer/mesher.ts`, `assets/model-assets.ts`; `mesher.test.ts`, `model-assets.test.ts` - custom elements/cullface, cutout alpha, missing cube, atlas UVs, fixed tints                                                                                                                                           |
| Resource-pack stack UI                            | `MinecraftAssetsSection.tsx`; test - add validated `.zip`, reorder, remove and clear through the saved stack API                                                                                                                                                                                                  |
| Real 1.21.4 render smoke                          | `docs/testing/evidence/2026-11-18-assets-render-ui.json` + `.png` - isolated APPDATA, 9-chunk demo, 12 models / 15 textures, actual WebGL screenshot, no asset error, launcher root unchanged                                                                                                                     |
| Real 1.21.4 asset discovery                       | `docs/testing/evidence/2026-11-18-minecraft-assets-1.21.4.json` - 13 versions, 1.21.4 client 28,335,587 bytes / SHA-1 verified / DV 4189 / pack 46, asset index 19 verified, 3 modded profiles, valid/wrong/missing packs, watched files unchanged                                                                |
| Import World analysis and limits                  | `crates/world-io/src/world_import.rs`; `crates/world-io/tests/world_import.rs` — supported world counts, blockers for 26.1+ split and non-4189 versions, source immutability, chunk/section/block limits, empty world                                                                                             |
| Import World project and source copy              | `crates/project-store/src/store.rs`; `crates/project-store/tests/source_overlay.rs` — `create_imported`, immutable tree manifest, traversal/missing-file refusal, generic creator still refuses `import`                                                                                                          |
| Import World wizard UI                            | `apps/desktop/src/features/project/ProjectManager.tsx`; `ProjectManager.test.tsx` — folder input, Analyze, supported metrics, blockers disable import, `project_import_world` called with the selected folder                                                                                                     |
| Real 1.21.4 Import World                          | `docs/testing/evidence/2026-11-18-world-import-1.21.4.json` — 4 regions / 841 chunks / 123,908 non-air blocks; 20 copied files / 3,481,260 bytes all matched source SHA-256; source unchanged; reopened project has 121 editable chunks; real 26.3 world refused as split/5023                                    |
| Classic/modern layout and version refusal         | `crates/world-io/tests/anvil.rs` — unsupported DataVersion, 26.1+ split, mixed markers and a missing `region/` are refused                                                                                                                                                                                        |

The Python inventory script is deliberately metadata-only: it does not decode chunks.
`crates/world-io/src/anvil.rs` and `crates/world-io/src/java_chunk.rs` are the read-only
decoder; `crates/world-io/src/passthrough.rs` is the byte-preserving copy,
`crates/world-io/src/overlay.rs` is the in-memory edit overlay,
`crates/world-io/src/region_write.rs` writes verified new region files,
`crates/world-io/src/world_export.rs` exports a complete new world folder and
`crates/world-io/src/world_selection.rs` classifies a selection as a complete save,
a bare region file, a region directory or an unsupported layout;
`crates/world-io/src/derived.rs` inventories chunk-derived data and
`crates/world-io/src/world_import.rs` is the analysis/editable-import layer used by the
`Import World` wizard. None of them writes in place. `cargo test -p furybuilder-world-io`
runs the anvil, java_chunk, passthrough, overlay, region_write, derived, world_export,
world_selection and world_import suites, and
`cargo run -p furybuilder-world-io --example anvil_probe -- <world-folder> --decode`
reproduces the real-world read evidence without modifying the world.
`cargo run -p furybuilder-world-io --example passthrough_world -- <source> <destination>`
reproduces the copy evidence.
`cargo run -p furybuilder-world-io --example overlay_probe -- <source> <dimension> <x> <y> <z> <state>`
reproduces the overlay evidence.
`cargo run -p furybuilder-world-io --example region_write_probe -- <source> <new-region-file> <dimension> <x> <y> <z> <state> [--rebuild-derived]`
reproduces the verified region write evidence without modifying the source world;
`--rebuild-derived` selects the `VanillaRebuild` hand-off.
`cargo run -p furybuilder-world-io --example derived_probe -- <source> --world`
reproduces the derived-data inventory, and `--chunk <cx> <cz>` or
`--blocks <x> <y> <z> ...` reports one chunk or decoded block states.
`cargo run -p furybuilder-world-io --example world_export_probe -- <source> <new-world-folder> [--rebuild-derived] [<dimension> <x> <y> <z> <state> ...]`
reproduces the complete world-folder export and re-reads the edited blocks.
`cargo run -p furybuilder-world-io --example world_selection_probe -- <path> [--passthrough <new-folder>]`
prints the selection/import classification and, optionally, the passthrough result.
`pwsh -File scripts/verify_java_world_export.ps1 -Java <java.exe> -ServerJar <server.jar> -ExpectedSha1 4707d00eb834b446575d89a61a11b5d548d8c001 -AcceptMinecraftEula`
runs the automated official-server fixture; it never downloads the JAR or accepts the EULA for you.
`cargo test -p furybuilder-world-io --lib support_matrix`
checks the machine-readable support matrix.
`cargo test -p furybuilder-asset-source`
runs asset discovery, JAR, resource-pack, blockstate/model/texture bundle and atlas tests.
`cargo run -p furybuilder-asset-source --example asset_source_probe -- discover`
prints conventional-root discovery; `-- inspect <path>` classifies a root, version JAR or pack.
`cargo run -p furybuilder-asset-source --example asset_bundle_probe -- --jar <client.jar> --state minecraft:stone --repeat`
builds a real render bundle for reference blocks; `--atlas <path>` writes the generated PNG.
`pwsh -File scripts/probe_minecraft_assets.ps1 -MinecraftRoot <launcher-root>`
reproduces the real JAR/index checksum evidence and the synthetic pack checks.
`node scripts/assets-render-webview2-smoke.mjs <out.json> <out.png> <launcher-root>`
reproduces the real WebView2 render screenshot with an isolated profile.
`cargo run -p furybuilder-desktop --example import_world_probe -- <source> <empty-project-root> "Imported Flat"`
reproduces the Import World evidence, and `--analysis-only` prints an analysis report.

## Implemented coverage (M3)

| Matrix row                | Automated evidence                                                                                     |
| ------------------------- | ------------------------------------------------------------------------------------------------------ |
| T-04 chunk boundary       | `renderer/world.test.ts`, `renderer/mesher.test.ts` (border culling and neighbour unload rebuild)      |
| T-05 negative coordinates | `renderer/world.test.ts`, `renderer/raycast.test.ts`, Rust `coords::tests`                             |
| T-20 mesh at chunk seam   | `renderer/mesher.test.ts` — faces heal after a neighbour chunk is unloaded                             |
| Transparency              | `renderer/mesher.test.ts` — glass/water stay in the translucent buffer; same-material faces are culled |
| Observation partial       | `renderer/observation.test.ts` — missing requested chunks set `partial` and `missingChunks`            |

## Implemented coverage (M4)

| Requirement                                                                   | Automated evidence                                               |
| ----------------------------------------------------------------------------- | ---------------------------------------------------------------- |
| Palette search and canonical state defaults                                   | `features/workspace/BottomPanel.test.tsx`                        |
| Place/remove/brush/line/fill/replace planning                                 | `editor/session.test.ts`                                         |
| Box selection with negative coordinates                                       | `editor/commands.test.ts` (`tool click state machine`)           |
| Selection volume limit (1,000,000 blocks)                                     | `editor/commands.test.ts`                                        |
| Conflicting actions (no selection/source, source = target, revision mismatch) | `editor/commands.test.ts`                                        |
| Undo/redo buttons and dirty chunk refresh                                     | `editor/commands.test.ts`, `features/viewport/Viewport.test.tsx` |
| Cancellable preview/read jobs and stale job completion                        | `editor/commands.test.ts`, `state/tool-store.test.ts`            |
| Hotkey protection in inputs and contenteditable                               | `editor/use-editor-hotkeys.test.tsx`, `editor/session.test.ts`   |
| Click → preview → apply → chunk refresh IPC path                              | `features/viewport/Viewport.test.tsx`                            |

## Implemented coverage (M5)

| Requirement                                                          | Automated evidence                                                                                                                                    |
| -------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- |
| Versioned manifest and validated fields                              | `project-store` `create_list_open_round_trips_world_and_manifest`, `unsupported_newer_schema_is_refused`                                              |
| Snapshot save/load incl. chunks, sections, metadata and editor state | `create_list_open_round_trips_world_and_manifest`, `snapshot_round_trip_preserves_states_across_dimensions`                                           |
| Immutable source layer with hashes and path safety                   | `source_files_are_immutable_copies_with_hashes`                                                                                                       |
| Atomic temp + verification + publish                                 | `save_is_atomic_and_keeps_previous_snapshot`, `corrupt_snapshot_is_rejected_by_checksum`                                                              |
| Autosave, checkpoints and crash recovery                             | `crash_recovery_uses_newer_verified_autosave`, `checkpoints_are_bounded_and_readable`, `state/project-store.test.ts`                                  |
| Undo limits and recreatable-cache cleanup                            | `world-application` history-limit test, `end_session_clears_only_recreatable_cache`                                                                   |
| Project list/create/rename/open and unsaved warning                  | `features/project/ProjectManager.test.tsx`                                                                                                            |
| Schema migrations with backup and refusal of newer versions          | `unsupported_newer_schema_is_refused`                                                                                                                 |
| Crash recovery, reopen, checksum and semantic equality               | `crash_recovery_uses_newer_verified_autosave`, `save_is_atomic_and_keeps_previous_snapshot`, `snapshot_round_trip_preserves_states_across_dimensions` |
| IPC project contract                                                 | `lib/project-api.test.ts`                                                                                                                             |

## Implemented coverage (M6, Java structure `.nbt`)

Target version: Minecraft Java `1.21.4`, `DataVersion` `4189`. The format is an
interchange format only; FuryBuilder project metadata is not claimed to be
included.

| Requirement                                      | Automated / recorded evidence                                                                                                                                |
| ------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Versioned adapter for one format                 | `world-io` `java_structure.rs`, `nbt.rs`, `structure.rs`, `exchange.rs`; `cargo test -p furybuilder-world-io`                                                |
| Real game-saved fixture parses                   | `crates/world-io/tests/game_fixtures.rs` uses `fixtures/java-1.21.4/structures/plain.nbt` saved by the official 1.21.4 server                                |
| Block state properties and orientation           | `official_plain_structure_round_trips_semantically`; Tauri IPC check for oak stairs facing east                                                              |
| Unsupported block entity is refused              | `official_structure_with_block_entity_is_refused_instead_of_dropped`; `chest_boundary.nbt` preview and insert rejection                                      |
| Import preview and new project/insertion         | `features/project/StructureExchange.test.tsx`, `state/project-store.test.ts`, real Tauri IPC probe recorded in `docs/testing/minecraft-structure-check.md`   |
| Export preview with bounds/version/warnings/path | `features/project/StructureExchange.test.tsx`, `structure-api.test.ts`, Tauri IPC probe                                                                      |
| Binary and semantic verification after writing   | `exchange::tests::prepare_export_is_verified_and_deterministic`, `export_verification_is_independent_of_world_iteration_order`; `project-store` `exports.rs` |
| Corrupt/incompatible input is not silent success | `nbt::tests`, `java_structure::tests`, `game_fixtures.rs`                                                                                                    |
| Official server can load adapter output          | `docs/testing/minecraft-structure-check.md`, evidence `docs/testing/evidence/2026-11-18-java-structure-interop.json`                                         |
| Build → export → load demonstration              | `world-io` example `export_demo.rs`; six exact block checks on official 1.21.4 server                                                                        |

T-07 in the original matrix (preserve a chest block entity) is **not** green
for this adapter: the current Java structure model refuses block entities
rather than dropping them. Supporting passthrough is future work outside M6.
Biomes, entities, ticks and full worlds remain M8 work. `.schem` remains the
second format and is not enabled in the UI.

## Implemented coverage (M7, first MCP slice)

| Requirement                                                         | Automated / recorded evidence                                                                                                                                                                                                                                                                |
| ------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Pinned MCP SDK and transport                                        | `docs/research/mcp-2026-07-28.md`; `@modelcontextprotocol/server` 2.4.0 in `mcp/bridge/package.json`                                                                                                                                                                                         |
| Modern and legacy protocol negotiation                              | `mcp/bridge/test/protocol.test.ts` spawns the real stdio bridge for both eras; OpenCode CLI 2.3.2 agent passed `protocol: auto` (modern 2026-07-28) and `protocol: legacy` against a live app                                                                                                |
| Real external MCP client and agent                                  | `docs/testing/evidence/2026-11-18-opencode-mcp-client.json`; OpenCode 2.3.2 `--agent plan` called `bridge_info`/`world_info` over MCP only, received ready read-only context and made no modifications                                                                                       |
| Tasks as an explicit option with fallback tools                     | `mcp/bridge/test/tasks-policy.test.ts`; job tools reach the authoritative queue when the desktop is live and return `job_api_unavailable` when it is absent                                                                                                                                  |
| Long-running job queue, progress, cancel and idempotent retry       | `world-application/src/jobs.rs` tests (queue limit, phases, rollback on cancel, replay, conflict, project scope, policy-source cancel); `world-operations` observer test; Rust endpoint state/confirmation tests; `desktop-jobs.ts` + live-endpoint job mapping in `desktop-bridge.test.ts`  |
| Authenticated local endpoint over the authoritative `WorldService`  | `apps/desktop/src-tauri/src/local_bridge.rs` tests: token rejection, protocol mismatch, `world_info`, bounded `query`, project mismatch, real local-socket round-trip                                                                                                                        |
| Discovery file and stale-endpoint handling                          | `local_bridge.rs` discovery test; `mcp/bridge/test/desktop-client.test.ts`                                                                                                                                                                                                                   |
| Real bridge process reaching a live local endpoint                  | `mcp/bridge/test/desktop-bridge.test.ts` (`bridge_info.desktop.connected`)                                                                                                                                                                                                                   |
| Explicit `project_id` / `world_revision` / `job_id` contract        | `mcp/bridge/test/context.test.ts`; `local_bridge.rs` stale-revision test; `job-tools.ts` schema exposes `job_id`, `world_revision`, `idempotency_key`                                                                                                                                        |
| Explicit connection lifecycle                                       | `bridge_info.connection` (`instanceId`, `startedAtMs`, `ephemeral-stdio`, `expiresWithProcess`, state); asserted in `protocol.test.ts` and `desktop-bridge.test.ts`                                                                                                                          |
| Read-only tools: world info, block, region summary/blocks           | `mcp/bridge/src/read-tools.ts`; `protocol.test.ts`; live-endpoint reads in `desktop-bridge.test.ts`                                                                                                                                                                                          |
| Camera and real viewport screenshot                                 | `apps/desktop/src/lib/renderer-bridge.test.ts`; Rust camera/capture/revision tests; real Tauri probe recorded in `docs/testing/mcp-compatibility.md`                                                                                                                                         |
| Preview, apply, batch and undo tools                                | `mcp/bridge/src/write-tools.ts`; `protocol.test.ts`; live-endpoint mapping in `desktop-bridge.test.ts`                                                                                                                                                                                       |
| Access modes, limits and confirmations                              | `apps/desktop/src-tauri/src/mcp_access.rs` tests (read-only, selection box, block/batch limits, undo extent, one-answer confirmation); Rust endpoint approval/denial tests                                                                                                                   |
| Access policy UI and confirmation host                              | `apps/desktop/src/lib/mcp-access.test.ts`; `McpAccessDialog` and `McpConfirmationHost`; policy is Tauri-only and the bridge cannot widen it                                                                                                                                                  |
| Tool calls bound to operation history and action journal            | `apps/desktop/src-tauri/src/mcp_activity.rs` (bounded journal, client/method/status/revision/operation link); Rust endpoint test; `mcp_activity` endpoint and History panel; live probe shows a denied call with `clientId` and `permission_denied`                                          |
| Read-only by default and explicit write grant                       | Rust `local_bridge` tests: preview access status, apply/undo `permission_denied`, granted batch apply + audit + undo; real Tauri probe                                                                                                                                                       |
| Real Tauri process                                                  | `scripts/probe-local-bridge.mjs`; recorded in `docs/testing/mcp-compatibility.md` — reads, PNG capture, read-only preview/deny, activity journal, status, wrong token `unauthorized`                                                                                                         |
| End-to-end AI edit loop (explore → build → screenshot → fix → undo) | `scripts/e2e-mcp-agent.mjs`; `docs/testing/evidence/2026-11-18-ai-edit-loop.json` — 17 MCP calls, four real dialog approvals, revisions 0→1→2→3→4, two fresh PNG captures, final world 0 blocks                                                                                              |
| MCP write propagation into the live viewport                        | Rust `WORLD_CHANGED_EVENT` after sync apply/undo and completed jobs; `McpWorldChangeHost.test.tsx`; E2E captures at render revision = authoritative revision, `stale=false`, legend 13 stone → 12 stone + 1 glass                                                                            |
| Structured observations with an explicit revision                   | Rust `local_bridge` test `query_observations_report_the_revision_they_read` (block, region summary, stale conflict); `desktop-bridge.test.ts` asserts `get_block` and `inspect_region`; live MCP probe `mcp/bridge/scripts/probe-live-observation.mjs`                                       |
| Truthful screenshot completeness and stale detection                | `finalize_renderer_response` + Rust camera/capture tests; bounded legend in `apps/desktop/src/renderer/observation.ts`; both probes check `partial`/`missingChunks`/`stale` consistency; `docs/testing/evidence/2026-11-18-mcp-observation.json` and `2026-11-18-structured-observation.txt` |
| MCP setup, mistakes and disabling access                            | `docs/mcp-setup.md` — OpenCode configuration, tool catalog, modes/limits, error table, revocation steps; numbers matched against `mcp_access.rs`, `local_bridge.rs` and `job-tools.ts`                                                                                                       |

M7 is complete. The full AI build/screenshot/fix/undo loop is verified through a real
external client and the real confirmation dialog; the model used was DeepSeek free because
GPT-6 Luna Max was refused by its provider for this region. Structured observations carry the
exact revision and do not claim a complete frame when chunks are missing. The setup guide
documents the client configuration and the fail-closed behaviour.
