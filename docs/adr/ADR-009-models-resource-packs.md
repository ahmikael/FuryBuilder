# ADR-009: Block models and resource packs

- **Status:** Accepted
- **Date:** 2026-11-17

## Decision

- MVP renders blocks from a **curated vanilla-compatible model set**: full cubes,
  cross plants, slabs, stairs, logs and a few special shapes, using placeholder textures.
- The renderer resolves `blockstates` JSON → `models` JSON → texture references for the
  supported 1.21.4 asset stack once asset discovery lands.
- Partial/custom models that cannot be represented fall back to a clearly marked
  placeholder cube; the block state data is preserved exactly.
- Resource packs form an ordered, validated stack. Invalid packs are isolated and do not
  corrupt the base stack.
- No claim of full vanilla visual parity; the UI labels approximations.

## Consequences

- Asset resolution is separate from block identity.
- Visual fallbacks never rewrite world data.
- Model/texture caches are derived and disposable.
