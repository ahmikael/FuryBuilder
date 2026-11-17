# Test matrix

Status: initial M0 matrix. Rows become green only with an automated or recorded manual run.

## Scenarios

| ID | Scenario | Expected | Automated layer |
|---|---|---|---|
| T-01 | Empty world, no chunks | 0 chunks, 0 blocks, revision 0 | Rust unit |
| T-02 | Set/get one block | same state incl. properties | Rust unit + contract |
| T-03 | Small house 9x5x7 | exact block count, palette matches | Rust unit |
| T-04 | Chunk boundary X=15/16 | no gaps, both chunks dirty | Rust unit + renderer |
| T-05 | Negative coordinates (-1,-17,-33) | floor division to correct chunk/section | Rust unit |
| T-06 | Y outside dimension range | structured `OutOfBounds` error | Rust unit |
| T-07 | Chest with NBT | block entity preserved through export/reimport | Rust + golden |
| T-08 | Biome palette | biome data round-trips | Rust + golden |
| T-09 | Two dimensions | identical coordinates do not collide | Rust unit |
| T-10 | Large region (>= 1 region file) | bounded memory, explicit progress | Rust benchmark/integration |
| T-11 | Unknown NBT fields | passthrough preserved or export refused | Rust integration |
| T-12 | Undo after 10k-block batch | world equals pre-batch snapshot | Rust unit |
| T-13 | Idempotency key replay | second call is a no-op | Rust unit |
| T-14 | Stale `expected_revision` | conflict error, no write | Rust unit |
| T-15 | Read-only MCP write | rejected by Core policy | Rust integration |
| T-16 | Selection boundary edit | rejected when outside allowed scope | Rust integration |
| T-17 | Crash during save | previous checkpoint recoverable, no partial publish | Rust integration |
| T-18 | `.nbt` structure round-trip | semantic equality of states/positions | Rust golden |
| T-19 | Malformed NBT / zip bomb | bounded error, no panic | Rust fuzz/unit |
| T-20 | Mesh at chunk seam | no holes/stale faces | Renderer test |

## Format fixtures

Minimum fixtures (self-authored or legally redistributable):

1. Empty structure (0 blocks).
2. 1-block structure with a directional block (`oak_stairs[facing=east]`).
3. Mixed palette with air gaps.
4. Negative-position structure with offset.
5. Structure crossing chunk boundaries.
6. Structure with a chest block entity.
7. Two-dimension sample world (for import tests).
8. Corrupted/truncated NBT sample.
9. Region file with negative region coordinates.
10. Unknown-tag passthrough sample.

Fixtures stay under `fixtures/` and must not contain Mojang binaries or textures.

## Manual / external checks

- Open exported `.nbt` in Minecraft 1.21.4 structure block or a compatible editor.
- Open exported world copy in Minecraft 1.21.4 client/server.
- Verify MCP with at least one real external client (OpenCode).
- Long-session memory/FPS run recorded in `docs/testing/perf-runs.md` once available.

## Implemented coverage (M3)

| Matrix row | Automated evidence |
|---|---|
| T-04 chunk boundary | `renderer/world.test.ts`, `renderer/mesher.test.ts` (border culling and neighbour unload rebuild) |
| T-05 negative coordinates | `renderer/world.test.ts`, `renderer/raycast.test.ts`, Rust `coords::tests` |
| T-20 mesh at chunk seam | `renderer/mesher.test.ts` — faces heal after a neighbour chunk is unloaded |
| Transparency | `renderer/mesher.test.ts` — glass/water stay in the translucent buffer; same-material faces are culled |
| Observation partial | `renderer/observation.test.ts` — missing requested chunks set `partial` and `missingChunks` |

## Implemented coverage (M4)

| Requirement | Automated evidence |
|---|---|
| Palette search and canonical state defaults | `features/workspace/BottomPanel.test.tsx` |
| Place/remove/brush/line/fill/replace planning | `editor/session.test.ts` |
| Box selection with negative coordinates | `editor/commands.test.ts` (`tool click state machine`) |
| Selection volume limit (1,000,000 blocks) | `editor/commands.test.ts` |
| Conflicting actions (no selection/source, source = target, revision mismatch) | `editor/commands.test.ts` |
| Undo/redo buttons and dirty chunk refresh | `editor/commands.test.ts`, `features/viewport/Viewport.test.tsx` |
| Cancellable preview/read jobs and stale job completion | `editor/commands.test.ts`, `state/tool-store.test.ts` |
| Hotkey protection in inputs and contenteditable | `editor/use-editor-hotkeys.test.tsx`, `editor/session.test.ts` |
| Click → preview → apply → chunk refresh IPC path | `features/viewport/Viewport.test.tsx` |
