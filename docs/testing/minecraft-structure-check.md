# Minecraft Java 1.21.4 structure check

Status: passed for the structures and block states listed below. This is a
format-level integration check, not a claim that every Java block state,
entity type or full world is supported.

## Environment

- Windows 11; Java Temurin `21.0.12.1+1`.
- Official Minecraft Java 1.21.4 `server.jar`, size `56,880,250` bytes and
  SHA-1 `4707d00eb834b446575d89a61a11b5d548d8c001` (matches Mojang's version
  metadata).
- A new disposable flat world and a new temporary server directory.
- The server bound only to `127.0.0.1` and was stopped after the check.
- The existing user EULA acceptance file was copied byte-for-byte. This check
  did not accept the EULA, download anything, or modify an existing world.
- No server JAR, game asset or EULA file is committed to this repository.

## Test A — build and export from a FuryBuilder world

The `world-io` example built a 5×4×5 test structure in the authoritative
`World` (stone-brick floor and pillars, oak-plank wall, oak stairs), captured
the bounds with `prepare_export`, verified the serialized bytes by reading them
back and re-encoding them, and wrote a 365-byte `.nbt`.

```powershell
cargo run -p furybuilder-world-io --example export_demo -- "$env:TEMP\furybuilder-demo.nbt"
python scripts/verify_minecraft_structure.py `
  --server-jar <official-server.jar> --java <java.exe> --eula <accepted-eula.txt> `
  --structure "$env:TEMP\furybuilder-demo.nbt" --origin 0 64 0 `
  --expected-server-sha1 4707d00eb834b446575d89a61a11b5d548d8c001 `
  --check "0,64,0=minecraft:stone_bricks" `
  --check "0,67,0=minecraft:stone_bricks" `
  --check "4,67,4=minecraft:stone_bricks" `
  --check "1,65,0=minecraft:oak_planks" `
  --check "2,65,0=minecraft:oak_stairs[facing=south,half=top,shape=straight,waterlogged=false]" `
  --check "2,65,2=minecraft:air"
```

Observed server lines:

```text
Loaded template "furybuilder:probe" at 0, 64, 0
FURY_CHECK_OK_0
FURY_CHECK_OK_1
FURY_CHECK_OK_2
FURY_CHECK_OK_3
FURY_CHECK_OK_4
FURY_CHECK_OK_5
SUCCESS: official server loaded furybuilder-demo.nbt (6 exact block check(s))
```

Structure SHA-256: `2c4b9041f89e14e953e4bca3d5edb5c341d6a5528e7bf4c02d6f1000fbd4e580`.

## Test B — Tauri IPC import and export

A real Tauri WebView2 process received `structure_import_preview` and
`structure_import_project` through its IPC bridge. The input was the official
server-saved `plain.nbt` fixture (211 bytes, SHA-256
`35a3f89de502cd9a6f1cc1b7496d4aebbfc336625389ef50dc5d22932849616b`).
The created project had its original bytes in `source/files/plain.nbt` with the
same SHA-256. A second `structure_import_preview` of `chest_boundary.nbt`
reported one block entity, and `structure_insert` rejected it with
`unsupported_format` instead of dropping the chest payload.

The export preview and export commands then produced a 230-byte file with
SHA-256 `fb6191c61aeecd9f2e0026f88ea9734496f32610b935f000db8c16d8e7165329`
and `verified: true`. The official server loaded it at `0 64 0` and these two
checks passed:

```text
0,64,0 = minecraft:stone_bricks
0,65,0 = minecraft:oak_stairs[facing=east,half=bottom,shape=straight,waterlogged=false]
```

A subsequent `structure_insert` of the same fixture changed two blocks,
advanced the world to revision 3 and left the project dirty, as expected.
The temporary app-data projects created by this probe were removed afterwards;
no user project was opened or changed.

## What this establishes and what it does not

Established:

- Official Minecraft 1.21.4 can load the adapter's output.
- Local structure coordinates, palette indices, block names, properties and
  stair orientation survive the round trip for the checked states.
- A game-saved structure can pass strict import and an unsupported block entity
  is refused rather than silently lost.
- Export verification is real: the published file is read back and compared,
  not merely trusted after writing.

Not established:

- Every block state in Minecraft, entities, block entities, biomes, tiles or
  full Java worlds.
- That every Minecraft version or modded server will accept the file.
- A full manual UI click-through on another machine; the Tauri check invoked the
  same IPC commands that the UI calls, and the UI contract has unit tests.
- Sponge `.schem` support. It remains the next format (ADR-007).

The reproducible utility is
[`scripts/verify_minecraft_structure.py`](../../scripts/verify_minecraft_structure.py).
It refuses a supplied EULA that does not already record acceptance.
