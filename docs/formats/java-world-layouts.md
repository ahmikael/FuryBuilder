# Minecraft Java world layouts and DataVersions

Status: M8 inventory, checked on 2026-11-18. This is a map of what exists, not a
claim that FuryBuilder can read or write these worlds. Only the classic 1.21.4
layout is the current implementation target, and even that is still read-only
work for M8.

## Why one layout is not enough

A Java world is a folder. `level.dat` is not the whole save, and a single
`.mca` file is not a world. Minecraft has changed the folder layout more than
once, and it changed it again in **26.1 Snapshot 6** (data version 4774). An
importer that only looks for `region/` will silently miss every dimension in a
26.1+ world, while an importer that assumes `dimensions/` will miss the
Overworld in every world up to 1.21.11.

FuryBuilder therefore detects the layout from observed files and checks it
against the data version. Unsupported layouts must fail closed rather than be
guessed.

## Classic Anvil layout (1.13 through 1.21.11; 1.21.4 observed here)

Observed on a real official 1.21.4 server world:

```text
world/
├── level.dat
├── level.dat_old
├── session.lock
├── region/                  r.<regionX>.<regionZ>.mca
├── entities/                r.<regionX>.<regionZ>.mca
├── poi/                     r.<regionX>.<regionZ>.mca
├── data/                    shared .dat files
├── datapacks/
├── playerdata/              <uuid>.dat
├── stats/                   <uuid>.json
├── advancements/            <uuid>.json
├── DIM-1/                   the Nether; region/entities/poi/data
├── DIM1/                    the End; region/entities/poi/data
├── dimensions/<namespace>/<path>/   custom dimensions only
├── generated/<namespace>/structures/
└── resources.zip            optional world resource pack
```

Notes:

- The Overworld's `region`, `entities` and `poi` live at the world root.
- `DIM-1` and `DIM1` are legacy names that still apply through 1.21.11. They
  are not custom dimensions.
- `dimensions/<namespace>/<path>/` exists for datapack/mod dimensions even
  before 26.1. Its presence alone does not make the world a 26.1 layout.
- A dimension that was never entered may have a `data/` folder but no
  `region/` folder; do not treat that as corruption.
- `level.dat` carries the data version twice: `Data.DataVersion` and
  `Data.Version.Id`. They must agree for an import to be trusted.
- `level.dat_old` is a backup and `level.dat_new` is a temporary file. Neither
  replaces the authoritative `level.dat`.

## Split dimension layout (26.1 and newer, data version 4774+)

Observed on a real official 26.3 server world:

```text
world/
├── level.dat                still carries DataVersion and Version.Id
├── level.dat_old
├── session.lock
├── data/
│   └── minecraft/
│       ├── game_rules.dat
│       ├── world_gen_settings.dat
│       ├── random_sequences.dat
│       ├── scoreboard.dat
│       ├── weather.dat
│       ├── world_clocks.dat
│       └── ...              namespaced shared data
├── datapacks/
├── dimensions/
│   └── minecraft/
│       ├── overworld/
│       │   ├── region/      r.<regionX>.<regionZ>.mca
│       │   ├── entities/
│       │   ├── poi/
│       │   └── data/minecraft/
│       │       ├── chunk_tickets.dat
│       │       ├── raids.dat
│       │       └── world_border.dat
│       ├── the_nether/      same shape
│       └── the_end/         same shape
├── players/
│   ├── data/                <uuid>.dat
│   ├── stats/               <uuid>.json
│   └── advancements/        <uuid>.json
├── resourcepacks/
│   └── resources.zip
└── generated/<namespace>/structure/
```

The 26.1 changes that matter most to an importer:

- Every default dimension, including the Overworld, moved under
  `dimensions/<namespace>/<path>/`. The root `region/`, `entities/` and `poi/`
  folders are gone in a freshly created world.
- `DIM-1` and `DIM1` were renamed to
  `dimensions/minecraft/the_nether` and `dimensions/minecraft/the_end`.
- `data/` is now namespaced. Vanilla shared data is under
  `data/minecraft/`, and dimension-specific data is inside that dimension.
- `world_gen_settings.dat`, `game_rules.dat`, weather, scheduled events, world
  clocks, boss events and the Ender Dragon fight moved out of `level.dat` into
  data files.
- Player storage moved from root `playerdata/`, `stats/` and `advancements/` to
  `players/data`, `players/stats` and `players/advancements`.
