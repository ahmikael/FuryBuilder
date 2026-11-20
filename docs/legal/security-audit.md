# Release security audit

Recorded: 2026-11-19. Scope: the Windows beta artifacts produced by `pnpm tauri:build`,
the Rust workspace and the two shipped Node packages (`apps/desktop`, `mcp/bridge`).

This is a dependency and provenance audit, not a third-party penetration test. It records
what was checked, what was fixed and what remains an external blocker.

## Results

| Check                         | Tool                                              | Result                                                                                                                                                                                |
| ----------------------------- | ------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Rust vulnerabilities          | `cargo audit` 0.23.0, RustSec database 2026-11-19 | 0 vulnerabilities after upgrading `zip`                                                                                                                                               |
| Rust advisory warnings        | `cargo audit`                                     | 3 warnings: one unsound advisory on Linux-only `glib 0.18.5`, plus unmaintained `proc-macro-error` and `chrono`; none are compiled into the shipped Windows artifacts (details below) |
| Node production advisories    | `pnpm audit --prod`                               | 0 advisories across 74 dependencies                                                                                                                                                   |
| Node full tree advisories     | `pnpm audit`                                      | 0 advisories across 399 dependencies (includes dev/optional)                                                                                                                          |
| License policy                | `scripts/check-dependencies.mjs --check`          | 469 Rust and 59 Node components; 0 denied, 0 unknown, 28 notice-required                                                                                                              |
| Installer hashes and versions | `scripts/verify-release-artifacts.ps1`            | SHA-256, `ProductVersion` and Authenticode status recorded for both packages                                                                                                          |
| Authenticode signature        | `Get-AuthenticodeSignature`                       | **Not signed** — see the external blocker below                                                                                                                                       |

The table above is the 2026-11-19 release snapshot. On 2026-11-20 the Tauri
updater/process plugins were added for signed GitHub releases. Re-running the tools reported
0 Rust vulnerabilities and 0 Node advisories (76 production and 401 full-tree dependencies);
`pnpm licenses:check` passed with 495 Rust and 61 Node components and 29 notice-required. The
original evidence files are kept unchanged.

Machine-readable evidence:

- `docs/testing/evidence/2026-11-19-release-security/cargo-audit.json`
- `docs/testing/evidence/2026-11-19-release-security-pnpm-audit.json` (production)
- `docs/testing/evidence/2026-11-19-release-security-pnpm-audit-all.json` (full tree)
- `docs/testing/evidence/2026-11-19-release-security/signature-evidence.json`
- `docs/legal/third-party-inventory.json` and `third-party-inventory.md`

## Fixed during this audit

The first `cargo audit` run reported six advisories against `zip 2.6.1`, which FuryBuilder
used to extract managed Java runtimes:

- RUSTSEC-2026-0367 — SUID/SGID preservation during extraction
- RUSTSEC-2026-0368 — symlink targets escaping the destination
- RUSTSEC-2026-0369 — Unix permissions applied through symlinks
- RUSTSEC-2026-0429 — abort on a symlink entry with a large declared size
- RUSTSEC-2026-0545 — unauthenticated AES entries when data ends early
- RUSTSEC-2026-0548 — local-header names differing from the central directory

The dependency was upgraded from `zip 2` to `zip 9.7.3`, which is patched for all six
(`>= 9.0.0`, and `>= 9.0.5` for RUSTSEC-2026-0548). `zip 9.7.3` requires Rust 1.88, so the
workspace minimum supported Rust version was raised from 1.80 to 1.88. The signed updater
and process plugins added on 2026-11-20 require Rust 1.90, so the current workspace MSRV is
1.90. The extraction path
still applies its own defensive checks: entry-count and size limits, backslash rejection,
`enclosed_name`, explicit symlink rejection and bounded streaming writes. The runtime-manager
tests that cover traversal, symlinks, missing Java and wrong versions pass.

The `zip` dependency uses `default-features = false, features = ["deflate-flate2"]` plus a
direct `flate2` dependency. `zip`'s broader `deflate` feature enables `flate2/zlib-rs`, which
feature-unifies into `world-io` and changes deterministic gzip bytes. Keeping the miniz_oxide
backend preserves the byte-stable world/structure fixtures. Windows does not gain or lose a
formats feature from this choice.

`cargo audit` now exits 0 with zero vulnerabilities. This is a point-in-time result: the
advisory database changes, so the audit must be repeated for every release.

## Rust advisory warnings

`cargo audit` still reports three non-vulnerability warnings from the whole lockfile. They
are recorded here instead of being silenced:

