# Java 1.21.4 chunk-derived data (M8.8)

Scope: classic Anvil layout, Minecraft Java **1.21.4**, `DataVersion` 4189. This
document records what a block chunk carries besides `block_states`, where the
sidecar data lives, and what the official 1.21.4 server actually changed after a
verified region write. It is a research record, not a promise that FuryBuilder
recalculates these fields.

The implementation is `crates/world-io/src/derived.rs`; the reproducible probe is
`crates/world-io/examples/derived_probe.rs`. The inventory is read-only and the
probe never writes to the source world.

## Where the data lives

### Block chunk NBT

Classic dimensions store block chunks in `region/`, `DIM-1/region/` and
`DIM1/region/`. The fields relevant to an edit are:

| Field                                | Meaning                                                                                    | M8.7 writer                        | Observed 1.21.4 server behavior                                |
| ------------------------------------ | ------------------------------------------------------------------------------------------ | ---------------------------------- | -------------------------------------------------------------- |
| `sections[].block_states`            | Paletted block states                                                                      | Replaced for edited sections only  | Kept the edited states; default state properties added on load |
| `sections[].biomes`                  | Paletted biomes per 4×4×4 cell                                                             | Preserved byte for byte            | Unchanged                                                      |
| `sections[].SkyLight` / `BlockLight` | 2048-byte light arrays per section                                                         | Preserved byte for byte            | Unchanged after edits                                          |
| `Heightmaps`                         | `MOTION_BLOCKING`, `MOTION_BLOCKING_NO_LEAVES`, `OCEAN_FLOOR`, `WORLD_SURFACE` long arrays | Preserved byte for byte            | Unchanged after a surface block was removed                    |
| `block_entities`                     | List of block-entity compounds                                                             | Preserved byte for byte            | No missing block entity was created                            |
| `block_ticks` / `fluid_ticks`        | Scheduled ticks                                                                            | Preserved byte for byte            | Unchanged                                                      |
| `PostProcessing`                     | Positions queued for light post-processing                                                 | Preserved byte for byte            | Unchanged                                                      |
| `isLightOn`                          | Whether the serialized light arrays are trusted                                            | Preserved byte for byte            | Unchanged; stayed `1`                                          |
| `structures`                         | Structure starts/references                                                                | Preserved byte for byte            | Unchanged in the flat fixture                                  |
| `blending_data`                      | Optional chunk-blending metadata                                                           | Preserved byte for byte            | Absent in the fixture                                          |
| Any unknown root/section tag         | Version- or mod-specific data                                                              | Preserved and compared with source | Not interpreted                                                |

The writer's contract remains narrow: it changes only
`sections[].block_states`, re-decodes the patched chunk with the strict reader,
and compares every other field with the source before publishing. The report
explicitly says `recomputesDerivedMetadata: false`.

### Sidecar region containers

Since Minecraft 1.17, entities are not normally stored in the chunk root; they
live in per-dimension `entities/` region files. Points of interest live in
per-dimension `poi/` region files. Both use the Anvil region container shape,
but their chunk roots do not carry `xPos`, `zPos`, `Status` or `sections`, so the
ordinary chunk-summary reader cannot be used for them.

Observed `poi/` shape in 1.21.4:

```text
root compound
├── DataVersion: int
└── Sections: compound
    └── "-4": compound
        ├── Valid: byte (1)
        └── Records: list<compound>
            ├── pos: int array [x, y, z]
            ├── type: string (for example minecraft:fisherman)
            └── free_tickets: int
```

Observed `entities/` shape:

```text
root compound
├── DataVersion: int
├── Position: long array
└── Entities: list<compound>   # ids such as minecraft:pig, minecraft:cow
```

`poi/` and `entities/` folders are optional. A zero-byte `.mca` file is a valid
placeholder used by the server for an empty sidecar; it is not a corrupt region.
The probe keeps these files untouched and reports whether they exist, how many
records they contain, and a bounded sample of entity/POI types.

Other world data (`level.dat`, `data/`, `playerdata/`, `datapacks/`, maps, raids,
advancements and so on) is passthrough in the current project model. Block edits
do not rewrite it.

## Real 1.21.4 inventory

Source: the same flat 1.21.4 world used by the M8.6/M8.7 evidence
(`fury-reference-server-11ppj36x/world`). The exact JSON is
`docs/testing/evidence/2026-11-18-derived-inventory-1.21.4.json`.

