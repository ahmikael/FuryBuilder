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

M9.11 persists biome data in `world-domain`: `Section` carries an optional
`BiomeSection` (a 4x4x4 YZX cell grid), `to_chunk()` copies it, snapshot format 2
stores it and the overlay/region/export paths write edited cells into
`sections[].biomes`. Raw chunk NBT is also kept on `RegionChunk` for passthrough
and overlay work. The byte-preserving copy and the in-memory overlay are described
below.

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
the in-memory overlay and the verified region writer below; a complete world
folder export is still pending.

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

## Import World wizard (M8.6)

The desktop wizard now offers `Import World` as a third creation mode. The folder
is analyzed before any project is created:

| API                            | What it does                                                                  |
| ------------------------------ | ----------------------------------------------------------------------------- |
| `world_import_preview`         | read-only `level.dat`, layout, region and chunk validation with blockers      |
| `project_import_world`         | creates the project, copies the source byte for byte, decodes editable blocks |
| `world_io::analyze_java_world` | library analysis used by the command and by the probe                         |
| `world_io::load_java_world`    | bounded block-state decoding into the sparse editor world                     |

Guarantees and checks:

- the selected folder is never modified; analysis only reads `level.dat`, region
  tables and chunk NBT;
- only classic Java 1.21.4 (`DataVersion` 4189) is accepted; 26.1+ split worlds,
  legacy `.mcr` worlds and mixed layouts are refused with explicit blockers;
- the analysis validates every allocated chunk and reports regions, chunks,
  dimensions, block entities and entities;
- the project stores an immutable source copy under `source/world/` using the M8.4
  passthrough adapter: byte equality, `session.lock` skipped, every copied file
  recorded with its SHA-256 in `source/source.json`;
- editable import limits are 20,000 chunks, 2,000,000 non-empty sections and
  50,000,000 non-air blocks; exceeding a limit fails before a project is kept;
- block states and 4x4x4 biome cells enter the editable project snapshot. Block
  entities, entities, ticks, heightmaps, lighting, POI and unknown tags remain in the
  source copy; the wizard lists these limitations instead of dropping them silently;
- a failed copy or manifest write removes the half-created project folder.

`Import World` itself does not write a new world or a region file; it creates the
read-only source copy and the editable snapshot. The verified writer below is the
write engine for the later full-world export.

```powershell
cargo test -p furybuilder-world-io --test world_import
cargo test -p furybuilder-project-store --test source_overlay
cargo run -p furybuilder-desktop --example import_world_probe -- <world> <empty-project-root> "Imported Flat"
cargo run -p furybuilder-desktop --example import_world_probe -- <world> --analysis-only
pnpm --filter @furybuilder/desktop test -- ProjectManager.test.tsx
```

## Verified region writing (M8.7)

`crates/world-io/src/region_write.rs` is a deliberately narrow writer. It refuses
anything outside this explicit combination:

| Axis                  | Allowed                                                                |
| --------------------- | ---------------------------------------------------------------------- |
| Game version          | Minecraft Java 1.21.4, `DataVersion` 4189                              |
| Layout                | classic Anvil (`region/`, `DIM-1/`, `DIM1/`), standard dimensions only |
| Editable chunk fields | `sections[].block_states` and 4x4x4 `sections[].biomes` (M9.11)        |
| Derived-data policy   | `preserve` (raw) or `vanillaRebuild` (explicit engine hand-off, M8.9)  |
| Chunk status          | edited chunks must have `Status = minecraft:full`                      |
| Source compression    | gzip, zlib or uncompressed; the writer emits zlib                      |
| Destination           | a new standalone `r.<x>.<z>.mca`, never the source region directory    |

Every replacement is checked against the source NBT before a byte is written:
root tags, section `Y` values, biomes, block entities, entities, ticks, heightmaps,
lighting, POI and unknown tags must be unchanged unless the selected policy
explicitly invalidates its documented derived fields. The patched chunk is then
re-decoded with the same strict decoder used for import. An identical replacement,
an unknown field change, a non-full chunk, an external `.mcc` chunk or a
destination that already exists is refused with a structured error.

