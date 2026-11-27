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
- a single `versions/<id>` folder or a managed cache folder containing `client.jar`;
- a `client.jar` or any JAR that identifies its exact release.

The component manager accepts a manual source for a project only when the version id and
`DataVersion` match the project target. When a sibling `<id>.json`/`client.json` declares the
client SHA-1 and size, the JAR is hashed and compared; a declared mismatch is refused. A
correct JAR without launcher metadata is still usable offline with an explicit warning in the
component status. FuryBuilder never copies, edits or deletes the selected file. Version folders
and managed cache folders are self-describing: they resolve the JAR directly instead of going
through `.minecraft` target discovery.

A direct JAR is also self-describing for catalog construction, independent of the application's
current target. `CatalogBuilder` normally reads its internal `version.json` and records that
exact version, `DataVersion` and pack formats. Client JARs for 1.13.x are the only flattened
releases without a root `version.json`; for those, the adjacent launcher metadata
(`<stem>.json`, or `client.json` beside `client.jar`) names the exact release and the matching
built-in profile supplies `DataVersion` and pack formats. That fallback always adds a warning
and is refused when the metadata is absent, invalid, or names a release without an exact
built-in profile. From 1.14 on, the internal file is present: 1.14–1.14.2 append a
`release / build-hash` id that is normalized to the release, and 1.14.3+ use the plain release
id. Adjacent launcher metadata is still used for checksum verification whenever it is present.
When both sources exist, their version ids must agree. This lets fixtures and version diffs
compare catalogs across releases. A project-pinned rebuild must still reproduce the manifest's
`catalogId`, `catalogHash`, client JAR SHA-1, Minecraft version and `DataVersion`; any mismatch
is a conflict, never a silent substitution.

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
| adjacent `<stem>.json`/`client.json` | Version id and client SHA-1/size when the JAR has no internal `version.json` (1.13.x); checksum metadata is verified for every release |
| `assets/<ns>/blockstates/<block>.json` | Block ids, variant/multipart properties, first model |
| `assets/<ns>/models/block/*.json` | Model parent chain and block icon texture |
| `assets/<ns>/models/item/*.json` | Item icon textures and the item-model fallback for pre-item-definition releases |
| `assets/<ns>/items/<item>.json` | Item ids and item model references (1.20.6+) |
| `assets/<ns>/textures/**/*.png` | Texture existence and counts; the icon atlas decodes only referenced icons |
| `assets/<ns>/lang/en_us.json` | Display names; other languages are counted |
| `data/<ns>/tags/block/**/*.json` | Direct block tag membership (1.21+; `tags/blocks` on older releases) |
| `data/<ns>/tags/item/**/*.json` | Direct item tag membership (1.21+; `tags/items` on older releases) |
| `assets/<ns>/atlases/*.json` | Atlas ids and source descriptors |

Only the `minecraft` namespace is vanilla. Other namespaces found in a modified JAR are
reported as entries with an explicit warning; they are not silently presented as vanilla.

Client JARs for 1.13–1.17 use the flattened asset layout: singular
`assets/minecraft/textures/block`/`item`, no `assets/minecraft/items` item definitions, no
`assets/minecraft/atlases`, item models under `models/item`, and plural
`data/minecraft/tags/blocks`/`items` folders. The extractor reads both layouts. Because those
releases have no item definitions, an item is admitted from the item-model fallback (and the
`item.<ns>.<path>` language key) only when a real non-empty item model exists; model templates
such as `item/generated`, empty placeholder models such as `item/air`, wall-fan variants and
entity-only blockstates never become items or blocks. Entity render blockstates
(`item_frame`, `glow_item_frame`) are skipped with a warning, and empty or special item models
keep a `null` icon instead of borrowing a `particle` placeholder.

## Snapshot contract

`CatalogSnapshot` contains:

- `catalogId` and `catalogHash`;
- exact Minecraft version, `DataVersion`, client JAR SHA-1 and pack formats. The pack formats
  come from the internal `version.json`; when it is absent they come from the exact built-in
  flattened profile named by SHA-1-matched adjacent launcher metadata, and the snapshot carries
  an explicit warning;
- one sorted `entries` list and atlas definitions;
- one `CatalogEntry` per id, merging its block and item forms. An entry with block states
  has `states`; an entry with an item form has `itemForm`; a normal block with an item has
  both, while block-only technical entries and item-only entries have one;
- `legacyId`/`metadata` for numeric registries, `displayName`, `category`, direct `tags`,
  `iconTexture` and aliases. Named-registry entries have `legacyId: null`. `category` stays
  `null` because neither the client JAR nor its tags carry Minecraft's creative-tab
  assignment; the desktop palette derives advisory creative-style groups instead. Aliases stay
  empty until a versioned alias source exists;
