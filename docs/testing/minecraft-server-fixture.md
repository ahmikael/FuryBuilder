# Automated Minecraft server fixture (M8.12)

This is the end-to-end integration check for the supported export path. It does
not use a hand-written fixture and it does not trust FuryBuilder's own decoder as
the only witness:

1. the **official Minecraft Java 1.21.4 server** generates a fresh flat world;
2. FuryBuilder edits one block and exports a complete new world folder;
3. the **same official server** starts on the exported folder;
4. the server itself verifies the edited block and an untouched neighbor with
   `execute if block`;
5. the server stops and saves, and FuryBuilder decodes the persisted block states
   again.

Implementation: `scripts/verify_java_world_export.ps1`.

## What the script needs

- A Java executable compatible with Minecraft 1.21.4 (Java 21 was used).
- A user-provided official `server.jar` for exactly 1.21.4.
- An explicit `-AcceptMinecraftEula` switch. The script never accepts the
  Minecraft EULA on the user's behalf.
- The expected server JAR SHA-1, so a wrong or replaced JAR fails before launch.

FuryBuilder does not download, bundle or redistribute the Java runtime or the
server JAR. They stay outside the repository.

```powershell
pwsh -File scripts/verify_java_world_export.ps1 `
  -Java <java.exe> `
  -ServerJar <server.jar> `
  -ExpectedSha1 4707d00eb834b446575d89a61a11b5d548d8c001 `
  -AcceptMinecraftEula
```

Optional parameters: `-WorkRoot <empty-dir>`, `-EvidencePath <json>`,
`-StartupTimeoutSeconds 180`, `-Keep`.

The script fails closed on a non-1.21.4 server version, a JAR checksum mismatch,
a missing EULA acceptance, chunk/command errors, a missing edit marker, a stale
grass block at the edited position, an unchanged saved world, or any source-tree
hash change.

## Fixture

The generated world is deliberately small and flat:

| Property           | Value                                                |
| ------------------ | ---------------------------------------------------- |
| Generator          | `minecraft:flat`                                     |
| Layers             | bedrock, two dirt layers, grass                      |
| Structures         | disabled                                             |
| Dimensions         | Overworld only for generation (`allow-nether=false`) |
| Spawn protection   | disabled                                             |
| Network            | loopback only, random free port                      |
| Chunk under test   | `(0, 0)`, force-loaded explicitly                    |
| Edited block       | `(0, -61, 0)` grass block → `minecraft:gold_block`   |
| Untouched neighbor | `(0, -62, 0)` dirt                                   |

The run produced 21 source files, 4 region files and 841 chunks, which is the
normal spawn-area footprint of this server configuration.

## Server-side verification

The verifier server receives these console commands after `Done`:

```text
forceload add 0 0
execute if block 0 -61 0 minecraft:gold_block run say FURY_EXPORT_EDIT_OK
execute if block 0 -61 0 minecraft:grass_block run say FURY_EXPORT_STALE_GRASS
execute if block 0 -62 0 minecraft:dirt run say FURY_EXPORT_NEIGHBOR_OK
```

Expected markers:

- `FURY_EXPORT_EDIT_OK` present;
- `FURY_EXPORT_STALE_GRASS` absent;
- `FURY_EXPORT_NEIGHBOR_OK` present.

This proves that the server generated the chunk from the exported region file,
read the edited block state, and did not shift or corrupt a neighboring block.

After `stop`, FuryBuilder decodes the saved world again and confirms
`minecraft:gold_block` at `(0, -61, 0)` and `minecraft:dirt` at `(0, -62, 0)`.
The verifier server rewrote 10 files, so the save/load path really ran rather
than merely opening the folder.

## Evidence

`docs/testing/evidence/2026-11-18-server-fixture-1.21.4.json`:

| Check                                  | Result                                             |
| -------------------------------------- | -------------------------------------------------- |
| Server version                         | 1.21.4                                             |
| Server JAR SHA-1                       | `4707d00eb834b446575d89a61a11b5d548d8c001`         |
| Fixture generator exit                 | 0                                                  |
| Export policy                          | `vanillaRebuild`                                   |
| Export chunks / replaced chunks        | 841 / 1                                            |
| Export source tree unchanged           | yes                                                |
| Export output revalidated              | yes                                                |
| Verifier server exit                   | 0                                                  |
| Verifier `Done`                        | `Done (0.302s)` (the full line is in the evidence) |
| Edited block marker                    | present                                            |
| Stale grass marker                     | absent                                             |
| Untouched neighbor marker              | present                                            |
| Files rewritten by the verifier server | 10                                                 |
| Post-save decoded block                | `minecraft:gold_block`                             |
| Post-save decoded neighbor             | `minecraft:dirt`                                   |
| Chunk/command issue lines              | 0                                                  |

## Limits

- The client was not launched. A licensed Minecraft client installation was not
  available in this environment; the official dedicated server is the real
  engine check for chunk loading and block states.
- One generated flat fixture and one edited block are not a universal
  compatibility proof.
- The fixture has no block entities, entities, scheduled ticks or non-empty POI
  records.
- The server JAR and Java runtime are user-provided and are not part of the
  FuryBuilder distribution.
- The work root is deleted after a successful run unless `-Keep` is passed. The
  evidence file keeps hashes and server log excerpts.
