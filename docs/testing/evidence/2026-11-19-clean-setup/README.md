# M11.7 clean user setup evidence

One recorded run of the real desktop app in an isolated per-user environment on Windows 11.

- `evidence.json` — machine-readable result (25 checks, all true).
- Harness: `scripts/clean-setup-webview2.mjs`.
- Explanation and reproduction: `docs/testing/clean-setup.md`.

Evidence SHA-256: `80bf039346869f6e7139ce3be9539048e76c5cdfc59f4540110dbfadffb0625f`.

The run is **not** a fresh Windows installation and **not** a set of real launcher brands.
It proves the app-level behaviour with isolated `APPDATA`, `LOCALAPPDATA`, `USERPROFILE`,
`ProgramFiles`, `PATH` and no `JAVA_HOME`: Empty Project works without Java or Minecraft,
Vanilla World dependencies are explicit and blocked until configured, a consentless run is
refused, a closed loopback proxy makes metadata requests fail without downloads, synthetic
`.minecraft`-style launcher layouts are discovered read-only, and corrupted/stale Java and
server caches are rejected. The remaining clean-VM and launcher-brand checks are documented
as external gaps.

The harness output also contains the isolated app data, synthetic launcher folders and
corrupted cache seeds. None of that is committed.
