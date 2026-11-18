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

## FuryBuilder support matrix (M8 planning)

| World                                                      | DataVersion | Layout  | Read | Write | Decision                        |
| ---------------------------------------------------------- | ----------- | ------- | ---- | ----- | ------------------------------- |
| Java 1.21.4, classic                                       | 4189        | classic | M8.2 | no    | first implementation target     |
| Java 1.21.5–1.21.11, classic                               | 4325–4671   | classic | no   | no    | needs a separate tested adapter |
| Java 26.1–26.3, split                                      | 4786–5023   | split   | no   | no    | needs a new layout adapter      |
| Java 26.4 snapshots                                        | 5119–5130+  | split   | no   | no    | unsupported, moving target      |
| Pre-1.13 saves (pre-flattening, including MCRegion worlds) | varies      | legacy  | no   | no    | out of scope                    |
| Bedrock worlds                                             | n/a         | Bedrock | no   | no    | out of scope                    |

“Read” here means “a verified importer exists”. A folder being recognisable by
the inventory script is not read support.

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
- `docs/testing/evidence/2026-11-18-world-layout-26.3.json` — a real official
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

1. Implement 1.21.4 classic read-only first: `level.dat` + one region at a
   time, with both version values checked.
2. Keep layout detection explicit. Opening a 26.1+ folder must return a
   structured `unsupported_layout`/`unsupported_version`, not a partial world.
3. Add a real 26.1+ fixture and adapter only when the game format and the
   importer can be tested together; do not promise it from the folder names.
4. Never write in place. Build an overlay and export a new world copy after
   the read path is verified.
