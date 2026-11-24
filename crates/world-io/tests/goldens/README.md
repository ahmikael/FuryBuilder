# Classic Anvil 1.21.4 export baselines

The `classic-anvil-*.sha256` manifests were captured from the synthetic world in
`tests/world_export.rs` against `9706fe1`, before `ClassicAnvilCodec` was extracted.
The fixture edits one block and biome in Overworld chunk `(0, 0)` and exports
under both `Preserve` and `VanillaRebuild`. Each manifest covers every output
file, including the other two dimensions, sidecars, and unknown files. Only the
edited chunk's runtime timestamp is zeroed before hashing; the rest of the
region header, compressed payloads, and untouched records are checked as written.
