# Minecraft Java assets (M9.1-M9.2)

Scope: find the user's existing Minecraft Java Edition files, verify that they match the
fixed target (Minecraft **1.21.4**, `DataVersion` 4189, resource pack format 46), and use
their blockstates, models and textures for the viewport. Empty Project still falls back to
the original placeholder atlas when no verified source is available.

Implementation: `crates/asset-source` (independent of Tauri and React) plus the
`Settings → Minecraft assets` section and the viewport asset loader in the desktop UI.

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
(schema version 2: only `kind`, `path` and an ordered list of resource-pack paths). It is
not part of a project, and it never stores Mojang files.

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

## Render bundles (M9.2)

`AssetBundleService` builds a bounded, read-only render bundle for the block states that
the viewport currently has loaded. The base client JAR is searched last; user packs are
searched from highest to lowest priority, so a pack overrides any blockstate, model or
texture path it contains.

What is resolved:

- `assets/<namespace>/blockstates/<path>.json` variants and `multipart` conditions
  (`AND`, `OR`, comma lists and `|` alternatives);
- `parent` chains with texture inheritance and cycle/depth limits;
- elements (`from`, `to`, `rotation`, `shade`, faces) and blockstate model rotations in
  90-degree steps;
- face `uv`, `rotation`, `cullface` and `tintindex`;
- PNG textures in memory: palette expansion, 16-bit stripping, grayscale/RGB/RGBA,
  alpha detection and vertical animated-frame stacks (the first frame is sampled; animation
  timing is not played back);
- a deterministic padded shelf atlas with the missing checker always present.

The bundle is cached in memory by a fingerprint of the source/pack files plus the
canonical requested block-state list. The cache is bounded (6 entries / 64 MiB of encoded
atlas data) and is exposed through `assets_clear_cache`.

Missing assets never change block data: an absent model or texture produces the magenta
missing checker, and the Performance panel shows `placeholder` with the error in its
tooltip. Block states that omit properties which vanilla fills in code (for example
`grass_block[snowy=false]`) use a deterministic default-like variant and record a warning
instead of failing the whole bundle. Vanilla fluids are not block models; FuryBuilder
renders a documented full-cube approximation with the real still/flow texture and a fixed
water tint.

Resource packs can be added as `.zip` or folder, reordered and removed in
`Settings → Minecraft assets`. Up to 16 packs are accepted. Every pack must pass the same
format-46 and archive-safety checks as discovery; the UI emits
`furybuilder-assets-changed` so the viewport rebuilds meshes without touching world data.

## Renderer limitations

- `uvlock` is recorded but not applied; element `rescale` is recorded but not applied.
- `ambientocclusion=false` is recorded; the fixed per-face shading is still used.
- Biome tint is a small fixed palette, not biome-accurate; `tintindex` values without a
  mapping stay white.
- Item models, builtin entity models (chests, signs), block entities and fluid flow/level
  metadata are not rendered. Fluids use the cube approximation described above.
- Animated textures show only their first frame. Mipmaps are disabled for the atlas.
- The renderer checks reference blocks through unit tests and a real WebView2 screenshot;
  it is not a pixel-perfect vanilla parity test.

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

`docs/testing/evidence/2026-11-18-assets-render-ui.json` and the matching PNG were
generated by `scripts/assets-render-webview2-smoke.mjs` against the same real launcher
root, with an isolated `APPDATA` and WebView2 profile:

| Check                              | Result                                                         |
| ---------------------------------- | -------------------------------------------------------------- |
| Asset source saved in isolation    | `minecraft_root`, valid report, 0 user packs                   |
| Demo world in the Rust core        | 9 chunks / 297,719 blocks / revision 1                         |
| Verified bundle active             | 12 models, 15 textures, atlas built from the real client JAR   |
| Actual WebGL screenshot            | 1200x825, 829+ quantized colors, non-blank, textured grass     |
| Startup overlay                    | hidden only for the screenshot; it is not part of the renderer |
| Asset error                        | none                                                           |
| User launcher root / real settings | never modified                                                 |

The screenshot shows real grass, oak planks, glass, stairs, logs, leaves and the water
cube approximation. The synthetic packs exist only in a temporary directory to exercise
the validators. No real resource pack was installed on this machine, so that path is
covered by tests and synthetic evidence rather than a real third-party pack.

## Reproduce

```powershell
cargo test -p furybuilder-asset-source
cargo run -p furybuilder-asset-source --example asset_source_probe -- discover
cargo run -p furybuilder-asset-source --example asset_source_probe -- inspect <launcher-root>
cargo run -p furybuilder-asset-source --example asset_source_probe -- inspect <client.jar>
cargo run -p furybuilder-asset-source --example asset_bundle_probe -- --jar <client.jar> --state minecraft:stone --state "minecraft:oak_stairs[facing=north,half=bottom,shape=straight,waterlogged=false]" --state "minecraft:grass_block[snowy=false]" --repeat
pwsh -File scripts/probe_minecraft_assets.ps1 -MinecraftRoot <launcher-root>
```

The render smoke needs the Vite dev server and the debug desktop binary:

```powershell
pnpm --filter @furybuilder/desktop dev
cargo build -p furybuilder-desktop
node scripts/assets-render-webview2-smoke.mjs <output.json> <output.png> <launcher-root>
```

The UI tests are:

```powershell
pnpm --filter @furybuilder/desktop test -- --run src/features/settings/MinecraftAssetsSection.test.tsx src/renderer/mesher.test.ts src/assets/model-assets.test.ts
```

## Limits and next steps

- The third-party launcher layout is accepted through **manual selection**; automatic
  detection targets conventional official launcher paths plus launcher profiles.
- Asset index/objects discovery is read-only inventory; sounds and other external assets
  are not used by the renderer.
- No JAR is launched. Java runtime discovery and managed downloads belong to M9.3–M9.4.
- Renderer approximations are listed above; they are intentionally reported rather than
  hidden behind the placeholder atlas.
- Mojang client JARs, assets and resource packs stay outside the repository and release
  artifacts.
