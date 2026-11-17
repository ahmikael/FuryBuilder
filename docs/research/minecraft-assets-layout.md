# Minecraft Launcher / client.jar / asset layout research

Status: researched 2026-11-17 for Windows-first scope. Layouts are detected, not guessed.

## Official launcher (Windows)

Root: `%APPDATA%\.minecraft` (Roaming). Key entries:

```text
.minecraft/
├── launcher_profiles.json        # profile metadata (no tokens are read)
├── versions/
│   └── <version-id>/
│       ├── <version-id>.jar      # client jar (the file commonly called client.jar)
│       └── <version-id>.json     # version metadata, asset index id, libraries
├── assets/
│   ├── indexes/<index-id>.json   # object hash -> logical path map
│   └── objects/<xx>/<hash>       # content-addressed asset objects
├── libraries/                    # launcher libraries (not needed for block assets)
├── resourcepacks/                # user packs (.zip or folders)
└── saves/                        # worlds
```

- The file users call `client.jar` is normally `versions/<id>/<id>.jar`. A renamed or
  copied JAR must be validated by inspecting its version metadata, not by filename.
- Asset objects use the modern hashed layout. Legacy `assets/minecraft/...` layouts are
  only relevant for old versions and are not a 1.21.4 requirement.
- `launcher_accounts.json` contains credentials; FuryBuilder must never read or log it.
- Custom launchers and non-standard paths are supported via manual selection.

## Detection strategy

1. Probe the standard root(s) for the current OS.
2. Enumerate `versions/*`; parse each `<id>.json` for `id`, `assetIndex.id`,
   `javaVersion.majorVersion`.
3. Require an exact match with the project's target version (1.21.4). Other versions are
   listed but marked incompatible.
4. Verify the JAR contains expected namespaces (e.g. `assets/minecraft/blockstates/` for
   older layouts) and matches the version metadata; otherwise reject with a reason.
5. Allow a manual JAR/folder path and a resource-pack stack; show detection status.

## Resource pack stack

- A pack is `.zip` or a directory containing `pack.mcmeta` and `assets/`.
- Packs are ordered by priority; later entries override earlier ones.
- `pack_format` compatibility is checked (1.21.4 uses resource pack format 46); a
  mismatch yields a warning and partial support, never a crash.
- ZIP handling enforces entry count/size limits, normalized paths, no traversal, no
  absolute paths, no symlinks, and image dimension limits.

## Security/legal notes

- Discovery never reads account files or uploads anything.
- No Mojang assets are copied into projects or published.
- If no installation is found, placeholders and user packs are used; block data is
  unaffected.
