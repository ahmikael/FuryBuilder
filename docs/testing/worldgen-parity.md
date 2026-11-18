# Vanilla worldgen research and reference-chunk parity

Status: **the M9.8 reference-chunk check is implemented and was run against real official
1.21.4 server output. It verifies block states in the requested overworld area; it does not
establish biome parity or general vanilla parity.**

The original-server bridge, isolated generation process and World Core import described
below now exist (M9.5-M9.7). The remaining scope is the bounded comparison documented in
[Implemented M9.8](#implemented-m98-reference-chunk-check).

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

## Independent test oracle

The Python oracle is retained as an independent, file-based cross-check. It is not used by
the application: M9.8 uses the native Rust comparator in
`crates/world-io/src/parity.rs`, which follows the same principles (expand palettes,
compare exact block states, bound the output) without serializing whole chunks to JSON.

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

## Implemented M9.8 reference-chunk check

`world_io::parity` compares a validated reference world with the editable
`world_domain::World`:

- `reference_chunks(center, radius)` returns all requested chunks, or a deterministic
  evenly spaced subset capped at 64. The center chunk is always included.
- `compare_java_world_to_world` opens the reference read-only, decodes every sampled
  chunk, and refuses missing chunks or chunks whose status is not `minecraft:full`.
- The desktop command `project_verify_vanilla_parity` first requires a `Vanilla World`
  project with an immutable generated source and a saved revision of exactly 1. It then
  requires the project provenance to match the completed run on seed, preset,
  center/radius, structures flag, ordered data-pack name/SHA-256/size/format, Java version,
  official server SHA-1, generated chunk count, region count and world bytes. Any mismatch
  is a structured refusal; no comparison is attempted.
- Only sections that are non-empty on at least one side are visited. Every compared
  section accounts for all 4096 block positions, missing sections read as air, and up to
  20 differences are returned with absolute coordinates, reference and project states. The
  hard budget is 50,000,000 block positions per check.
- Unsupported reference versions/layouts and incomplete generator output are refused via the
  normal read-only `analyze_java_world` blockers. If the original run was already cleared,
  the project's immutable byte-for-byte source copy is used and the report says so.
- The UI panel `Generated base verification` displays the scope, counts, bounded
  differences, refusals and limitations. It never reports a match for an edited base.

### Real result and evidence

`docs/testing/evidence/2026-11-19-vanilla-parity-ui.json` and the two screenshots record a
real WebView2 flow:

- official 1.21.4 server generated 9/9 requested chunks with seed `FuryBuilderM96UI`,
  preset `normal`, center `(4500, 4500)` and radius 1;
- the completed run was imported as a Vanilla project and the saved revision-1 world
  matched the reference chunks exactly: 9 chunks, 77 non-empty sections, 315,392 block
  positions, 0 differences;
- the UI showed `Block parity matched` with the same counts;
- a second completed run with seed `FuryBuilderM96Pack`, center `(5000, 5000)`,
  `generate-structures=false` and one data pack was refused because seed, center,
  structures, data-pack count and generated bytes differed;
- the backend recorded 5 native comparator tests (palette-aware equality, mismatch
  coordinates, missing/extra sections, bounded details and deterministic sampling).

This establishes parity between the original generator output and the FuryBuilder project
representation **for this configuration and sampled area**. It is not a universal parity
claim, and it says nothing about biomes or derived data.

## Remaining parity work

- Biomes and derived data (heightmaps, lighting, entities, block entities, POI) are not
  modelled by the editable world, so only their preserved source copy is verified. Do not
  claim biome parity until the world model and comparator cover it.
- Run an independent second generation with the same configuration and compare it with the
  first, to test generator determinism across separate runs and generation orders.
- Extend real runs to negative coordinates, region/chunk boundaries, multiple biomes and
  every supported preset/data-pack combination. The comparator already has negative-
  coordinate unit coverage, but that is not a real server result.
- Wire the Python oracle to emitted JSON samples if a fully independent implementation
  cross-check is required; the runtime check intentionally compares native structures.
- Never infer parity from matching seeds alone, and record failures as well as successes.

[mojang]: https://piston-meta.mojang.com/v1/packages/a3bcba436caa849622fd7e1e5b89489ed6c9ac63/1.21.4.json
[setup]: https://help.minecraft.net/hc/en-us/articles/360058525452-How-to-Set-Up-a-Minecraft-Server
[eula]: https://www.minecraft.net/en-us/eula
[usage]: https://www.minecraft.net/en-us/usage-guidelines
[adoptium]: https://adoptium.net/installation/ci-scripts/
[forceload]: https://minecraft.wiki/w/Commands/forceload
[jobs]: https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects
