# Formats and world support

Target: Minecraft Java **1.13–1.21.11** classic Anvil and numeric **1.8.9–1.12.2**, with **1.21.4**
/ `DataVersion` 4189 as the full baseline. Every one of the 46 classic releases in that range
can be imported, edited,
saved through project snapshots, exported same-version and generated with that release's exact
official server JAR. The 15 numeric classic releases from **1.8.9–1.12.2** add import,
block/biome editing, project snapshots and same-version export, without generation. Modern
classic releases can also be upgraded to a newer same-family release through a verified copy.
Exact client-JAR block/item catalogs cover all 19 flattened releases, the numeric releases and
the modern releases with single-integer pack formats; 1.21.9–1.21.11 keep the explicit
placeholder workflow until major/minor pack formats are supported.

The version picker lists releases and snapshots from the official Mojang manifest. Project
creation requires a profile with verified write support, so the modern, flattened and numeric
classic releases can advance the wizard. The 1.8–1.8.8 releases, all pre-1.8 releases and
split 26.1+ releases have exact profiles but stop at the profile check.

FuryBuilder has built-in metadata for all 46 Java releases from 1.13 through 1.21.11 and for
all 52 releases from 1.0 through 1.12.2. The 15 numeric releases from 1.8.9 through 1.12.2 have
a verified chunk adapter; 1.8–1.8.8 and 1.0–1.7.10 are metadata-only because they predate the
`DataVersion` tag. A `1.8–1.12.2` client JAR builds a block/item catalog with numeric ids and
metadata; pre-1.8 releases still have no catalog. The
Java major comes from [Mojang's launcher metadata](https://piston-meta.mojang.com/mc/game/version_manifest_v2.json);
the `DataVersion` and resource/data pack formats come from `version.json` inside each matching
client JAR, or from the exact built-in profile for 1.13.x, which has no internal file. Starting
with 1.21.9, pack formats have both a major and a minor component (for example, 1.21.11 uses
data pack format `94.1`). For these classic Anvil releases, the vanilla
Overworld spans Y −64 through 319, and the Nether and End span Y 0 through 255. These facts
alone do not create a split or legacy adapter. The dimension set, vertical bounds and region
folder are read from the exact profile's
`dimensionProfiles`; the classic reader does not assume fixed `DIM-1`/`DIM1` names. Project
snapshots encode the same set and reject a dimension whose bounds differ from the profile.
Render snapshots carry the selected dimension's `minY`/`height`, and the viewport uses them for
chunk borders instead of a fixed Y range.

The 19 flattened releases from 1.13 through 1.17.1 use `DataVersion` 1519–2730, Java 8 for
1.13–1.16.5 and 16 for 1.17–1.17.1, and resource/data pack formats 4–7. Their Overworld,
Nether and End span Y 0–255, with region folders `region`, `DIM-1/region` and `DIM1/region`.
The exact facts come from the built-in profiles and are checked against each client JAR's
internal `version.json`; 1.13.x has no internal file and is identified by SHA-1-matched launcher
metadata instead.

## Pre-1.13 profiles: numeric Anvil and metadata-only legacy

The 24 releases from 1.8 through 1.12.2 use numeric block ids plus metadata in classic Anvil
chunks, column biomes and Y 0–255. `DataVersion` is 47 for 1.8.9, 169–184 for 1.9–1.9.4,
510–512 for 1.10–1.10.2, 819–922 for 1.11–1.11.2 and 1139–1343 for 1.12–1.12.2. The 1.8–1.8.8
releases predate the `DataVersion` tag and are recorded with `-1`; they share the same 1.8 world
format. Resource pack formats are 1 for 1.8, 2 for 1.9–1.10.2 and 3 for 1.11–1.12.2; there is
no data pack format.

The numeric chunk reader and writer are enabled for the 15 releases from **1.8.9 through
1.12.2**. Each section's `Blocks` byte array, `Data` metadata nibbles and optional `Add` high
bits are decoded through the shared 1.12.2 numeric registry, while `Level.Biomes` is projected
from 256 columns to the editor's 4×4×4 cells by sampling the center column of each footprint.
Edits re-encode only changed sections and rebuild the 256-byte column array; every other tag,
including unknown section and Level tags, is preserved. A 1.8.9 chunk without a root
`DataVersion` inherits the exact level.dat profile. Unknown ids are rejected, never replaced.
Vanilla generation stays disabled. The 1.8–1.8.8 releases have no exact `DataVersion`, so they
remain metadata-only.

The 28 releases from 1.0 through 1.7.10 also predate `DataVersion`. 1.0–1.1 use McRegion
(`region/*.mcr`) with 128-block dimensions; 1.2.1–1.7.10 use classic Anvil with 256-block
dimensions. Resource packs are absent before 1.6.1 and use format 1 from 1.6.1. Official server
JARs exist from 1.2.5 onward, but generation remains disabled in this build.

The metadata-only profiles are visible in the version picker with an `unverified` badge.
Selecting one does not create a project: import, editing, export and generation are limited to
the classic 1.13–1.21.11 and numeric 1.8.9–1.12.2 releases above. The client-JAR catalog still
supports all of `1.8–1.12.2` with numeric block ids, per-variant metadata and item ids.

## Differences within modern classic Anvil

The 1.18–1.21.11 releases share the section and palette layout, but the on-disk `Status`
value for a fully generated chunk is `full` in 1.18–1.19 and `minecraft:full` from 1.20
onward. The adapter compares against the exact profile's spelling. It does not rewrite
the status or treat an unfamiliar value as fully generated. Block-entity payloads,
including the change from item tags to data components in 1.20.5, remain opaque.

The vanilla biome registry contains 61 IDs in 1.18, 63 from 1.19 (adding `deep_dark` and
`mangrove_swamp`), 64 from 1.20 (adding `cherry_grove`), and 65 from 1.21.4
(adding `pale_garden`). The sets were cross-checked for all 27 releases against the
[vanilla registry reports mirrored by mcmeta](https://github.com/misode/mcmeta/tree/registries).
Import preserves biome IDs outside the vanilla registry, including datapack names.
The import report and desktop diagnostics include a warning with the number of
affected sections. No fallback biome is substituted.

## Supported versions

| Version family      | DataVersion | Layout                            | Status                 | Read                        | Edit                      | Write                              | Passthrough     |
| ------------------- | ----------- | --------------------------------- | ---------------------- | --------------------------- | ------------------------- | ---------------------------------- | --------------- |
| Java 1.21.4         | 4189        | classic Anvil                     | **supported**          | `supported_subset`          | `block_states_and_biomes` | `verified_block_states_and_biomes` | `byte_for_byte` |
| Java 1.18-1.21.11   | 2860-4671   | classic Anvil                     | `supported_subset`     | `supported_subset`          | `block_states_and_biomes` | `verified_block_states_and_biomes` | `byte_for_byte` |
| Java 1.13-1.17.1    | 1519-2730   | classic Anvil (flattened)         | `supported_subset`     | `supported_subset`          | `block_states_and_biomes` | `verified_block_states_and_biomes` | `byte_for_byte` |
| Java 1.8.9-1.12.2   | 47, 169-1343 | classic Anvil (numeric ids)      | `supported_subset`     | `supported_subset`          | `block_states_and_biomes` | `verified_block_states_and_biomes` | `byte_for_byte` |
| Java 1.8-1.8.8      | predates DataVersion | classic Anvil (numeric ids) | `detected_unsupported` | `metadata_only`         | `none`                    | `none`                             | `none`          |
| Java 1.0-1.7.10     | predates DataVersion | McRegion (1.0-1.1), classic Anvil (1.2.1-1.7.10) | `detected_unsupported` | `metadata_only` | `none`             | `none`                             | `none`          |
| Java 26.1-26.3      | 4786-5023   | split (`dimensions/`, `players/`) | `detected_unsupported` | `layout_and_region_headers` | `none`                    | `none`                             | `none`          |
| Java 26.4 snapshots | 5119+       | split                             | `out_of_scope`         | `none`                      | `none`                    | `none`                             | `none`          |
| Bedrock Edition     | n/a         | Bedrock                           | `out_of_scope`         | `none`                      | `none`                    | `none`                             | `none`          |

A bare `.mca` file or a `region/` directory is not a complete save. The support matrix
describes analysis and import boundaries. Only a version with a verified profile can be
selected for a new project. The `1.18-1.21.11` row includes 1.21.4; the `1.13-1.17.1` row
covers all 19 flattened classic releases. Both support import, block/biome editing, project
snapshots, same-version export and generation with the exact official server JAR. The
`1.8.9-1.12.2` row supports the same import/edit/save/export subset but not generation.
The `1.8-1.8.8` and `1.0-1.7.10` rows are metadata-only and cannot advance the wizard.

P1 verification covers all 27 modern releases with import and same-version export round-trips.
The catalog-pinned create → edit → save → reopen → export → re-import path is covered once per
modern family (`1.18.2`, `1.19.4`, `1.20.6`, `1.21.4`), rebuilding the exact catalog from the
same client JAR after reopen. P2 verification covers all 19 flattened releases with import,
edit, project save/reopen and same-version export, and runs the same catalog-pinned project
round-trip for every release. Client JARs for all 19 were also checked against real official
archives. P3-03 verification covers all 15 numeric releases from 1.8.9 through 1.12.2 with
import, block/biome editing, same-version export, re-import, unknown-tag preservation and a
byte-identical untouched neighbour chunk; the 1.8.9 path also covers chunks without a root
`DataVersion`. Both modern and flattened families support same-version Java structure
interchange; `Full` remains limited to 1.21.4.

## Java 26.1–26.3 split saves: inspection only

The built-in metadata identifies Java 26.1 (`4786`), 26.1.1 (`4788`), 26.1.2 (`4790`),
26.2 (`4903`), and 26.3 (`5023`); each requires Java 25. Snapshots are not admitted.
The [26.1 storage layout](https://www.minecraft.net/en-us/article/minecraft-java-edition-26-1)
moves terrain to `dimensions/<namespace>/<path>/region/` and player files to
`players/data/`, `players/advancements/`, and `players/stats/`. The inspector:

- Reads `level.dat` and lists saved dimension declarations from
  `data/minecraft/world_gen_settings.dat`. It does not resolve datapacks or infer height
  bounds for the save.
- Counts allocated terrain slots from `.mca` headers, separately from `entities/` and
  `poi/`. It lists player storage folders when present but does not read the player files.
- Requires readable metadata and the split Overworld terrain directory for a structurally
  complete selection. A classic terrain folder alongside split markers is reported as
  mixed, not as two copies of the same dimension.

The preview does not decode blocks, biomes, entities, or compression records. In particular,
the [26.3 palette change](https://www.minecraft.net/en-us/article/minecraft-26-3-snapshot-7)
means an Anvil header does not establish classic chunk compatibility. Split saves cannot
be imported into a project, compared, copied through world passthrough, or exported.

## What is editable (classic 1.13–1.21.11)

Import, block/biome editing, project snapshots, same-version export and generation are enabled
for every exact classic profile in this range. The shared codec is verified for all 27 modern
and all 19 flattened releases.

| Category                             | Read            | Edit                 | Write                       | Passthrough     | Note                                                                                                                         |
| ------------------------------------ | --------------- | -------------------- | --------------------------- | --------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `level.dat` metadata                 | `supported`     | `not_exposed`        | `preserved_raw`             | `byte_for_byte` | `DataVersion`, `Version.Id`, `LevelName`, `WorldGenSettings` are validated; `level.dat` is not rewritten by the block writer |
| Region containers                    | `supported`     | `chunk_records_only` | `verified_replaced_regions` | `byte_for_byte` | Untouched region files/records are exact bytes; only dirty regions are rebuilt and verified for every classic release |
| Block states and palettes            | `supported`     | `supported`          | `verified`                  | `byte_for_byte` | Modern `sections[].block_states` or flattened `Level.Sections[].Palette`/`BlockStates` in fully generated chunks   |
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

## Vanilla generation inputs (1.13–1.21.11)

Generation is driven by the exact `VersionProfile`, never by a global baseline:

- `level-type` uses the modern namespaced spelling for 1.18–1.21.11: `minecraft:normal`,
  `minecraft:flat`, `minecraft:large_biomes`, `minecraft:amplified`. The 1.13–1.15 mapping uses
  `default`, `flat`, `largeBiomes`, `amplified`; 1.16–1.17.1 uses `largebiomes`.
- Java comes from the exact profile: 8 for 1.13–1.16.5, 16 for 1.17–1.17.1,
  17 for 1.18–1.20.4 and 21 for 1.20.5–1.21.11.
- The server JAR must match the profile's exact version and `DataVersion`. For 1.13.x, which
  has no internal `version.json`, the official SHA-1 identifies the exact profile. The selected
  JAR's SHA-1, EULA acceptance and per-run consent are recorded in `.furybuilder-run.json` schema 3.
- Data packs are checked against the profile's `datapack_format`. A newer pack without a
  matching `supported_formats` range is refused; an older one produces a warning.
- `server.properties` binds the server to `127.0.0.1`, disables online mode/RCON/query, and
  sets `level-seed`, `level-type`, `generate-structures` and view distance. Status and chunk
  sync settings start with 1.16; simulation distance starts with 1.18.
- A run is bounded by radius, time, memory and CPU. Only a completed run whose manifest,
  report and generated world agree can be imported.

## Same-family project upgrade (1.18–1.21.11)

The backend can upgrade a project to a newer release in the same modern classic
family. This is not an in-place retarget and not a downgrade:

1. Preflight requires a newer target `DataVersion`, the same layout, chunk family,
   biome format and dimension bounds, verified target client assets, an exact target
   block catalog and the target biome registry.
2. The source project must be `Empty`, `Flat` or `Import`; `Vanilla World` is refused
   because its generator provenance is bound to the original seed, server JAR and
   version. The saved project is used, so unsaved editor state is never silently
   converted.
3. A mandatory checkpoint of the saved source revision is written to the original
   project's `changes/` directory.
4. The project folder is copied to a hidden staging sibling. The copy excludes the
   session marker, cache, old checkpoints, exports, autosaves and old active snapshots.
   An imported immutable source tree is copied unchanged.
5. The copied world is rebuilt from the target profile, with every source section
   transferred. The target profile and revision are written to a new `project.json`;
   the source project keeps its original manifest and active snapshot byte for byte.
6. Every used block state and biome is checked against the target. Unknown states are
   listed with counts and block publication; IDs are never silently substituted.
   Editor workspace/scene/parametric state is preserved without remapping.
7. The staged project is reopened, its snapshot checksum and semantics are verified,
   the source manifest and snapshot are rechecked, and only then is the staging folder
   renamed to the final project folder.
8. The report includes mapped and unknown counts, the checkpoint file and revision,
   copied file/byte counts, the new snapshot checksum and revision, and confirmation
   that the source stayed unchanged.

This path is verified in the Rust backend and tests. The Project Settings action and
user-facing report arrive with the Project Settings UI work (P4-06).

## Flattened chunk layout (1.13–1.17)

The pre-1.18 chunk keeps `DataVersion` at the root and wraps terrain in a `Level` compound.
`Level.Sections` carries `Y`, `Palette`, `BlockStates`, `SkyLight` and `BlockLight` directly;
`Level.Biomes`, `Level.TileEntities` and `Level.Entities` stay on the same wrapper. Block-state
indices are one continuous bit stream in 1.13–1.15 and become padded per 64-bit word from 1.16,
so a 17-entry palette occupies 320 longs before 1.16 and 342 longs from 1.16. The decoder
validates the exact `DataVersion`, palette values, section bounds and light-only halo records,
and maps the chunk-wide biome array through the exact numeric registry. Mixed 1.13–1.14 4×4
footprints are normalized by center sampling at `(4x+2, 4z+2)`—the same choice as the vanilla
1.15 upgrade—and the decoded chunk reports how many footprints were sampled. Unchanged
footprints keep all original columns when written; changing one projected cell replaces that
whole footprint. Heightmaps and every other tag remain raw. Final chunk status is
`postprocessed` in 1.13.x and `full` from 1.14. Vanilla singleton palettes store 256 zero longs
in `BlockStates`; an empty proto-chunk can have no biome values and contributes no editor
sections.

The region reader, import, overlay editing, region writer and same-version export all accept
these releases. The writer patches only `Level.Sections[].Palette`/`BlockStates` and
`Level.Biomes`; every other root/`Level`/section tag is compared with the source and must stay
unchanged.

## Flattened biome conversion (1.13–1.17)

The flattened releases store biomes in `Level.Biomes`, outside the block-section palettes:
256 block-column entries in 1.13–1.14 and 1024 YZX cells (16 sections of 4×4×4 cells) in
1.15–1.17. `minecraft-version` carries the exact release metadata (DataVersion, Java 8/16,
pack formats and the 0–256 vertical bounds; the extended Overworld height starts with 1.18) and
the numeric registry for each exact release. ID 8 is `minecraft:nether` through 1.15 and
`minecraft:nether_wastes` from 1.16. The chunk decoder uses this converter; the region reader,
project wizard, overlay, region writer and exporter use it through the same profile. The
converter also operates on one chunk's NBT so the P2 adapter and version conversion can share
one mapping.

- Decoding produces the normalized 4×4×4 `BiomeSection` grid used by 1.18+.
- Every source ID must exist in that exact release. An unknown ID stops conversion and is
  never replaced by a default biome.
- A 1.13–1.14 4×4 footprint may contain 16 different columns. `RequireUniform` (the default)
  refuses that footprint. `SampleCenter` samples `(4x+2, 4z+2)`, matching the vanilla 1.15
  upgrade, and reports how many footprints were reduced.
- Patching a column chunk applies an edited cell to every vertical section, because the source
  can store only one biome per horizontal cell. A change that asks for different values by
  height is refused instead of silently dropping one of them. A changed mixed footprint is
  written under `SampleCenter` and replaces all 16 columns; an unchanged footprint keeps its
  original columns.
- A biome that does not exist in the source release cannot be patched into it.

## Client JAR layout (1.13–1.17)

The flattened releases use the older asset layout: singular
`assets/minecraft/textures/block` and `textures/item`, item models instead of
`assets/minecraft/items` item definitions, no `assets/minecraft/atlases`, and plural
`data/minecraft/tags/blocks`/`tags/items` folders. The catalog extractor reads this layout as
well as the modern one.

Official 1.13.x client JARs have no root `version.json`. For those JARs, adjacent launcher
metadata (`<stem>.json`, or `client.json` beside `client.jar`) must name the exact release and
match its SHA-1. The built-in profile supplies `DataVersion` and both pack formats.
Official 1.14–1.14.2 JARs use an internal ID of the form `release / build-hash`; inspection and
provisioning use the release part and still check `DataVersion`. When both launcher and internal
metadata exist, the release IDs must agree. A declared size or checksum mismatch is refused.
Manual `.minecraft` roots and version folders are resolved against the project's exact profile.

Resource packs for flattened releases use a single integer `pack_format`: 4 for 1.13–1.14.4,
5 for 1.15–1.16.1, 6 for 1.16.2–1.16.5 and 7 for 1.17–1.17.1. Later `supported_formats`
ranges do not override it. ZIP and folder packs use the same validation and project priority.

Entity render blockstates (`item_frame`, `glow_item_frame`) do not create blocks, empty item
models and model templates do not create items, and wall-fan variants never gain an item form
from their base fan. A special item model with only a `particle` texture keeps a `null` icon.
The exact client JAR is pinned with the project catalog. The viewport and palette atlas load
textures from that JAR with the project's resource-pack stack applied.

## Structure `.nbt`

Java structure `.nbt` is the first interchange format. It stores a bounded block region
and is accepted by structure blocks, `/place template`, datapacks and structure-aware
editors. It is never a project store: objects, DSL, history and source provenance are not
representable.

- Root is gzip-compressed big-endian NBT. Import and export support the exact release
  `DataVersion` of each classic Java profile from 1.13 through 1.21.11. Creating a project
  from a file uses its profile; inserting a file or template requires the open project's
  exact `DataVersion`. Export writes the project's version.
- No structure data fixing is performed. The version in the file is authoritative even for
  bundled game templates: for example, `end_city/base_roof.nbt` shipped with 1.13.x has
  `DataVersion` 1457 and is refused.
- `size` axes are `1..=1024`, volume `<= 1,000,000`, palette `<= 4,096`, blocks are sparse
  non-air positions. Missing positions are air.
- Block states and properties are preserved exactly; orientation is a property, not a
  separate field.
- Strict import accepts one palette and refuses entities, block entities and unknown tags.
  Export writes block states only. The file is re-encoded and re-parsed before success is
  reported.
- Placement uses the selected dimension's full build bounds. Classic 1.13–1.17.1
  dimensions allow Y 0–255 inclusive; modern classic Overworld allows Y −64–319, and its
  Nether and End allow Y 0–255. Live insertion also validates states against the project's
  write registry, which is limited when no client catalog is pinned.
- Limits: 32 MiB compressed over IPC, 64 MiB decompressed NBT, depth 64, collection length
  8,000,000.

Sponge `.schem` v3 is the planned second adapter and is not enabled. `.litematic` and
legacy `.schematic` are not implemented. `.mcstructure` is Bedrock and out of scope.

## Project files and schema versions

FuryBuilder writes only the current internal formats. The schema numbers are exact; an older
project, snapshot or checkpoint is refused rather than migrated or read through a compatibility
path.

| File                                         | Version | Contents                                                                                   |
| -------------------------------------------- | ------- | ------------------------------------------------------------------------------------------ |
| `project.json`                               | 2       | verified target, component ids/hashes, active world revision/checksum, editor state, source |
| `data/world-*.fwl`, `data/autosave.fwl`      | 3       | binary snapshot: profileId, DataVersion, layout, biome mode, dimensions and chunks          |
| `changes/checkpoint-*.json`                  | 2       | checkpoint metadata: profileId, revision, checksum, label, reason, editorState             |
| `source/source.json`                         | 1       | immutable source file list and SHA-256 values                                              |
| `components/.../.furybuilder-assets.json`    | 1       | managed client JAR version, SHA-1, size, DataVersion and pack formats                      |
| `components/.../.furybuilder-server.json`    | 2       | managed server version, SHA-1, size, DataVersion and Java major                            |
| `server.json`                                | 3       | selected server per version and per-JAR/version EULA consent                               |
| `.furybuilder-run.json`                      | 3       | generation profile/version/DataVersion, JAR SHA-1, EULA time and per-run consent           |

Project files live inside the `.furybuilder` folder. The `components/...` manifests and
`server.json` belong to the shared application-data component cache, not to one project.

Desktop world creation stores the verified target profile and component identities in the
manifest. For an as-is import, that target must match the Minecraft version and DataVersion
confirmed after previewing the source. The editable snapshot and the immutable copy of `level.dat`
must agree with it; unsupported or inconsistent version metadata is refused without opening a project.
An installed Java runtime is recorded by mode, logical id and executable
SHA-256; it is optional for editing-only projects. The client is recorded by mode, client
JAR SHA-1 and the catalog id and SHA-256 derived from that exact JAR. An explicit
placeholder records only its mode. Vanilla projects also record the server version and
SHA-1; the checksum must agree with the completed generation run.

These records contain no absolute component paths and are preserved across saves and
reopens. `components.assets.packStack` is a separate, highest-priority-first list of
`{path, sha1}` pins, limited to 16 entries. Paths are relative to the local asset cache
(`resource-packs/<sha1>/pack-<name>.zip`); the original paths and pack bytes aren't stored
in the project. Removing a pack from a stack does not delete the cache copy. The catalog
id and hash always describe the base client JAR, not these packs.

## Complete world-folder export

The backend can export a complete new world folder, not a bare `.mca`:

1. Validate a supported classic 1.13–1.21.11 source and a new destination.
2. Copy every file byte for byte, skipping only `session.lock`.
3. Rebuild only dirty regions; untouched records are compared byte for byte.
4. Reopen and decode every staged region and chunk.
5. Recheck the source tree hashes and publish by atomic rename.

`Preserve` keeps heightmaps, light and `PostProcessing` raw. `VanillaRebuild` removes them
for replaced chunks so the vanilla engine rebuilds them on the next load. The source world
is opened read-only and is never modified. This path is verified in the backend/test flow
but has no one-click UI action in the beta.

Source and replacement chunks must match the world's exact `DataVersion`, even when another
release uses the same section layout. Edits rewrite only the selected block or biome palette
and its packed indices; other keys in those containers are kept. Both policies preserve the
entire block-entity NBT list, including entry order, unknown tags and nested item data. They
do not check whether the edited blocks still match the preserved block entities.

Modern classic Anvil may store light-only records one section below or above
the dimension's terrain. They do not contain editable blocks or biomes.
`Preserve` keeps these records intact; `VanillaRebuild` clears their light
arrays but retains the section positions and unknown tags. The derived-data
scan warns if stored heightmaps do not have the packed length expected for
the dimension. It does not rewrite or recalculate them.

## Selection rules

Before import, export or copy, the selected path is classified:

| Selection                                       | Result                                        |
| ----------------------------------------------- | --------------------------------------------- |
| Full classic 1.13–1.21.11 save                 | supported import/edit/snapshot/export path     |
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
