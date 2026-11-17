# Minecraft Java version and world-layout risks

Status: M0 risk register. Target 1.21.4 / DataVersion 4189.

## Known risk areas

1. **World directory layout changed across releases.** Older versions stored dimensions
   under `DIM-1/`, `DIM1/`; newer versions use `dimensions/<namespace>/<dimension>/`.
   Never assume one layout. The importer detects layout from `level.dat` + directory
   probes and records which layout was found.
2. **`DataVersion` is not a compatibility guarantee.** Two saves with the same version
   can carry different mod/custom fields. Unknown fields are passthrough or the write is
   refused.
3. **Height and Y range depend on dimension and version.** 1.21.4 overworld is
   -64..319; nether/end differ. Do not hardcode 256/384.
4. **Section palettes and packing.** 1.21.x uses `block_states.palette` + `data`
   (long array) and `biomes` similarly. Packed values may span longs; never assume one
   value per long.
5. **Block state names change between versions** (renames/additions). Porting a palette
   across versions requires explicit mapping; unsupported states must fail or be reported.
6. **Block entities, entities, ticks, POI, heightmaps and lighting** can become stale when
   blocks change. FuryBuilder must either recompute, preserve with a documented caveat, or
   have Minecraft recompute on load. Silent loss is forbidden.
7. **Region files are 32×32 chunks.** Negative region coordinates and local chunk indices
   require floor division/modulo. A single `.mca` is not a world.
8. **Compression types** in Anvil (gzip, zlib, uncompressed, LZ4, external `.mcc`) must be
   handled explicitly; unsupported types produce a diagnostic, not a crash.
9. **level.dat** carries `DataVersion` twice (root `Data.DataVersion`, `Data.Version.Id`);
   both must agree or the import is flagged.
10. **Snapshots/newer versions** (1.21.5+, 26.x) may change formats. They are read-only
    candidates until a dedicated adapter and fixtures exist.

## Mitigation

- Versioned adapters + compatibility matrix (`docs/formats/format-matrix.md`).
- Fail-closed writes for unverified combinations.
- Immutable source + overlay; export always to a new copy.
- Golden fixtures for negative coordinates, chunk borders, unknown tags and malformed data.
- Real Minecraft smoke test before claiming an export is loadable.
