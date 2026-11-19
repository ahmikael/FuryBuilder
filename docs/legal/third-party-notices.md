# Third-party notices and legal boundaries

FuryBuilder code is MIT-licensed. This file tracks non-code obligations.

## Minecraft

- Minecraft is a trademark of Mojang Synergies AB. FuryBuilder is not affiliated with,
  endorsed by, or sponsored by Mojang Studios or Microsoft.
- FuryBuilder does **not** include Minecraft `client.jar`, `server.jar`, textures, sounds,
  language files, data packs or other proprietary game assets in its repository,
  installer or releases.
- Local assets may be read from a user's own installation for rendering; they remain
  subject to the Minecraft EULA and Usage Guidelines.
- The official server JAR for `Vanilla World` is downloaded only by explicit user action
  from Mojang metadata, with checksum verification, into a local cache. It is not
  redistributed.
- The Minecraft EULA is accepted by the user, never by the application.

## Runtime downloads

- Eclipse Temurin binaries are downloaded from the official Adoptium API only after user
  consent, with SHA-256 verification, into the FuryBuilder data directory. Redistribution
  follows the Adoptium license terms.

## JavaScript/Rust dependencies

- The maintained inventory and license policy result are in
  [third-party-inventory.md](third-party-inventory.md) (generated) and
  [third-party-inventory.json](third-party-inventory.json) (machine-readable).
- `pnpm licenses:update` regenerates the inventory; `pnpm licenses:check` fails CI when a
  dependency introduces a denied license or is missing license metadata.
- Allowed licenses: MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, 0BSD, Zlib, Unicode-3.0,
  CC0-1.0, MIT-0, Unlicense, MPL-2.0 and CDLA-Permissive-2.0. Copyleft (GPL/AGPL/SSPL) and
  non-commercial licenses are rejected. A dual license passes when at least one branch is
  allowed.
- MPL-2.0 components are used unmodified as dependencies; their source is available from the
  published package archives and upstream projects. FuryBuilder does not relicense them.
  Unicode-3.0 and CDLA-Permissive-2.0 components are data/license texts used under their terms.
- Placeholder textures are original works created for FuryBuilder and licensed MIT.

## Release security

- Dependency advisories and signature status for the beta are recorded in
  [security-audit.md](security-audit.md).
- The beta installers are **not code-signed**. A code-signing certificate and a protected key
  are external requirements; until they exist, releases are distributed with published
  SHA-256 hashes and Windows SmartScreen warnings are documented.
- `scripts/verify-release-artifacts.ps1` records hashes, versions and Authenticode status and
  fails on an invalid or tampered signature. With `-RequireSigned` it also fails on unsigned
  artifacts, which is the intended gate once signing is set up.

## References

- Minecraft EULA: https://www.minecraft.net/en-us/eula
- Minecraft Usage Guidelines: https://www.minecraft.net/en-us/usage-guidelines
- Adoptium: https://adoptium.net/
