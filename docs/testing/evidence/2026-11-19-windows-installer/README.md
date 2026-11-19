# M11.1 — Windows installer, update and removal

Recorded: 2026-11-19 on Windows 11 Pro `10.0.26300` with WebView2
`157.0.4341.60`. The session ran as a normal, non-elevated user.

## Artifacts

| Package                                                      |     Bytes | SHA-256                                                            |
| ------------------------------------------------------------ | --------: | ------------------------------------------------------------------ |
| `FuryBuilder_0.1.0_x64-setup.exe` (NSIS)                     | 5,879,723 | `ad21b7f37ed092fa1b7a0e82b3b30631deb62cdda6f392037d85ed1e02ec209c` |
| `FuryBuilder_0.1.1_x64-setup.exe` (NSIS, upgrade-test build) | 5,880,272 | `e66b6fd24f81db5517edc44a2562403e68654fc5a02d02b4149d03dfd7806514` |
| `FuryBuilder_0.1.0_x64_en-US.msi`                            | 7,802,880 | `52622c9b503e7a3b0156d3649c1c87b25fc85f58558e6d4c4caf06e0fadc618e` |

The 0.1.1 NSIS package exists only to exercise the update path. It was built by
temporarily setting the desktop package version to `0.1.1` and merging a
`{"version":"0.1.1"}` Tauri config; the repository source remains at 0.1.0 after
the test. The MSI contains the same 0.1.0 application executable whose extracted
SHA-256 is `75968698ebb46bc4b5e09be5df9f984ce3c59ac130b5badaad40a30ff8954111`.

## Method

`scripts/test-windows-installer.ps1` backs up the real
`%APPDATA%\com.furybuilder.desktop` directory and restores it in a `finally`
block. It then performs these steps with the real packages:

1. Runs the 0.1.0 NSIS installer with `/S` and waits for the per-user `HKCU`
   uninstall entry.
2. Checks that the installed executable's `ProductVersion` and the registry
   `DisplayVersion` are `0.1.0`.
3. Starts the installed `furybuilder-desktop.exe`, waits for a real main window,
   requires an `app.start` diagnostics record with `appVersion` 0.1.0, closes it
   through `CloseMainWindow` and requires a clean exit.
4. Writes a random marker file into the application data directory.
5. Runs the 0.1.1 NSIS installer over the existing installation and repeats the
   version, registry and launch checks. It also requires the marker to be
   unchanged byte for byte.
6. Runs `uninstall.exe /S`, waits for removal of the executable and the
   uninstall registry entry, and confirms that the application data marker is
   still present.
7. Expands the MSI with `msiexec /a` (administrative extraction) and reads the
   embedded executable version and SHA-256. A silent per-machine install is not
   attempted without elevation.

## Results

| Scenario                     | Result                                                                                                  |
| ---------------------------- | ------------------------------------------------------------------------------------------------------- |
| NSIS clean install           | Passed. Per-user location `%LOCALAPPDATA%\FuryBuilder`; publisher `FuryBuilder contributors`            |
| Installed application launch | Passed. Window ready in 1.17 s, one matching `app.start` record, graceful close                         |
| NSIS 0.1.0 → 0.1.1 update    | Passed. Registry and executable report 0.1.1, app-data marker preserved, upgraded app started in 0.94 s |
| NSIS uninstall               | Passed. Executable and `HKCU` uninstall entry removed; app data preserved                               |
| MSI package extraction       | Passed. Embedded executable reports 0.1.0 and matches the recorded hash                                 |

The full machine-readable result and transcript are in
`installer-evidence.json` and `installer-test.log`. `scripts/test_windows_installer_evidence.py`
validates both files (8 tests).

## Interpretation and limitations

- Both packages are **unsigned**. Windows SmartScreen can show an unknown
  publisher warning until a code-signing certificate is configured. This task
  did not test signature verification.
- The MSI is configured for a per-machine install and needs elevation. This
  non-elevated session only proved package integrity by administrative
  extraction; MSI install, update and uninstall are not claimed as tested.
- The run used one Windows 11 machine and one WebView2 version. It does not
  prove behavior on Windows 10, a clean Windows install without Minecraft/Java,
  other launchers, offline mode or a corrupted cache; those are separate M11
  tasks.
- Uninstall intentionally keeps the user data directory. Removing it is a
  separate, manual action.
- FuryBuilder does not configure the Tauri auto-updater. An update is a manual
  run of the newer installer; downgrades are blocked by
  `bundle.windows.allowDowngrades: false`.
- The installer installs `furybuilder-desktop.exe`; the Start menu shortcut and
  the uninstall entry use the product name `FuryBuilder`.
- The first harness run observed a stale uninstaller that delayed registry
  cleanup for over a minute. The harness now retries the silent uninstall once;
  the final recorded run passed on the first attempt without retrying.

## Reproduce

```powershell
pnpm tauri:build

powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-windows-installer.ps1 `
  -OldInstaller target/release/bundle/nsis/FuryBuilder_0.1.0_x64-setup.exe `
  -NewInstaller target/release/bundle/nsis/FuryBuilder_0.1.1_x64-setup.exe `
  -MsiInstaller target/release/bundle/msi/FuryBuilder_0.1.0_x64_en-US.msi

python scripts/test_windows_installer_evidence.py -v
```

The 0.1.1 package is not produced by the default build. To repeat the update
scenario, bump the desktop package version to 0.1.1 and run the build with
`-b nsis -c` pointing at a temporary `{"version":"0.1.1"}` config, then revert
the version bump.
