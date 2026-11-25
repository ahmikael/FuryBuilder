# Content catalog

Status: the project-pinned client-JAR catalog is authoritative for the `Blocks` palette and
command writes. The curated `BlockRegistry` is only a quick-pick list and the explicit fallback
when a project has no client JAR catalog (for example, placeholder assets).

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

## Placeholder mode

Placeholder textures are an explicit selection, not a silent fallback. Placeholder mode has no
client JAR, so FuryBuilder cannot build a block/item catalog for it: the `Blocks` palette shows
curated quick picks with a visible `Curated fallback` notice, the viewport uses original
FuryBuilder textures, and the component status reports `placeholder`/`unverified` with a warning.
Block names, states and world data are unchanged. Selecting a real client source replaces
placeholder mode; choosing placeholder clears the resource-pack stack because packs require a
client JAR.

## What is read

| JAR path | Catalog use |
| --- | --- |
| `version.json` | Exact version id, `DataVersion`, resource/data pack formats |
| `assets/<ns>/blockstates/<block>.json` | Block ids, variant/multipart properties, first model |
| `assets/<ns>/models/block/*.json` | Model parent chain and block icon texture |
| `assets/<ns>/models/item/*.json` | Item icon textures and the pre-item-definition fallback |
| `assets/<ns>/items/<item>.json` | Item ids and item model references (1.21.4+) |
| `assets/<ns>/textures/**/*.png` | Texture existence and counts; the icon atlas decodes only referenced icons |
| `assets/<ns>/lang/en_us.json` | Display names; other languages are counted |
| `data/<ns>/tags/block/**/*.json` | Direct block tag membership (1.21+; `tags/blocks` on older releases) |
| `data/<ns>/tags/item/**/*.json` | Direct item tag membership (1.21+; `tags/items` on older releases) |
| `assets/<ns>/atlases/*.json` | Atlas ids and source descriptors |

Only the `minecraft` namespace is vanilla. Other namespaces found in a modified JAR are
reported as entries with an explicit warning; they are not silently presented as vanilla.

## Snapshot contract

`CatalogSnapshot` contains:

- `catalogId` and `catalogHash`;
- exact Minecraft version, `DataVersion`, client JAR SHA-1 and pack formats;
- sorted blocks, items and atlas definitions;
- declared property keys and values, whether those values are complete, and the single
  declared default state when it is unambiguous;
- item definition model types and resolved concrete models;
- counts for blockstates, models, item definitions, textures, languages, tags and atlases;
- bounded inspection warnings.

`catalogHash` is SHA-256 over the canonical blocks/items/atlases, the version, the
`DataVersion` and the client JAR SHA-1. `catalogId` is
`java-<version>-<first 16 hex of catalogHash>`. The registry cache is process-local and keyed
by the JAR path, size, modification time and catalog schema. A project pins `catalogId` and
`catalogHash` in its schema-2 manifest.

## Icon atlas

`CatalogIconAtlas` is the presentation companion to the registry snapshot: a packed PNG plus
the rectangle of every icon texture. It contains only the distinct textures referenced by
`iconTexture` on catalog blocks and items, so a palette can draw an icon without decoding
individual files in the WebView. The render atlas used by the viewport is separate and only
contains the block states currently visible.

- The atlas is built from the exact client JAR and the ordered resource-pack stack. Packs may
  replace a texture, but they never add or remove catalog entries.
- The cache key is exactly
  `minecraftVersion|clientJarSha1|packStackHash|catalogSchema.atlasSchema`; changing the JAR,
  any pack in the stack, the version or the catalog/atlas schema rebuilds it. The cache is
  process-local, bounded to four entries and 64 MiB, and is cleared by `assets_clear_cache`.
- `assets_catalog_atlas`/`loadAssetCatalogAtlas()` build the atlas for the wizard source;
  `project_catalog_atlas`/`loadProjectCatalogAtlas()` use the catalog pinned by the open
  project and return `null` for placeholder projects.
