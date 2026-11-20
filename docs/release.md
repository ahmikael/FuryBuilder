# Release and data policy

Status: Windows beta 0.1.0. The application version is independent from project schema,
snapshot format, desktop protocol and MCP protocol versions.

## Versioning

`version.json` is the single source of truth. `pnpm version:check` requires every manifest to
agree:

| File                                     | Field                         |
| ---------------------------------------- | ----------------------------- |
| `version.json`                           | `version`                     |
| `Cargo.toml`                             | `[workspace.package].version` |
| `package.json`                           | `version`                     |
| `apps/desktop/package.json`              | `version`                     |
| `apps/desktop/src-tauri/tauri.conf.json` | `version`                     |

Use `pnpm version:set <version>` and then `pnpm version:check`. The command does not update
`Cargo.lock`; run `cargo build` or `cargo metadata` and commit the refreshed lock file.

FuryBuilder uses `MAJOR.MINOR.PATCH`: major for a migration that older versions cannot open,
minor for new user-visible tools or opt-in schema additions, patch for compatible fixes and
security patches. Pre-1.0 minor releases may contain documented breaking changes. Package
versions are numeric only; the NSIS derivation expects numeric components. Git tags use
`v<version>`.

Independent axes:

| Axis                       | Source                                                                                       | Rule                                                          |
| -------------------------- | -------------------------------------------------------------------------------------------- | ------------------------------------------------------------- |
| Desktop application        | `version.json`                                                                               | Release flow below                                            |
| Project manifest           | `PROJECT_SCHEMA_VERSION` in `crates/project-store/src/manifest.rs`                           | Bump only with a migration and backup test                    |
| World snapshot             | `SNAPSHOT_FORMAT_VERSION` / `SNAPSHOT_MIN_VERSION` in `crates/project-store/src/snapshot.rs` | Bump on binary layout changes; keep old readers or refuse     |
| Desktop discovery protocol | `DESKTOP_PROTOCOL_VERSION` in the MCP bridge                                                 | Bump on wire changes                                          |
| MCP protocol               | Negotiated modern/legacy era                                                                 | Follows the MCP specification; never assumed                  |
| MCP tool schema            | Bridge tool definitions                                                                      | Additive changes preferred; destructive changes need a rename |

A new app version does not automatically bump a schema version. A schema bump does not
require a major app version if the migration is automatic, tested and reversible.

## Release checklist

1. Move `CHANGELOG.md` entries from `[Unreleased]` into `## [<version>] - YYYY-MM-DD`.
2. Run `pnpm version:set <version>`, then `pnpm version:check`.
3. Run `pnpm typecheck`, `pnpm lint`, `pnpm test`, `cargo fmt --all -- --check`,
   `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`.
4. Run `cargo audit`, `pnpm audit --prod`, `pnpm licenses:check`; refresh the inventory with
   `pnpm licenses:update` when dependencies changed. Zero denied or unknown licenses.
5. Build the NSIS installer with `pnpm tauri:build` and the updater signing environment, or
   let the tag workflow build it on `windows-latest`.
6. Run `scripts/test-windows-installer.ps1` with the previous and new installers to prove
   clean install, update and uninstall.
7. Run `scripts/verify-release-artifacts.ps1` on the NSIS package. Use `-RequireSigned` for a
   public stable release; an unsigned beta passes only when the limitation is recorded.
8. Commit the version bump, changelog, lock file and artifacts together; tag `v<version>`.
9. Publish only after the workflow confirms that the NSIS installer, updater signature and
   `latest.json` are attached.

Pushing a `v<version>` tag runs `.github/workflows/release.yml`: it checks tag/manifest
agreement, runs the checks, generates release notes from Conventional Commits with
`scripts/release-notes.mjs`, builds on `windows-latest`, uploads the NSIS package, its updater
signature and `latest.json`, verifies the expected assets and publishes the release. It
refuses to overwrite a published release.

## Changelog

