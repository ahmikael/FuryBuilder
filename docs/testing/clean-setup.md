# Clean user setup, launchers, offline and damaged cache (M11.7)

This page records the app-level test of the beta setup path on Windows. It is deliberately
narrower than “works on a clean Windows PC”: the machine is an existing Windows 11
installation, and the app runs in an isolated per-user environment.

Evidence:
[`evidence/2026-11-19-clean-setup/evidence.json`](evidence/2026-11-19-clean-setup/evidence.json).
Harness: `scripts/clean-setup-webview2.mjs`.

## What was isolated

The app ran with its own `FURYBUILDER_APP_DATA_DIR`, `APPDATA`, `LOCALAPPDATA`,
`USERPROFILE`, `ProgramFiles`, `ProgramFiles(x86)` and `PATH`. `JAVA_HOME` and related
variables were removed. The clean session therefore saw:

- no `.minecraft` folder,
- no Java 21 runtime in the managed cache, PATH or common vendor directories,
- no managed server JAR,
- no network route for managed dependencies.

The host machine still had network access. To test the failure path deterministically, every
managed-dependency request was routed through a closed loopback proxy
(`http://127.0.0.1:9`). The metadata commands failed with a connection-refused error and
left no download or staging files. This is a simulated network failure, not a physically
disconnected machine.

## Session 1 — no Minecraft, no Java

| Check                    | Recorded result                                                                                                  |
| ------------------------ | ---------------------------------------------------------------------------------------------------------------- |
| Asset discovery          | `defaultRootPresent: false`, 0 installations; the report says no launcher folder was found                       |
| Java discovery           | 0 candidates, 0 compatible, 0 managed; the report says no Java 21 runtime was found                              |
| Server discovery         | no selection, 0 managed JARs, no problems                                                                        |
| Vanilla readiness        | `ready: false`, Java/server/EULA not ready, 3 blockers                                                           |
| Settings                 | “Optional for Empty Project” is visible; readiness says “Setup required” and that it never downloads             |
| Empty Project            | created, revision 1, 0 chunks/sections/blocks, saved manually, closed and reopened at revision 1                 |
| Vanilla World wizard     | “Setup is incomplete”, no compatible Java, no JAR, EULA required; Generate is disabled and run consent unchecked |
| Wizard side effects      | 0 Java files, 0 server files, 0 generation-run files; the download/choose buttons are offers only                |
| Consentless backend call | rejected with “Select a compatible Java 21 runtime before starting generation.”; no active run                   |

## Session 2 — launchers and damaged caches

The second session reused the same isolated app data and seeded:

- a conventional `.minecraft` root with a synthetic 1.21.4 client JAR,
- two `launcher_profiles*.json` game directories with their own synthetic 1.21.4 JARs,
- a managed Java runtime whose `bin/java.exe` is a text file,
- a managed server JAR whose size does not match its manifest,
- a stale `runtime.json` selection pointing to a missing Java path,
- a stale `server.json` selection pointing to a missing server JAR.

| Check                       | Recorded result                                                                                                                |
| --------------------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| Launcher discovery          | 3 installations: the default root + 2 `launcher_profile` roots; each reports `1.21.4` as `compatible`                          |
| Read-only discovery         | All three synthetic roots had identical file hashes before and after; no writes                                                |
| Real third-party root       | An existing locally installed third-party root passed manual inspection: `minecraft_root`, 13 versions, 1.21.4 matched         |
| Broken managed Java         | Listed once in the managed cache; probed as `unreadable`; 0 compatible candidates; “no Java 21 runtime” remains                |
| Damaged managed server      | Not listed; the cache reports “changed on disk (expected 123456 bytes, found 21)”                                              |
| Stale Java selection        | Reported as `unreadable` with an explicit missing-path error                                                                   |
| Stale server selection      | `ready: false` with an explicit missing-file error                                                                             |
| Corrupted readiness         | Vanilla setup stays blocked; no Java/server/run download artifacts appear                                                      |
| Consent gate                | With a compatible Java selected and no server JAR, `consent: false` is rejected with “explicit generation consent is required” |
| Consent alone is not enough | `consent: true` still fails on the missing server JAR; no run starts                                                           |

## Recorded result

All 25 checks in `evidence.json` are true. The app version was 0.1.0, the executable was
`target/debug/furybuilder-desktop.exe` (SHA-256
`2fed0d81c89ef966fc09a09a8407a61af61e78a5f607f369f924a4bff40488c5`), and no console errors
were recorded.

## Reproduce

The debug desktop app and the Vite dev server must be ready:

```powershell
pnpm dev
cargo build -p furybuilder-desktop
node scripts/clean-setup-webview2.mjs <new-output-directory> `
  <third-party-root>|<placeholder> <java21.exe>
```

The Java executable is not used by the clean session. It is selected explicitly only in the
final consent-gate check, after the clean no-Java scenario has been recorded.

## Limits — why M11.7 remains open

- This is not a fresh Windows installation or a clean VM. A machine without any Minecraft
  installation and without Java, or a disposable Windows VM, is still external work.
- Launcher coverage uses conventional `.minecraft`-style roots and
  `launcher_profiles*.json` game directories with synthetic client metadata. Real launcher
  brands and their instance layouts were not installed for this run; one real third-party
  root was inspected through manual selection only.
- The synthetic client JARs contain only version metadata. They prove discovery and
  validation, not rendering or client compatibility.
- Offline is a closed-proxy simulation, not an unplugged network.
- Corrupted caches are seeded test directories, not a real user cache.
- No external beta user performed this setup. KL-43 stays open.