- The world resource pack moved from `resources.zip` to
  `resourcepacks/resources.zip`.
- `chunks.dat` became `chunk_tickets.dat`; `idcounts.dat` became
  `last_id.dat`; map data is under `data/minecraft/maps/`.
- Structure-block output uses the singular `generated/<namespace>/structure/`.
- The End no longer has a special `raids_end.dat`; it uses `raids.dat` like the
  other dimensions.
- 26.3 adds `version_history` to `level.dat`. It is useful as a migration
  record but is not a substitute for `DataVersion`/`Version.Id`.

## Layout markers used by the inspector

`scripts/inspect_world_layout.py` reads only metadata and directory names. It
does not decode chunks. The markers are:

| Marker                                 | Meaning                                                        |
| -------------------------------------- | -------------------------------------------------------------- |
| `rootRegion`                           | classic Overworld `region/` exists at the world root           |
| `rootMcaRegion`, `rootMcrRegion`       | root region contains `.mca` files and/or legacy `.mcr` files   |
| `netherDimFolder`, `endDimFolder`      | classic `DIM-1` / `DIM1` folders exist                         |
| `modernOverworldRegion`                | `dimensions/minecraft/overworld/region/` exists                |
| `worldGenSettingsFile`                 | split `data/minecraft/world_gen_settings.dat` exists           |
| `playersDirectory`                     | 26.1-style `players/` exists                                   |
| `rootPlayerdata`, `rootStats`          | classic player directories still exist                         |
| `rootResourcesZip`, `resourcepacksZip` | where the optional world resource pack lives                   |
| `unrecognizedTopLevel`                 | unknown entries that must be preserved or refused, not dropped |

A folder is classified as `modern_26_1` when the split Overworld region exists,
or when both `players/` and `world_gen_settings.dat` exist. A root `region/`
with `.mca` files is `classic_anvil`; a root region with only legacy `.mcr`
files is `legacy_mcr` and is reported as out of scope. Mixed markers (for
example, a root region plus `players/`) are reported as issues instead of being
silently normalized.

## DataVersions

The only version FuryBuilder currently targets is **4189** (Minecraft Java
1.21.4). The table records the releases around it so a future adapter starts
from facts rather than guesses.

| Release | DataVersion | World layout | Source                               |
| ------- | ----------- | ------------ | ------------------------------------ |
| 1.21.4  | 4189        | classic      | official `server.jar` `version.json` |
| 1.21.5  | 4325        | classic      | Minecraft Wiki data-version table    |
| 1.21.6  | 4435        | classic      | Minecraft Wiki data-version table    |
| 1.21.7  | 4438        | classic      | Minecraft Wiki data-version table    |
| 1.21.8  | 4440        | classic      | Minecraft Wiki data-version table    |
| 1.21.9  | 4554        | classic      | Minecraft Wiki data-version table    |
| 1.21.10 | 4556        | classic      | Minecraft Wiki data-version table    |
| 1.21.11 | 4671        | classic      | Minecraft Wiki data-version table    |
| 26.1    | 4786        | split        | Minecraft Wiki data-version table    |
| 26.1.1  | 4788        | split        | Minecraft Wiki data-version table    |
| 26.1.2  | 4790        | split        | Minecraft Wiki data-version table    |
| 26.2    | 4903        | split        | Minecraft Wiki data-version table    |
| 26.3    | 5023        | split        | official `server.jar` `version.json` |

The split layout starts at **26.1 Snapshot 6** with data version **4774**.
Snapshot data versions between 4671 and 4774 are a transition zone; the
inspector reports their layout as `unknown` consistency rather than pretending
to know which side of the change a particular build is on.

Current official release on 2026-11-18: `26.3` (released 2026-09-15). Latest
snapshot: `26.4-snapshot-10` (data version 5130). FuryBuilder does not claim
either as a supported import target.

### Runtime metadata

| Version | Java | Protocol | Resource pack | Data pack |
| ------- | ---- | -------- | ------------- | --------- |
| 1.21.4  | 21   | 769      | 46            | 61        |
| 26.3    | 25   | 777      | 97.1          | 121.0     |

Java 25 is a real requirement for 26.3; it is not enough to reuse the Java 21
runtime from the 1.21.4 target.

## Read-only Rust reader (M8.2–M8.3)

