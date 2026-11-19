# Official Minecraft server JAR (M9.4)

Scope: obtain the **official Minecraft Java 1.21.4 server JAR** for the future Vanilla
World flow, either from Mojang's version metadata or from a local file the user already has.
The editor never bundles or redistributes the JAR, never accepts the Minecraft EULA and does
not start a server in this task.

Implementation: `crates/runtime-manager/src/server/` plus the Tauri commands in
`apps/desktop/src-tauri/src/commands.rs` and the `Settings → Minecraft server JAR` section.

## Official metadata chain

Only HTTPS URLs from an explicit Mojang allowlist are requested, and redirects are validated
hop by hop:

1. `https://piston-meta.mojang.com/mc/game/version_manifest_v2.json`
2. the exact `1.21.4` entry's version JSON URL
3. that JSON's `downloads.server` URL from `piston-data.mojang.com`

The version JSON body is checked against the SHA-1 published in the manifest. The server
download must be a `release` for Minecraft `1.21.4`, declare Java 21, and announce a SHA-1
plus a size between 10 MiB and 200 MiB. The preview shown in Settings contains the version,
release time, size, Java requirement, target `DataVersion` 4189, SHA-1 and download URL.

For the real 1.21.4 release the metadata is:

| Field              | Value                                      |
| ------------------ | ------------------------------------------ |
| Version JSON SHA-1 | `c88775961e316f2be09859c54e13017660ee7770` |
| Server JAR SHA-1   | `4707d00eb834b446575d89a61a11b5d548d8c001` |
| Server JAR size    | 56,880,250 bytes                           |
| Required Java      | 21                                         |
| World DataVersion  | 4189                                       |

These values are reproduced from Mojang metadata and are not used to skip validation.

## Verification

After the download (or manual selection), the JAR itself is inspected:

- it must be a regular file, not a symlink, in the accepted size range;
- it must be a readable ZIP with a bounded entry count;
- it must contain exactly one root `version.json` with `id` `1.21.4` and
  `world_version` 4189;
- `java_version` must be 21 when present;
- it must contain a known server entry point (`net/minecraft/bundler/Main.class` or
  `net/minecraft/server/Main.class`), so a `client.jar` or library archive is rejected;
- the downloaded file's SHA-1 and size must match the Mojang announcement;
- the protocol version (769) and resource-pack format (46) are reported for transparency.

A manual JAR gets its own SHA-1 and is labeled `manual`; its provenance is not inferred from
the file name. Exact version, `DataVersion` and server entry point are still enforced. A
manual file that changes on disk is reported as stale instead of being silently accepted.

## Cache and selection

```text
<app-data>/
  server.json
  servers/
    minecraft-1.21.4/
      .furybuilder-server.json
      server.jar
```

- Downloads stream to a temporary `.download-*.jar` file in the cache root.
- After verification the JAR is moved into a staging directory, a manifest is written, and
  the directory is published with one rename.
- The manifest records schema version, Minecraft version, DataVersion, Java major, SHA-1,
  size, release time, source URL and install time.
- `server.json` stores only the selected JAR path plus its verified SHA-1/size/DataVersion.
- `Clear managed server JARs` removes only directories carrying the FuryBuilder manifest and
  our temporary entries. A manually selected file outside the cache is never deleted.
- Offline reuse is possible because the managed JAR and its manifest are local; no network
  request is needed to select or report an already cached JAR. Metadata and download requests
  honor the standard `HTTPS_PROXY`/`HTTP_PROXY`/`ALL_PROXY` variables; a broken proxy fails
  with an explicit network error and never skips SHA-1 verification.

## Commands and events

| Command                  | Purpose                                                           |
| ------------------------ | ----------------------------------------------------------------- |
| `server_discover`        | Selected report, managed cache inventory and problems             |
| `server_saved`           | Re-inspect only the saved JAR                                     |
| `server_select_jar`      | Validate and persist a manual local JAR                           |
| `server_clear_selection` | Forget the saved selection                                        |
| `server_release`         | Mojang metadata only (no download)                                |
| `server_install`         | Explicit download, SHA-1/size check, internal inspection, publish |
| `server_managed_list`    | Managed cache inventory                                           |
| `server_clear_managed`   | Delete only FuryBuilder-owned cached JARs                         |

`furybuilder-server-changed` is emitted after a selection or cache change;
`furybuilder-server-download` reports throttled metadata/download/verification/inspection
progress.

## Evidence

`docs/testing/evidence/2026-11-18-minecraft-server-jar.json` and `.png` record a real
WebView2 run with an isolated app-data directory:

- the official Mojang metadata resolved Minecraft 1.21.4 with SHA-1
  `4707d00e…8c001`, 56,880,250 bytes, Java 21 and target DataVersion 4189;
- the JAR was downloaded, checked against the announced SHA-1/size, inspected (`version.json`
  1.21.4 / `world_version` 4189 / protocol 769 / resource pack 46) and published into the
  isolated cache;
- the Settings section rendered the selected managed JAR and its SHA-1;
- `server_clear_managed` removed the managed JAR and cleared the selection;
- no console errors occurred and no Mojang binary was written to the repository.

Reproduce it with:

```powershell
cargo test -p furybuilder-runtime-manager
cargo run -p furybuilder-runtime-manager --example server_probe -- --release
cargo run -p furybuilder-runtime-manager --example server_probe -- --root <dir> --install
node scripts/server-webview2-smoke.mjs <out.json> <out.png>
```

The WebView2 smoke needs the Vite dev server at `127.0.0.1:1420` and downloads the JAR only
because it explicitly calls `server_install`. The probe example needs `--install` as well.

## Limits

- No EULA is shown or accepted here; the user must do that explicitly in M9.5 before any
  server process is started.
- The JAR is not executed and no world is generated by this task.
- Only the fixed 1.21.4 release is accepted; 26.x snapshots and split world layouts are not
  treated as compatible.
- Interrupted downloads are restarted; there is no resume.
- Manual JARs are validated by content, but their origin is not proven. The UI labels them
  as manual and shows the computed SHA-1.
- There is no automatic update, disk quota UI beyond `Clear managed server JARs`, or
  dependency signature verification (Mojang publishes SHA-1, not a signature, for this file).