- declared property keys and values under `states`, whether those values are complete, and
  the single declared default state when it is unambiguous;
- every explicit `variants` entry with its exact property key and weighted model choices,
  including `x`/`y` rotation and `uvlock`. Multipart blocks have no finite variant list;
- item definition model type and resolved concrete model under `itemForm`;
- typed `flags` for placement rules. A flag is `null` when the fact is not known yet, never
  a guessed `false`. `placeable`, `hasItem` and `hasVariants` follow directly from the JAR;
  `obtainable`, `creativeOnly`, `technical`, `fluid`, `airlike` and `blockEntity` use explicit
  vanilla registry rules for the supported vanilla range. Non-vanilla namespaces leave those rules `null`
  instead of claiming vanilla semantics; transparent/full-cube/gravity/waterloggable/tinted
  geometry flags also remain `null` until a later pass;
- counts for blockstates, models, item definitions, textures, languages, tags and atlases;
- bounded inspection warnings.

`catalogHash` is SHA-256 over the canonical entries/atlases, the version, the
`DataVersion` and the client JAR SHA-1. `catalogId` is
`java-<version>-<first 16 hex of catalogHash>`. `CatalogBuilder` resolves the client JAR and
resolves its version identity; the service cache is process-local and keyed by exact version,
client JAR SHA-1 and catalog schema. Path, size and modification time are not
identity, so moving or copying a byte-identical JAR reuses the same snapshot and updates only
the reported display path. A JAR without a readable SHA-1 falls back to its path, size and
modification time. A project pins `catalogId` and `catalogHash` in its schema-2 manifest.

## Icon atlas

`CatalogIconAtlas` is the presentation companion to the registry snapshot: one lazy packed PNG
batch plus the rectangles of the requested icon textures. A batch contains only distinct
textures named in the request, so a palette can draw icons without decoding individual files in
the WebView and without building a full-catalog atlas. The render atlas used by the viewport is
separate and only contains the block states currently visible.

- The batch is built from the exact client JAR and the ordered resource-pack stack. Packs may
  replace a texture, but they never add or remove catalog entries.
- A request must name 1–512 textures that the exact catalog declares as `iconTexture`; empty,
  unknown or oversized key lists are rejected. Arbitrary archive paths are never accepted.
- The cache key is
  `minecraftVersion|clientJarSha1|packStackHash|catalogSchema.atlasSchema|requestedKeysHash`;
  changing the JAR, the order or bytes of usable packs, the version, the catalog/atlas schema or
  the requested key set changes the key. The cache is process-local, bounded to 32 entries and
  16 MiB, and is cleared by `assets_clear_cache`.
- `assets_catalog_atlas`/`loadAssetCatalogAtlas(textureKeys)` build a vanilla batch for the
  selected client JAR. `project_catalog_atlas`/`loadProjectCatalogAtlas(textureKeys)` and
  `project_render_bundle` use the JAR and pack stack pinned by the open project, and return
  `null` for placeholder projects.
- Missing or malformed textures use the same marked checker as the render atlas. Animated
  textures use their first frame. The atlas never changes block names, states or world data.
- The atlas is not part of `catalogHash`; changing a resource pack must not invalidate the
  catalog identity pinned by a project. The `Blocks` palette requests only the visible window,
  its overscan and the selected entry, then merges the returned texture rectangles.

## Palette UI

The `Blocks` palette is a fixed-row virtualized list: only the visible window plus a small
overscan is mounted, and scrolling moves the window without changing the catalog. Rows use the
project icon atlas when it is available. The selected entry shows a larger isometric preview
built from the same catalog texture. That preview is a presentation of the icon texture, not
a full block-model render; complex and multipart models are not reconstructed.

State editing is constrained where the client JAR can prove the values:

- A blockstate file that enumerates every value of every declared property renders a select
  with those values plus `Game default`. Choosing `Game default` removes the override, so
  Minecraft resolves its own default state.
- An incomplete blockstate file keeps a text input because undeclared values are accepted. The
  declared values are offered as suggestions.
- An imported value outside the declared set stays visible and is labelled `(not declared)`;
  it is never silently dropped or replaced. The palette explains that the value is preserved.
- Multipart blocks do not invent a single default. `Game default` is still available as an
  explicit empty selection.

A selected block or state absent from the exact catalog is labelled `Not in this version`.
The `Game default` action is disabled with a visible reason, while `Use as replace source`
remains available: existing imported blocks keep their state, and replacement matching is
read-only. New placement is still refused by the write registry.