`crates/world-io/src/anvil.rs` implements the container half and
`crates/world-io/src/java_chunk.rs` the section half of the 1.21.4 adapter.
There is no write path.

| API                             | What it does                                                                                                       |
| ------------------------------- | ------------------------------------------------------------------------------------------------------------------ |
| `LevelDat::read`                | bounded gzip/NBT read; requires `Data.DataVersion` and `Data.Version.Id` to agree                                  |
| `LevelDat::ensure_supported`    | refuses any version other than 4189                                                                                |
| `LevelDat::declared_dimensions` | lists `WorldGenSettings.dimensions` keys; missing tags yield an empty list, not invented dimensions                |
| `JavaWorld::open`               | validates a classic layout, discovers the standard dimensions with their `min_y`/`height`, and counts region files |
| `JavaWorld::region_coordinates` | lists `r.<x>.<z>.mca` files after validating every name                                                            |
| `RegionFile::open`              | validates the two 4 KiB header tables, allocation ranges and overlaps                                              |
| `RegionFile::read_chunk`        | decodes one chunk, checks its position and returns the complete `NbtFile`                                          |
| `RegionFile::summarize_all`     | decodes every allocated chunk, discards its NBT and returns structural summaries; peak memory stays at one chunk   |
| `RegionChunk::decode`           | turns one chunk into block [`Section`]s and biome palettes for its dimension                                       |
| `JavaWorld::decode_chunk`       | the same operation, resolving the dimension by id                                                                  |
| `DecodedChunk::to_chunk`        | copies non-empty block sections into the editor chunk model                                                        |

Supported at this stage:

- classic Anvil layout (1.13 through 1.21.11), with the Overworld at `region/`
  and `DIM-1` / `DIM1` as the Nether and End;
- compression ids 1 (gzip), 2 (zlib) and 3 (uncompressed);
- negative chunk and region coordinates via floor division;
- `level.dat` up to 8 MiB compressed, a region file up to 1 GiB, and one
  decompressed chunk up to the shared 64 MiB NBT limit;
- empty `sections`, `block_entities` and `entities` lists with either
  `TAG_Compound` or `TAG_End` element type, as produced by the real game;
- block-state palettes with properties, using Minecraft's non-spanning bit
  packing and the 4-bit minimum; 4096 indices in YZX order;
- biome palettes, using `ceil(log2(palette))` bits and 64 cells of 4×4×4
  blocks; a one-entry biome palette may omit `data`;
- proto-chunks whose absent sections are reported in `missing_sections` and
  read as air, while every present section is still decoded.

Refused explicitly instead of guessed:

- the 26.1+ split layout, and a folder that mixes classic and split markers;
- classic worlds that contain a non-empty `dimensions/` folder (custom
  dimensions), rather than silently importing only the Overworld; an empty
  `dimensions/` folder is accepted;
- external `.mcc` chunks (compression flag `0x80`), LZ4 (4), custom algorithms
  (127) and unknown compression ids;
- region files that are shorter than 8 KiB, not 4 KiB aligned, have partially
  allocated entries, point into the header, overlap, or extend past the file;
- a chunk length larger than its allocated sectors, an NBT `xPos`/`zPos`
  mismatch, a chunk `DataVersion` other than 4189, a malformed section, or
  trailing bytes after the chunk NBT root;
- a chunk `yPos` that does not match the dimension's `min_y`, a section `Y`
  outside the dimension height, duplicate section Y values, or more sections
  than the dimension can hold;
- block palettes that are empty, too large, use unsupported tag types, have
  the wrong long-array length, a non-empty `data` with a one-entry palette, or
  an index outside the palette;
- biome palettes that are empty, too large, non-string, have an invalid
  resource location, the wrong long-array length, or an out-of-range cell;
- symlinked world files and region files.

Biomes are decoded but not yet persisted in `world-domain`; they remain on
`DecodedChunk`, and `to_chunk()` copies blocks only. Raw chunk NBT is also kept
on `RegionChunk` for passthrough and overlay work. The byte-preserving copy and
the in-memory block overlay are described below; import UI and any write path
are later M8 stages.

Probe and tests:

```powershell
cargo test -p furybuilder-world-io
cargo run -p furybuilder-world-io --example anvil_probe -- <world-folder>
cargo run -p furybuilder-world-io --example anvil_probe -- <world-folder> --decode
cargo run -p furybuilder-world-io --example anvil_probe -- <world-folder> --decode --all-chunks
```

