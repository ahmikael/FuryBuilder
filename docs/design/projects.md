# Project storage (M5)

A FuryBuilder project is a self-contained folder ending in `.furybuilder`. It stores the
editable world layer, a versioned manifest and crash-recovery artifacts. Imported source
data lives in a separate immutable layer so an overlay project can never rewrite it.

```text
My Castle.furybuilder/
  project.json                 # ProjectManifest, project_schema_version = 1
  data/
    world-<revision>.fwl      # deterministic binary world snapshot
    autosave.fwl              # crash-recovery snapshot (only after edits)
    autosave.json             # revision + SHA-256 + timestamp of the autosave
  changes/
    checkpoint-<revision>.fwl
    checkpoint-<revision>.json
  source/
    source.json               # immutable file manifest with SHA-256 hashes
    files/                    # verbatim copies of imported source files
  cache/                      # recreatable only; cleared on graceful close
  session.lock                # present while a session is open
```

Project folders live in the Tauri app-data directory (`.../FuryBuilder/projects`), not in
the repository or next to user documents.

## Manifest

```ts
interface ProjectManifest {
  projectSchemaVersion: 1;
  projectId: string;
  name: string;
  creationMode: "empty" | "flat" | "vanilla" | "import";
  targetMinecraftVersion: "1.21.4";
  targetDataVersion: 4189;
  worldRevision: number;
  worldFile: string;            // "data/world-00000000000000000004.fwl"
  worldChecksum: string;        // lowercase SHA-256 of the snapshot bytes
  worldBytes: number;
  createdAtMs: number;
  updatedAtMs: number;
  lastSavedAtMs: number;
  lastSaveKind: "manual" | "autosave" | "checkpoint";
  editorState: { selectedDimension: string; loadRadius: 1..4 };
  source: { kind: "generated" | "imported"; immutable: boolean; reference?: string };
  createdBy: string;
  appVersion: string;
}
```

Validation rejects an empty or oversized name, an invalid project id, a world file that is
not exactly below `data/`, a checksum that is not a lowercase SHA-256, and an editor state
outside the supported radius. Unknown/empty creation modes are refused at create time;
`vanilla` and `import` are declared in the schema and shown as future modes in the UI, but
`project_create` rejects them until their milestones land.

## Snapshot format

`world-<revision>.fwl` is a little-endian binary format owned by `project_store`:

```text
magic "FURYSNAP" | u16 format version | u64 world revision
u32 dimension count
  string dimension id | i32 minY | i32 height
  u32 chunk count
    i32 chunkX | i32 chunkZ
    u32 section count
      i32 sectionY | u8 bitsPerIndex | u32 palette length
        string canonical block state per palette entry
      u32 packed word count | u64 words
```

- `string` is a `u16` byte length followed by UTF-8. Lengths and counts are bounded before
  allocation; trailing bytes after the declared payload are corruption.
- Block states are stored in canonical `Display` form (`minecraft:oak_stairs[facing=north,...]`)
  and parsed back strictly. Property order is canonicalized by `BTreeMap`.
- Sections are rehydrated through `Section::from_raw`, which recomputes the non-air count
  instead of trusting the file and rejects palette/bit-width/word-count mismatches.
- Dimension bounds are checked against the target version, so a snapshot cannot smuggle
  sections outside `-64..=319` (Overworld).
- Decoding is deterministic: for the same world, encoding the decoded result is
  byte-identical to the input. Save verification depends on this property.

The snapshot is the world layer only. It deliberately does not persist undo history,
idempotency keys or the audit log: those are session state, bounded in memory, and rebuilt
empty when a project opens. This keeps files small and prevents stale undo from applying
to a different session.

## Atomic save

```text
encode world → write data/.tmp → fsync
             → decode + re-encode byte comparison
             → SHA-256
             → rename data/world-<rev>.fwl (atomic publish)
             → copy project.json to project.json.bak-<prevRev>
             → write project.json.tmp → rename project.json
             → prune snapshots beyond 8
```

- The manifest is never rewritten before the snapshot it references is durable. A crash
  between the two renames leaves an unreferenced snapshot, never a manifest pointing at
  partial bytes.
- A manual save clears stale autosave artifacts. `project_close(save=false)` also removes
  them, because an explicit discard must not be resurrected by recovery.
- Old snapshots are pruned to the newest eight revisions, so a save-heavy session cannot
  grow without bound. `data/` is the only directory pruning touches.

