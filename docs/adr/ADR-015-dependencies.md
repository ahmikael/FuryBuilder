# ADR-015: Dependency provisioning (Java and official server)

- **Status:** Accepted
- **Date:** 2026-11-17

## Decision

### Discovery first

1. Check FuryBuilder's managed cache for a verified runtime/JAR matching the project.
2. Check `JAVA_HOME`, `PATH` and a user-selected executable/JAR. Run a real probe
   (`java -version` plus a controlled launch check), not a string match alone.
3. If none is usable, offer an explicit download.

### Managed downloads (opt-in only)

- **Java:** Eclipse Temurin JRE/JDK 21 from the official Adoptium API, platform-specific
  archive, SHA-256 verified, extracted into the FuryBuilder data directory. No admin
  rights, no global PATH changes.
- **Server:** `server.jar` for exactly 1.21.4 resolved via Mojang
  `version_manifest_v2.json` → version JSON → `downloads.server.url` + `sha1`.
  Download into a versioned cache, verify SHA-1, atomically publish.
- Downloads are HTTPS-only from pinned official hosts, with progress, size, source and
  cancel. Partial/corrupt files are discarded.
- Cache layout: `<app-data>/runtime/java/<vendor-version-arch>/`,
  `<app-data>/runtime/server/<minecraft-version>/`. A size limit and a clear-cache action
  exist; clearing never deletes user projects or resource packs.

### EULA and process isolation

- The user must open and accept the Minecraft EULA themselves; the acceptance record is
  stored per installation with a timestamp. No automatic `eula=true`.
- The server runs headless in a project-specific temporary directory with fixed
  world/level settings, a loopback-only or disabled network surface, resource/time limits,
  structured progress, cancel, and child-process cleanup on exit/crash.
- Failures (offline, wrong Java, bad hash, EULA declined, insufficient disk, timeout)
  produce explicit messages. **No fallback to a hand-written world generator.**

## Consequences

- `runtime-manager` is a separate crate/module with no dependency on the renderer.
- All managed binaries are optional; Empty Project never touches this path.
- Legal notices list Adoptium/Mojang sources and state that binaries are not redistributed.