Publication is staged and verified:

- the source is opened read-only; a SHA-256 of the source region is taken before
  and after the write and a mismatch aborts publication;
- untouched chunk records are copied byte for byte, including their timestamps;
  the region container itself is rebuilt with regenerated location/timestamp
  tables;
- the output is written inside a temporary sibling directory, reopened with the
  strict region reader, and every untouched record is compared byte for byte with
  the source before the file is renamed into place;
- a failure removes the staging directory and leaves no destination file.

Known limitations, recorded in the report as `recomputesDerivedMetadata: false`:
block edits do **not** recompute anything in Rust. The default `preserve` policy
copies heightmaps, lighting, block entities, ticks, POI and other derived data as
raw bytes. The opt-in `vanillaRebuild` policy removes `Heightmaps`, `SkyLight`,
`BlockLight` and `PostProcessing`, and sets `isLightOn` to `0`, so the target
vanilla engine rebuilds them on the next load; the report lists the invalidated
fields. An official 1.21.4 server was verified to rebuild all four heightmaps,
both light arrays, `isLightOn` and `PostProcessing` while preserving the edited
block states. Block entities and POI are still not synthesized or rewritten. See
`docs/formats/java-chunk-derived-data.md` and
`docs/testing/evidence/2026-11-18-derived-rebuild-1.21.4.json`. A full world-folder
export is still M8.10.

```powershell
cargo test -p furybuilder-world-io --test region_write
cargo run -p furybuilder-world-io --example region_write_probe -- <world> <new-region-file> minecraft:overworld -15 -61 -15 minecraft:gold_block
cargo run -p furybuilder-world-io --example region_write_probe -- <world> <new-region-file> minecraft:overworld 0 -61 0 minecraft:air 3 -60 3 minecraft:torch --rebuild-derived
```

## Chunk-derived data (M8.8)

`crates/world-io/src/derived.rs` is a read-only inventory of heightmaps, light
arrays, scheduled ticks, post-processing positions, block entities, structure
starts and the `entities/`/`poi/` sidecar containers. It never writes a chunk or
a sidecar. The probe reports per-chunk fingerprints and an aggregate inventory;
the full findings are in `docs/formats/java-chunk-derived-data.md`.

The real 841-chunk world had 49 full chunks, 81 chunks with heightmaps, 49 chunks
with `SkyLight` (146 sections, no `BlockLight` because all-zero light is omitted),
24 `PostProcessing` entries per chunk, empty block entities/ticks, and four
0-byte `poi/` plus four 0-byte `entities/` placeholders. A controlled official
server round-trip over five edits found:

| Finding                | Result                                                                                            |
| ---------------------- | ------------------------------------------------------------------------------------------------- |
| `sections` fingerprint | changed, as expected for edited block states                                                      |
| heightmaps             | unchanged after removing the surface block                                                        |
| sky/block light        | unchanged after removing a block and placing a torch                                              |
| block entities         | no records created for a chest, sign or barrel                                                    |
| `PostProcessing`       | unchanged                                                                                         |
| scheduled ticks        | unchanged                                                                                         |
| `poi/`                 | one `minecraft:fisherman` record for the placed barrel, written by the server, not by FuryBuilder |
| source world           | all files byte-identical; server exited 0                                                         |

This is evidence, not a universal parity proof. The default writer policy changes
only `sections[].block_states` and, since M9.11, explicitly painted
`sections[].biomes`; do not place or remove block-entity blocks, and do not claim
that heightmaps or light are correct when the raw `preserve` policy is used. M8.9
adds an explicit `vanillaRebuild` policy; see below.

## Vanilla rebuild policy (M8.9)

`DerivedDataPolicy::VanillaRebuild` is a documented hand-off to the target
vanilla engine, not a Rust-side recomputation. For replaced full chunks the
writer removes `Heightmaps`, `SkyLight`, `BlockLight` and `PostProcessing`, sets
`isLightOn` to `0`, and keeps every other field byte-equivalent to an identically
invalidated source clone. The report exposes `derivedDataPolicy`,
`derivedDataAction` and `invalidatedFields`.

