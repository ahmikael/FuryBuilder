# Minecraft Java asset discovery (M9.1)

Scope: find the user's existing Minecraft Java Edition files, verify that they match the
fixed target (Minecraft **1.21.4**, `DataVersion` 4189, resource pack format 46), and
expose a manual fallback. This task does **not** render with those assets yet; model,
blockstate and texture loading is M9.2.

Implementation: `crates/asset-source` (independent of Tauri and React) plus the
`Settings → Minecraft assets` section in the desktop UI.

## What is discovered

FuryBuilder checks conventional launcher roots only:

| Platform | Root                                                                                   |
| -------- | -------------------------------------------------------------------------------------- |
| Windows  | `%APPDATA%\.minecraft` (and `%USERPROFILE%\AppData\Roaming\.minecraft` when different) |
| macOS    | `~/Library/Application Support/minecraft`                                              |
| Linux    | `~/.minecraft`                                                                         |

From a launcher root it also reads `launcher_profiles.json` and
`launcher_profiles_microsoft_store.json` and adds existing `profiles.*.gameDir` folders.
No arbitrary disk scanning is performed.

For each root:

- `versions/<id>/<id>.json` is parsed for id, type, release time, Java major version,
  `inheritsFrom`, asset index and the declared client JAR SHA-1/size.
- `versions/<id>/<id>.jar` is a candidate client JAR.
- `assets/indexes/*.json` and `assets/objects/` are inventoried. The target asset index
  (`19` for 1.21.4) is checksum-verified when the metadata declares a SHA-1.
- `resourcepacks/` entries are validated as packs.

## Version statuses

| Status                   | Meaning                                                                                                                                   |
| ------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------- |
| `compatible`             | Target version; client JAR SHA-1 and size match launcher metadata, and the internal `version.json` reports 1.21.4 / `world_version` 4189. |
| `compatible_unverified`  | Internal `version.json` reports the target, but no SHA-1 was available to verify against. Shown as a warning.                             |
| `wrong_version`          | A different Minecraft version. Non-target JARs are not hashed during a root scan.                                                         |
| `modded`                 | A profile with `inheritsFrom` (Fabric/NeoForge/OptiFine). Reported, never used as vanilla assets.                                         |
| `missing_metadata`       | No `<id>.json`; the JAR may still be inspected directly.                                                                                  |
| `missing_client_jar`     | The target metadata exists but the JAR is absent.                                                                                         |
| `checksum_mismatch`      | Size or SHA-1 does not match the declared client download.                                                                                |
| `world_version_mismatch` | The JAR's internal `world_version` is not 4189.                                                                                           |
| `invalid_metadata`       | Launcher JSON is unreadable or malformed.                                                                                                 |
| `invalid_client_jar`     | The JAR is not a readable zip, lacks root `version.json`, or its identity is inconsistent.                                                |

Only `compatible` and `compatible_unverified` count as a target match. A root is usable
only when it contains at least one such version.

## Manual selection

`Settings → Minecraft assets` offers native pickers for:

1. a `.minecraft`-style folder (one with `versions/`, or `assets/indexes`, or launcher
   profile metadata);
2. a client `client.jar` (or any version JAR);
3. a resource pack `.zip`;
4. a resource pack folder.

Selecting a client JAR reads the sibling `<name>.json` when present, so the SHA-1 can be
verified even outside a launcher root. Without sibling metadata the JAR is still
identified by its internal `version.json`, and the status is `compatible_unverified`.

The chosen source is saved in the application data directory as `assets.json`
(schema version 1, only `kind` + `path`). It is not part of a project, and it never stores
Mojang files.

## Resource pack validation

`pack.mcmeta` is parsed as untrusted JSON. `pack_format` may be an integer, or
`supported_formats` may be an integer, `[min, max]`, or
`{min_inclusive, max_inclusive}`. A pack is compatible only if that range includes
format 46.

Archive handling:

- zip/JAR files are parsed in memory and never extracted;
- entry names are validated against absolute paths, `..` traversal and backslashes;
- encrypted entries and zip64 are refused;
- limits: 50,000 entries, 1 GiB total uncompressed, 256 MiB per entry, 16 MiB central
  directory;
- a resource pack is further capped at 20,000 entries and 512 MiB;
- folder packs skip symlinks/junctions and cap recursion depth.

A pack with the wrong format, missing metadata, unsafe contents or unreadable data is
listed but cannot be selected.

## Real evidence

`docs/testing/evidence/2026-11-18-minecraft-assets-1.21.4.json`, generated by
`scripts/probe_minecraft_assets.ps1` against a real third-party launcher root
(`D:\mitrium-launcher\data`, selected manually and read-only):

| Check                                           | Result                                                     |
| ----------------------------------------------- | ---------------------------------------------------------- |
| Default `%APPDATA%\.minecraft`                  | absent and reported explicitly                             |
| Selected root                                   | 13 versions, 5 asset indexes, 0 resource packs             |
| Compatible versions                             | `1.21.4` only                                              |
| Client JAR                                      | 28,335,587 bytes, SHA-1 `a7e5a602…` matches metadata       |
| Internal JAR metadata                           | id `1.21.4`, `world_version` 4189, pack format 46, Java 21 |
| Asset index `19.json`                           | present and SHA-1 verified                                 |
| Modded profiles                                 | 3 reported as `modded`, none accepted                      |
| Manual JAR inspection                           | valid, checksum verified                                   |
| Synthetic valid pack                            | accepted                                                   |
| Synthetic wrong-format pack                     | rejected as `incompatible_format`                          |
| Synthetic missing-metadata pack                 | rejected as `missing_metadata`                             |
| Watched source files (JAR, version JSON, index) | byte-identical before/after                                |

The synthetic packs exist only in a temporary directory to exercise the validators. No
real resource pack was installed on this machine, so that path is covered by tests and
synthetic evidence rather than a real third-party pack.

## Reproduce

```powershell
cargo test -p furybuilder-asset-source
cargo run -p furybuilder-asset-source --example asset_source_probe -- discover
cargo run -p furybuilder-asset-source --example asset_source_probe -- inspect <launcher-root>
cargo run -p furybuilder-asset-source --example asset_source_probe -- inspect <client.jar>
pwsh -File scripts/probe_minecraft_assets.ps1 -MinecraftRoot <launcher-root>
```

The UI tests are:

```powershell
pnpm --filter @furybuilder/desktop test -- --run src/features/settings/MinecraftAssetsSection.test.tsx
```

## Limits and next steps

- The third-party launcher layout is accepted through **manual selection**; automatic
  detection targets conventional official launcher paths plus launcher profiles.
- Asset index/objects discovery is read-only inventory; sounds and other external assets
  are not used by the renderer yet.
- No JAR is launched. Java runtime discovery and managed downloads belong to M9.3–M9.4.
- M9.2 will read blockstates/models/textures from the verified source, apply resource
  packs in a deterministic order, cache atlases and test reference blocks.
- Mojang client JARs, assets and resource packs stay outside the repository and release
  artifacts.
