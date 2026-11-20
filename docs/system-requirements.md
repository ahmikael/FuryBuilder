# System requirements (Windows beta)

Status: **Windows beta 0.1.0**, target Minecraft Java **1.21.4 / DataVersion 4189**.
Last reviewed: 2026-11-19.

This page states the supported baseline, the machine used for the recorded release checks,
and what has **not** been tested. It is deliberately narrower than “runs on Windows”:
unsupported configurations should fail or be reported as unverified, not be treated as
validated because the window opens. The maintained limitation list is
[known-limitations.md](known-limitations.md).

## Supported baseline

| Area             | Requirement                                                                                                                                        | Verification status                                                                                                                           |
| ---------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- |
| Operating system | 64-bit Windows 10 or Windows 11                                                                                                                    | Windows 11 Pro `10.0.26300` tested; Windows 10 is not tested (KL-01)                                                                          |
| WebView2         | Microsoft Edge WebView2 Runtime                                                                                                                    | Preinstalled on current Windows 10/11; tested `157.0.4341.60`. The installer bootstrapper needs network only when the runtime is missing      |
| CPU              | 64-bit x86-64 CPU supported by the installed Windows version                                                                                       | Intel Core i5-9600KF (6 cores / 6 threads) tested; older/low-core CPUs and ARM64 are not tested                                               |
| Memory           | 8 GB is the practical baseline target; 16 GB is the tested reference                                                                               | 16 GB measured; 8 GB has **not** been validated with this evidence set                                                                        |
| Graphics         | WebGL 2.0-capable GPU with a working Direct3D 11 path; software WARP/SwiftShader can run but is much slower                                        | One physical NVIDIA GeForce GTX 1060 3 GB (driver `32.0.15.6094`) tested; integrated GPUs, switching and other vendors are not tested (KL-32) |
| Disk             | About 19 MB for the installed executable; installer is 5.9 MB (NSIS) or 7.8 MB (MSI). Keep at least 1 GB free for projects and caches              | Sizes measured from the 0.1.0 packages. Vanilla World dependencies took about 270 MB in the server test                                       |
| Display/input    | A normal desktop session with a visible window; keyboard and mouse                                                                                 | MCP screenshots require a visible viewport (KL-36); headless and minimized capture are not supported                                          |
| Network          | Not required for `Empty Project`, `Flat base`, `Import World`, structure exchange or local MCP. The updater contacts GitHub Releases after startup | Core offline work is the design; optional downloads, update checks and the WebView2 bootstrapper need network when used                       |
| Permissions      | The recommended NSIS package is per-user and does not require administrator rights; the MSI is per-machine and requires elevation                  | NSIS install/update/uninstall tested without elevation; MSI only extracted administratively                                                   |
| User data        | `%APPDATA%\com.furybuilder.desktop` for projects, settings, logs and optional runtime/server caches                                                | Installer and uninstall preserve this directory                                                                                               |

## Feature dependencies

| Feature                         | Minecraft install | Java 21 | Official `server.jar` | Node.js 26+ | Network                                          |
| ------------------------------- | ----------------- | ------- | --------------------- | ----------- | ------------------------------------------------ |
| `Empty Project` / `Flat base`   | No                | No      | No                    | No          | No                                               |
| `Import World` (1.21.4)         | No                | No      | No                    | No          | No                                               |
| Structure `.nbt` import/export  | No                | No      | No                    | No          | No                                               |
| Real Minecraft textures         | Optional          | No      | No                    | No          | No                                               |
| `Vanilla World` generation      | No                | Yes     | Yes                   | No          | Only for optional downloads; cache works offline |
| MCP bridge (external AI client) | No                | No      | No                    | Yes         | No (local named pipe)                            |
| Installing a newer beta         | No                | No      | No                    | No          | Yes (manual installer download)                  |

Notes:

- `Vanilla World` also requires the user to read and accept the Minecraft EULA. FuryBuilder
  never accepts it automatically and never bundles Java or the server JAR.
- Real Minecraft textures use the user's legally installed 1.21.4 client JAR/assets or a
  chosen resource pack. Placeholder textures are used when no assets are configured.
- The MCP bridge is a separate local process. The desktop application itself does not need
  Node.js; only the external MCP client setup does.
- The official server and Java runtime stay in FuryBuilder-managed or user-selected caches;
  they are not part of the installer.

## Tested reference

This is the machine used for the release evidence, not a recommended purchase list:

| Item              | Recorded value                                                                                                                 |
| ----------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| OS                | Windows 11 Pro, version `10.0.26300`, 64-bit                                                                                   |
| CPU               | Intel Core i5-9600KF, 6 cores / 6 threads                                                                                      |
| Memory            | 16 GB                                                                                                                          |
| GPU               | NVIDIA GeForce GTX 1060 3 GB, driver `32.0.15.6094`; only one physical display adapter                                         |
| WebView2          | `157.0.4341.60`                                                                                                                |
| Application       | FuryBuilder `0.1.0`; debug executable SHA-256 `209d4f0c849ad952494cbd4a7947f880cb2c1ac61402bf848b504014eae7eea9`               |
| NSIS package      | `FuryBuilder_0.1.0_x64-setup.exe`, 5,879,723 bytes, SHA-256 `ad21b7f37ed092fa1b7a0e82b3b30631deb62cdda6f392037d85ed1e02ec209c` |
| MSI package       | `FuryBuilder_0.1.0_x64_en-US.msi`, 7,802,880 bytes, SHA-256 `52622c9b503e7a3b0156d3649c1c87b25fc85f58558e6d4c4caf06e0fadc618e` |
| Java (optional)   | Eclipse Temurin `21.0.12.1+1` for Vanilla World and server tests                                                               |
| Server (optional) | Official Minecraft Java `1.21.4` server JAR, SHA-1 `4707d00eb834b446575d89a61a11b5d548d8c001`                                  |
| MCP client        | OpenCode CLI `2.3.2` is the verified client; other clients are not guaranteed                                                  |

Because only one physical GPU and one Windows 11 machine were used, the performance and
rendering evidence is a reference point, not a cross-vendor or minimum-hardware guarantee.

## Not tested / not supported

- Windows 10, Windows on ARM, Windows Server and non-Windows hosts.
- 8 GB RAM systems, lower-core CPUs and very old drivers.
- Integrated/discrete switching, multi-GPU laptops, other GPU vendors and display capture
  under remote desktop.
- A clean Windows installation without Minecraft or Java. Several M11 release checks still
  need that environment.
- Modded servers, data packs, other server versions, Bedrock worlds and Java versions other
  than 1.21.4 / DV 4189.
- Code-signed installers and a stable (non-beta) update channel. The beta updater uses
  GitHub Releases and signed Tauri packages; Authenticode signing and a stable channel are
  still pending.
- Streamable HTTP or remote MCP; the bridge is local Windows stdio only.
- External beta-user field data. The recorded checks are automated and use the reference
  machine above; see KL-43.

## Related pages

- [Windows installation and updates](install-windows.md) — packages, hash verification,
  install/update/uninstall and user-data location.
- [Known limitations](known-limitations.md) — maintained, example-backed list.
- [Java runtime and world generation](runtime/java-runtime.md) — optional Java/server setup.
- [MCP setup](mcp-setup.md) — Node.js bridge and client configuration.
- [Test matrix](testing/test-matrix.md) — what is actually verified and where the evidence is.