| Metric                                    | Value                              |
| ----------------------------------------- | ---------------------------------- |
| Chunks scanned                            | 841                                |
| Full chunks (`Status = minecraft:full`)   | 49                                 |
| Proto chunks                              | 792                                |
| Sections                                  | 20,184                             |
| Chunks with `Heightmaps`                  | 81                                 |
| Heightmap kinds                           | 4, each with 37 words per chunk    |
| Chunks with `SkyLight`                    | 49                                 |
| Sections with `SkyLight`                  | 146                                |
| Sections with `BlockLight`                | 0 (all-zero light is omitted)      |
| Chunks with `isLightOn = 1`               | 49                                 |
| Chunks with `PostProcessing`              | 841, 24 entries each, 20,184 total |
| Chunks with non-empty `block_entities`    | 0                                  |
| Chunks with `block_ticks` / `fluid_ticks` | 0 non-empty                        |
| Chunks with legacy `entities` field       | 792, all empty                     |
| Chunks with structure starts              | 0                                  |
| Chunks with `blending_data`               | 0                                  |
| Overworld `poi/` / `entities/` files      | 4 + 4, all 0-byte placeholders     |
| Nether/End sidecar folders                | absent                             |

The 792 proto chunks are not "full" and have no heightmaps or light arrays.
They still carry `block_entities`, `block_ticks`, `fluid_ticks`, `entities` and
`structures` (mostly empty lists). The analyzer counts those fields only when
they are non-empty, so a field being present and being meaningful are not the
same thing.

## Controlled server round-trip

The experiment used the official Minecraft 1.21.4 server JAR
(SHA-1 `4707d00eb834b446575d89a61a11b5d548d8c001`) and the already accepted local
EULA record from the same fixture. It did not generate or change EULA acceptance.
The server listened only on `127.0.0.1`, used `online-mode=false`,
`random-tick-speed=0`, `sync-chunk-writes=true` and a copied world. It started,
ran for 25 seconds without a player, received `stop`, and exited with code 0.

Five edits were made in chunk `(0, 0)` of the Overworld:

| Position      | Before                               | After write          | After server load/save                                        |
| ------------- | ------------------------------------ | -------------------- | ------------------------------------------------------------- |
| `(0, -61, 0)` | `minecraft:grass_block[snowy=false]` | `minecraft:air`      | `minecraft:air`                                               |
| `(2, -60, 2)` | air                                  | `minecraft:chest`    | `minecraft:chest[facing=north,type=single,waterlogged=false]` |
| `(3, -60, 3)` | air                                  | `minecraft:torch`    | `minecraft:torch`                                             |
| `(4, -60, 4)` | air                                  | `minecraft:oak_sign` | `minecraft:oak_sign[rotation=0,waterlogged=false]`            |
| `(6, -60, 6)` | air                                  | `minecraft:barrel`   | `minecraft:barrel[facing=north,open=false]`                   |

The server did not reject the chunk and logged no block-entity or chunk error.
It did add default state properties that were omitted from the palette entries.
The source region and every source file stayed byte-identical.

### What the server did not recalculate

Before/after fingerprints of the same chunk:

| Derived field    | Changed by the server         |
| ---------------- | ----------------------------- |
| `heightmaps`     | no                            |
| `skyLight`       | no                            |
| `blockLight`     | no                            |
| `blockEntities`  | no                            |
| `blockTicks`     | no                            |
| `fluidTicks`     | no                            |
| `blockEntities`  | no                            |
| `postProcessing` | no                            |
| `structures`     | no                            |
| `sections`       | yes (the edited block states) |

The chunk still had `blockEntities = 0` after the round-trip. The chest, sign and
barrel survived as block states, but **no block-entity records were created**.
The server trusted the serialized chunk and did not infer missing block entities
from the block states.

The `Heightmaps` arrays and light arrays also stayed unchanged. Removing the top
surface block and placing a torch did not make the server recompute them during
load/save. This is the key limitation for the current writer: a data-valid chunk
can still contain **stale heightmaps and stale light**.

### What the server did update

The Overworld `poi/r.0.0.mca` file grew from 0 bytes to 12,288 bytes. It contained
one record:

```text
pos: [6, -60, 6]
type: minecraft:fisherman
free_tickets: 1
```