Every mode decodes all allocated chunks before printing, so a zero exit code
means the whole region table was read. `--decode` additionally reports block
and biome aggregates; `--all-chunks` prints every per-chunk summary.

## Byte-preserving passthrough copy (M8.4)

`crates/world-io/src/passthrough.rs` copies a supported classic world to a new
folder without re-encoding any data. It is the safe baseline for an overlay:
unchanged chunks, unknown NBT fields, unknown files and `datapacks/` never pass
through a lossy parser.

Guarantees and checks:

- the source is opened read-only and must pass `JavaWorld::open` and the fixed
  `DataVersion` check;
- every allocated chunk is decoded and structurally validated before copying
  (default `validate_chunks: true`);
- every regular file is copied byte for byte, including unknown top-level files
  and unknown directories; `.mca` files are copied as complete containers, not
  rebuilt from decoded chunks;
- `session.lock` is the only skipped file, because it is a transient game lock;
  the report records this explicitly;
- symlinks, junctions, reparse points and special files are refused, including a
  symlinked source root;
- the destination must not exist and must not be inside the source world;
- files are written to a temporary sibling directory, hashed with SHA-256,
  re-read and compared, then the copied world is opened and validated again;
- only after those checks is the temporary directory renamed to the destination;
- limits default to 100,000 files, 8 GiB per file and 64 GiB total; tests can
  lower them through `PassthroughOptions`.

File names, directory layout and file contents are preserved. OS timestamps,
ACLs and alternate data streams are outside the passthrough contract.

The report contains the canonical source and destination, every copied relative
path with its size and SHA-256, skipped entries, region/chunk counts and whether
chunks were validated. `PassthroughReport::bytes()` returns the copied byte
count. Errors have stable codes such as `unsupported_format`,
`verification_failed`, `unsafe_path`, `already_exists` and `limit_exceeded`.

Unknown fields inside a region are preserved because the whole region file is
copied. Re-encoding a _modified_ chunk while carrying unknown tags is handled by
the in-memory overlay below; writing a new region file is a later export stage.

```powershell
cargo test -p furybuilder-world-io --test passthrough
cargo run -p furybuilder-world-io --example passthrough_world -- <source> <new-destination> --all-files
```

## Block-edit overlay (M8.5)

`crates/world-io/src/overlay.rs` implements copy-on-write block edits over an
imported classic world. The source stays read-only; edits are stored as complete
replacement sections per chunk and merged at read time.

| API                                         | What it does                                                                    |
| ------------------------------------------- | ------------------------------------------------------------------------------- |
| `WorldOverlay::open`                        | validates the source world and starts with no edits                             |
| `get_block`                                 | reads a source block or an edited block with the same call                      |
| `set_block` / `apply`                       | edits one block or a validated atomic batch; duplicate positions are refused    |
| `read_chunk`                                | returns a decoded chunk with edited sections merged and source biomes untouched |
| `dirty_chunks` / `dirty_section_count`      | reports exactly which chunks/sections differ from the source                    |
| `clear_chunk` / `clear`                     | drops edits without touching source files                                       |
| `materialize_chunk_nbt`                     | re-encodes one dirty chunk to NBT in memory                                     |
| `document` / `to_json_pretty` / `from_json` | persists and reloads the edits as a small versioned JSON document               |

Guarantees and checks:

- the source world passes `JavaWorld::open` and the fixed `DataVersion` 4189
  check; the source is opened read-only and is never written;
- blocks outside the dimension's `min_y`/`height` are refused, as are unknown
  dimensions, unallocated chunks and sections that do not exist in the source;
- a batch is all-or-nothing: every edit is validated before any overlay state is
  staged, and a failure leaves the previous overlay untouched;
- edits use the same validated `BlockState` and canonical section model as the
  rest of the editor;
- `materialize_chunk_nbt` replaces only `sections[].block_states` for edited
  sections, uses Minecraft 1.21.4's non-spanning packing with the 4-bit minimum,
  keeps properties and preserves every other root tag, section tag (including
  biomes), block entity and entity;
- the patched NBT is re-decoded with the strict import decoder and every
  non-block-state field is compared with the source before the value is returned;
- overlay JSON has a schema version, the source `DataVersion`, palette strings
  and canonical packed words as hexadecimal strings (exact above 2^53);
- loading revalidates chunk existence, section existence, palette syntax, packed
  word size and canonical section layout, then prunes sections equal to source;
