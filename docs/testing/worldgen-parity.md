# Vanilla worldgen research and parity test

Status: **test oracle implemented; real FuryBuilder-versus-Minecraft parity
not run**. This research does not implement the M9 generator, process isolation,
or a full-world importer.

## Sources and legal boundary

- Mojang's [1.21.4 version metadata][mojang] identifies Java major **21**, and
  identifies the official server as 56,880,250 bytes with SHA-1
  `4707d00eb834b446575d89a61a11b5d548d8c001`. Metadata was read; the server JAR
  was not downloaded or executed for this research. Resolve the version from
  Mojang's manifest and verify the downloaded bytes before use.
- [Minecraft's server setup guidance][setup] describes running the server and
  the user's EULA decision. [Minecraft EULA][eula] and [Usage Guidelines][usage]
  control actual use; review the current terms before distributing or enabling
  the feature. The official legal pages timed out when directly fetched during
  this research, although their search-indexed text was available. This is not
  legal advice or a grant to redistribute game files.
- [Adoptium's documentation][adoptium] describes the API, SHA-256 and optional
  GPG verification for Eclipse Temurin. A managed runtime must be an explicit
  opt-in, not a global installation or silent download.

The user must independently accept the Minecraft EULA. No test or tool in this
repository writes `eula=true`. No proprietary server, client, textures or
generated world are bundled with FuryBuilder. Reference data must be
self-created or otherwise used with adequate rights, and separately reviewed
before redistribution.

## Proposed controlled generation procedure

1. Record the exact game/runtime versions and checksum of the official JAR.
   Establish explicit user consent and independently provided EULA acceptance.
   Create a fresh, isolated working directory, not a user's existing world.
2. Record seed, `level-type` preset, `generator-settings`, structures flag,
   ordered enabled/disabled datapacks and their content hashes. These settings
   apply at world creation; changing `server.properties` after creating a world
   is not equivalent to generating a new world.
3. Request a small, bounded chunk sequence. The
   [`/forceload` documentation][forceload] specifies block-coordinate arguments
   and a maximum of 256 chunks per request; since 1.17 load is asynchronous.
   Request completion is not proof that all chunk generation is complete.
   Spawn chunks and neighbours may also be generated. A real bridge must bound
   the total resources and verify full chunk status, not just command success.
4. Freeze or otherwise control tick-dependent changes as appropriate for the
   tested version, wait for completion, `save-all flush`, stop the process and
   check its exit status. Retain reproducibility metadata and compare immutable
   copies only. Exact command sequencing remains to be validated with 1.21.4.
5. The eventual Windows bridge must enforce loopback-only or disabled networking,
   CPU/memory/time limits, cancellation and process-tree cleanup. Windows
   [Job Objects][jobs] are a possible process-lifetime mechanism, but do not
   alone provide network isolation or sandbox untrusted datapacks. Fail closed
   if isolation or completion cannot be established.

These are acceptance requirements, not claims that the current app already
implements them.

## Test oracle

`scripts/compare_worldgen_samples.py` compares two **independently produced**
UTF-8 JSON snapshots. Each has `schema_version: 1`, a non-empty `producer`,
`config`, and `chunks`. Both configs must agree exactly on:

`minecraft_version`, `data_version`, `server_sha1`, signed integer `seed`,
namespaced `preset`, `generator_settings`, `generate_structures`, ordered
`datapacks` (`id` and SHA-256 for each additional pack), `generation_order`
(ordered `[chunkX, chunkZ]` requests), `dimension`, `min_y` and `height`.
`server_sha1` is checked against Mojang's selected 1.21.4 metadata; a claim in
a JSON document alone does **not** prove which JAR actually ran. Input
provenance must be validated independently.

Every listed chunk has `x`, `z` and `sections`; the comparison rejects missing
chunks and sections. Every section, even an all-air section, has:

- `y`: signed section coordinate.
- `block_palette`: entries `{ "name": "minecraft:…", "properties": {…} }`.
- `block_indices`: exactly 4096 palette indices ordered by
  `x + 16*z + 256*y`, each local axis 0..15.
- `biome_palette`: namespaced biome IDs.
- `biome_indices`: exactly 64 indices ordered by `x + 4*z + 16*y`, each local
  quart axis 0..3.

The oracle expands palettes before comparison, so different palette ordering
does not affect the result. It compares all sampled block-state IDs, properties
and biome IDs, rejects malformed or incomplete samples and reports up to 20
differences. It checks full vanilla dimension height for the supported three
dimensions and bounds input to 16 chunks and 128 MiB per file. It does not
compare entities, lighting, ticks, structures or full-world metadata.

```powershell
python scripts/compare_worldgen_samples.py <reference.json> <candidate.json>
python -m unittest discover -s scripts -p "test_compare_worldgen_samples.py" -v
```

Exit codes: `0` sampled blocks and biomes match; `1` sampled content differs;
`2` input is invalid or settings do not match. A match is limited to the sampled
chunks and configuration; it is not a proof of general parity.

The unit tests exercise the oracle with **synthetic** data: equal states with
different palette ordering, changed orientations/biomes, different seed/preset,
wrong server identity, incomplete chunks and malformed JSON. They are not a
Minecraft-generated golden and do not constitute the required real parity run.

## Outstanding before a real run

- Implement the original-server bridge and validate its isolation and lifecycle.
- Implement independent, versioned extraction from the server output and the
  FuryBuilder world; the JSON contract above is not yet produced by either.
- Obtain appropriately licensed reference data, consent and EULA acceptance;
  pin provenance, configuration, chunk completion status and hashes.
- Run the comparison for positive and negative coordinates, chunk boundaries,
  multiple biomes and relevant generation orders. Record failures as well as
  successes; never infer parity from matching seeds alone.

[mojang]: https://piston-meta.mojang.com/v1/packages/a3bcba436caa849622fd7e1e5b89489ed6c9ac63/1.21.4.json
[setup]: https://help.minecraft.net/hc/en-us/articles/360058525452-How-to-Set-Up-a-Minecraft-Server
[eula]: https://www.minecraft.net/en-us/eula
[usage]: https://www.minecraft.net/en-us/usage-guidelines
[adoptium]: https://adoptium.net/installation/ci-scripts/
[forceload]: https://minecraft.wiki/w/Commands/forceload
[jobs]: https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects
