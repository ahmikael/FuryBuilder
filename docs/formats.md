# Formats and world support

Target: Minecraft Java **1.21.4**, `DataVersion` 4189. This is the only write-verified
version. Anything else is detected and refused rather than guessed.

## Supported versions

| Version family      | DataVersion | Layout                            | Status                 | Read                        | Edit                      | Write                              | Passthrough     |
| ------------------- | ----------- | --------------------------------- | ---------------------- | --------------------------- | ------------------------- | ---------------------------------- | --------------- |
| Java 1.21.4         | 4189        | classic Anvil                     | **supported**          | `supported_subset`          | `block_states_and_biomes` | `verified_block_states_and_biomes` | `byte_for_byte` |
| Java 1.21.5-1.21.11 | 4325-4671   | classic Anvil                     | `detected_unsupported` | `version_refused`           | `none`                    | `none`                             | `none`          |
| Java 26.1-26.3      | 4786-5023   | split (`dimensions/`, `players/`) | `detected_unsupported` | `layout_and_region_headers` | `none`                    | `none`                             | `none`          |
| Java 26.4 snapshots | 5119+       | split                             | `out_of_scope`         | `none`                      | `none`                    | `none`                             | `none`          |
| Pre-1.13            | varies      | McRegion (`region/*.mcr`)         | `detected_unsupported` | `legacy_marker`             | `none`                    | `none`                             | `none`          |
| Bedrock Edition     | n/a         | Bedrock                           | `out_of_scope`         | `none`                      | `none`                    | `none`                             | `none`          |

A complete 26.1+ split save is structurally complete but not readable, editable or
comparable. A bare `.mca` file or a `region/` directory is not a complete save at all.

## What is editable (classic 1.21.4)

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
| `poi/` sidecars                      | `inventoried`   | `not_exposed`        | `preserved_raw`             | `byte_for_byte` | FuryBuilder never rewrites POI                                                                                               |
| Structures and references            | `not_exposed`   | `not_exposed`        | `preserved_raw`             | `byte_for_byte` | Raw starts/references survive; block edits do not update them                                                                |
| Unknown chunk tags                   | `preserved_raw` | `not_exposed`        | `preserved_raw`             | `byte_for_byte` | Compared with the source and must stay unchanged                                                                             |
| Unknown files/directories            | `preserved_raw` | `not_exposed`        | `preserved_raw`             | `byte_for_byte` | Copied, not interpreted or dropped                                                                                           |
| `playerdata/`, `data/`, `datapacks/` | `preserved_raw` | `not_exposed`        | `preserved_raw`             | `byte_for_byte` | Copied as whole files/folders                                                                                                |
| Custom dimensions                    | `refused`       | `not_supported`      | `not_supported`             | `refused`       | Classic worlds declaring custom dimensions are refused                                                                       |
| `session.lock`                       | `detected`      | `not_supported`      | `skipped`                   | `skipped`       | Transient game lock; the only deliberately skipped file                                                                      |

The machine-readable source is `crates/world-io/src/support_matrix.rs`; the same data is
returned by `analyze_java_world` and shown in the Import World wizard.

## Structure `.nbt`

Java structure `.nbt` is the first interchange format. It stores a bounded block region
and is accepted by structure blocks, `/place template`, datapacks and structure-aware
editors. It is never a project store: objects, DSL, history and source provenance are not
representable.

- Root is gzip-compressed big-endian NBT; `DataVersion` must be `4189` on strict import.
- `size` axes are `1..=1024`, volume `<= 1,000,000`, palette `<= 4,096`, blocks are sparse
  non-air positions. Missing positions are air.
- Block states and properties are preserved exactly; orientation is a property, not a
  separate field.
- Entities, block entities and unknown tags are refused on strict import and never written
  on export. The file is re-encoded and re-parsed before success is reported.
- Limits: 32 MiB compressed over IPC, 64 MiB decompressed NBT, depth 64, collection length
  8,000,000.

Sponge `.schem` v3 is the planned second adapter and is not enabled. `.litematic` and
legacy `.schematic` are not implemented. `.mcstructure` is Bedrock and out of scope.

## Project manifest (`project.json`, schema 2)

Desktop world creation stores the verified target profile and component identities in the
first manifest. An installed Java runtime is recorded by mode, logical id and executable
SHA-256; it is optional for editing-only projects. The client is recorded by mode, client
JAR SHA-1 and the catalog id and SHA-256 derived from that exact JAR. An explicit
placeholder records only its mode. Vanilla projects also record the server version and
SHA-1; the checksum must agree with the completed generation run.

These records contain no absolute component paths and are preserved across saves and
reopens. The manifest records the vanilla catalog, not the global resource pack stack.
Per-project pack selection and resolving pinned components for rendering on reopen are
separate from writing the manifest.

## Complete world-folder export

The backend can export a complete new world folder, not a bare `.mca`:

1. Validate a supported classic 1.21.4 source and a new destination.
2. Copy every file byte for byte, skipping only `session.lock`.
3. Rebuild only dirty regions; untouched records are compared byte for byte.
4. Reopen and decode every staged region and chunk.
5. Recheck the source tree hashes and publish by atomic rename.

`Preserve` keeps heightmaps, light and `PostProcessing` raw. `VanillaRebuild` removes them
for replaced chunks so the vanilla engine rebuilds them on the next load. The source world
is opened read-only and is never modified. This path is verified in the backend/test flow
but has no one-click UI action in the beta.

## Selection rules

Before import, export or copy, the selected path is classified:

| Selection                                       | Result                                        |
| ----------------------------------------------- | --------------------------------------------- |
| Full classic 1.21.4 save                        | supported import/export path                  |
| Full save, other `DataVersion`                  | version blocker                               |
| Full 26.1+ split save                           | layout blocker; regions inventoried read-only |
| Classic `region/` plus split markers            | mixed-layout blocker                          |
| `level.dat` plus only legacy `.mcr`             | legacy blocker                                |
| `level.dat` without any region/dimension folder | incomplete-save blocker                       |
| Single `r.<x>.<z>.mca`                          | "not a complete world save"                   |
| Directory of `.mca` files, no `level.dat`       | "select its parent world folder"              |
| Unrelated folder                                | not-a-save blocker                            |
| Missing path / symlink                          | fail closed                                   |

## What "write" does not mean

- It does not recalculate preserved fields. A `preserved_raw` write copies the source
  representation.
- `VanillaRebuild` does not write new heightmaps or light; it removes them so the target
  engine rebuilds them. No Rust-side derived-data simulation is claimed.
- Block entities, entities, POI and ticks are not synthesized, moved or cleared. Placing a
  chest, sign or barrel through the block-state writer is not semantically supported.
- Biome edits write only the selected cells. A world without biome data is refused instead
  of receiving a default biome.
- The complete world export is not a backup/versioning system; the destination must not
  exist.
