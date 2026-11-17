# ADR-007: Format scope and first export direction

- **Status:** Accepted
- **Date:** 2026-11-17

## Decision

1. **First export adapter: Java structure `.nbt`** for `DataVersion` 4189.
   It has a small, well-understood schema (size, palette, blocks, entities), is accepted
   by structure blocks/datapacks, and does not require region/level.dat handling.
2. **Second adapter: Sponge Schematic v3 `.schem`**, including the VarInt block index
   stream. It is the interoperability format for WorldEdit-style tooling.
3. **Native project format is primary**. Structure formats are export/import targets,
   never a storage backend for editor metadata.
4. **`.litematic` is P2** and legacy `.schematic` is P3. `.mcstructure` is Bedrock and is
   not part of the Java pipeline.
5. Full Anvil world write support is only enabled per verified version/field combination;
   untouched regions use byte-level passthrough.

## Rationale

`.nbt` structure and `.schem` are the smallest steps that produce a result a user can
actually load in Minecraft or a compatible editor. Attempting full world export before
structure round-trip is proven would stack unverified Anvil metadata on top of an
unverified block/state pipeline.

## Consequences

- `world-io` exposes explicit versioned adapters and a shared `Structure` model.
- Export always emits a report of supported/unsupported data.
- Import refuses unknown `DataVersion` for write, allows inspection only.
- `.nbt` export/import round-trip tests are a release gate for M6.