- Missing or malformed textures use the same marked checker as the render atlas. Animated
  textures use their first frame. The atlas never changes block names, states or world data.
- The atlas is not part of `catalogHash`; changing a resource pack must not invalidate the
  catalog identity pinned by a project.

## Project authority

Opening a project rebuilds the catalog from the selected client JAR and checks the catalog id,
catalog hash, client JAR SHA-1, Minecraft version and `DataVersion` against the manifest. A
different JAR is a conflict, not a silent substitution. The resulting catalog is installed as
the write-admission registry for the open world, so every block placed, filled, painted or
generated through the command API must exist in the same catalog. The `Validation` rules use
that registry too.

`project_catalog` (desktop) / `loadProjectCatalog()` (UI) returns the rebuilt catalog. If the
project explicitly uses placeholder assets, it returns `null`; the palette then loads
`block_catalog` curated quick picks and displays the fallback reason. A rebuild failure is also
shown as a fallback notice with the underlying error; the palette never pretends the curated
list is the exact version catalog.

Exposed as the `assets_catalog` Tauri command and `loadAssetCatalog()` in the desktop client
for the project wizard, and `project_catalog`/`loadProjectCatalog()` for an open project.

## Honest limits

- `defaultProperties` is the only state declared by the blockstate file. When several
  variants exist it is `null`; the palette then omits properties, which lets Minecraft
  resolve the exact game default. `propertyValuesComplete` is true only when the file
  enumerates every value of every property it declares. Multipart conditions and single
  empty variants are incomplete, and properties that never affect a model are not declared
  at all.
- Write validation rejects unknown block names and values outside a complete declared set.
  A key declared by any block in the same catalog is accepted on another block because
  blockstate files omit properties that do not affect a model (for example `waterlogged` on
  stairs); such values are not checked. A key that is unknown across the catalog is
  rejected. Exact per-block property registries remain a later catalog stage.
- Creative categories, technical/creative-only flags and item components are not assigned yet.
- The palette lists blocks only. Item definitions are extracted and counted, but the item
  browser and placeable-item duplication are later stages.
- Item definitions resolve the first concrete model from `model`, `base`, `select`,
  `range_dispatch`, `condition` and `composite` shapes; `modelType` records the top-level
  dispatcher. The full definition stays in the JAR; item components and stack data are not
  modelled.
- The icon atlas is exposed through `assets_catalog_atlas` and `project_catalog_atlas`, but
  the `Blocks` palette still draws flat colour chips. Wiring the atlas into the virtualized
  palette and adding 3D previews belong to the palette/UI stages.
- Tag membership is direct. Transitive `#tag` references remain available in the JAR and are
  not flattened into each entry.
- A texture missing from the JAR produces a null icon and a warning. The asset-index fallback
  is not implemented yet. Special item renderers (shields, heads, banners) have no baked item
  texture; their `particle` placeholder is not used as an icon, so the icon is null instead
  of a misleading block texture.
- Versions before item definitions fall back to item models and language keys. Numeric ids and
  pre-1.8 registries are outside the baseline catalog and belong to the legacy stage.
- Tests use a synthetic 1.21.4 client JAR. The real-JAR matrix and captured catalog snapshots
  belong to the full fixture stage; manual QA downloads a real JAR. A real 1.21.4 client JAR
  currently reports 1,097 blockstate entries, 1,385 item definitions, 186 block tag files,
  173 item tag files and 14 atlases. Its icon atlas packs 1,105 textures (including the
  missing checker) into a 256×2048 PNG of 684,731 bytes; a repeated request is a cache hit.

## Safety bounds

- At most 32,768 blockstates and 32,768 item definitions.
- At most 65,536 textures and 65,536 `en_us` keys.
- At most 16,384 tag files, 256 atlas sources per atlas and 256 warnings.
- One catalog JSON document is capped at 4 MiB.
- The icon atlas is capped at 4,096 distinct textures and 32 million pixels; one decoded
  texture is capped at 1024×1024.
- A malformed file is skipped with a warning; a JAR without blockstates is refused instead of
  being reported as an empty catalog.