The official 1.21.4 server round-trip over two edits confirmed that the engine
rebuilds what the policy invalidates:

| Finding             | Result                                                          |
| ------------------- | --------------------------------------------------------------- |
| heightmaps          | rebuilt: all four kinds, 37 words each                          |
| sky light           | rebuilt (non-null fingerprint)                                  |
| block light         | rebuilt (non-null fingerprint)                                  |
| `isLightOn`         | restored to `1`                                                 |
| `PostProcessing`    | rebuilt to 24 entries                                           |
| edited block states | preserved exactly                                               |
| block entities      | still `0`; the engine did not synthesize missing block entities |
| source world        | byte-identical; server exited 0                                 |

The default remains `preserve`; the caller must choose the engine hand-off
explicitly and must accept that heightmap/light correctness is then owned by the
vanilla engine. POI and block-entity semantics remain outside both policies.

```powershell
cargo test -p furybuilder-world-io --test derived --test region_write
cargo run -p furybuilder-world-io --example derived_probe -- <world> --world
cargo run -p furybuilder-world-io --example derived_probe -- <world> --chunk 0 0
cargo run -p furybuilder-world-io --example derived_probe -- <world> --blocks 0 -61 0 2 -60 2
cargo run -p furybuilder-world-io --example region_write_probe -- <world> <new-region-file> minecraft:overworld 0 -61 0 minecraft:air 3 -60 3 minecraft:torch --rebuild-derived
```

## Full world-folder export (M8.10)

`crates/world-io/src/world_export.rs` combines the byte-preserving passthrough copy
with the verified region writer. It exports a complete save folder: `level.dat`,
all dimensions, sidecars, unknown files and untouched regions are copied byte for
byte, while only dirty regions are rebuilt. The destination must be a new folder;
the source is never written. The staged folder is reopened, every chunk is
decoded again, every staged file is compared with the passthrough manifest or the
rebuilt-region SHA-256, and the source tree hash is rechecked before an atomic
rename publishes the result.

`DerivedDataPolicy::Preserve` keeps derived fields raw; `VanillaRebuild` performs
the M8.9 hand-off for replaced chunks. A single `.mca` is not a world, and this
module does not treat it as one. See `docs/formats/java-world-export.md`.

```powershell
cargo test -p furybuilder-world-io --test world_export
cargo run -p furybuilder-world-io --example world_export_probe -- <world> <new-world-folder> minecraft:overworld 0 -61 0 minecraft:gold_block
cargo run -p furybuilder-world-io --example world_export_probe -- <world> <new-world-folder>
```

## World selection and layout boundary (M8.11)

`crates/world-io/src/world_selection.rs` classifies the selected path before any
import or copy starts. A single `r.<x>.<z>.mca` file and a bare `region/`
directory are explicitly reported as **not a complete save**; `level.dat`,
dimensions and sidecars are part of the boundary. The same classifier separates a
complete classic 1.21.4 save from a complete-but-unsupported 26.1+ split save, a
mixed-layout folder, legacy `.mcr` worlds and a `level.dat`-only folder.

`analyze_java_world` includes `selectionKind` and `completeSave`, and the Import
World wizard shows both. Passthrough now refuses an incomplete selection with
`unsupported_format` before constructing any destination; split, mixed and legacy
layouts fail with `unsupported_version`. See
`docs/formats/java-world-selection.md`.

```powershell
cargo test -p furybuilder-world-io --test world_selection
cargo run -p furybuilder-world-io --example world_selection_probe -- <world>
cargo run -p furybuilder-world-io --example world_selection_probe -- <world>/region/r.0.0.mca
cargo run -p furybuilder-world-io --example world_selection_probe -- <world> --passthrough <new-folder>
```

## Automated official-server fixture (M8.12)

`scripts/verify_java_world_export.ps1` closes the loop between the reader, the
writer and the real engine. It requires a user-provided Java 21 and official
1.21.4 `server.jar`, an expected SHA-1 and an explicit `-AcceptMinecraftEula`
switch. It never downloads or redistributes either dependency.

