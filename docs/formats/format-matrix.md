# Format support matrix

Target game version: Minecraft Java **1.21.4** (`DataVersion` 4189).

Legend: **R** read, **W** verified write of a new file, **P** passthrough untouched data, **E** in-memory block edits, **I** Import World project wizard, **—** unsupported.

| Format                         | Native container                                 | DataVersion | MVP scope                                                                                                                                                                                                                                                                                                                                                  | Confidence                    |
| ------------------------------ | ------------------------------------------------ | ----------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------- |
| `.furybuilder/` project        | Folder: JSON metadata + chunk journal            | n/a         | **R/W**                                                                                                                                                                                                                                                                                                                                                    | Native, versioned schema      |
| Java structure `.nbt`          | Uncompressed/gzip NBT compound                   | 4189        | **R/W** (first export)                                                                                                                                                                                                                                                                                                                                     | Spec + synthetic golden       |
| Sponge `.schem` v3             | Gzip NBT, varint block data                      | 4189        | **R/W** (second adapter)                                                                                                                                                                                                                                                                                                                                   | Spec + synthetic golden       |
| Anvil `.mca`                   | Region header + compressed chunk NBT             | 4189        | **R** (1.21.4 classic: containers, block states, biomes; M8.3); **P** byte copy (M8.4); **E** edits (M8.5); **W** new region, `sections[].block_states` only (M8.7); derived data inventoried (M8.8) and preserve/vanilla-rebuild policy (M8.9); explicitly a region container, not a save (M8.11)                                                         | Synthetic + real 1.21.4 world |
| Java world folder              | `level.dat`, `region/`, dimension dirs           | 4189        | **R** (classic: version, dimensions, chunks; M8.3); **P** (M8.4); **E** overlay (M8.5); **I** wizard with immutable source copy (M8.6); **W** verified region files (M8.7); derived-data inventory and server round-trip (M8.8); vanilla-rebuild hand-off verified (M8.9); complete new world-folder export (M8.10); save-vs-region classification (M8.11) | Real 1.21.4 world + tests     |
| Java world folder, 26.1+ split | `level.dat`, `dimensions/<ns>/<id>/`, `players/` | 4786–5023   | **D** detect complete save and inventory region containers (M8.11); no chunk read/edit/write/passthrough                                                                                                                                                                                                                                                   | Not supported                 |
| `.litematic`                   | Gzip NBT, regions + packed longs                 | varies      | — (P2)                                                                                                                                                                                                                                                                                                                                                     | Not started                   |
| Legacy `.schematic`            | Gzip NBT, numeric Blocks/Data                    | <= 1.12 era | — (P3)                                                                                                                                                                                                                                                                                                                                                     | Not started                   |
| `.mcstructure`                 | Bedrock NBT                                      | n/a         | — (future)                                                                                                                                                                                                                                                                                                                                                 | Not started                   |
| `.snbt`                        | Text NBT                                         | debug       | **R/W** debug helper                                                                                                                                                                                                                                                                                                                                       | Parser only                   |

## What each format can represent

| Data                      | `.nbt` structure     | `.schem` v3 | `.mca` chunk     | Native project |
| ------------------------- | -------------------- | ----------- | ---------------- | -------------- |
| Block states + properties | Yes                  | Yes         | Yes              | Yes            |
| Structure bounds / offset | Yes                  | Yes         | n/a              | Yes            |
| Block entities            | Yes (`blocks[].nbt`) | Yes         | Yes              | Yes            |
| Entities                  | Yes (separate list)  | Yes         | Yes              | Yes            |
| Biomes                    | No                   | Optional    | Yes              | Yes            |
| Selection / history / DSL | No                   | No          | No               | Yes            |
| Semantic objects          | No                   | No          | No               | Yes            |
| Unknown NBT fields        | No                   | No          | Passthrough only | Yes            |

Formats that cannot carry FuryBuilder metadata must never claim to do so.
The Anvil writer only changes `sections[].block_states`. Biomes, block entities,
entities and unknown NBT are preserved as raw data but are not semantically
recomputed; heightmaps, lighting and POI are likewise not recomputed in Rust.
M8.8 inventoried those fields on a real 1.21.4 world and confirmed with an
official server that a normal load/save does not rebuild stale heightmaps or
light and does not create missing block entities. M8.9 adds an explicit
`vanillaRebuild` policy: the writer invalidates heightmaps, light, `isLightOn`
and `PostProcessing` for replaced chunks and the official 1.21.4 server rebuilt
them on the next load/save. M8.10 exports a complete new world folder, copying
`level.dat`, dimensions, sidecars and unknown files byte for byte and rebuilding
only dirty regions. M8.11 classifies the selection first: a bare `.mca` file or a
`region/` directory is explicitly not a complete save, and split, mixed and legacy
layouts are refused by both import and passthrough. See
`docs/formats/java-chunk-derived-data.md`, `docs/formats/java-world-export.md` and
`docs/formats/java-world-selection.md`.
