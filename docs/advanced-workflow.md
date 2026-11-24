# Advanced workflows

This guide is for users who already completed [Getting started](getting-started.md) and want
to use FuryBuilder for larger, repeatable or automated work. It describes the current alpha.
Every mutating workflow below ends in the same Rust transaction pipeline; there is no
“expert mode” that bypasses revisions, previews or history.

## 1. How a write actually happens

```
UI or MCP request
  → Rust preview (bounds, policy, revision, changed blocks)
  → user confirmation when the caller is MCP or the paste is large
  → atomic Rust apply (expectedRevision)
  → new world revision + dirty chunks
  → renderer rebuilds only affected chunks
  → history/audit refresh
```

- The Rust core is authoritative. The viewport is a derived view; a screenshot is not a
  substitute for `get_block`.
- A stale `expectedRevision` is rejected with `revision_mismatch` instead of overwriting
  newer work. Re-read the world and preview again.
- Normal edits are one reversible operation. Undo/redo uses the same pipeline and reports
  its own dirty chunks.
- History is bounded (100 entries by default). A checkpoint is the durable escape hatch for
  long sessions.
- Large operations run through the Job API with progress and cancellation. A cancelled job
  rolls back before writing, so it cannot leave a partial structure.

## 2. Precise area editing

### Selection and replace

1. Press `5`, click one corner, then the opposite corner. The selection is inclusive and may
   span negative coordinates.
2. Use the Inspector to confirm the exact min/max and volume before writing.
3. `6` fills with the palette block; `8` replaces one source state inside the box.
4. The replace source can come from the palette, `Alt`-click or the context menu. Source and
   target must differ, both in the UI and in Rust.
5. Preview the operation before applying when the area is expensive or the edit is
   destructive. The ghost is a local geometric hint; the server preview is the authoritative
   diff.

Limits: a selection is capped at 1,000,000 blocks; bounds must be valid and inside the
dimension. Empty or fully out-of-range edits are rejected rather than reported as success.

### Copy and paste

- A copy is clipped to the current selection and stored relative to its minimum corner, so it
  can be pasted at negative coordinates or in another dimension.
- Copies above the 65,536-block region-read limit are split into deterministic tiles with
  percentage progress. Cancel discards partial reads and keeps the previous clipboard.
- A paste below 65,536 non-air blocks uses the immediate preview → apply path.
- A paste at or above that threshold is staged: a pink ghost shows the exact target and the
  Inspector shows the size. Press `Ctrl+V` again, use the context menu or click **Confirm
  paste**. `Escape` cancels without writing.
- Confirmed large pastes use the Rust Job API. Cancellation rolls back before writing. Both
  paths commit one reversible `Batch`.

### When an operation is refused

Check the message and the Inspector first. Typical causes are: no selection, source equals
target, volume/limit exceeded, a locked layer, a mask that does not contain the whole
operation, a protected-region intersection, or a stale revision. Fix the cause and re-preview;
do not retry a blind write.

## 3. Landscape tools

All landscape tools are **local construction tools**, not a replacement for vanilla world
generation. They change block states only. Heightmaps, light and other derived data are not
recalculated by these tools; biome cells are edited explicitly in the `Biomes` tab.

### Terrain brushes

`Terrain` operates on the topmost non-air block of every column inside the box selection:
raise, lower, smooth, flatten, noise, erode and terrace. Amount is 1–16 and terrace steps are
2–16. Height reads happen before writes, so smoothing and erosion are order-independent.
The footprint keeps the selected X/Z extent and adds a bounded vertical margin for headroom.
Empty columns are skipped with warnings.

### Heightmap generator

`Heightmap` writes a deterministic integer fBm surface patch from a seed, scale, octaves,
amplitude, base Y, surface depth and explicit surface/filler materials. The same inputs
reproduce the same blocks. It clears above-surface blocks inside the bounds and leaves blocks
below `baseY` untouched.

### Biome painter

`Biomes` paints the 4×4×4 biome cells touched by the selection. It requires a section that
already has source biome data; the core refuses to invent a default biome. Biome painting is
one reversible history entry and is written into the overlay, region writer and world export.
It does not run vanilla biome generation and makes no seed-parity claim.

