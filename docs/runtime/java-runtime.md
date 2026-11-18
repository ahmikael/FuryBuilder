# Java runtimes (M9.3)

Scope: find a Java runtime that is actually compatible with Minecraft Java **1.21.4**,
let the user choose one manually, and optionally download the official Eclipse Temurin JRE
into FuryBuilder's own application-data directory. Editing an Empty Project and editing an
imported world never need Java; this runtime is used only by the later Vanilla World
generation flow.

Implementation: the independent Rust crate `crates/runtime-manager` plus the Tauri commands
in `apps/desktop/src-tauri/src/commands.rs` and the `Settings → Java & world generation`
section in the desktop UI.

## What is discovered

| Source      | How it is located                                                                                                                               |
| ----------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| Managed     | Every manifest-valid runtime under `<app-data>/runtimes/java/*`                                                                                 |
| Manual      | The saved `java` executable, remembered in `<app-data>/runtime.json`                                                                            |
| `JAVA_HOME` | `$JAVA_HOME/bin/java.exe` (or `$JAVA_HOME` when it already ends in `bin`)                                                                       |
| `PATH`      | Existing `java.exe` entries, capped at eight                                                                                                    |
| Common      | Vendor folders under `Program Files`, `Program Files (x86)` and `%LOCALAPPDATA%\Programs` (Adoptium, Java, Microsoft, Corretto, Zulu, BellSoft) |
| Launcher    | `%APPDATA%\.minecraft\runtimes`                                                                                                                 |

Folder walks are bounded (depth, visited entries and candidates). Symlinks and junctions are
skipped while scanning. No arbitrary disk scan is performed.

Discovery is not a filename guess: **every candidate is executed** with
`java -XshowSettings:properties -version`. The probe has an eight-second deadline, a bounded
output capture and no stdin. It reads `java.version`, `java.vendor`, `os.arch`,
`java.vm.name` and `java.home`.

## Statuses

| Status             | Meaning                                                                               |
| ------------------ | ------------------------------------------------------------------------------------- |
| `compatible`       | Major version 21 and a 64-bit architecture. The only status that can be selected.     |
| `outdated`         | Older than Java 21.                                                                   |
| `newer`            | Newer than the verified Java 21. Shown with a warning and not selected automatically. |
| `unsupported_arch` | 32-bit Java (`x86`/`i386`) cannot run the target worldgen.                            |
| `not_java`         | The process ran but reported no usable Java version.                                  |
| `unreadable`       | The process is missing, not a regular file, failed to start or timed out.             |

A stale saved path is reported as `unreadable` instead of being hidden or silently switched
to another runtime. The user can clear the selection without touching any files.

## Manual selection

`Settings → Java & world generation → Choose java.exe` opens a file dialog. The chosen path
is canonicalized, must be a regular file, and must pass the same probe. An incompatible or
non-Java executable is rejected and nothing is persisted. Selecting a runtime only stores
its path in `<app-data>/runtime.json`; no environment variable or registry entry is changed.

## Managed Temurin

The user must explicitly press **Check official Temurin download** and then
**Download & install**. No request happens in the background.

1. The backend calls the official Adoptium API v3
   (`/v3/assets/latest/21/hotspot?architecture=…&image_type=jre&os=…&vendor=eclipse&project=jdk`).
2. The returned metadata must describe a Java 21 Eclipse Temurin JRE for the current
   platform. The download URL must use HTTPS and an allowlisted GitHub host.
3. The announced SHA-256 must be 64 hex characters and the size must be between 10 MiB and
   200 MiB. The preview shows version, size, platform, checksum and URL before any bytes are
   requested.
4. The download streams to a temporary file under the managed cache, enforces the announced
   size and a hard 200 MiB cap, and computes SHA-256 while writing. A mismatch deletes the
   archive and aborts.
5. The ZIP is extracted with zip-slip, absolute-path, backslash, symlink and size/count
   limits. The archive must contain exactly one Java installation with a `bin/java`
   executable.
6. The extracted Java is probed before publication; anything other than Java 21 is rejected
   and the staging directory is removed.
7. A manifest (`.furybuilder-runtime.json`) is written, the install directory is published
   with a single rename, and the runtime becomes the selected Java.

The resulting cache layout is:

```text
<app-data>/
  runtime.json
  runtimes/
    java/
      temurin-21.0.13+9-windows-x64/
        .furybuilder-runtime.json
        bin/java.exe
        ...
```

`Clear managed Java` removes only directories that carry a FuryBuilder manifest. Unrelated
files in the cache root, external Java installations and user settings outside the managed
root are never touched. If the selected runtime was managed, its stale selection is cleared
after removal.

## Commands and events

| Command                   | Purpose                                                       |
| ------------------------- | ------------------------------------------------------------- |
| `runtime_discover`        | Probe and list candidates, saved runtime and managed cache    |
| `runtime_saved`           | Re-probe only the saved executable                            |
| `runtime_select_java`     | Validate and persist a manual executable                      |
| `runtime_clear_selection` | Forget the saved executable                                   |
| `runtime_temurin_latest`  | Adoptium metadata only (no download)                          |
| `runtime_install_temurin` | Explicit download, checksum check, extraction, probe, install |
| `runtime_managed_list`    | Managed cache inventory                                       |
| `runtime_clear_managed`   | Delete only managed runtimes                                  |

`furybuilder-runtime-changed` is emitted after a selection or cache change;
`furybuilder-runtime-download` reports throttled download/extract/probe progress.

## Evidence

`docs/testing/evidence/2026-11-18-java-runtime-ui.json` and `.png` record a real WebView2
run with an isolated app-data directory:

- a user-provided Temurin 21.0.12.1 JRE was probed and accepted;
- discovery reported the target Java 21 / Minecraft 1.21.4;
- Adoptium announced Temurin 21.0.13+9, 49,106,397 bytes, SHA-256
  `2332c0b4…58628`;
- the JRE was downloaded, verified, extracted, probed, published under the isolated app data
  and selected;
- the Settings section rendered the managed runtime;
- `runtime_clear_managed` removed it and cleared the stale selection;
- the external `java.exe` SHA-256 was identical before and after, and no console errors were
  recorded.

Reproduce the checks with:

```powershell
cargo test -p furybuilder-runtime-manager
cargo run -p furybuilder-runtime-manager --example runtime_probe -- --discover
node scripts/runtime-webview2-smoke.mjs <out.json> <out.png> <java.exe>
```

The smoke requires the Vite dev server at `127.0.0.1:1420` and downloads the JRE only
because it explicitly calls the install command.

## Limits

- Only Java 21 is accepted; the name of a folder or executable is never trusted.
- Managed downloads currently support Windows, macOS and Linux on x64/aarch64 through the
  Adoptium API; unsupported platforms return an explicit error.
- There is no resume for an interrupted download and no automatic update of an installed
  runtime. Re-run the explicit download to install a newer Temurin build.
- This task does **not** download `server.jar`, accept the Minecraft EULA or start a server;
  M9.4 now provides the verified official `server.jar` cache separately (see
  `docs/runtime/minecraft-server-jar.md`); EULA acceptance and the server process remain
  M9.5 work.
- No global installation, `PATH` edit, registry edit or background provisioning is done.
