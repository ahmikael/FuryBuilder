# Import/export verification on the target Java server (M11.5)

FuryBuilder supports exactly one world format for writing: **Minecraft Java 1.21.4**,
classic Anvil layout, `DataVersion` 4189. This page records the release-level test of
both directions against the official 1.21.4 dedicated server.

Evidence: [`evidence/2026-11-19-java-server-interop/evidence.json`](evidence/2026-11-19-java-server-interop/evidence.json).

## What was exercised

1. The official 1.21.4 server generated a fresh fixed-seed normal overworld with
   structures disabled.
2. The real desktop app analyzed the world and imported it as an `Import` project with an
   immutable source copy.
3. The app applied an air → `minecraft:gold_block` edit at `(0, 300, 0)` through the
   normal preview/apply command path, saved the project, closed it and reopened it.
4. The backend exported a complete new world folder from the immutable source copy,
   replacing exactly one chunk and revalidating the whole output.
5. The same official server loaded the exported world and checked the edited block, an
   untouched air neighbor and a terrain block with `/execute if block`.
6. The app re-imported the server-saved world and read the persisted edited and terrain
   blocks again.

The block is at `y=300` on purpose: it is far above normal terrain, so the test exercises
creating and writing a previously empty chunk section, not just replacing a surface block.

## Recorded result

| Stage           | Result                                                                                                       |
| --------------- | ------------------------------------------------------------------------------------------------------------ |
| Fixture         | Official server 1.21.4, seed `4189`, `minecraft:normal`, structures disabled                                 |
| Fixture size    | 1,416 chunks, 4 regions, 21 files, 6,286,738 bytes                                                           |
| App import      | `Import` project, target 1.21.4 / 4189, revision 1, 1,416 chunks, 33,984 sections, 6,490,689 editable blocks |
| Source copy     | 20 files, 6,286,735 bytes; original server world unchanged                                                   |
| App edit        | 1 block changed at `(0, 300, 0)`; revision 2; save and reopen both keep the gold block                       |
| Export          | 20 files, 6,286,735 bytes, 4 regions / 1,416 chunks, 1 region / 1 chunk replaced                             |
| Export checks   | source unchanged, copied files match, output revalidated, `VanillaRebuild` policy                            |
| Verifier server | 1.21.4, `Done (1.964s)`, 11 files rewritten, no chunk/command issue lines                                    |
| Server markers  | gold block present, untouched neighbor air present, bedrock present, stale-air marker absent                 |
| App re-import   | 1,416 chunks, gold block at `(0, 300, 0)`, bedrock at `(0, -64, 0)`                                          |
| App console     | no errors                                                                                                    |

The verifier logged one non-fatal Yggdrasil public-key fetch error because this machine
had no network access and no player joined. It did not affect chunk loading, saving or
the `/execute if block` checks; the evidence keeps that line instead of hiding it.

## Reproduce

The debug desktop app, the world-io examples and the Vite dev server must be ready:

```powershell
pnpm dev
cargo build -p furybuilder-world-io --examples
node scripts/java-server-interop-webview2.mjs <new-output-directory> `
  <java.exe> <server.jar> 4707d00eb834b446575d89a61a11b5d548d8c001 `
  --accept-minecraft-eula
```

The server JAR and Java runtime are user-provided. FuryBuilder does not download, bundle
or redistribute them. The harness fails closed on a SHA-1 mismatch, a non-1.21.4 server,
a missing EULA flag, a failed app import/edit/save, an unchanged export source, missing
server markers or a chunk/command issue line.

## Limits

- Only the supported 1.21.4 classic format is claimed; no other version is tested here.
- The official dedicated server is the engine check. A licensed Minecraft client was not
  launched in this environment.
- One fixed-seed normal world and one edited block are evidence, not a universal
  compatibility proof.
- Block entities, entities, scheduled ticks and POI are preserved in the immutable source
  copy but were not edited or semantically validated by this run.
- The complete world-folder export is a backend path ([KL-10](../known-limitations.md));
  the beta UI exposes structure `.nbt` export, not a one-click world copy.
- The generated/exported worlds and full server logs stay in the harness output and are
  not committed.
