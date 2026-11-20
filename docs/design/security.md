# Import and MCP security boundaries

Status: implemented for the current 1.21.4 / DataVersion 4189 target. The desktop
process is authoritative; importers and MCP clients are untrusted input.

## Imported files

| Input                                 | Enforced checks                                                                                                                                                                                                                                                                                                                 | Implementation                                           |
| ------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------- |
| Java world save                       | Classic Anvil only, `DataVersion` 4189, `level.dat` bounded to 8 MiB, region file bounded to 1 GiB, at most 4,096 `.mca` files per dimension, external `.mcc`, LZ4 and custom compression refused, region offsets/sector lengths validated                                                                                      | `crates/world-io/src/anvil.rs`                           |
| World import analysis                 | At most 20,000 chunks, 2,000,000 sections and 50,000,000 non-air blocks; unsupported layouts are reported as blockers, not decoded                                                                                                                                                                                              | `crates/world-io/src/world_import.rs`                    |
| Passthrough copy                      | Source and every nested entry must be a real file/directory: symlinks, junctions and reparse points are refused; destination must be new and outside the source; 100,000 files, 8 GiB per file, 64 GiB total, 64 directory levels; copied bytes are hashed, re-read, re-opened and compared; a source change aborts publication | `crates/world-io/src/passthrough.rs`                     |
| NBT (`level.dat`, chunks, structures) | 256 MiB compressed input, 64 MiB decompressed NBT, depth 64, collection length 8,000,000, name 32,767 bytes; lengths are checked against remaining bytes before allocation                                                                                                                                                      | `crates/world-io/src/nbt.rs`                             |
| Java structure `.nbt`                 | Target `DataVersion` required, palette/state/position validation, axis limit 1,024, entities and block entities refused instead of dropped, deterministic re-encode                                                                                                                                                             | `crates/world-io/src/java_structure.rs`, `structure.rs`  |
| Structure upload over IPC             | Base64 payload capped at 32 MiB before and after decoding                                                                                                                                                                                                                                                                       | `apps/desktop/src-tauri/src/commands.rs`                 |
| Template library                      | 128 templates, 32 MiB each, 256 MiB total, 1 MiB index, SHA-256 verified on read, UUID-only object paths, 64-character names                                                                                                                                                                                                    | `crates/structure-library/src/lib.rs`                    |
| JAR / ZIP / resource pack             | Parsed in memory, never extracted to disk; traversal, absolute paths, backslashes, NUL bytes and empty segments refused; 50,000 entries, 16 MiB central directory, 1 GiB uncompressed total, 256 MiB per entry; resource packs additionally cap depth at 32 and total size at 512 MiB                                           | `crates/asset-source/src/archive.rs`, `resource_pack.rs` |
| Data pack `.zip`                      | 16 packs, 64 MiB each, 256 MiB total, 512 MiB uncompressed, 20,000 entries, 64 KiB `pack.mcmeta`; symlink entries and unsafe paths refused; format checked against 61                                                                                                                                                           | `crates/runtime-manager/src/server/datapack.rs`          |
| Live resource-pack/asset discovery    | Symlinked roots and version folders are reported and skipped; walks are bounded                                                                                                                                                                                                                                                 | `crates/asset-source/src/discovery.rs`                   |
| Source files attached to a project    | Bare file name only: no traversal, separators, control characters, trailing dots/spaces or Windows device names; immutable copy plus SHA-256 in `source.json`                                                                                                                                                                   | `crates/project-store/src/store.rs`                      |
| Structure export                      | `.nbt`/`.schem` only, the same safe-name rules, atomic write and byte-for-byte read-back                                                                                                                                                                                                                                        | `crates/project-store/src/store.rs`                      |
| World export                          | Writes only a new folder through a staging directory and atomic rename; source hashes are rechecked; the source world is never modified                                                                                                                                                                                         | `crates/world-io/src/world_export.rs`                    |

Windows device names (`CON`, `PRN`, `AUX`, `NUL`, `COM1`-`COM9`, `LPT1`-`LPT9`)
are rejected even with an extension, because Windows resolves `NUL.nbt` to the
device. User-facing structure names that would hit this rule fall back to
`structure.nbt`.

## MCP

| Control               | Enforced behaviour                                                                                                                                                                                                              | Implementation                                            |
| --------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------- |
| Default access        | `read_only`; every write returns `permission_denied`                                                                                                                                                                            | `apps/desktop/src-tauri/src/mcp_access.rs`                |
| Scopes                | `edit_in_selection` requires a dimension (1-128 characters, no control characters) and an inclusive box whose volume is within `max_blocks`; `full_project` drops the box but keeps limits                                      | same                                                      |
| Per-operation limits  | At most 1,000,000 affected blocks and 8,192 batch changes; undo is checked against the actual stored extent                                                                                                                     | same                                                      |
| Confirmation          | Writes require a desktop dialog unless the user disabled confirmation; the policy and revision are checked again after approval, so shrinking the selection or switching to read-only while the dialog is open aborts the write | `mcp_access.rs`, `local_bridge.rs`                        |
| Pending confirmations | At most 8; deny, timeout or a missing UI is fail-closed                                                                                                                                                                         | `mcp_access.rs`                                           |
| Transport             | Local socket with a random per-launch 64-hex token and a user-private discovery file; no filesystem, shell or network tools are exposed                                                                                         | `apps/desktop/src-tauri/src/local_bridge.rs`              |
| Request bounds        | 4 MiB per request, request id at most 128 characters, at most 8 pending renderer requests, captures capped at 12 MiB and bridge responses at 16 MiB                                                                             | `local_bridge.rs`, `crates/mcp-bridge/src/desktop.rs`       |
| Job queue             | Only `apply_operation` and `undo_operation`; 16 queued jobs, 256 retained, idempotency key 1-128 characters; policy is applied before queueing                                                                                  | `crates/world-application/src/jobs.rs`, `local_bridge.rs` |
| Activity journal      | 1,000 entries, query limit 100, client/method/error text stripped of control characters and length-capped, no token logging                                                                                                     | `apps/desktop/src-tauri/src/mcp_activity.rs`              |

## Residual risks

- A local user who can already modify project files is outside the app's threat
  boundary. The app validates on read and refuses unsafe paths, but it cannot
  protect a project from that same OS account.
- The MCP bridge's stdin is owned by the MCP client. The desktop endpoint caps
  what reaches the Core, but a hostile client can still consume its own process
  memory before the request is sent.
- Downloads (Mojang server JAR, Adoptium runtime) are opt-in and verified by
  checksum; FuryBuilder never bundles Mojang binaries or assets.
- Reference images are decoded by the browser and are editor-only; they do not
  grant filesystem access.

## Reproduce

```powershell
cargo test -p furybuilder-world-io --lib
cargo test -p furybuilder-project-store --test exports
cargo test -p furybuilder-desktop mcp_access
```

The passthrough integration suite also exercises symlinked files/directories,
destination refusal and file-count limits; the anvil suite covers symlinked
worlds, custom dimensions and malformed region records.