### Roads, rivers, vegetation and site queries

`Generators` provides:

- roads that follow the existing surface and clear headroom;
- rivers that carve a bed and water column without deepening on reapply;
- vegetation that only fills free air and checks trunk/canopy space;
- a read-only structure-site query bounded to 2,000,000 checks and 64 sites.

Route endpoints come from the selection edges. Columns outside the selection, without a
surface or resting on fluid are skipped with warnings instead of being bridged. No generator
claims vanilla structure-placement parity.

## 4. Layers, masks and protected regions

`Layers` stores editor-only policy in `project.json`. It is not Minecraft data and does not
change blocks by itself.

- **Layer** — named edit layer with visibility and lock state. Edits record the layer; the
  active layer is used when an operation omits one. A locked layer blocks writes.
- **Active mask** — at most one, in one dimension. The entire operation bounds must fit
  inside it. Operations in other dimensions are not constrained by that mask.
- **Protected region** — any intersection is refused for preview and apply, regardless of
  layer or mask.
- Policy updates are metadata: they do not bump the world revision and are not in block
  history. Undo/redo still restore earlier block snapshots.

Limits: 64 layers, 64 masks, 64 protected regions, 64-character names and a 16 MiB volume
limit per region. The last layer and the active layer cannot be deleted. Removing a mask
clears the active mask. The Core re-validates the whole policy; the UI never sends a
half-applied state.

## 5. Reusable templates

`Templates` stores verified Java structure `.nbt` payloads inside the project:

- save the current selection as a template;
- import a verified `.nbt` file;
- preview at the selection minimum and insert as one reversible `Batch`.

Template ids are UUIDs, never file paths. Every read re-verifies the SHA-256 recorded in the
project index. Limits: 128 templates, 32 MiB per file and 256 MiB total. Traversal names,
tampered payloads, duplicate ids and unknown formats are refused.

Insertion obeys layers, masks and protected regions, uses the same preview/audit path and is
recorded on the chosen layer. It is construction data, not vanilla structure placement.
MCP can list templates read-only and insert one through the confirmed write path.

## 6. Semantic objects

`Objects` adds editor-only districts, buildings, roads, entrances, landmarks and similar
named areas. Each object has a name, kind, dimension, inclusive bounds, optional parent,
bounded tags and an optional note.

- Creating, renaming or deleting an object never places or removes blocks.
- A child must share its parent's dimension; depth is capped at 8. Deleting a parent with
  children is refused until they are removed or reparented.
- Inspection shows volume, non-air blocks, the current palette and direct children at the
  authoritative revision.
- `Use bounds as selection` only applies when the object is in the displayed dimension; it
  never switches dimensions silently.
- Scene changes are metadata: no world revision bump and no block-history entry.

Limits: 1,024 objects, 64-character names, 16 tags per object, 32-character keys,
128-character values and 1,024-character notes. Cycles, duplicate ids and unknown parents are
refused. MCP exposes read-only listing and inspection; scene writes remain in the desktop UI.

## 7. Parametric objects

`Parametric` compiles versioned recipes (currently a house, generator version 1) into a
reversible voxel overlay. The recipe stores dimension, origin, rotation, size and materials.

- **Create** compiles the recipe, captures the blocks that existed at every generated
  position and applies the overlay as one operation.
- **Transform** restores the old footprint to its captured baseline and writes the new
  footprint in one operation. Rotation also turns `facing`, `axis` and `rotation` values.
- **Conflict view** compares generator output with authoritative blocks. Manual edits inside
  the overlay produce a conflict and block transform/rollback by default. Choosing
  **Override** accepts the loss explicitly.
- **Bake/detach** forgets the recipe and baseline but keeps the blocks in the world.
- **Rollback** writes the captured baseline back and removes the recipe, returning the
  original terrain.

The baseline is palette-compressed and stored in `project.json`. Unknown generator versions
are rejected. Create, transform and rollback write blocks and appear in history; bake and
metadata-only changes do not. MCP exposes read-only listing and inspection with bounded
conflict details.