| Warning                          | Package                  | Why it does not affect the Windows beta                                                                                                                                                                                                                                                    |
| -------------------------------- | ------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| RUSTSEC-2024-0429 (unsound)      | `glib 0.18.5`            | Reached only through Tauri's Linux GTK3 stack (`gtk 0.18` → `glib 0.18.5`). The Windows build does not compile this path; `cargo tree -i glib` on Windows prints nothing. Fixed in `glib >= 0.20`, which requires the Tauri Linux stack to move off GTK3 and needs separate Linux testing. |
| RUSTSEC-2024-0370 (unmaintained) | `proc-macro-error 1.0.4` | Transitive through `glib-macros`, therefore the same Linux-only GTK3 path.                                                                                                                                                                                                                 |
| RUSTSEC-2026-0543 (unmaintained) | `chrono 0.4.45`          | Present in `Cargo.lock` only as an optional dependency of `zip`, `schemars`, `serde_with` and `winreg`; no enabled feature selects it, and `cargo tree -i chrono` prints nothing.                                                                                                          |

The first two are a Linux-support follow-up, not a Windows release blocker. The third
disappears when the optional features are not used. A future audit must re-check all three
instead of assuming this assessment stays valid.

## Signature status

The 2026-11-19 evidence set includes an MSI that was evaluated before the package was
removed from the release on 2026-11-20. Only the NSIS package is shipped; the MSI row
below records the historical audit, not a current artifact.

The 0.1.0 NSIS package and the application executable are **unsigned**:

| Artifact                          | Version | Authenticode |
| --------------------------------- | ------- | ------------ |
| `FuryBuilder_0.1.0_x64-setup.exe` | 0.1.0   | `NotSigned`  |
| `FuryBuilder_0.1.0_x64_en-US.msi` | 0.1.0   | `NotSigned`  |

`scripts/verify-release-artifacts.ps1` records hashes and signature state and fails when a
present signature is invalid or tampered. The intended public-release gate is
`-RequireSigned`, which currently fails. A code-signing certificate, a protected private key
and a timestamp service are external requirements that cannot be created in this repository.
Until they exist, Windows SmartScreen may show an unknown-publisher warning and users must
verify the published SHA-256 before running a package.

The release is therefore **reproducible and hash-verifiable, but not publisher-authenticated**.
This limitation is stated in `docs/install-windows.md`, `docs/known-limitations.md` and the
release checklist.

Automatic updates use a separate Tauri updater keypair. The public key is embedded in
`apps/desktop/src-tauri/tauri.conf.json`; the private key and password are repository secrets
used by the release workflow. A valid updater signature proves that the package was signed by
the holder of that key, but it is not an Authenticode publisher signature and does not remove
the SmartScreen warning on a first manual install. Losing the updater private key prevents
already installed copies from accepting newly signed updates, so both key and password must be
backed up offline.

## Third-party notices

`docs/legal/third-party-inventory.md` is generated from resolved `cargo metadata` and
`pnpm licenses list --prod`. It lists every package, version and SPDX-style license expression.

Policy:

- Allowed: MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, 0BSD, Zlib, Unicode-3.0,
  CC0-1.0, MIT-0, Unlicense, MPL-2.0 and CDLA-Permissive-2.0. A dual license passes when at
  least one branch is allowed.
- Rejected: GPL, AGPL, SSPL, BUSL, Commons Clause and non-commercial variants.
- MPL-2.0 components (`cssparser`, `cssparser-macros`, `dtoa-short`, `option-ext`,
  `selectors`) are used unmodified as dependencies. Their source is available from the
  published crates and upstream projects; FuryBuilder does not relicense them.
- Unicode-3.0 (`icu_*`, `litemap`, `writeable`, `yoke`, `zerofrom`, `zerovec`) and
  CDLA-Permissive-2.0 components are used under their respective data/license terms.

`pnpm licenses:check` fails CI when a dependency introduces a denied license, changes to an
unknown license or loses license metadata. Regenerate the inventory with
`pnpm licenses:update` when dependencies change.

## Residual risks

- No code signature; publisher identity is not cryptographically proven.
- The Linux-only GTK3 path carries one unsound (`glib 0.18.5`) and one unmaintained
  (`proc-macro-error`) warning. Linux is not a released target in this beta.
- Registry advisories cover known published issues only. They do not prove the absence of
  vulnerabilities and do not replace a manual review of security-sensitive code.
- The audit covers resolved dependencies, not the Rust toolchain, WebView2 runtime, GPU driver
  or Windows itself.
- The historical MSI was validated by administrative extraction only; it is no longer
  shipped and no elevated MSI test is required.
- The beta was audited on one Windows 11 machine; platform-specific behavior on Windows 10
  is not proven by this audit.

## Reproduce

```powershell
cargo install cargo-audit --locked
cargo audit
pnpm audit --prod
pnpm audit
pnpm licenses:check

pnpm tauri:build
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/verify-release-artifacts.ps1 `
  -Artifact target/release/bundle/nsis/FuryBuilder_0.1.0_x64-setup.exe `
  -ExpectedVersion 0.1.0
```

Add `-RequireSigned` once a signing certificate is configured; without a certificate that
mode is expected to fail.
