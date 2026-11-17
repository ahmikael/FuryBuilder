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

- Dependencies must use MIT-compatible licenses. A generated notice list is part of the
  release checklist.
- Placeholder textures are original works created for FuryBuilder and licensed MIT.

## References

- Minecraft EULA: https://www.minecraft.net/en-us/eula
- Minecraft Usage Guidelines: https://www.minecraft.net/en-us/usage-guidelines
- Adoptium: https://adoptium.net/