The script lets the official server generate a fresh flat fixture, exports an
edited complete world folder with `world_export_probe`, starts the same server on
the exported folder, verifies the edited block and an untouched neighbor through
`execute if block`, stops and saves, then decodes the saved block states again.
The source fixture is hashed before and after and must stay byte-identical. See
`docs/testing/minecraft-server-fixture.md`.

```powershell
pwsh -File scripts/verify_java_world_export.ps1 `
  -Java <java.exe> -ServerJar <server.jar> `
  -ExpectedSha1 4707d00eb834b446575d89a61a11b5d548d8c001 `
  -AcceptMinecraftEula
```

## FuryBuilder support matrix (M8 planning)

| World                                                      | DataVersion | Layout  | Read                                              | Passthrough                                                   | Overlay                                           | Write                                                                                                                                                                                                | Decision                        |
| ---------------------------------------------------------- | ----------- | ------- | ------------------------------------------------- | ------------------------------------------------------------- | ------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------- |
| Java 1.21.4, classic                                       | 4189        | classic | `level.dat`, regions, block states, biomes (M8.3) | byte copy of whole files (M8.4); selection classified (M8.11) | in-memory block/biome edits + Import World wizard (M8.5-M8.6, M9.11) | new region file, block states and biomes (M8.7, M9.11); derived-data policies (M8.8-M8.9); complete world-folder export (M8.10); save-vs-region classification (M8.11); official server load/save fixture (M8.12) | first implementation target     |
| Java 1.21.5–1.21.11, classic                               | 4325–4671   | classic | no                                                | no                                                            | no                                                | no                                                                                                                                                                                                   | needs a separate tested adapter |
| Java 26.1–26.3, split                                      | 4786–5023   | split   | no                                                | no                                                            | no                                                | no                                                                                                                                                                                                   | needs a new layout adapter      |
| Java 26.4 snapshots                                        | 5119–5130+  | split   | no                                                | no                                                            | no                                                | no                                                                                                                                                                                                   | unsupported, moving target      |
| Pre-1.13 saves (pre-flattening, including MCRegion worlds) | varies      | legacy  | no                                                | no                                                            | no                                                | no                                                                                                                                                                                                   | out of scope                    |
| Bedrock worlds                                             | n/a         | Bedrock | no                                                | no                                                            | no                                                | no                                                                                                                                                                                                   | out of scope                    |

“Read” here means “a verified importer exists”. A folder being recognisable by
the inventory script is not read support. “Passthrough” means unchanged files
are copied byte for byte. “Overlay/wizard” means block edits are kept in memory,
the desktop wizard can create an imported project, and edits can be materialized
to chunk NBT. “Write” means a new verified region file for the listed fields; it
is never an in-place write. A full world-folder export exists for classic 1.21.4
(M8.10). Derived data is preserved raw by default; M8.8 showed that an official
server does not automatically rebuild stale heightmaps, light or missing block
entities. M8.9 adds an opt-in policy that invalidates heightmaps, light,
`isLightOn` and `PostProcessing`, and the official server did rebuild them on
the next load/save.

The detailed per-data-category matrix - block states, biomes, block entities,
entities, ticks, heightmaps, lighting, POI, structures, unknown tags and unknown
files - is in `docs/formats/support-matrix.md`. The same matrix is returned by
`analyze_java_world` and shown in the Import World wizard.

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
- `docs/testing/evidence/2026-11-18-world-import-1.21.4.json` - the full Import
  World pipeline over the same 841-chunk world: analysis accepted 4 regions and
  decoded 123,908 non-air blocks; the source copy copied 20 files / 3,481,260
  bytes, all 20 copied files matched the source SHA-256, the source stayed
  byte-identical, and the reopened project had 121 editable chunks. The same
  evidence records a real 26.3 split world being refused with `DataVersion 5023`
  and split-layout blockers.
- `docs/testing/evidence/2026-11-18-region-write-1.21.4.json` - the first real
  verified region write over the same world: chunk (-1, -1) was edited from
  `minecraft:grass_block[snowy=false]` to `minecraft:gold_block`, 1 of 196 chunk
  records was replaced and 195 untouched records were byte-identical with
  unchanged timestamps; the 811,008-byte output re-decoded with `minecraft:plains`
  and `minecraft:full`; all 21 source files stayed byte-identical.
- `docs/testing/evidence/2026-11-18-derived-inventory-1.21.4.json` - read-only
  derived-data inventory of the same 841-chunk world: 49 full chunks, 81 chunks
  with heightmaps, 49 chunks with `SkyLight`, 20,184 `PostProcessing` entries,
  empty block entities/ticks and four 0-byte `poi/` plus four 0-byte `entities/`
  placeholders; the source tree stayed unchanged.
- `docs/testing/evidence/2026-11-18-derived-data-1.21.4.json` - the official
  1.21.4 server round-trip over five block edits: only the `sections` fingerprint
  changed; heightmaps, light, block entities, ticks, `PostProcessing` and
  structures stayed unchanged; a chest, sign and barrel survived without block
  entities; the server added one `minecraft:fisherman` POI record for the barrel;
  the source world stayed byte-identical and the server exited 0.
- `docs/testing/evidence/2026-11-18-derived-rebuild-1.21.4.json` - the official
  1.21.4 server round-trip with `DerivedDataPolicy::VanillaRebuild` over two
  edits: the written chunk had no heightmaps/light/`PostProcessing` and
  `isLightOn = 0`; after load/save the server rebuilt all four heightmaps (37
  words each), both light arrays, `isLightOn = 1` and 24 `PostProcessing`
  entries; the edited block states were preserved, `blockEntities` stayed 0 and
  the source world stayed byte-identical.
- `docs/testing/evidence/2026-11-18-world-export-1.21.4.json` - a complete
  world-folder export of the same 841-chunk world with one edited block:
  20 files / 3,481,260 bytes exported, only `session.lock` skipped; 4 regions /
  841 chunks, 1 region / 1 chunk replaced; all non-region files matched source
  SHA-256; the source tree stayed byte-identical; the exported chunk re-decoded to
  `minecraft:gold_block` with `minecraft:plains`.
- `docs/testing/evidence/2026-11-18-world-selection-1.21.4.json` - selection
  classification over a real classic 1.21.4 save and a real 26.3 split save:
  the full save was a supported `world_folder` (841 chunks), its bare
  `region/r.0.0.mca` was a `region_file` (225 header records) and its `region/`
  folder was a `region_directory`; neither is a complete save. The split save was
  complete but unsupported. Classic passthrough copied 20 files / 4 regions /
  841 chunks without `session.lock`; the region-directory attempt returned
  `unsupported_format` and the split attempt `unsupported_version`, neither
  created a destination, and both source trees stayed byte-identical.
- `docs/testing/evidence/2026-11-18-server-fixture-1.21.4.json` - the automated
  official-server fixture: the server generated a fresh flat 1.21.4 world,
  FuryBuilder exported an edited complete folder with `VanillaRebuild`, and the
  same server loaded it. `execute if block` found the gold block, did not find
  stale grass, and found the untouched dirt neighbor; the server exited 0,
  rewrote 10 files, and FuryBuilder decoded the persisted gold block and dirt
  neighbor again. The generated fixture stayed byte-identical.
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

1. The M8 reader/writer chain now covers `level.dat`, region containers,
   block-state palettes and biomes (M8.3), byte-preserving copy (M8.4), the
   copy-on-write overlay (M8.5), the Import World wizard (M8.6), verified region
   writing (M8.7), derived-data inventory and policies (M8.8–M8.9), complete
   world-folder export (M8.10), save-vs-region classification (M8.11) and an
   automated official-server fixture (M8.12). The remaining M8 item is the
   consolidated read/edit/write/passthrough support matrix (M8.13).
2. Keep layout detection explicit. Opening a 26.1+ folder must return a
   structured `unsupported_layout`/`unsupported_version`, not a partial world.
3. Add a real 26.1+ fixture and adapter only when the game format and the
   importer can be tested together; do not promise it from the folder names.
4. Never write in place. An export must copy the world first, replace only
   verified chunks, re-open the result, and publish a new folder atomically.
5. The official server is verified. A licensed Minecraft client was not launched
   in the automated fixture; client rendering remains a separate manual check.