- limits are 100,000 dirty chunks, 1,000,000 dirty sections and 64 MiB of JSON.

No region file is written in M8.5. The overlay document and the materialized NBT
are the inputs for the later export stage, which must still publish a new world
copy atomically.

```powershell
cargo test -p furybuilder-world-io --test overlay
cargo run -p furybuilder-world-io --example overlay_probe -- <world> <dimension> <x> <y> <z> <block-state>
cargo run -p furybuilder-world-io --example overlay_probe -- <world> <dimension> <x> <y> <z> <block-state> --document
```

## FuryBuilder support matrix (M8 planning)

| World                                                      | DataVersion | Layout  | Read                                              | Passthrough                     | Overlay                      | Write | Decision                        |
| ---------------------------------------------------------- | ----------- | ------- | ------------------------------------------------- | ------------------------------- | ---------------------------- | ----- | ------------------------------- |
| Java 1.21.4, classic                                       | 4189        | classic | `level.dat`, regions, block states, biomes (M8.3) | byte copy of whole files (M8.4) | in-memory block edits (M8.5) | no    | first implementation target     |
| Java 1.21.5–1.21.11, classic                               | 4325–4671   | classic | no                                                | no                              | no                           | no    | needs a separate tested adapter |
| Java 26.1–26.3, split                                      | 4786–5023   | split   | no                                                | no                              | no                           | no    | needs a new layout adapter      |
| Java 26.4 snapshots                                        | 5119–5130+  | split   | no                                                | no                              | no                           | no    | unsupported, moving target      |
| Pre-1.13 saves (pre-flattening, including MCRegion worlds) | varies      | legacy  | no                                                | no                              | no                           | no    | out of scope                    |
| Bedrock worlds                                             | n/a         | Bedrock | no                                                | no                              | no                           | no    | out of scope                    |

“Read” here means “a verified importer exists”. A folder being recognisable by
the inventory script is not read support. “Passthrough” means unchanged files
are copied byte for byte. “Overlay” means block edits are kept in memory and can
be materialized to chunk NBT; it does not mean modified region files can already
be exported.

## Safety rules for the M8 importer

- Read `level.dat` with bounded NBT: size, nesting, collection length and
  decompression limits; never follow symlinks outside the selected world.
- Require `DataVersion` and `Version.Id` to agree. Report `version_history`
  when present but do not use it as the sole version check.
- Do not edit a world while Minecraft or a server holds `session.lock`.
  Treat the lock as a warning and a concurrency guard, not as proof of safety.
- Preserve unknown top-level files and unknown NBT fields, or refuse the write.
  Do not silently drop `datapacks/`, `players/`, `data/`, custom dimensions,
  `generated/` or resource packs.
- Do not infer a dimension list from folder names alone. A dimension with no
  region files is still a dimension, and custom dimensions come from datapacks.
- Treat `level.dat_old`, `level.dat_new` and `.mcc` external chunk files as
  separate cases with their own policy.
- Region coordinates use floor division; negative region and chunk coordinates
  are normal. The file name is `r.<regionX>.<regionZ>.mca`.
- Export to a **new copy**. Never modify the user's source world by default.
- A single `.mca` is a region, not a world; do not open it as one.

## Evidence and reproduction

The inspector is standard-library Python and read-only:

```powershell
python scripts/inspect_world_layout.py <world-folder>
python scripts/inspect_world_layout.py <world-folder> --require-target
python scripts/inspect_world_layout.py <world-folder> --expect-data-version 4189
python -m unittest discover -s scripts -p "test_inspect_world_layout.py" -v
```

Recorded evidence:

- `docs/testing/evidence/2026-11-18-world-layout-1.21.4.json` — a real
  official 1.21.4 server world: classic layout, `DataVersion` 4189,
  `Version.Id` 4189, 4 Overworld region files, `targetMatch: true`.
- `docs/testing/evidence/2026-11-18-anvil-read-1.21.4.json` — the Rust reader
  decoded the same real world: 4 regions and 841 allocated chunks, every chunk
  `DataVersion` 4189, `yPos` -4 and 24 sections. The status breakdown is 672
  `structure_starts`, 48 `biomes`, 40 `carvers`, 32 `initialize_light` and 49
  `full`; the non-full chunks are proto-chunks, not import failures. The Nether
  and End had no region files in this world. The probe exited 0 and wrote
  nothing to the world.