## Autosave, checkpoints and recovery

- The UI autosaves every 30 seconds while the world revision differs from the saved
  revision, and when the document becomes hidden. The Tauri window close handler writes a
  last-chance autosave if edits are unsaved; a clean session only removes `session.lock`.
- Autosave never changes `worldRevision` in the manifest. It writes `data/autosave.fwl`
  plus checksum metadata and updates editor state.
- On the next open, a `session.lock` plus a newer, checksum-verified autosave is adopted
  automatically and reported as `RecoveryInfo`. The UI shows a banner with **Keep recovered
  data** (saves later) and **Discard and reload saved** (clears autosave + marker and opens
  the manifest snapshot).
- An autosave that fails its checksum is never silently loaded; the safe saved revision is
  opened instead with a warning.
- Checkpoints are explicit full snapshots with a reason, kept to five per project under
  `changes/`. Restoring one replaces the open world but not the manifest: the restored
  revision is unsaved until the user saves.

## Migrations and unsupported versions

- A manifest with a lower `projectSchemaVersion` is migrated in place after writing a
  verbatim `project.json.v<old>.bak`; a failed migration never destroys the only copy.
- A newer schema, or a newer snapshot format, is refused with `unsupported_version`. The
  app does not guess at unknown fields or downgrade data.
- Target-version mismatches are refused rather than converted; a future installer/adapter
  will own that conversion.

## Source overlay

`source/source.json` lists files copied into `source/files/` with byte counts and SHA-256
hashes. `attach_source` only accepts a single safe path component (no `..`, separators or
absolute paths), copies bytes atomically, and marks the layer immutable. The editable
overlay remains the project snapshot; imported source is never mutated. The UI only shows
a warning that immutable source files are referenced until the import/export milestones
add source browsing.

## Limits and filesystem safety

- Project folder names are validated single path components ending in `.furybuilder`.
  Display names reject path separators and control characters; renaming changes the
  display name only so open folders never move under the user.
- `project.json`, snapshots, autosave metadata and checkpoints are bounded before
  allocation (dimensions, chunks, sections, palette, words, string lengths, total bytes).
- Save never writes outside the project folder; world files must match
  `data/world-<20-digit revision>.fwl`.
- Snapshots are verified semantically (revision, chunk count, block count) in addition to
  the checksum before being referenced.
- The cache directory is the only thing cleared automatically; `data/`, `changes/` and
  `source/` survive a graceful close.

## Testing

| Requirement | Evidence |
|---|---|
| Create/list/open round trip incl. negative Y and flat base | `project-store` `create_list_open_round_trips_world_and_manifest` |
| Corrupt bytes rejected by checksum | `corrupt_snapshot_is_rejected_by_checksum` |
| Atomic save keeps the previous revision and verifies | `save_is_atomic_and_keeps_previous_snapshot` |
| Crash recovery adopts only a newer verified autosave | `crash_recovery_uses_newer_verified_autosave` |
| Manual save / explicit discard clears autosave | `manual_save_clears_stale_autosave` |
| Checkpoints bounded and readable | `checkpoints_are_bounded_and_readable` |
| Newer schema refused, draft migrated with backup | `unsupported_newer_schema_is_refused` |
| Rename safety | `rename_updates_display_name_only_and_rejects_paths` |
| Immutable source copies and path safety | `source_files_are_immutable_copies_with_hashes` |
| Duplicate names get unique folders | `duplicate_names_get_unique_folders` |
| Cache cleared without touching data | `end_session_clears_only_recreatable_cache` |
| Snapshot determinism across dimensions | `snapshot_round_trip_preserves_states_across_dimensions` |
| UI list/create/rename/open, unsaved warning, recovery, checkpoints | `features/project/ProjectManager.test.tsx` |
| Client session state, autosave and close | `state/project-store.test.ts` |
| IPC argument contract | `lib/project-api.test.ts` |

## Limitations (M5)

- No project deletion or folder relocation yet; renaming is display-only.
- `Vanilla World` and `Import World` creation modes are schema placeholders until M7/M8.
- Autosave is interval/visibility/close driven, not every edit; a crash can lose up to the
  last 30 seconds.
- Checkpoints are manual and bounded to five; there is no checkpoint browser for projects
  that are not open.
- Source attachment is backend-only; import UI and validation of source formats arrive in
  M6–M8.