The palette also compares the version-independent curated quick-pick ids from
`curated_block_catalog` with the exact catalog. A quick pick that is absent from the exact
version is shown under `Quick picks not in this version` with selection and placement disabled.
This comparison is deliberately narrow: it proves absence of those ids, but it is not a full
cross-version catalog diff, and it does not apply rename hints or write aliases. A renamed block
can therefore appear under its old quick-pick id. Full diff-driven suggestions wait for a
reference catalog to be pinned; `CatalogDiff` itself is read-only and already available.

## Performance and memory

The palette is designed for a catalog well beyond the current 1,097 blocks and 1,385 items:

- The atlas is lazy. The UI asks only for icon textures in the virtualized window plus the
  selected entry, debounces requests while scrolling and keeps at most eight returned batches
  or 16 MiB in the WebView. The backend independently caps its batch cache at 32 entries and
  16 MiB. A batch is never allowed to grow past 512 textures.
- Search builds one lowercase haystack per entry and a trigram posting index. A query is
  normalized once, then a three-character-or-longer query starts from its rarest trigram; shorter
  queries scan the cached haystacks. The index is capped at an estimated 24 MiB. If a future
  catalog exceeds that budget, the palette still searches with the linear matcher instead of
  retaining an unbounded index.
- The canonical catalog payload held in the backend and sent to the WebView is capped at 64 MiB.
  The cap is checked against the same canonical JSON used for `catalogHash`, so an oversized
  catalog fails with `limit_exceeded` instead of exhausting memory. A synthetic 10,000-entry
  catalog test stays under this budget.
- A real 1.21.4 client JAR reports 990 distinct palette icon textures. A 256-texture lazy batch
  measures 256×512 PNG and 191,547 bytes, and a repeated request is a cache hit. The previous
  eager atlas measurement is historical; the UI no longer builds it.

## Project authority

Opening a project rebuilds the catalog from the selected client JAR and checks the catalog id,
catalog hash, client JAR SHA-1, Minecraft version and `DataVersion` against the manifest. A
different JAR is a conflict, not a silent substitution. The resulting catalog is installed as
the write-admission registry for the open world, so every block placed, filled, painted or
generated through the command API must exist in the same catalog. The `Validation` rules use
that registry too.

`project_catalog` (desktop) / `loadProjectCatalog()` (UI) returns the rebuilt catalog. If the
project explicitly uses placeholder assets, it returns `null`; the palette then loads
`curated_block_catalog` quick picks and displays the fallback reason. A rebuild failure is also
shown as a fallback notice with the underlying error; the palette never pretends the curated
list is the exact version catalog. The project write registry still comes only from
`project_catalog`; curated picks are never write authority.

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

## Version diff

`CatalogDiff::between(from, to)` compares two exact-version snapshots without mutating either
one:

- `added` and `removed` carry id, display name, block/item form and a vanilla-namespace marker.
  A removed id is the explicit unknown set for the target version; it is never mapped to air or
  to another id.
- `renamed` contains alias hints with evidence. Verified vanilla renames for the modern range
  are listed explicitly (`grass` → `short_grass`, `scute` → `turtle_scute`). Additional pairs
  are only inferred when icon, display name, tags and block/item form all match and the pairing
  is unique in both directions. Ambiguous or non-vanilla pairs remain in `added`/`removed`.
- `changed` covers common entries: display name, category, icon, aliases, tags, item form,
  flags, legacy id, and detailed state changes (block form, property keys, accepted values,
  default state, completeness, variant count/list, multipart and model).
- `unchanged` counts common entries with none of those changes. The diff also carries both
  catalog ids, versions, `DataVersion`s and schema versions.

Duplicate entry ids in either snapshot are rejected. Rename hints do not populate
`CatalogEntry.aliases`, do not authorize placement and do not convert imported worlds, and they
are not surfaced in the palette yet. The palette's `not in this version` badges use proven
absence from the exact catalog (an unknown selected state or a curated quick-pick id); full
diff-driven suggestions remain later because a second reference catalog is not pinned in the
project.

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
- Creative-style palette groups (`Building Blocks`, `Colored Blocks`, `Natural Blocks`,
  `Functional Blocks`, `Redstone Blocks`, `Technical`, `Other`) are derived in the desktop UI
  from direct tags, catalog flags and conservative id patterns. They do not change
  `catalogHash` and are not an official copy of Minecraft's creative tabs; a block can appear
  in several groups, while unmatched or non-vanilla entries fall into `Other`. The catalog
  `category`/`aliases` fields stay `null`/empty until official metadata or an alias source can
  fill them honestly. Search covers the full id, display name and direct tags; the flag filter
  covers placeable, has-item, technical, creative-only, fluid and block-entity entries. The
  state picker constrains complete declared sets and preserves undeclared imported values with
  a warning. The item browser and per-swatch color/shape/version filters remain later catalog
  stages; item components are preserved opaque, not modelled or edited.
