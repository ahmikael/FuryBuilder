# Java world-folder export (M8.10)

Scope: classic Anvil layout, Minecraft Java **1.21.4**, `DataVersion` 4189. This
module exports a **complete new world folder**, not a bare `.mca` file and not a
structure. The source world is opened read-only and is never written.

Implementation: `crates/world-io/src/world_export.rs`.

## API

```rust
export_java_world(&overlay, destination)
export_java_world_with_options(&overlay, destination, options)
```

`WorldExportOptions`:

| Field                 | Meaning                                                      |
| --------------------- | ------------------------------------------------------------ |
| `derived_data_policy` | `Preserve` (default) or `VanillaRebuild` for replaced chunks |
| `passthrough`         | file/chunk limits and chunk validation used by the copy      |
| `max_regions`         | maximum number of region files that may be rebuilt           |
| `verify_source_tree`  | hash the complete source tree before and after the export    |

`WorldExportReport` returns the source and destination paths, the copied file and
byte counts, skipped files, total region/chunk counts, replaced region/chunk
counts, each rebuilt region's SHA-256, the derived-data policy, source-tree
hashes and verification flags.

## Pipeline

1. **Validate the request.** The source must be a supported classic 1.21.4 world.
   The destination must have a final directory name, its parent must exist, and
   the destination must not exist, be inside the source world, or be an ancestor
   of it.
2. **Snapshot the source.** With `verify_source_tree = true`, a SHA-256 over every
   source file (relative path, length, bytes) is taken.
3. **Copy the complete world.** `passthrough_java_world_with_options` copies every
   file byte for byte into a temporary sibling `world/` folder, validates the
   source and the copy, and skips only `session.lock`.
4. **Rebuild dirty regions.** Dirty overlay chunks are grouped by dimension and
   region. For each group, `write_region_from_overlay_with_policy` writes a new
   region file, re-decodes it, and verifies untouched chunk records. The new file
   is hashed, moved over the copied region in the staged world, and hashed again.
5. **Validate the staged save.** Every staged file must match the passthrough
   manifest or the rebuilt-region SHA-256. The staged folder is reopened with
   `JavaWorld::open`; every region and every allocated chunk is decoded again,
   and the region/chunk totals must match the copy report.
6. **Recheck the source.** With `verify_source_tree = true`, the source tree hash
   must be unchanged. Any change aborts the export.
7. **Publish atomically.** The staged world is renamed to the destination only
   after all checks pass. A destination created concurrently is refused. On any
   failure the staging directory is removed and no destination is published.

## What is preserved

- `level.dat`, `data/`, `playerdata/`, `datapacks/`, maps, raids, advancements and
  any unknown files/directories are copied byte for byte.
- `poi/` and `entities/` sidecar containers are copied byte for byte.
- Dimensions and their folder layout are preserved; this module rejects the
  26.1+ split layout at open time.
- Untouched region files are copied byte for byte; untouched records inside a
  rebuilt region are compared byte for byte by the region writer.
- `session.lock` is the only skipped file. It is a transient game lock.
- All three classic dimensions can be present; the tests cover Overworld, Nether
  and End together.

## What is not claimed

- The writer still edits only `sections[].block_states` in replaced chunks.
  `Preserve` keeps derived fields raw; `VanillaRebuild` invalidates heightmaps,
  light, `isLightOn` and `PostProcessing` for the vanilla engine to rebuild.
- Block entities are not synthesized. Placing a chest/sign/barrel through the
  block-state writer does not create a usable block entity.
- `poi/` is not rewritten by FuryBuilder. The server added one barrel POI in the
  M8.8 experiment; that is not a general guarantee.
- Scheduled ticks are preserved, not recomputed.
- Empty directories are copied but are not represented in the file-hash
  manifest; file contents and paths are verified.
- The destination must not exist. This is not an in-place update or a backup
  versioning system.
- Opening the exported folder in a real Minecraft server is a separate integration
  check (M8.12). This task reopens and validates it with FuryBuilder only.

## Real 1.21.4 evidence

`docs/testing/evidence/2026-11-18-world-export-1.21.4.json`:

| Metric                        | Result                                             |
| ----------------------------- | -------------------------------------------------- |
| Source files before/after     | 21 / 21, all SHA-256 hashes unchanged              |
| Exported files                | 20 (`session.lock` skipped)                        |
| Exported bytes                | 3,481,260                                          |
| Regions / chunks              | 4 / 841                                            |
| Replaced regions / chunks     | 1 / 1                                              |
| Edited block                  | `(0, -61, 0)` grass block → `minecraft:gold_block` |
| Verified read-back            | `minecraft:gold_block`, biome `minecraft:plains`   |
| Non-region files match source | yes, every file                                    |
| Replaced region differs       | yes                                                |
| Source tree hash before/after | `b9a77b52…`, identical                             |
| Copied files match manifest   | yes                                                |
| Output re-decoded             | yes, all 841 chunks                                |

The test used the same real 841-chunk flat world as the M8.6/M8.7 evidence. The
exported world was not yet started with a Minecraft server in this task; see the
limitations above.

## Reproduce

```powershell
cargo test -p furybuilder-world-io --test world_export
cargo run -p furybuilder-world-io --example world_export_probe -- <source-world> <new-world-folder> minecraft:overworld 0 -61 0 minecraft:gold_block
cargo run -p furybuilder-world-io --example world_export_probe -- <source-world> <new-world-folder>
cargo run -p furybuilder-world-io --example world_export_probe -- <source-world> <new-world-folder> --rebuild-derived minecraft:overworld 0 -61 0 minecraft:gold_block
```

The first command proves the unit/integration path. The second performs a pure
byte-preserving copy. The third selects the vanilla-rebuild hand-off for the
replaced chunk.
