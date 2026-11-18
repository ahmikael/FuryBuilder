# Test matrix

Status: initial M0 matrix. Rows become green only with an automated or recorded manual run.

## Scenarios

| ID   | Scenario                          | Expected                                            | Automated layer            |
| ---- | --------------------------------- | --------------------------------------------------- | -------------------------- |
| T-01 | Empty world, no chunks            | 0 chunks, 0 blocks, revision 0                      | Rust unit                  |
| T-02 | Set/get one block                 | same state incl. properties                         | Rust unit + contract       |
| T-03 | Small house 9x5x7                 | exact block count, palette matches                  | Rust unit                  |
| T-04 | Chunk boundary X=15/16            | no gaps, both chunks dirty                          | Rust unit + renderer       |
| T-05 | Negative coordinates (-1,-17,-33) | floor division to correct chunk/section             | Rust unit                  |
| T-06 | Y outside dimension range         | structured `OutOfBounds` error                      | Rust unit                  |
| T-07 | Chest with NBT                    | block entity preserved through export/reimport      | Rust + golden              |
| T-08 | Biome palette                     | biome data round-trips                              | Rust + golden              |
| T-09 | Two dimensions                    | identical coordinates do not collide                | Rust unit                  |
| T-10 | Large region (>= 1 region file)   | bounded memory, explicit progress                   | Rust benchmark/integration |
| T-11 | Unknown NBT fields                | passthrough preserved or export refused             | Rust integration           |
| T-12 | Undo after 10k-block batch        | world equals pre-batch snapshot                     | Rust unit                  |
| T-13 | Idempotency key replay            | second call is a no-op                              | Rust unit                  |
| T-14 | Stale `expected_revision`         | conflict error, no write                            | Rust unit                  |
| T-15 | Read-only MCP write               | rejected by Core policy                             | Rust integration           |
| T-16 | Selection boundary edit           | rejected when outside allowed scope                 | Rust integration           |
| T-17 | Crash during save                 | previous checkpoint recoverable, no partial publish | Rust integration           |
| T-18 | `.nbt` structure round-trip       | semantic equality of states/positions               | Rust golden                |
| T-19 | Malformed NBT / zip bomb          | bounded error, no panic                             | Rust fuzz/unit             |
| T-20 | Mesh at chunk seam                | no holes/stale faces                                | Renderer test              |

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

| Matrix row                | Automated evidence                                                                                     |
| ------------------------- | ------------------------------------------------------------------------------------------------------ |
| T-04 chunk boundary       | `renderer/world.test.ts`, `renderer/mesher.test.ts` (border culling and neighbour unload rebuild)      |
| T-05 negative coordinates | `renderer/world.test.ts`, `renderer/raycast.test.ts`, Rust `coords::tests`                             |
| T-20 mesh at chunk seam   | `renderer/mesher.test.ts` — faces heal after a neighbour chunk is unloaded                             |
| Transparency              | `renderer/mesher.test.ts` — glass/water stay in the translucent buffer; same-material faces are culled |
| Observation partial       | `renderer/observation.test.ts` — missing requested chunks set `partial` and `missingChunks`            |

## Implemented coverage (M4)

| Requirement                                                                   | Automated evidence                                               |
| ----------------------------------------------------------------------------- | ---------------------------------------------------------------- |
| Palette search and canonical state defaults                                   | `features/workspace/BottomPanel.test.tsx`                        |
| Place/remove/brush/line/fill/replace planning                                 | `editor/session.test.ts`                                         |
| Box selection with negative coordinates                                       | `editor/commands.test.ts` (`tool click state machine`)           |
| Selection volume limit (1,000,000 blocks)                                     | `editor/commands.test.ts`                                        |
| Conflicting actions (no selection/source, source = target, revision mismatch) | `editor/commands.test.ts`                                        |
| Undo/redo buttons and dirty chunk refresh                                     | `editor/commands.test.ts`, `features/viewport/Viewport.test.tsx` |
| Cancellable preview/read jobs and stale job completion                        | `editor/commands.test.ts`, `state/tool-store.test.ts`            |
| Hotkey protection in inputs and contenteditable                               | `editor/use-editor-hotkeys.test.tsx`, `editor/session.test.ts`   |
| Click → preview → apply → chunk refresh IPC path                              | `features/viewport/Viewport.test.tsx`                            |

## Implemented coverage (M5)