That is the job-site POI for the placed barrel. In this one case the server added
the missing POI record while loading the chunk. This was observed for one POI
block only; the writer itself never edits `poi/`, and the behavior is not a
general guarantee for beds, portals, workstations or other POI blocks. The
`entities/` sidecar stayed at 0 bytes because the fixture had no entities.

## Consequences for block edits

| Data            | Consequence of a block-state-only edit                                                                         | Current status                                                |
| --------------- | -------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------- |
| Block states    | Intended change; properties default on load if omitted                                                         | Supported, verified                                           |
| Biomes          | Unchanged; editing biomes is not exposed                                                                       | Preserved, not edited                                         |
| Block entities  | Existing records stay raw; missing records are **not** created; removed blocks can leave orphan records        | **Unsupported semantically**; avoid adding/removing BE blocks |
| Entities        | `entities/` sidecars are copied byte for byte; an entity is not moved or removed when a block appears under it | Preserved, not edited                                         |
| Scheduled ticks | Existing lists stay raw; new blocks are not scheduled; stale entries may be discarded by the game              | Preserved, not recomputed                                     |
| Heightmaps      | Stale after surface changes; the server did not fix them on load/save                                          | **Not recalculated**; target of M8.9                          |
| Lighting        | Stale after block removal/placement; `isLightOn` stays trusted                                                 | **Not recalculated**; target of M8.9                          |
| POI             | Writer does not update `poi/`; one barrel POI was added by the server, not by FuryBuilder                      | Not guaranteed; target of M8.9                                |
| Structures      | Raw starts/references are preserved; block edits do not update them                                            | Preserved; non-empty fixture not tested                       |
| Unknown tags    | Preserved and compared with the source                                                                         | Supported                                                     |

## Decisions for M8.9 and M8.10

Do not call a block-state-only export "fully updated" or "vanilla-parity". The
following choices are deliberately left to M8.9:

1. **Heightmaps** — implement a deterministic recalculation for edited sections,
   or explicitly document that the game must rebuild them. The experiment shows
   the game does not rebuild them on a normal load/save.
2. **Lighting** — implement a relight, or test a bounded way to make the game
   rebuild it (for example clearing light state). Do not silently keep stale
   arrays and claim correct lighting.
3. **Block entities** — either synthesize/remove the corresponding compounds for
   supported blocks, or refuse edits that add/remove block-entity blocks. The
   current writer must not imply that a chest placed this way has an inventory.
4. **POI** — rebuild `poi/` records for edited sections, or rely on the server
   only with an explicit warning. The barrel observation is not a general proof.
5. **Ticks** — decide whether to clear stale ticks for changed positions and
   whether any supported block needs a newly scheduled tick.
6. **Full-world export** — copy `level.dat`, sidecars and untouched regions,
   replace only changed region files, then re-read and validate the whole folder.
   A single `.mca` is not a world.

## Reproduce

```powershell
cargo test -p furybuilder-world-io --test derived
cargo run -p furybuilder-world-io --example derived_probe -- <world> --world
cargo run -p furybuilder-world-io --example derived_probe -- <world> --chunk 0 0
cargo run -p furybuilder-world-io --example derived_probe -- <world> --blocks 0 -61 0 2 -60 2
```

Evidence:

- `docs/testing/evidence/2026-11-18-derived-inventory-1.21.4.json` — read-only
  inventory of 841 chunks and the sidecar scan; the source tree is hashed before
  and after.
- `docs/testing/evidence/2026-11-18-derived-data-1.21.4.json` — the controlled
  server round-trip, before/after chunk fingerprints, block states, sidecar
  inventory, source hashes and the server log summary.

## Limits of this research

- One flat 1.21.4 world, one full chunk, five block edits, one server run. It is
  evidence, not a universal compatibility proof.
- The fixture had no entities, non-empty block entities, scheduled ticks or
  structure starts, so those paths were inventoried as empty fields but not
  exercised with live records.
- The server was run offline on loopback; the authlib public-key fetch timed out,
  which is unrelated to chunk loading. The server still reached `Done`, saved all
  dimensions and exited with code 0.
- No claim is made that the Minecraft client renders the stale light identically
  to a fully relit chunk. That is exactly why M8.9 must resolve lighting before
  the export is called verified for gameplay.