## 8. Build DSL

`Build DSL` accepts a typed, data-only JSON recipe of houses and repeat groups. It is not a
scripting sandbox: the frontend parses JSON, the Rust core validates, compiles and previews,
and the result is an ordinary `Batch`.

- House nodes have an absolute origin, optional rotation and bounded parameters. Version 1 is
  the only generator.
- `Repeat` expands a child 1–16 times along X, Y or Z with 1–64 block spacing; nesting depth
  is capped at 4.
- Budgets: at most 64 top-level nodes, 64 expanded houses and 16,384 blocks. Overlaps and
  malformed recipes are rejected before any write.
- **Preview** shows bounds, counts, changed blocks, dirty chunks and warnings. **Apply** sends
  the exact compiled operation through the normal history path.

Recipes are transient in this milestone; they are not persisted to `project.json` and no
export format claims to preserve DSL metadata. There is no `eval`, dynamic import, file or
shell access. MCP `preview_build_dsl` returns the same preview and typed operation; the agent
must still use confirmed `apply_operation` to commit.

## 9. Observation and validation

### Observation maps

`Observation` renders reproducible maps from the authoritative Rust world: top-down,
heightmap, material map, biome map, cross-section and revision diff. Every result carries
bounds, scale, legend, completeness, warnings, notes and the exact `worldRevision`.

- Modes are bounded to 512×512 pixels and 8,000,000 scanned/compared blocks.
- Revision diff needs retained section history. If the requested revision can no longer be
  reconstructed, the core returns `no_history` rather than drawing a misleading diff.
- Maps are summaries. Coordinate-level decisions still use exact `get_block`/region reads.
- The `Observation` tab is read-only; it never bumps a revision or creates history.

### Validation

`Validation` runs bounded read-only rules: floating-support heuristic, allowed-build bounds,
path-corridor continuity, palette diversity, export-catalog compatibility, block-state
orientation and protected-region conflicts. Every finding has `rule_id`, severity,
confidence, bounds, position count and the authoritative revision.

- Limits: 2,000,000 scanned blocks and 128 findings.
- Findings are sorted deterministically.
- Suggested repairs are ordinary operations. Nothing is auto-fixed: preview the suggestion,
  then apply it through the shared confirmed transaction and re-run validation.
- Repairs larger than 64 positions are left for manual review instead of being truncated.

## 10. Assets, Java and vanilla generation

### Managed downloads

Every managed download — the Mojang version manifest, version JSON, client JAR, server JAR and
the Temurin archive — goes through `crates/download-pipeline`:

- the URL must be HTTPS and belong to an exact host allowlist before the request and after
  every redirect; credentials and non-default ports are refused;
- an explicit `FURYBUILDER_PROXY` or `FURYBUILDER_HTTPS_PROXY` is used when set, otherwise the
  standard proxy environment is honored;
- `FURYBUILDER_OFFLINE=1` refuses every request; the cached version manifest is still served,
  including a stale cache, while uncached downloads fail with an `offline` error;
- transport failures and HTTP 408/425/429/500/502/503/504 are retried with bounded backoff;
  checksum, size, policy and disk-space failures are final;
- free disk space is checked before writing, with a safety margin for temporary files;
- bytes stream into a same-directory temporary file, are verified by size and SHA-1/SHA-256,
  and only then replace the destination atomically. Cancellation and every error remove the
  temporary file.

### Minecraft assets

The project wizard's **Components** step discovers conventional launcher roots and launcher
profiles, or accepts a manual `.minecraft` folder, a `versions/<id>` folder, a `client.jar` or
a resource pack. A 1.21.4 client is accepted after its internal `version.json` and
`DataVersion` 4189 agree. When launcher metadata is present, the declared SHA-1 and size are
checked too; a declared mismatch is refused, while a correct JAR without metadata is used with
a warning.

Configured resource packs are applied in a saved priority order (up to 16). Blockstates,
parent models and PNG textures are resolved into a cached atlas. A missing asset shows a
clearly marked checker and never changes block data. The wizard offers a separate explicit
action that clears only the derived cache; it does not touch projects or user resource packs.

