# Content catalog

Status: baseline client-JAR catalog (`P0-24`). One project has one Minecraft version,
and its block/item catalog is built from that version's exact client JAR.

## Source and licensing

- The catalog is built at runtime from the `client.jar` selected for the project. FuryBuilder
  never bundles, mirrors or redistributes Mojang files.
- Only the base client JAR defines registry contents. Resource packs change rendering; they do
  not add or remove catalog entries. The pack stack enters the render/atlas cache key.
- `asset-source` reads the JAR in memory. No archive entry is extracted to disk.

## Manual source selection

The catalog accepts a manually selected asset source in three forms:

- a `.minecraft` root, resolved to its compatible `versions/<id>/<id>.jar`;
- a single `versions/<id>` folder;
- a `client.jar` or any JAR whose internal `version.json` declares the project version.

A selection is accepted only when the internal version id and `DataVersion` match the project
target. When a sibling `<id>.json`/`client.json` declares the client SHA-1 and size, the JAR is
hashed and compared; a declared mismatch is refused. A correct JAR without launcher metadata
is still usable offline with an explicit warning in the component status. FuryBuilder never
copies, edits or deletes the selected file.

## What is read

| JAR path | Catalog use |
| --- | --- |
| `version.json` | Exact version id, `DataVersion`, resource/data pack formats |
| `assets/<ns>/blockstates/<block>.json` | Block ids, variant/multipart properties, first model |
| `assets/<ns>/models/block/*.json` | Model parent chain and block icon texture |
| `assets/<ns>/models/item/*.json` | Item icon textures and the pre-item-definition fallback |
| `assets/<ns>/items/<item>.json` | Item ids and item model references (1.21.4+) |
| `assets/<ns>/textures/**/*.png` | Texture existence and counts (catalog does not decode images) |
| `assets/<ns>/lang/en_us.json` | Display names; other languages are counted |
| `assets/<ns>/tags/block/**/*.json` | Direct block tag membership |
| `assets/<ns>/tags/item/**/*.json` | Direct item tag membership |
| `assets/<ns>/atlases/*.json` | Atlas ids and source descriptors |

Only the `minecraft` namespace is vanilla. Other namespaces found in a modified JAR are
reported as entries with an explicit warning; they are not silently presented as vanilla.

## Snapshot contract

`CatalogSnapshot` contains:

- `catalogId` and `catalogHash`;
- exact Minecraft version, `DataVersion`, client JAR SHA-1 and pack formats;
- sorted blocks, items and atlas definitions;
- counts for blockstates, models, item definitions, textures, languages, tags and atlases;
- bounded inspection warnings.

`catalogHash` is SHA-256 over the canonical blocks/items/atlases, the version, the
`DataVersion` and the client JAR SHA-1. `catalogId` is
`java-<version>-<first 16 hex of catalogHash>`. The catalog cache is process-local and keyed
by the JAR path, size, modification time and catalog schema; a persistent cache and the
project `catalogId`/`catalogHash` pin arrive with the project catalog task.

Exposed as the `assets_catalog` Tauri command and `loadAssetCatalog()` in the desktop client.

## Honest limits

- `defaultProperties` is the first declared blockstate variant. For multipart blocks it is
  `null`, because the real default state lives in game code, not in the JAR. Exact registry
  defaults remain a later catalog task.
- Creative categories, technical/creative-only flags and item components are not assigned yet.
- Tag membership is direct. Transitive `#tag` references remain available in the JAR and are
  not flattened into each entry.
- A texture missing from the JAR produces a null icon and a warning. The asset-index fallback
  is not implemented yet.
- Versions before item definitions fall back to item models and language keys. Numeric ids and
  pre-1.8 registries are outside the baseline catalog and belong to the legacy stage.
- Tests use a synthetic 1.21.4 client JAR. The real-JAR matrix and captured catalog snapshots
  belong to the full fixture stage; manual QA downloads a real JAR.

## Safety bounds

- At most 32,768 blockstates and 32,768 item definitions.
- At most 65,536 textures and 65,536 `en_us` keys.
- At most 16,384 tag files, 256 atlas sources per atlas and 256 warnings.
- One catalog JSON document is capped at 4 MiB.
- A malformed file is skipped with a warning; a JAR without blockstates is refused instead of
  being reported as an empty catalog.
