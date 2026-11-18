# World selection: save folder, region file or region directory (M8.11)

Scope: classic Anvil, Minecraft Java **1.21.4**, `DataVersion` 4189 for supported
editing, plus explicit detection of other real layouts. Implementation:
`crates/world-io/src/world_selection.rs`.

## Why this exists

An Anvil region file `r.<x>.<z>.mca` is one 32x32 chunk container. A `region/`
directory is a folder of such containers. Neither carries `level.dat`, dimension
metadata, player data, POI/entity sidecars or the rest of a save. Treating either
as a world would produce a misleading "empty world" import.

The classifier answers one question before any import, export or copy starts:
**what did the user actually select?**

```rust
classify_world_selection(path) -> WorldSelectionReport
```

The report includes:

| Field                                                    | Meaning                                                                                                                   |
| -------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| `kind`                                                   | `world_folder`, `region_file`, `region_directory`, `not_found`, `unsafe_link`, `unknown`                                  |
| `layout`                                                 | `classic_anvil`, `split_26_1_plus`, `mixed`, `legacy_mcr`, `level_dat_only`, `region_file`, `region_directory`, `unknown` |
| `completeSave`                                           | structurally a save: `level.dat` plus classic or split world markers                                                      |
| `supported`                                              | complete, classic, structurally valid 1.21.4 save; full chunk validation still follows                                    |
| `hasLevelDat`, `dataVersion`, `versionName`, `levelName` | metadata read without decoding chunks                                                                                     |
| `dimensions`                                             | id, relative folder, region files and header-declared chunk count                                                         |
| `relatedFolders`                                         | present known folders such as `region`, `poi`, `entities`, `data`, `players`                                              |
| `regionFile`                                             | for a single `.mca`: file name, region coordinates, bytes, chunk count                                                    |
| `blockers`, `warnings`                                   | concrete reasons shown by the Import World wizard                                                                         |

Classification is read-only and does not decode chunk payloads. It validates
`level.dat`, layout markers and every region header. `analyze_java_world` reuses
the same classifier, so selecting a `.mca` file or a `region/` folder produces a
clear blocker instead of a `level.dat` error.

## Layout matrix

| Selection                                       | `kind`                      | `layout`           | Complete save | Supported | Result                                        |
| ----------------------------------------------- | --------------------------- | ------------------ | ------------- | --------- | --------------------------------------------- |
| Full classic 1.21.4 save                        | `world_folder`              | `classic_anvil`    | yes           | **yes**   | import/export path                            |
| Full classic save, other DataVersion            | `world_folder`              | `classic_anvil`    | yes           | no        | version blocker                               |
| Full 26.1+ split save                           | `world_folder`              | `split_26_1_plus`  | yes           | no        | layout blocker; regions inventoried read-only |
| Classic `region/` plus split markers            | `world_folder`              | `mixed`            | yes           | no        | mixed-layout blocker                          |
| `level.dat` plus only legacy `.mcr`             | `world_folder`              | `legacy_mcr`       | yes           | no        | legacy blocker                                |
| `level.dat` without any region/dimension folder | `world_folder`              | `level_dat_only`   | no            | no        | incomplete-save blocker                       |
| Single `r.<x>.<z>.mca`                          | `region_file`               | `region_file`      | no            | no        | "not a complete world save"                   |
| Directory of `.mca` files, no `level.dat`       | `region_directory`          | `region_directory` | no            | no        | "select its parent world folder"              |
| Directory of `.mcr` files, no `level.dat`       | `region_directory`          | `legacy_mcr`       | no            | no        | legacy blocker                                |
| Unrelated folder                                | `unknown`                   | `unknown`          | no            | no        | not-a-save blocker                            |
| Missing path / symlink                          | `not_found` / `unsafe_link` | `unknown`          | no            | no        | fail closed                                   |

Structural completeness and support are deliberately separate. A real 26.3 split
save is a complete save, but this build neither reads its chunks nor writes it.

## Passthrough boundary

`passthrough_java_world` now classifies the source before copying:

| Selection                       | Passthrough result                                            |
| ------------------------------- | ------------------------------------------------------------- |
| Complete classic 1.21.4 save    | copies every file byte for byte, skipping only `session.lock` |
| Region file or region directory | `unsupported_format` (not a complete save), no destination    |
| Split or mixed layout           | `unsupported_version`, no destination                         |
| Legacy `.mcr` world             | `unsupported_version`, no destination                         |
| Other DataVersion               | `unsupported_version`, no destination                         |
| Symlink or junction             | `unsafe_path`                                                 |

The source tree is never written. Existing passthrough tests for unknown files,
sidecars, symlinks and corrupt chunks remain in force.

## Real evidence

`docs/testing/evidence/2026-11-18-world-selection-1.21.4.json`:

| Selection              | Kind / layout                           | Complete | Supported | Chunks                                 |
| ---------------------- | --------------------------------------- | -------- | --------- | -------------------------------------- |
| Real flat 1.21.4 world | `world_folder` / `classic_anvil`        | yes      | yes       | 841 in 4 regions                       |
| Its `region/r.0.0.mca` | `region_file` / `region_file`           | no       | no        | 225 allocated records in the container |
| Its `region/` folder   | `region_directory` / `region_directory` | no       | no        | 4 region files                         |
| Real 26.3 split world  | `world_folder` / `split_26_1_plus`      | yes      | no        | 529 in 4 regions                       |

The classic passthrough copied 20 files, 4 regions and 841 chunks, and did not
copy `session.lock`. The region-directory attempt returned `unsupported_format`
and created no destination. The split attempt returned `unsupported_version` and
created no destination. Both source trees stayed byte-identical (SHA-256 per
file before/after).

## Reproduce

```powershell
cargo test -p furybuilder-world-io --test world_selection
cargo run -p furybuilder-world-io --example world_selection_probe -- <world>
cargo run -p furybuilder-world-io --example world_selection_probe -- <world>/region/r.0.0.mca
cargo run -p furybuilder-world-io --example world_selection_probe -- <world>/region
cargo run -p furybuilder-world-io --example world_selection_probe -- <world> --passthrough <new-folder>
```

## Limits

- Two real worlds were tested: one flat classic 1.21.4 save and one split 26.3
  save. This is evidence, not a universal layout-proof.
- The classifier reads region headers, not chunk payloads. `analyze_java_world`
  still decodes every chunk before import is enabled.
- Split-layout region containers are counted read-only; no split chunk decoding,
  editing, writing or passthrough is claimed.
- Empty directories are not represented in file hashes; file paths and contents
  are.