Placeholder textures are an explicit mode, not a silent fallback. Placeholder mode has no
client JAR, so no block/item catalog or resource-pack stack is available: the viewport shows
original FuryBuilder textures and labels them, while the palette stays on the curated quick
picks. Selecting a client source replaces placeholder mode.

Settings' **Components** tab lists each managed client JAR with its version, DataVersion, size
and cache path. `Verify` re-hashes the JAR and compares it with the cache manifest, `Remove`
deletes only the FuryBuilder-owned copy after confirmation, and `Open folder` opens the known
assets cache root. Settings never changes the asset source, the pack stack or placeholder mode.

### Java and the official server

The project wizard's **Components** step is where Java and the server JAR are selected. It
provides:

- Java discovery and manual selection, with newer-than-target allowed and older/32-bit
  runtimes blocked;
- an opt-in Adoptium Temurin download into a managed, clearable cache;
- exact-version `server.jar` resolution through Mojang metadata with SHA-1/size and internal
  `version.json`/DataVersion/Java checks;
- a managed cache at `components/servers/<version>/` with one remembered selection per
  Minecraft version;
- manual JAR selection with the same exact-version checks.

The Vanilla Source step consumes these verified choices from Components and does not ask for
Java or the server JAR again; it only adds the EULA acceptance and generation settings. The
backend accepts any exact server target and never substitutes another version. Settings
lists the managed Java runtimes and server JARs with version, size and cache path and can
re-probe Java, re-inspect a server JAR, remove a FuryBuilder-owned copy or open a known cache
folder. It never downloads, selects a manual file, accepts an EULA or starts a server.

Nothing is installed globally, no `PATH` is changed, and no EULA is accepted automatically.
Each acceptance is stored for the exact server JAR SHA-1 and Minecraft version; another JAR
or version needs its own explicit acceptance. The run marker records the accepted version,
JAR SHA-1 and both approval timestamps, and a completed run is refused if its report does not
match those recorded facts.

### Vanilla generation lifecycle

After the explicit EULA acceptance for the exact JAR and version, plus a separate per-run
consent, the server runs in an isolated temporary directory without a game window. It listens
on loopback only, has memory/time limits, is contained in a kill-on-close Windows Job Object
and reports real region-header progress. Cancel or crash terminates the whole Java tree. The
completed run is imported as a `Vanilla World` project: the generated folder is copied byte
for byte, decoded into the Rust core, and seed/preset/data packs/Java/server identity are
recorded in `project.json`.

The parity panel compares a saved, untouched revision-1 generated base with the original
generator's reference chunks. It refuses unsupported or incomplete references and reports
block differences with coordinates. It does not prove parity for arbitrary settings or for
an edited world, and it does not compare biome cells.

## 11. Save, recovery and diagnostics

- **Saves** are atomic: temp file → decode and re-encode verification → checksum → publish →
  manifest. A failed save leaves the previous snapshot intact.
- **Autosave** runs every 30 seconds while edits are unsaved and on window close. A hard crash
  can lose up to half a minute of edits.
- **Checkpoints** are verified full snapshots with schema-v2 metadata (profile, label,
  reason, editor state), up to five per project. Restoring one replaces the open world and
  editor state after confirmation.
- **Recovery.** On open, FuryBuilder validates the autosave metadata, checksum, round-trip and
  revision. A verified autosave is offered explicitly. A damaged autosave is reported and the
  last saved revision is opened with `available = false`; artifacts are kept for inspection.
- **Stale temp files** are cleaned only inside FuryBuilder-owned project directories on open.
- **Diagnostics** are local JSON Lines, 2 MiB per file and four files, with paths and control
  characters redacted. Clearing removes only `furybuilder*.log`. Attach the log to a bug
  report; it is not uploaded anywhere automatically.

## 12. AI and MCP workflow

Read [MCP setup](mcp-setup.md) for installation, client configuration, access modes and the
error table. The operational rules for advanced work:

- Read-only is the default. Writes require **Edit in selection** or **Full project**, and
  every write still obeys the per-operation/batch limits.
