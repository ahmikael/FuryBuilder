# ADR-001: Minecraft Java target version for the MVP

- **Status:** Accepted
- **Date:** 2026-11-17
- **Deciders:** FuryBuilder maintainers

## Context

FuryBuilder must write data that a real Minecraft Java client/server can open. World and
structure layouts, block state names and `DataVersion` values change between releases.
Supporting "all versions" at once would make the first verified export impossible.

## Decision

The first target for implementation is **Minecraft Java Edition 1.21.4**. The
`DataVersion` has been checked against an external, real region file, but this
does **not** yet establish write compatibility with the game.

| Property | Value |
|---|---|
| Release date | 2024-12-03 |
| `DataVersion` | **4189** |
| Protocol version | 769 |
| Required Java | Java SE 21 |
| Resource pack format | 46 |
| Data pack format | 61 |

The published 1.21.4 region fixture from PrismarineJS has `DataVersion = 4189`
in all 341 allocated chunks. Its source, hashes, limitations and reproducible
inspection command are recorded in
[`docs/testing/java-1.21.4-reference.md`](../testing/java-1.21.4-reference.md).
The fixture's contributor reports that it was created by the official server;
that provenance has not been independently verified by FuryBuilder. The
Minecraft Wiki version table independently identifies 4189 as Java 1.21.4.

- Reading data with a *newer* `DataVersion` is **not** silently accepted: the importer
  reports a version mismatch and opens read-only or refuses the write path.
- Reading a *known older* version is allowed only through an explicit, tested adapter.
  No automatic upgrade is promised until a migration is implemented and verified.
- The target constants live in `world-domain` and are surfaced by `world_info`.
- Adding a version requires: real reference save, format notes, golden fixtures, and an
  entry in the compatibility matrix.

## Consequences

- Block catalog and NBT fixtures target 4189.
- `Vanilla World` must use a 1.21.4 server JAR or an explicitly confirmed compatible
  runtime; it may not silently substitute another version.
- Export reports always state the target version and `DataVersion`.
- Newer versions (1.21.5+, 26.x) are not verified write targets. Read-only
  support must not be claimed until an appropriate importer is implemented.
- A `DataVersion` match does not prove block-state, structure or full-world
  compatibility; those require separate format and game/editor tests.