- Recent selections and starred favorites are local UI preferences keyed by `catalogId`; they
  are not part of the project manifest, do not authorize writes, and are filtered against the
  current catalog when shown. Preferences retain up to 24 recent selections and 256 favorites
  per catalog across the eight most recently updated catalogs. Curated quick picks use a
  separate history.
- Palette flags are advisory labels, not write gates. Technical and creative-only blocks stay
  in the write registry and remain placeable; fluids and airlike entries are marked for editor
  rules; a non-vanilla namespace keeps all semantic flags `null`. Unknown imported block states
  are still refused for new placement exactly as before.
- The palette lists block-form entries only. Item definitions are extracted and counted, but
  the item browser and placeable-item duplication are later stages.
- Item definitions resolve the first concrete model from `model`, `base`, `select`,
  `range_dispatch`, `condition` and `composite` shapes; `modelType` records the top-level
  dispatcher. The full definition stays in the JAR. Data components and stack contents are
  not modelled or edited; imported item stacks inside block entities and NBT are preserved
  opaquely, including every component key and value.
- The icon atlas is loaded lazily in visible-window batches, and the search index is rebuilt
  from the loaded catalog rather than persisted. Neither the atlas nor the search index changes
  catalog identity or write admission. The current real-JAR measurement is 990 distinct icon
  textures; the 10k+ entry target is covered by the bounded atlas, index and payload budgets
  above.
- Tag membership is direct. Transitive `#tag` references remain available in the JAR and are
  not flattened into each entry.
- A texture missing from the JAR produces a null icon and a warning. The asset-index fallback
  is not implemented yet. Special item renderers (shields, heads, banners) have no baked item
  texture; their `particle` placeholder is not used as an icon, so the icon is null instead
  of a misleading block texture.
- Releases before item definitions fall back to item models and language keys. Only a real,
  non-empty `models/item/<path>` model backed by a valid id is admitted; templates
  (`item/generated`), empty placeholders (`item/air`), wall-fan variants and entity-backed
  blockstates are excluded. Numeric ids and pre-1.8 registries remain outside the catalog and
  belong to the legacy stage.
- Entity render blockstates are not blocks. `item_frame` and `glow_item_frame` have blockstate
  files for the entity renderer, so they are skipped with a warning while their item forms stay
  item-only. An imported world containing such names still preserves the stored id and
  properties; it simply is not admitted as a placeable block.
- Tests build synthetic client JARs from the built-in version profiles. A 1.21.4 contract test
  checks sorted unique ids, count parity, flag/form agreement and state/variant legality; a
  checked-in golden JSON snapshot fixes the full diff wire shape for added, removed, renamed,
  changed and unchanged entries. The end-to-end fixture covers one representative release of
  each modern family (`1.18.2`, `1.19.4`, `1.20.6`, `1.21.4`): it builds the exact catalog,
  imports the world, creates a project pinned to that catalog, edits a block, saves, reopens,
  rebuilds the catalog from the same JAR, exports the world and re-imports the export while
  checking the edit, the pin and preserved unknown states. Flattened fixtures cover all 19
  releases `1.13`–`1.17.1`: launcher-metadata resolution, internal `version.json`, checksum
  refusal, version folders, weighted variants, plural tags, entity blockstates, wall fans,
  empty item models and special items. Every flattened fixture builds the exact catalog,
  checks block/item/state/tag facts, pins it in a project, edits, saves, reopens, rebuilds
  the catalog from the same JAR, exports and re-imports while checking the edit and the
  preserved unknown state. Real-JAR verification counted 598/789 blocks/items for `1.13.2`,
  676/876 for `1.14.4`, 680/883 for `1.15.2`, 763/975 for `1.16.5` and 898/1,099 for `1.17.1`,
  matching the PrismarineJS registries with no missing or extra entries. A real 1.21.4 client
  JAR currently reports 1,097 blockstate entries, 1,385 item definitions, 186 block tag files,
  173 item tag files, 14 atlases and 990 distinct palette icon textures. Icons are served lazily
  in bounded batches; the 256-texture measurement is in Performance and memory.

## Safety bounds

- At most 32,768 blockstates and 32,768 item definitions.
- At most 65,536 textures and 65,536 `en_us` keys.
- At most 16,384 tag files, 256 atlas sources per atlas and 256 warnings.
- One catalog JSON document is capped at 4 MiB.
- One lazy icon-atlas batch requests at most 512 catalog textures and 32 million packed pixels;
  one decoded texture is capped at 1024×1024.
- A malformed file is skipped with a warning; a JAR without blockstates is refused instead of
  being reported as an empty catalog.