- `docs/testing/evidence/2026-11-18-anvil-decode-1.21.4.json` — the same world
  fully decoded: 841 chunks, 20,184 sections, 125 non-empty sections, 123,908
  non-air blocks, 0 missing sections in present chunks, 5 distinct block states
  and the `minecraft:plains` biome. All negative regions `r.-1.-1`,
  `r.-1.0` and `r.0.-1` decoded successfully. `--decode` exited 0.
- `docs/testing/evidence/2026-11-18-anvil-biomes-1.21.4.json` — a separately
  generated vanilla 1.21.4 world with five biomes and 98 block states:
  841 chunks and 1,040 non-empty sections decoded. The running server was asked
  for the biome at 16 cell centres in chunk (-2, -3) at y=63, 65 and 100; the
  decoded `beach` cell at (2, 3) and `ocean` elsewhere matched all 48 answers.
  The world and server logs are not committed. A caveat is recorded: at y=241,
  in an empty section above terrain, the server reported `ocean` while the
  stored NBT says `beach`; the adapter reports stored NBT and does not claim
  that the game uses those cells.
- `docs/testing/evidence/2026-11-18-anvil-passthrough-1.21.4.json` — the
  841-chunk world copied to a new temporary folder: 20 files, 3,481,260 bytes,
  4 regions, all chunks validated, and a deterministic manifest SHA-256. The
  source had 21 files; only `session.lock` was skipped. An independent
  PowerShell SHA-256 comparison confirmed that all 20 destination files match
  the source and that all 21 source files were unchanged after the copy.
- `docs/testing/evidence/2026-11-18-anvil-overlay-1.21.4.json` - a real block
  edit over the same world: `minecraft:grass_block[snowy=false]` at
  (-15, -61, -15) was replaced by `minecraft:gold_block`; one chunk and one
  section became dirty, the biome stayed `minecraft:plains`, the patched chunk
  re-decoded to the new block, the overlay document was 6,634 bytes, and all 21
  source files were byte-identical before and after the probe.
- `docs/testing/evidence/2026-11-18-world-layout-26.3.json` - a real official
  26.3 server world: split layout, `DataVersion` 5023, `Version.Id` 5023,
  `version_history: [5023]`, all three default dimensions under
  `dimensions/minecraft/`, `targetMatch: false`.
- `docs/testing/evidence/2026-11-18-world-layout-26.3-server-version.txt` —
  official `server.jar` SHA-1 `33680f5f…`, size 62,294,556 bytes and
  `version.json` showing `world_version: 5023`, Java 25 and protocol 777.
- The 1.21.4 server jar used for the real world is the user's local copy:
  SHA-1 `4707d00e…`, size 56,880,250 bytes, `version.json` showing
  `world_version: 4189`, Java 21 and protocol 769. It is not redistributed.

The 26.3 world was generated in a temporary directory with the official
server. The existing EULA acceptance record from the user's server profile was
copied unchanged; the test did not accept the EULA, did not touch the existing
world or launcher settings, and bound the server to loopback.

Sources checked on 2026-11-18:

- Mojang `version_manifest_v2.json` — latest release 26.3, latest snapshot
  26.4-snapshot-10.
- Minecraft Wiki, [Java Edition level format](https://minecraft.wiki/w/Java_Edition_level_format)
  — current/deprecated directory structures and the 26.1/26.3 history.
- Minecraft Wiki, [Data version](https://minecraft.wiki/w/Data_version) —
  release data versions.
- Official `server.jar` `version.json` for 1.21.4 and 26.3 — runtime metadata
  above.

## Consequences for the next M8 tasks

1. The M8.3 reader covers `level.dat`, region containers, block-state palettes
   and biomes for 1.21.4 classic. M8.4 adds a byte-preserving, validated copy of
   unchanged files, including unknown metadata. M8.5 adds an in-memory
   copy-on-write overlay and verified chunk NBT materialization. The next step is
   connecting that overlay to the project wizard.
2. Keep layout detection explicit. Opening a 26.1+ folder must return a
   structured `unsupported_layout`/`unsupported_version`, not a partial world.
3. Add a real 26.1+ fixture and adapter only when the game format and the
   importer can be tested together; do not promise it from the folder names.
4. Never write in place. An export must copy the world first, replace only
   verified chunks, re-open the result, and publish a new folder atomically.