`CHANGELOG.md` follows Keep a Changelog. User-visible changes, migrations, security fixes and
known breaks go under `Added`, `Changed`, `Deprecated`, `Removed`, `Fixed` or `Security`.
`[Unreleased]` is always present. An initial release normally uses only `Added` and
`Security`.

## Installer and updates

- NSIS installs per user under `%LOCALAPPDATA%\FuryBuilder`; no elevation.
- `bundle.windows.allowDowngrades` is `false`.
- The updater points at
  `https://github.com/ahmikael/FuryBuilder/releases/latest/download/latest.json`, checks four
  seconds after startup, downloads only after confirmation and verifies the package against
  the configured updater public key.
- Updater signing uses `TAURI_SIGNING_PRIVATE_KEY` and
  `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` repository secrets. Back them up offline: if lost,
  installed copies will refuse updates signed with a replacement key.
- Beta releases are normal GitHub releases, not prereleases, because `/releases/latest`
  ignores prereleases and drafts.
- Authenticode signing is separate and still pending. SmartScreen may warn about an unknown
  publisher on the unsigned beta.

## Backups and migrations

User data lives under `%APPDATA%\com.furybuilder.desktop`:

```text
projects\<name>.furybuilder\   # the only irreplaceable user data
assets.json / runtime.json / server.json
runtimes\java\                 # re-downloadable
servers\                       # re-downloadable server JAR cache
logs\                          # bounded diagnostics
mcp\                           # local bridge discovery metadata
```

Inside a project:

```text
project.json                  # schema, active snapshot, provenance
data\world-<revision>.fwl     # snapshots, newest 8
data\autosave.fwl/.json       # one crash-recovery candidate
changes\checkpoint-*          # newest 5
source\                       # immutable imported source
cache\                        # regenerable
session.lock                  # present while a session is open
```

Protections:

| Protection             | Behavior                                                                                                                              | Bound                 |
| ---------------------- | ------------------------------------------------------------------------------------------------------------------------------------- | --------------------- |
| Atomic save            | Temp file, fsync, decode/re-encode, rename; `project.json` is written only after the referenced snapshot is durable                    | One publish at a time |
| Snapshot history       | Each save publishes `data/world-<revision>.fwl`                                                                                       | Newest 8              |
| Per-save manifest copy | Previous manifest is copied to `project.json.bak-<previous revision>` before publish                                                  | Newest 5              |
| Explicit checkpoints   | User-requested recovery points                                                                                                        | Newest 5              |
| Autosave               | Every 30 s while dirty, on window hide and on close; revision, byte count and SHA-256 are verified before adoption                     | One current autosave  |
| Schema migration       | Original manifest is copied byte for byte to `project.json.v<old>.bak`; a failed copy aborts migration                                | One per migration     |
| Immutable source       | Imported files are copied under `source/files/` with SHA-256 and never edited by overlay operations                                   | Source tree           |
| Diagnostics            | Rotating JSON Lines log, no world payloads or secrets                                                                                 | 2 MiB x 4 files       |

If the migration backup fails, the project is not migrated. A newer unknown schema is refused
with `unsupported_version`; no downgrade or guess is attempted. Settings files migrate in
memory; an unreadable or newer settings file falls back to defaults with a visible warning
and is never deleted silently. Managed runtimes, the server cache and `cache/` are
re-downloadable or regenerable and can be cleared without touching projects.

Manual backup: close the app and copy `%APPDATA%\com.furybuilder.desktop`, or only
`projects\`. Restore while closed; rename the current directory instead of deleting it, copy
the backup, then verify each project's revision before removing the renamed copy.

`source/` is a copy of the imported world, not a live link. Editing the original world
outside FuryBuilder does not change the project, and FuryBuilder never writes to the original.

## Not covered

- Disk failure, ransomware, profile corruption or deleting the application data directory.
- Synchronising projects between machines or users; copying is manual.
- Recovering an original Java world after a third-party tool modified it.
- Automatic cloud backup. None is configured and no project data leaves the machine.
