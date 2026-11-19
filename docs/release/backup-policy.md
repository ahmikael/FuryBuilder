# Migration and backup policy

Status: beta policy. It separates **project data** (must not be lost) from **settings and
caches** (replaceable, with visible warnings). All paths below are Windows paths; the code
uses Tauri's per-user application data directory.

## Where data lives

```text
%APPDATA%\com.furybuilder.desktop\
  projects\<name>.furybuilder\   # projects; the only irreplaceable user data
  assets.json                    # selected Minecraft asset source and pack order
  runtime.json                   # selected Java executable
  server.json                    # selected server JAR and EULA acceptance
  runtimes\java\                 # managed Java runtimes (re-downloadable)
  servers\                       # official server JAR cache (re-downloadable)
  logs\                          # bounded diagnostic log
  mcp\                           # local bridge discovery metadata
```

Inside a project:

```text
project.json                  # schema version, active snapshot and provenance
data\world-<revision>.fwl     # world snapshots (newest eight kept)
data\autosave.fwl/.json       # crash-recovery candidate
changes\checkpoint-*          # five explicit checkpoints
source\                       # immutable imported source copies
cache\                        # regenerable; safe to delete
session.lock                  # present while a session is open
```

## Automatic protections

| Protection             | Behavior                                                                                                                                                           | Bound                 |
| ---------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ | --------------------- |
| Atomic save            | Snapshot is written to a temporary file, fsynced, decoded and re-encoded, then renamed. `project.json` is written only after the snapshot it references is durable | One publish at a time |
| Snapshot history       | Each successful save publishes `data/world-<revision>.fwl`; older revisions remain readable                                                                        | Newest 8              |
| Per-save manifest copy | The previous `project.json` is copied to `project.json.bak-<previous revision>` before the new manifest is published                                               | Newest 5              |
| Explicit checkpoints   | User-requested recovery points in `changes/`                                                                                                                       | Newest 5              |
| Autosave               | Saved every 30 seconds while dirty, on window hide and on close; contains revision, byte count and SHA-256                                                         | One current autosave  |
| Autosave verification  | Checksum, round-trip decode, revision and byte count must agree before adoption                                                                                    | Fails closed          |
| Schema migration       | The original manifest is copied **byte for byte** to `project.json.v<old>.bak` before migration; a failed copy aborts the migration                                | One per migration     |
| Immutable source       | Imported world files are copied under `source/files/` with SHA-256 and are never edited by overlay operations                                                      | Source tree           |
| Diagnostics            | Rotating JSON Lines log, no world payloads or secrets                                                                                                              | 2 MiB × 4 files       |

If creating the migration backup fails, the project is **not** migrated. If a new schema is
newer than the application supports, the project is refused with `unsupported_version`; no
downgrade or guess is attempted.

## Settings and caches

`assets.json`, `runtime.json` and `server.json` carry their own schema numbers and migrate
older drafts in memory. They contain only paths and explicit choices. If a settings file is
unreadable or has an unsupported newer schema, the store starts from defaults and reports a
load warning; it never deletes the file silently and never touches project data.

Managed runtimes, the server JAR cache and `cache/` are re-downloadable or regenerable. The UI
offers explicit cache clearing. Clearing a cache must not remove projects, source files,
checkpoints or the selected settings.

## User backup procedure

Close FuryBuilder before copying. The simplest complete backup is the whole application data
directory:

```powershell
$source = Join-Path $env:APPDATA "com.furybuilder.desktop"
$target = Join-Path $env:USERPROFILE "Documents\FuryBuilder-backup-$(Get-Date -Format yyyyMMdd-HHmmss)"
Copy-Item -LiteralPath $source -Destination $target -Recurse
```

For a smaller backup, copy only `projects\`. A project folder is self-contained; copying it
with normal file tools is supported.

Restore while the application is closed:

1. Rename the current `%APPDATA%\com.furybuilder.desktop` directory instead of deleting it.
2. Copy the backup into place.
3. Start FuryBuilder, open each project and confirm the revision and expected content before
   deleting the renamed directory.

`source/` is a copy of the imported world, not a live link. Editing the original world outside
FuryBuilder does not change the project, and FuryBuilder does not write to the original.

## Update and release policy

- Beta users should take a manual backup before installing a new version. Installer updates
  preserve `%APPDATA%\com.furybuilder.desktop`, but a backup is the only protection against
  unrelated disk or profile problems.
- Before changing `PROJECT_SCHEMA_VERSION` or `SNAPSHOT_FORMAT_VERSION`, add a migration from a
  fixture of the previous version, test the byte-identical pre-migration backup, and verify
  that a failed migration leaves the original openable.
- Migration backups (`project.json.v<old>.bak`) are never pruned automatically. They are small
  and matter for post-incident diagnosis.
- Routine per-save manifest copies and snapshots are pruned only by the bounds above. No
  background job deletes `data/`, `changes/` or `source/`.
- The release checklist in [versioning.md](versioning.md) requires installer update testing and
  backup-policy review whenever a schema changes.

## What is not covered

- Disk failure, ransomware, profile corruption or deleting the application data directory.
- Synchronising projects between machines or users. Copying is manual.
- Recovering an original Java world after a third-party tool has modified it. FuryBuilder keeps
  only the immutable copy made at import time.
- Automatic cloud backup. None is configured and no project data leaves the machine.

## Verification

```powershell
cargo test -p furybuilder-project-store
python scripts/test_release_process.py -v
```

The project-store tests cover atomic save, bounded snapshots/checkpoints/manifest backups,
byte-identical migration backups, newer-schema refusal and autosave verification.
