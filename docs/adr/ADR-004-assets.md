# ADR-004: Minecraft assets, textures and resource packs

- **Status:** Accepted
- **Date:** 2026-11-17

## Decision

Sources are resolved in this order:

1. **Automatic discovery** of the official launcher installation
   (`%APPDATA%\.minecraft`): installed versions, matching `client.jar`, asset index and
   asset objects. The JAR is accepted only if its version metadata matches the project's
   `target_minecraft_version`.
2. **Manual selection** in Settings → Minecraft Assets: `.minecraft` folder, client JAR,
   or a local resource pack (`.zip`/folder). The UI shows the detected version and any
   incompatibility reason.
3. **Built-in placeholders**: original, openly licensed textures generated for
   FuryBuilder. Missing vanilla assets never change block data.

Resource packs are applied as an ordered stack over the base assets:
base version assets → user packs in priority order. `pack.mcmeta` pack format is checked;
partial compatibility is reported. Derivative atlases are cached with the key
`(minecraft_version, asset_fingerprint, pack_stack_hash, renderer_schema)`, and can be
cleared safely.

Zip/resource pack inputs are untrusted: path traversal, absolute paths, symlinks,
zip bombs, oversized images and malformed JSON/models are rejected within limits.
No Mojang JARs or textures are committed, redistributed or bundled; documentation
records the legal constraints. Download of official client files is out of scope unless
a future legal review explicitly approves it.

## Consequences

- `asset-source` is a separate Rust module plus a renderer adapter.
- Unknown block states fall back visually but keep their exact data.
- Empty Project remains usable with placeholders and user packs only.
