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
Block names, states and world data are unchanged. Placeholder projects have no resource-pack
stack because packs require a pinned client JAR. Changing the global asset selection does not
edit any project's pins.

## Project resource packs

In **Project Manager → Project Settings → Resource packs**, add a local ZIP or folder,
move it up or down, or remove it. Highest priority comes first; the client JAR supplies any
assets the stack does not override. Adding a pack explicitly copies it into the local asset
cache; a folder is snapshotted as a deterministic ZIP. It is not a live link to the selected
file, and the pack is not bundled into the project or exported with the world.

FuryBuilder verifies the cached SHA-1 and the resource pack format declared by `pack.mcmeta`
against the project's exact profile. Missing, changed, invalid or incompatible packs remain in
the stack but are skipped and reported. **Check pack files** verifies them again and refreshes
the viewport. Re-add the original bytes to restore a missing or changed snapshot without
duplicating the pin (if the stack is full, remove the unavailable entry first). To use an
updated version of a pack, remove the old entry and add the new file.

Format compatibility does not imply full Minecraft rendering parity: FuryBuilder resolves
the pack's root `assets/` entries. It does not apply `pack.mcmeta` overlays.

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
- one sorted `entries` list and atlas definitions;
- one `CatalogEntry` per id, merging its block and item forms. An entry with block states
  has `states`; an entry with an item form has `itemForm`; a normal block with an item has
  both, while block-only technical entries and item-only entries have one;
- `legacyId`/`metadata` for numeric registries, `displayName`, `category`, direct `tags`,
  `iconTexture` and aliases. Named-registry entries have `legacyId: null`; category and
  aliases stay `null`/empty until their catalog passes fill them;
- declared property keys and values under `states`, whether those values are complete, and
  the single declared default state when it is unambiguous;
- every explicit `variants` entry with its exact property key and weighted model choices,
  including `x`/`y` rotation and `uvlock`. Multipart blocks have no finite variant list;
- item definition model type and resolved concrete model under `itemForm`;
- typed `flags` for placement rules. A flag is `null` when the fact is not known yet, never
  a guessed `false`. The JAR pass currently fills `placeable`, `hasItem` and `hasVariants`;
  survival obtainability, technical/creative-only, fluid, block-entity and geometry flags
  remain `null` until later passes;
- counts for blockstates, models, item definitions, textures, languages, tags and atlases;
- bounded inspection warnings.

`catalogHash` is SHA-256 over the canonical entries/atlases, the version, the
`DataVersion` and the client JAR SHA-1. `catalogId` is
`java-<version>-<first 16 hex of catalogHash>`. `CatalogBuilder` resolves the client JAR and
reads its embedded `version.json`; the service cache is process-local and keyed exactly by
exact version, client JAR SHA-1 and catalog schema. Path, size and modification time are not
identity, so moving or copying a byte-identical JAR reuses the same snapshot and updates only
the reported display path. A JAR without a readable SHA-1 falls back to its path, size and
modification time. A project pins `catalogId` and `catalogHash` in its schema-2 manifest.

## Icon atlas

`CatalogIconAtlas` is the presentation companion to the registry snapshot: a packed PNG plus
the rectangle of every icon texture. It contains only the distinct textures referenced by
`iconTexture` on catalog entries, so a palette can draw an icon without decoding
individual files in the WebView. The render atlas used by the viewport is separate and only
contains the block states currently visible.

- The atlas is built from the exact client JAR and the ordered resource-pack stack. Packs may
  replace a texture, but they never add or remove catalog entries.
- The cache key is exactly
  `minecraftVersion|clientJarSha1|packStackHash|catalogSchema.atlasSchema`; changing the JAR,
  the order or bytes of usable packs, the version or the catalog/atlas schema changes the key.
  The cache is process-local, bounded to four entries and 64 MiB, and is cleared by
  `assets_clear_cache`.
- `assets_catalog_atlas`/`loadAssetCatalogAtlas()` build the vanilla atlas for the selected
  client JAR. `project_catalog_atlas`/`loadProjectCatalogAtlas()` and `project_render_bundle`
  use the JAR and pack stack pinned by the open project, and return `null` for placeholder projects.
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

## Imported block states

World import preserves original block IDs and properties even when the pinned catalog does
not admit the state. On import or reopen, the editor warns if any used states fail the same
registry check that governs writes. The notice gives a count of affected sections and a sample
state; **Analysis → Validation → Export compatibility** locates issues in a selected area.
Without a pinned JAR, the notice explicitly refers to the limited write registry, not to a
complete Minecraft catalog.

Existing unsupported states survive saving and are not mapped to air or to another block.
They can be read or matched as a replace source, and undo can restore them at their original
positions. Placing them as new blocks, including through paste, structure insertion or
template insertion, is refused. Importing a structure as a **new project** instead preserves
its source states in the initial snapshot. Technical support for carrying stored states through
same-version export does not guarantee that a vanilla Minecraft client recognizes those states.

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
- Basic palette groups (`Building`, `Natural`, `Functional`, `Redstone`, `Technical`, `Other`)
  are derived in the desktop UI from direct block tags and conservative id patterns. They do
  not change `catalogHash` and are not Minecraft creative-inventory categories. The catalog
  model already carries `category`, `aliases` and the full flag set, but those values stay
  `null`/empty until official metadata passes fill them. Full creative categories and
  technical/creative-only flags remain later stages; item components are preserved opaque,
  not modelled or edited.
- Recent selections and starred favorites are local UI preferences keyed by `catalogId`; they
  are not part of the project manifest, do not authorize writes, and are filtered against the
  current catalog when shown. Preferences retain up to 24 recent selections and 256 favorites
  per catalog across the eight most recently updated catalogs. Curated quick picks use a
  separate history.
- The palette lists blocks only. Item definitions are extracted and counted, but the item
  browser and placeable-item duplication are later stages.
- Item definitions resolve the first concrete model from `model`, `base`, `select`,
  `range_dispatch`, `condition` and `composite` shapes; `modelType` records the top-level
  dispatcher. The full definition stays in the JAR. Data components and stack contents are
  not modelled or edited; imported item stacks inside block entities and NBT are preserved
  opaquely, including every component key and value.
- The icon atlas is exposed through `assets_catalog_atlas` and `project_catalog_atlas`, but
  the `Blocks` palette still draws flat colour chips. Wiring the atlas into the virtualized
  palette and adding 3D previews remain later UI stages.
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
