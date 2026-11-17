# ADR-014: Project creation modes and Vanilla World generation

- **Status:** Accepted
- **Date:** 2026-11-17

## Decision

Three modes only:

1. **Empty Project (MVP).** Sparse internal world. Optional flat base. No Java, no server,
   no Minecraft installation required.
2. **Vanilla World (M9).** Chunks produced by the **original Minecraft Java 1.21.4
   server engine** in a controlled local process. Requires exact version, seed, preset,
   structures flag and supported datapacks. No hand-written vanilla parity generator.
3. **Import World (M8).** Existing Java save read through a versioned adapter, immutable
   source copy, copy-on-write overlay, export as a new world copy.

## Vanilla World rules

- The app first looks for a compatible Java runtime and a legally obtained `server.jar`
  in its managed cache or a user-selected path.
- Missing dependencies are **offered**, never installed silently. Downloads use official
  Mojang metadata and Adoptium; checksums are verified; assets are cached by version.
- The user must accept the Minecraft EULA themselves before the first run. FuryBuilder
  never writes `eula=true` on its own.
- The planned server runs headless in an isolated temporary directory, with bounded
  CPU/RAM/time, no external listener, logs/progress/cancel, and child-process termination
  on cancel and crash. These controls are not implemented yet.
- The planned bridge requests bounded regions, but the server may also generate spawn or
  neighbouring chunks; `/forceload` is asynchronous. Resource limits must account for
  that overhead, and the bridge must verify the requested chunks have finished before
  reporting success. The result will be read through `world-io`; generated base and
  FuryBuilder overlay must stay distinct.
- If runtime, version or generator is unavailable, the operation fails with an explicit
  message. It **never** falls back to a custom noise generator.

## Parity testing

The test design and sample comparator are described in
[`docs/testing/worldgen-parity.md`](../testing/worldgen-parity.md). It requires matching
version, server identity, seed, preset, settings, ordered datapacks, dimension and
generation order, then compares all block states and biomes in the sampled chunks.
Only synthetic comparator tests have been run so far. Neither the original-server bridge
nor the production snapshot exporter exists yet: **vanilla parity has not been tested or
established**.

## Consequences

- `runtime-manager` and `asset-source` are separate modules, not part of `world-domain`.
- Empty Project remains fully functional with no Java/Minecraft present.
- The `creation_mode` field is mandatory in `project.json`.
