# Versioning and release process

Status: release policy for the Windows beta. The application version is independent from
the project schema, the world snapshot format and protocol versions.

## Single source of truth

`version.json` at the repository root is authoritative. The release tooling requires every
manifest that Cargo, pnpm or the installer reads to agree with it:

| File                                     | Field                         |
| ---------------------------------------- | ----------------------------- |
| `version.json`                           | `version` (source)            |
| `Cargo.toml`                             | `[workspace.package].version` |
| `package.json`                           | `version`                     |
| `apps/desktop/package.json`              | `version`                     |
| `apps/desktop/src-tauri/tauri.conf.json` | `version`                     |

The Rust MCP crate inherits `version.workspace = true`, so it cannot drift from the
workspace version.

```powershell
pnpm version:check          # exits non-zero on drift
pnpm version:set 0.2.0      # updates all five manifests
```

`pnpm version:set` does not edit `Cargo.lock`; run `cargo build` or `cargo metadata`
afterwards and commit the refreshed lock file with the bump. CI runs `pnpm version:check`
before the TypeScript and Rust jobs.

Do not hand-edit one manifest. A mismatch produces an installer whose file version, uninstall
registry entry and about screen can disagree, which makes support and migration decisions
unreliable.

## Semantic versioning

FuryBuilder uses `MAJOR.MINOR.PATCH` with [Semantic Versioning](https://semver.org/) meaning:

- **MAJOR** — a project, snapshot, IPC or MCP change that requires an explicit migration and
  cannot be opened safely by the previous stable version.
- **MINOR** — new user-visible tools or an opt-in schema addition that remains backward
  compatible. Pre-1.0, minor releases may still contain documented breaking changes; the
  changelog must call them out.
- **PATCH** — compatible fixes, data-loss prevention, security patches and documentation.

The release package version is numeric only. Pre-release channels are expressed by release
notes and installer distribution, not by suffixes such as `-beta.1`, because the NSIS
version derivation expects numeric components.
If a suffix is needed later, the release tooling and the installer must be tested
together first.

Git tags use `v<version>` (for example `v0.1.0`) and point at the commit whose changelog
section and installer hashes were verified.

## Independent version axes

| Axis                       | Source                                                                                       | Change rule                                                                                    |
| -------------------------- | -------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| Desktop application        | `version.json`                                                                               | Follows the release flow below                                                                 |
| Project manifest           | `PROJECT_SCHEMA_VERSION` in `crates/project-store/src/manifest.rs`                           | Bump only with an explicit migration and pre-migration backup test                             |
| World snapshot             | `SNAPSHOT_FORMAT_VERSION` / `SNAPSHOT_MIN_VERSION` in `crates/project-store/src/snapshot.rs` | Bump when the binary layout changes; keep old readers or refuse with a clear error             |
| Desktop discovery protocol | `DESKTOP_PROTOCOL_VERSION` in the MCP bridge                                                 | Independent of the app version; bump on wire changes                                           |
| MCP protocol               | `MODERN_PROTOCOL_VERSION` / legacy negotiation                                               | Follows the negotiated MCP specification; never assumed                                        |
| MCP tool schema            | Tool definitions in the bridge                                                               | Additive changes preferred; destructive changes require a new tool name or explicit capability |

A new application version does not automatically bump any schema version. A schema bump does
not require a major application version if the migration is automatic, tested and reversible
through the backup.

## Release checklist

1. Move the relevant `CHANGELOG.md` entries from `[Unreleased]` into a dated
   `## [<version>] - YYYY-MM-DD` section.
2. Run `pnpm version:set <version>`, then `pnpm version:check`.
3. Run `pnpm typecheck`, `pnpm lint`, `pnpm test`, `cargo fmt --all -- --check`,
   `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`.
4. Run `cargo audit`, `pnpm audit --prod`, `pnpm licenses:check` and refresh the inventory
   with `pnpm licenses:update` when dependencies changed. Zero denied/unknown licenses are
   required.
5. Build the NSIS installer with `pnpm tauri:build` and the updater signing environment, or let
   the tag workflow build it on `windows-latest`.
6. Run `scripts/test-windows-installer.ps1` with the previous and new NSIS installers to
   prove clean install, update and uninstall. Record new SHA-256 values.
7. Run `scripts/verify-release-artifacts.ps1` on the NSIS package. Use `-RequireSigned` for a
   public stable release; an unsigned beta passes only when the limitation is recorded and
   published hashes are available.
8. Run `scripts/test_windows_installer_evidence.py`, `scripts/test_release_security.py` and
   the remaining release documentation tests.
9. Review the migrations section of [backup-policy.md](backup-policy.md) whenever a schema
   constant changed.
10. Commit the version bump, changelog, lock file and evidence together; tag `v<version>`
    and push the tag. The release workflow builds the packages, publishes `latest.json`
    and makes the GitHub release public.
11. Do not publish a package whose installer hash is missing from the release notes.

## GitHub release automation

Pushing a tag named `v<version>` runs `.github/workflows/release.yml`. The workflow:

1. checks that the tag matches `version.json` and that every manifest agrees;
2. runs the frontend and Rust checks, including the release-notes tool tests;
3. generates release notes from Conventional Commits with `scripts/release-notes.mjs`;
   every commit type gets its own section, including `chore`, `test`, `ci`, `build` and
   `style`, while untyped commits go to Other Changes and breaking changes are listed
   separately;
4. creates a draft GitHub release and builds the Windows NSIS installer on `windows-latest`;
5. uploads the NSIS package, its updater signature and the static `latest.json`
   manifest, then appends SHA-256 checksums and publishes the release.

The workflow may also be started manually with the exact existing tag. It refuses to
overwrite an already published release; bump the version and tag a new commit instead.

Updater signing is independent from Authenticode. The public key lives in
`apps/desktop/src-tauri/tauri.conf.json`; the private key and its password are stored as
the `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` repository
secrets. Back both up offline: if they are lost, already installed copies will refuse
future updates signed with a replacement key.

## Changelog policy

`CHANGELOG.md` follows Keep a Changelog. Every user-visible change, data migration, security
fix and known break goes under `Added`, `Changed`, `Deprecated`, `Removed`, `Fixed` or
`Security`. Internal refactors without user impact may be omitted. The `[Unreleased]` section
is always present so release preparation has a single place to curate. An initial release
normally uses only `Added` and `Security`; `Changed`, `Deprecated`, `Removed` and `Fixed`
describe differences from a previously published version and appear once one exists.

## Installer metadata and updates

- NSIS installs per user under `%LOCALAPPDATA%\FuryBuilder` and does not require elevation.
- `bundle.windows.allowDowngrades` is `false`, so a silent or passive older package cannot
  replace a newer installation.
- The Tauri auto-updater is configured against
  `https://github.com/ahmikael/FuryBuilder/releases/latest/download/latest.json`. Installed
  copies check four seconds after startup, download only after confirmation and verify the
  NSIS package against the configured updater public key. Update signing
  uses the `TAURI_SIGNING_PRIVATE_KEY` secret; Authenticode signing remains a separate
  external gate.
- Beta releases are published as normal GitHub releases, not prereleases, because the
  `/releases/latest` updater endpoint does not consider prereleases or drafts.
