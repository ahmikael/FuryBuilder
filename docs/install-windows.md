# Windows installation, update and removal

This page describes the Windows beta installer produced by `pnpm tauri:build`.
The installer built from this source contains FuryBuilder and its `furybuilder-mcp.exe`
sidecar. It does not contain Minecraft, Java, Mojang assets or a `server.jar`.

## Package

| Package                                    | Default scope | Elevation    | Notes                                                                  |
| ------------------------------------------ | ------------- | ------------ | ---------------------------------------------------------------------- |
| NSIS `FuryBuilder_<version>_x64-setup.exe` | Current user  | Not required | Installs under `%LOCALAPPDATA%\FuryBuilder`.                           |

Only the per-user NSIS package is published. It declares the application
identifier and version from `version.json`.

## Requirements

- 64-bit Windows 10 or Windows 11.
- Microsoft Edge WebView2 Runtime. It is preinstalled on current Windows 10/11;
  the installer can run the official bootstrapper when it is missing, which
  needs an internet connection.
- No Minecraft installation and no Java runtime are required to start
  FuryBuilder or to use `Empty Project`/`Flat base`.
- `Vanilla World` additionally needs the Java major from the selected version
  profile (8, 16, 17 or 21 depending on the release) and that version's official
  `server.jar`. FuryBuilder offers to locate or download them only after explicit
  consent, verifies checksums and keeps them in its own cache. `Import World`
  needs a user-owned classic Java 1.13–1.21.11 world folder.

The supported baseline, memory/disk guidance, tested reference machine and
configurations that are not validated are in
[reference.md](reference.md).

## Install

1. Download the NSIS package from the official GitHub Releases page.
2. Run the NSIS package and follow the wizard. The default installation is
   per-user and does not request administrator rights.
3. Start FuryBuilder from the Start menu or the installer's finish page.

The beta installer is currently **not code-signed**. Windows SmartScreen may
show an "Unknown publisher" warning; this is expected until a code-signing
certificate is configured. Do not disable SmartScreen globally and do not run a
package from an unofficial source. The signing and update policy is in
[release.md](release.md#installer-and-updates). `scripts/verify-release-artifacts.ps1`
reproduces the hash, version and signature check for a local build; `-RequireSigned`
turns missing signatures into a failure once a certificate exists.

## Update

The built-in auto-update channel checks GitHub Releases four seconds after
startup. When a newer published version exists, a banner and
**Settings → Updates** show the version and release notes. Choosing **Install and restart**
downloads the NSIS package, verifies its Tauri updater signature, then Windows
briefly closes FuryBuilder and installs the new build. Projects, settings and
downloaded runtime caches are preserved. The NSIS package updates per user
without elevation. A failed or unavailable update check does not block the
editor; the newer installer can still be run manually over the current
installation.

Update signing uses a dedicated Tauri key, separate from Authenticode. The
published `latest.json` contains the signed NSIS package URL; the application
refuses a package whose signature does not match the embedded public key. If the
check fails, confirm that GitHub Releases is reachable and download the package
again from the official release page before running it manually.

The beta installer is still not Authenticode-signed, so Windows SmartScreen can
warn about an unknown publisher on the first manual install. Downgrades are
blocked by the bundle configuration, so an older installer cannot silently
replace a newer one. Back up projects before a beta update anyway.

## User data and uninstall

FuryBuilder stores settings, logs, downloaded runtimes, the server JAR cache and
projects under:

```text
%APPDATA%\com.furybuilder.desktop
```

Uninstalling through **Settings → Apps → FuryBuilder** removes program files,
shortcuts and the uninstall registry entry. It does not delete the user data
directory, so projects and the downloaded runtime cache survive an uninstall or
a reinstall. Delete that directory manually only when you also want to remove
all projects and cached downloads.

## Build the installer

From the repository root:

```powershell
pnpm install
pnpm tauri:build
```

Artifacts are written to:

```text
target\release\bundle\nsis\
```

A production build that should produce updater artifacts also needs the signing
environment used by the release workflow:

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY = Get-Content $HOME\.tauri\furybuilder-updater.key -Raw
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = Get-Content $HOME\.tauri\furybuilder-updater.password -Raw
pnpm tauri:build
```

Without the key Tauri cannot create the `.sig` files or `latest.json` used by
auto-update. GitHub Actions reads the same values from the
`TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` repository
secrets.

The repeatable install/update/uninstall check is
`scripts/test-windows-installer.ps1`. It backs up and restores the real
application data directory, so it does not disturb existing projects.