| Requirement                                                          | Automated evidence                                                                                                                                    |
| -------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- |
| Versioned manifest and validated fields                              | `project-store` `create_list_open_round_trips_world_and_manifest`, `unsupported_newer_schema_is_refused`                                              |
| Snapshot save/load incl. chunks, sections, metadata and editor state | `create_list_open_round_trips_world_and_manifest`, `snapshot_round_trip_preserves_states_across_dimensions`                                           |
| Immutable source layer with hashes and path safety                   | `source_files_are_immutable_copies_with_hashes`                                                                                                       |
| Atomic temp + verification + publish                                 | `save_is_atomic_and_keeps_previous_snapshot`, `corrupt_snapshot_is_rejected_by_checksum`                                                              |
| Autosave, checkpoints and crash recovery                             | `crash_recovery_uses_newer_verified_autosave`, `checkpoints_are_bounded_and_readable`, `state/project-store.test.ts`                                  |
| Undo limits and recreatable-cache cleanup                            | `world-application` history-limit test, `end_session_clears_only_recreatable_cache`                                                                   |
| Project list/create/rename/open and unsaved warning                  | `features/project/ProjectManager.test.tsx`                                                                                                            |
| Schema migrations with backup and refusal of newer versions          | `unsupported_newer_schema_is_refused`                                                                                                                 |
| Crash recovery, reopen, checksum and semantic equality               | `crash_recovery_uses_newer_verified_autosave`, `save_is_atomic_and_keeps_previous_snapshot`, `snapshot_round_trip_preserves_states_across_dimensions` |
| IPC project contract                                                 | `lib/project-api.test.ts`                                                                                                                             |

## Implemented coverage (M6, Java structure `.nbt`)

Target version: Minecraft Java `1.21.4`, `DataVersion` `4189`. The format is an
interchange format only; FuryBuilder project metadata is not claimed to be
included.

| Requirement                                      | Automated / recorded evidence                                                                                                                                |
| ------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Versioned adapter for one format                 | `world-io` `java_structure.rs`, `nbt.rs`, `structure.rs`, `exchange.rs`; `cargo test -p furybuilder-world-io`                                                |
| Real game-saved fixture parses                   | `crates/world-io/tests/game_fixtures.rs` uses `fixtures/java-1.21.4/structures/plain.nbt` saved by the official 1.21.4 server                                |
| Block state properties and orientation           | `official_plain_structure_round_trips_semantically`; Tauri IPC check for oak stairs facing east                                                              |
| Unsupported block entity is refused              | `official_structure_with_block_entity_is_refused_instead_of_dropped`; `chest_boundary.nbt` preview and insert rejection                                      |
| Import preview and new project/insertion         | `features/project/StructureExchange.test.tsx`, `state/project-store.test.ts`, real Tauri IPC probe recorded in `docs/testing/minecraft-structure-check.md`   |
| Export preview with bounds/version/warnings/path | `features/project/StructureExchange.test.tsx`, `structure-api.test.ts`, Tauri IPC probe                                                                      |
| Binary and semantic verification after writing   | `exchange::tests::prepare_export_is_verified_and_deterministic`, `export_verification_is_independent_of_world_iteration_order`; `project-store` `exports.rs` |
| Corrupt/incompatible input is not silent success | `nbt::tests`, `java_structure::tests`, `game_fixtures.rs`                                                                                                    |
| Official server can load adapter output          | `docs/testing/minecraft-structure-check.md`, evidence `docs/testing/evidence/2026-11-18-java-structure-interop.json`                                         |
| Build → export → load demonstration              | `world-io` example `export_demo.rs`; six exact block checks on official 1.21.4 server                                                                        |

T-07 in the original matrix (preserve a chest block entity) is **not** green
for this adapter: the current Java structure model refuses block entities
rather than dropping them. Supporting passthrough is future work outside M6.
Biomes, entities, ticks and full worlds remain M8 work. `.schem` remains the
second format and is not enabled in the UI.

## Implemented coverage (M7, first MCP slice)

