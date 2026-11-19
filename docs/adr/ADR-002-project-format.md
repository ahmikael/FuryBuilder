# ADR-002: Native project format

- **Status:** Accepted
- **Date:** 2026-11-17

## Decision

A FuryBuilder project is a **folder** ending in `.furybuilder/`:

```text
MyCastle.furybuilder/
├── project.json          # schema version, name, mode, target version, revisions
├── source/               # immutable imported source (read-only)
├── data/                 # chunk payloads (sparse, content-addressed segments)
├── changes/              # transaction journal + checkpoints
├── assets/               # user-approved references/resource packs
├── previews/             # thumbnails and agreed screenshots
└── cache/                # regenerable meshes/atlases (safe to delete)
```

- `project.json` is the only eagerly parsed document. It contains
  `project_schema_version`, `project_id`, `name`, `creation_mode`, `target_minecraft_version`,
  `target_data_version`, `world_revision`, `created_at`, `updated_at` and provenance.
- World chunks are stored as individual versioned payload files under `data/`, written
  atomically (temp file + fsync + rename). The first implementation stores a compact
  custom binary encoding per section with a schema version.
- `cache/` may be deleted at any time. `data/` and `changes/` may not.
- Saves are atomic at the project level: write to a staging area, verify, publish,
  keep a backup of the previous manifest.
- Schema migrations are explicit functions with a pre-migration backup. Unknown newer
  schema versions are refused (read-only inspection only).

## Rationale

Folder storage is inspectable, recoverable and testable. It avoids rewriting a monolithic
file on every autosave. Separating `source/` supports the copy-on-write import model.

## Consequences

- Need atomic directory publish and crash-recovery tests.
- A project can be copied and diffed with ordinary file tools.
- `project_id` is stable and independent of the folder name.

## Implementation note (M5)

The MVP stores one deterministic whole-world snapshot per committed revision
(`data/world-<revision>.fwl`, newest eight kept) plus `autosave.fwl` and rolling
checkpoints. The manifest carries the SHA-256 of the active snapshot. This preserves the
decision's atomicity, versioning, backup and recovery guarantees without inventing a
segment format before profiling shows a need. Per-chunk content-addressed segments remain
the planned direction for large worlds; `data/` is already the only payload directory, so
that change is internal to `project_store`.

`assets/` and `previews/` are reserved for the asset/capture milestones; the M5 folder
contains `data/`, `changes/`, `source/` and `cache/`. Immutable imported source bytes live
under `source/files/` with hashes in `source/source.json`; the editable overlay is the
versioned snapshot.

Schema migrations copy the original `project.json` byte for byte before changing it. Routine
per-save manifest backups are bounded to five; migration backups are retained. The release
policy and manual backup procedure are in `docs/release/versioning.md` and
`docs/release/backup-policy.md`.
