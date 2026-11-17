# Registry vs asset source

The block domain and the visual asset stack are deliberately separate.

## Block registry (authoritative)

- Lives in Rust (`world_domain::BlockRegistry`) and is versioned by Minecraft version
  (`1.21.4`).
- Owns block IDs, property names and defaults. It never knows textures, colours or models.
- Imported unknown states are preserved in world data. Command writes to unknown target
  states are rejected unless explicitly allowed for import tooling.

## Asset source (derived)

- Lives in the renderer (`renderer/assets.ts` for placeholders, later
  `asset-source` adapters for a user's Minecraft installation and resource packs).
- Maps a state to a placeholder tile, render category and tint. Unknown states fall back
  to a clearly marked tile and remain full cubes.
- A missing or incompatible asset never rewrites block state data. Export uses the Rust
  model, not renderer state.

## Placeholder atlas

- 8×8 original canvas atlas (128×128 px), nearest-neighbour filtering, no Mojang assets.
- No network access and no filesystem reads occur while building it.
- Tiles are identified by stable integer constants; adding a tile is additive.
- `CanvasTexture.flipY = false` matches the top-left tile UV convention. Forgetting this
  mirrors rows into unpainted tiles and is covered by renderer smoke checks.

## Future resource packs (ADR-004)

1. Base version assets discovered from the user's installation.
2. Ordered user resource packs, validated against `pack.mcmeta` and zip-safety rules.
3. Placeholder atlas as the final fallback.

The cache key must include the Minecraft version, asset fingerprint, pack-stack hash and
renderer schema. Clearing the cache never touches projects.