| Requirement                                                         | Automated / recorded evidence                                                                                                                                                                                                                                                                |
| ------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Pinned MCP SDK and transport                                        | `docs/research/mcp-2026-07-28.md`; `@modelcontextprotocol/server` 2.4.0 in `mcp/bridge/package.json`                                                                                                                                                                                         |
| Modern and legacy protocol negotiation                              | `mcp/bridge/test/protocol.test.ts` spawns the real stdio bridge for both eras; OpenCode CLI 2.3.2 agent passed `protocol: auto` (modern 2026-07-28) and `protocol: legacy` against a live app                                                                                                |
| Real external MCP client and agent                                  | `docs/testing/evidence/2026-11-18-opencode-mcp-client.json`; OpenCode 2.3.2 `--agent plan` called `bridge_info`/`world_info` over MCP only, received ready read-only context and made no modifications                                                                                       |
| Tasks as an explicit option with fallback tools                     | `mcp/bridge/test/tasks-policy.test.ts`; job tools reach the authoritative queue when the desktop is live and return `job_api_unavailable` when it is absent                                                                                                                                  |
| Long-running job queue, progress, cancel and idempotent retry       | `world-application/src/jobs.rs` tests (queue limit, phases, rollback on cancel, replay, conflict, project scope, policy-source cancel); `world-operations` observer test; Rust endpoint state/confirmation tests; `desktop-jobs.ts` + live-endpoint job mapping in `desktop-bridge.test.ts`  |
| Authenticated local endpoint over the authoritative `WorldService`  | `apps/desktop/src-tauri/src/local_bridge.rs` tests: token rejection, protocol mismatch, `world_info`, bounded `query`, project mismatch, real local-socket round-trip                                                                                                                        |
| Discovery file and stale-endpoint handling                          | `local_bridge.rs` discovery test; `mcp/bridge/test/desktop-client.test.ts`                                                                                                                                                                                                                   |
| Real bridge process reaching a live local endpoint                  | `mcp/bridge/test/desktop-bridge.test.ts` (`bridge_info.desktop.connected`)                                                                                                                                                                                                                   |
| Explicit `project_id` / `world_revision` / `job_id` contract        | `mcp/bridge/test/context.test.ts`; `local_bridge.rs` stale-revision test; `job-tools.ts` schema exposes `job_id`, `world_revision`, `idempotency_key`                                                                                                                                        |
| Explicit connection lifecycle                                       | `bridge_info.connection` (`instanceId`, `startedAtMs`, `ephemeral-stdio`, `expiresWithProcess`, state); asserted in `protocol.test.ts` and `desktop-bridge.test.ts`                                                                                                                          |
| Read-only tools: world info, block, region summary/blocks           | `mcp/bridge/src/read-tools.ts`; `protocol.test.ts`; live-endpoint reads in `desktop-bridge.test.ts`                                                                                                                                                                                          |
| Camera and real viewport screenshot                                 | `apps/desktop/src/lib/renderer-bridge.test.ts`; Rust camera/capture/revision tests; real Tauri probe recorded in `docs/testing/mcp-compatibility.md`                                                                                                                                         |
| Preview, apply, batch and undo tools                                | `mcp/bridge/src/write-tools.ts`; `protocol.test.ts`; live-endpoint mapping in `desktop-bridge.test.ts`                                                                                                                                                                                       |
| Access modes, limits and confirmations                              | `apps/desktop/src-tauri/src/mcp_access.rs` tests (read-only, selection box, block/batch limits, undo extent, one-answer confirmation); Rust endpoint approval/denial tests                                                                                                                   |
| Access policy UI and confirmation host                              | `apps/desktop/src/lib/mcp-access.test.ts`; `McpAccessDialog` and `McpConfirmationHost`; policy is Tauri-only and the bridge cannot widen it                                                                                                                                                  |
| Tool calls bound to operation history and action journal            | `apps/desktop/src-tauri/src/mcp_activity.rs` (bounded journal, client/method/status/revision/operation link); Rust endpoint test; `mcp_activity` endpoint and History panel; live probe shows a denied call with `clientId` and `permission_denied`                                          |
| Read-only by default and explicit write grant                       | Rust `local_bridge` tests: preview access status, apply/undo `permission_denied`, granted batch apply + audit + undo; real Tauri probe                                                                                                                                                       |
| Real Tauri process                                                  | `scripts/probe-local-bridge.mjs`; recorded in `docs/testing/mcp-compatibility.md` — reads, PNG capture, read-only preview/deny, activity journal, status, wrong token `unauthorized`                                                                                                         |
| End-to-end AI edit loop (explore → build → screenshot → fix → undo) | `scripts/e2e-mcp-agent.mjs`; `docs/testing/evidence/2026-11-18-ai-edit-loop.json` — 17 MCP calls, four real dialog approvals, revisions 0→1→2→3→4, two fresh PNG captures, final world 0 blocks                                                                                              |
| MCP write propagation into the live viewport                        | Rust `WORLD_CHANGED_EVENT` after sync apply/undo and completed jobs; `McpWorldChangeHost.test.tsx`; E2E captures at render revision = authoritative revision, `stale=false`, legend 13 stone → 12 stone + 1 glass                                                                            |
| Structured observations with an explicit revision                   | Rust `local_bridge` test `query_observations_report_the_revision_they_read` (block, region summary, stale conflict); `desktop-bridge.test.ts` asserts `get_block` and `inspect_region`; live MCP probe `mcp/bridge/scripts/probe-live-observation.mjs`                                       |
| Truthful screenshot completeness and stale detection                | `finalize_renderer_response` + Rust camera/capture tests; bounded legend in `apps/desktop/src/renderer/observation.ts`; both probes check `partial`/`missingChunks`/`stale` consistency; `docs/testing/evidence/2026-11-18-mcp-observation.json` and `2026-11-18-structured-observation.txt` |

Remaining M7 work: user-facing MCP setup documentation (M7.14). The full AI
build/screenshot/fix/undo loop is verified through a real external client and the real
confirmation dialog; the model used was DeepSeek free because GPT-6 Luna Max was refused by
its provider for this region. Structured observations now carry the exact revision and do
not claim a complete frame when chunks are missing.
