# Crash recovery and local diagnostics (M10.6)

Scope: keep unsaved work recoverable after an abnormal process exit, make recovery failures
explicit, and provide a bounded local log that is useful for support without collecting
world data, credentials or arbitrary filesystem paths.

## Crash-recovery contract

On open, FuryBuilder checks `session.lock`. When the marker exists, it inspects
`data/autosave.fwl` and `data/autosave.json`:

1. The metadata must parse and carry a SHA-256 value.
2. The autosave bytes must match that checksum.
3. The snapshot must decode and re-encode byte-for-byte.
4. The decoded revision must equal the metadata revision.
5. Only then is `RecoveryInfo.available` set to `true` and the autosave loaded into memory.

A failure at any step is reported as `available: false`; the last verified manifest snapshot
is opened instead of failing the whole `project_open`. The reason is preserved in
`RecoveryInfo.reason`, and `autosaveRevision` still identifies the rejected artifact. Nothing
on disk is overwritten until the user saves or explicitly discards.

The UI keeps a persistent banner:

- verified recovery: **Save now**, **Discard recovered changes**, **Later**;
- rejected recovery: **Dismiss**, with the saved revision and the reason visible.

`project_close(save=false)` and the discard action remove autosave artifacts before ending
the session, so discarded work cannot reappear on the next open. Opening also removes stale
`*.tmp` files inside FuryBuilder-owned directories (`data/`, `changes/`, `source/`,
`exports/`, project root), cleaning up an interrupted atomic publish without touching any
other file.

The window-close handler writes a last-chance autosave when the world differs from the saved
revision. Autosave remains interval/visibility/close driven, so a hard crash can lose up to
the last 30 seconds of edits; this is documented as a limitation rather than hidden.

## Diagnostics log

Logs live under `<app-data>/logs/`:

```
furybuilder.log
furybuilder.1.log
furybuilder.2.log
furybuilder.3.log
```

Each file is capped at 2 MiB; at most four files are retained. Rotation happens before an
append that would exceed the cap. Each line is a JSON object:

```json
{
  "tsMs": 1763550000000,
  "level": "info",
  "event": "project.open",
  "appVersion": "0.1.0",
  "data": { "folder": "My Castle.furybuilder", "revision": 42 }
}
```

Recorded events include application start/exit, panics, project create/open/save/autosave,
clean and crash exit paths, recovery availability and reasons, MCP bridge startup failures,
and sanitized UI errors (`ui.error`). World payloads, tokens, MCP sockets, API keys and full
project contents are never written.

### Sanitization

- Control characters become spaces; text is capped at 1,000 characters.
- Windows drive paths, UNC paths and common Unix home paths are replaced with `<path>`.
- Nested JSON is bounded in depth, field count and array length; event/source names are
  restricted to a safe token alphabet.
- Backend error records use stable codes rather than raw I/O messages where possible.
- Logging is best effort. A failed write never fails an editor operation.

### Commands and UI

| Command                       | Purpose                                                     |
| ----------------------------- | ----------------------------------------------------------- |
| `diagnostics_info`            | Version, directory, owned files, sizes and retention limits |
| `diagnostics_clear`           | Removes only `furybuilder*.log` files                       |
| `diagnostics_open_folder`     | Opens the fixed application log directory                   |
| `diagnostics_record_ui_error` | Appends a sanitized message and source from the UI          |

`Settings → Diagnostics` shows the location, file count/size and retention, with explicit
Refresh, Open logs folder and Clear logs actions. Clearing never touches projects,
autosaves, selected assets, Java runtimes or server JARs.

## Error messages

`lib/error-messages.ts` maps stable backend codes (`out_of_bounds`, `unsupported_version`,
`conflict`, `permission_denied`, `limit_exceeded`, `backend_unavailable`, and others) to an
actionable hint while preserving the original backend message. Project toasts use the same
mapping.

The React shell has two safety nets:

- `ErrorBoundary` catches render failures, records them as `react.error_boundary`, and offers
  reload and log-folder actions without touching project data;
- `installGlobalErrorReporting` records uncaught window errors and rejected promises, with at
  most one toast every two seconds to avoid flooding a render loop.

## Testing

| Requirement                              | Evidence                                                                                         |
| ---------------------------------------- | ------------------------------------------------------------------------------------------------ |
| Verified autosave is adopted             | `project-store` `crash_recovery_uses_newer_verified_autosave`                                    |
| Damaged autosave cannot block opening    | `damaged_autosave_opens_last_saved_revision_and_reports_unavailable`                             |
| Unreadable metadata falls back           | `unreadable_autosave_metadata_does_not_block_open`                                               |
| Temporary files are cleaned safely       | `open_removes_stale_atomic_write_temporary_files`                                                |
| Banner actions and wording               | `features/project/RecoveryBanner.test.tsx`                                                       |
| Clear diagnostic logs / open folder      | `features/settings/DiagnosticsSection.test.tsx`                                                  |
| Error-code hints preserve the message    | `lib/error-messages.test.ts`                                                                     |
| Render boundary keeps a safe fallback    | `app/ErrorBoundary.test.tsx`                                                                     |
| Rotation, redaction, owned-file clearing | `apps/desktop/src-tauri/src/diagnostics.rs` unit tests                                           |
| Real desktop UI                          | `scripts/diagnostics-webview2-smoke.mjs` + `scripts/test_diagnostics_evidence.py` (see evidence) |

## Honest limitations

- Logs are local and are not uploaded or bundled automatically.
- There is no live second-instance detection at the OS lock level; the desktop workflow
  assumes one editor instance per project.
- A panic record contains the panic message but not a full native backtrace.
- Corrupt autosave bytes are preserved for support but cannot be browsed block-by-block in
  the UI.
- React component stacks may contain source paths; the backend redacts path-like text before
  writing, but the message is still a developer-facing diagnostic.