- **Ask me to approve every apply and undo** is enabled by default. The confirmation shows
  the action, project, revision, mode, operation and bounds. Deny or a 90-second timeout
  fails closed. A narrowed policy is re-checked after approval.
- Use the `plan_complex_build` and `report_complex_build` prompts (or
  `bridge_info.bridge.workflow`) for a bounded inspect → plan → checkpoint → execute →
  verify → report loop. Prompts are guidance, not permissions.
- Prefer preview at an explicit revision, then apply. After a write, verify exact blocks and
  structured validation before taking a screenshot; a screenshot can be `partial` or
  `stale`.
- `observe` provides deterministic maps; `capture_viewport` provides the real screenshot
  fallback. MCP Apps is optional and never required: if the client does not negotiate it, the
  ordinary PNG/JSON result remains.
- The MCP activity journal is in-memory and bounded. It helps answer “what did the agent do?”
  but is not a permanent audit log.
- A restart returns access to read-only. Access is a per-user named pipe; no TCP port or
  firewall rule is involved.

## 13. Performance and scale

The renderer has explicit budgets rather than an unbounded cache:

- at most 4,096 resident chunks with deterministic farthest-first eviction;
- synchronous meshing at ≤256 snapshot chunks or ≤64 dirty chunks;
- at most 16 mesh jobs / about 12 ms per animation frame;
- at most 8,192 pending mesh positions;
- captures flush pending work before claiming completeness.

Measured guidance from the real WebView2 renderer on this test machine:

| Envelope       | Radius | Chunks | Use                                                 |
| -------------- | -----: | -----: | --------------------------------------------------- |
| Comfortable    |     16 |  1,089 | Everyday interactive editing.                       |
| Upper playable |     24 |  2,401 | Larger scenes with some frame-time cost.            |
| Tested ceiling |     32 |  4,225 | Stress evidence only; not an interactive guarantee. |

The M10.8 long-edit run applied 1,165 operations in 10 minutes on the NVIDIA D3D11 path with
zero failed operations, zero revision mismatches, zero context losses and a save/reopen match,
then repeated 5-minute sessions on Microsoft WARP and Google SwiftShader. The machine has only
one physical GPU, so integrated/discrete and cross-vendor coverage is not claimed. WARP and
SwiftShader are software rasterizers, not hardware performance representatives. Frame metrics
are renderer/JS timings; GPU timer queries were not used.

For large work: raise the load radius deliberately, prefer bounded operations and checkpoints,
and avoid treating the tested ceiling as a normal working radius.

## 14. Deliberate boundaries

- The UI's interchange export is Java structure `.nbt` for 1.21.4. Sponge `.schem`,
  `.litematic` and legacy `.schematic` are not enabled.
- Structure export/import refuses entities, block entities and unknown tags instead of
  silently dropping them.
- Import World supports classic Java 1.21.4 layouts. A bare `.mca` or `region/` directory is
  not a world. Split, mixed and legacy layouts are detected but not supported.
- Edits to imported worlds preserve untouched raw data. Heightmaps, light, ticks, block
  entities, entities and POI are not semantically recalculated after an edit; the optional
  `VanillaRebuild` writer policy invalidates some derived data for the vanilla engine to
  rebuild, as documented in the format notes.
- The complete world-folder export is verified in the backend/test path. This alpha does not
  expose it as a one-click user action; the user-facing export is a structure `.nbt`.
- Reference overlays (PNG blueprint plane and `.nbt` schematic ghost) were researched only.
  The overlay UI is intentionally not enabled; do not expect reference guides in the editor.
- MCP has no scene-graph write tools and no direct file/shell access. Semantic and parametric
  authoring stays in the desktop UI.
- Minecraft assets and Mojang JARs are never redistributed. Network downloads require an
  explicit user action and verification.

## 15. Where to read more

- [Getting started](getting-started.md)
- [MCP setup](mcp-setup.md)
- [Security boundaries](architecture.md#security-boundaries)
- [Format support](formats.md)
- [World export](formats.md#complete-world-folder-export)
- [Known limitations](reference.md#known-limitations)
