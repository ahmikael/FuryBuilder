# Windows installation, update and removal

This page describes the Windows beta packages produced by `pnpm tauri:build`.
The installers contain FuryBuilder only. They do not contain Minecraft, Java,
Mojang assets or a `server.jar`.

## Packages

| Package                                    | Default scope | Elevation    | Notes                                                                  |
| ------------------------------------------ | ------------- | ------------ | ---------------------------------------------------------------------- |
| NSIS `FuryBuilder_<version>_x64-setup.exe` | Current user  | Not required | Recommended beta package. Installs under `%LOCALAPPDATA%\FuryBuilder`. |
| MSI `FuryBuilder_<version>_x64_en-US.msi`  | All users     | Required     | For managed deployment; installs under `Program Files`.                |

Both packages declare the same application identifier and version. Install one
format at a time; do not keep an NSIS and an MSI installation side by side.

## Requirements

- 64-bit Windows 10 or Windows 11.
- Microsoft Edge WebView2 Runtime. It is preinstalled on current Windows 10/11;
  the installer can run the official bootstrapper when it is missing, which
  needs an internet connection.
- No Minecraft installation and no Java runtime are required to start
  FuryBuilder or to use `Empty Project`/`Flat base`.
- `Vanilla World` additionally needs a Java 21 runtime and the official
  Minecraft 1.21.4 `server.jar`. FuryBuilder offers to locate or download them
  only after explicit consent, verifies checksums and keeps them in its own
  cache. `Import World` needs a user-owned Java 1.21.4 world folder.

## Install

1. Verify the downloaded file against the SHA-256 published with the release:

   ```powershell
   Get-FileHash .\FuryBuilder_<version>_x64-setup.exe -Algorithm SHA256
   ```

2. Run the NSIS package and follow the wizard. The default installation is
   per-user and does not request administrator rights.
3. Start FuryBuilder from the Start menu or the installer's finish page.

The beta packages are currently **not code-signed**. Windows SmartScreen may
show an "Unknown publisher" warning; this is expected until a code-signing
certificate is configured. Do not disable SmartScreen globally and do not run a
package whose hash does not match the published value. The audit record and the
reason signing is still pending are in
[legal/security-audit.md](legal/security-audit.md). `scripts/verify-release-artifacts.ps1`
reproduces the hash, version and signature check for a local build; `-RequireSigned`
turns missing signatures into a failure once a certificate exists.

## Update

Run the installer for a newer version over the existing installation. The
installer replaces program files in place and leaves user data untouched.
Downgrades are blocked by the bundle configuration, so an older installer cannot
silently replace a newer one. Back up projects before a beta update anyway.

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

## Build the packages

From the repository root:

```powershell
pnpm install
pnpm tauri:build
```

Artifacts are written to:

```text
target\release\bundle\nsis\
target\release\bundle\msi\
```

The repeatable install/update/uninstall check is
`scripts/test-windows-installer.ps1`. It backs up and restores the real
application data directory, so it does not disturb existing projects.
