# Dependency sources: Mojang manifest and Adoptium API

Status: researched 2026-11-17. These endpoints are used only after explicit user consent.

## Mojang version metadata

1. `https://piston-meta.mojang.com/mc/game/version_manifest_v2.json`
   - lists versions with `id`, `type`, `url`, `sha1`, `releaseTime`.
2. The per-version JSON (URL from `url`) contains:
   - `id`, `type`, `releaseTime`
   - `javaVersion`: `{ component, majorVersion }` (1.21.4 → Java 21)
   - `downloads.server`: `{ sha1, size, url }` (official server JAR)
   - `downloads.client`: `{ sha1, size, url }`
   - `assetIndex`: `{ id, sha1, size, totalSize, url }`
3. Download rules:
   - Exact version id `1.21.4`, server download only from the URL in this metadata.
   - Verify declared **SHA-1** and size after download; reject mismatches.
   - HTTPS only; no mirrors or third-party JARs.
   - Store under `runtime/server/1.21.4/server.jar` with metadata and verification record.

## Adoptium (Eclipse Temurin)

- Latest-release API family:
  `https://api.adoptium.net/v3/assets/latest/{feature_version}/hotspot?architecture=x64&image_type=jre&os=windows&vendor=eclipse`
- Response entries include `release_name`, `version`, `binary.package.link/name/checksum`
  (SHA-256) and a detached `.sig` for GPG verification where supported.
- Selection: Java **21** feature version, x64, JRE for normal use (JDK if the user chooses).
- Verify **SHA-256** and optionally the GPG signature before extraction.
- Extract into `runtime/java/<release_name>/` under the FuryBuilder data directory.
  No administrator rights, no global PATH modification, no silent install.

## Failure behavior

- Offline / HTTP error / hash mismatch / disk full / extraction failure → structured
  error with an actionable message and no partially published runtime.
- A downloaded dependency can be deleted from the cache without affecting projects.
- EULA acceptance is a separate, recorded user action; no auto-accept.

## Legal

Both Mojang and Adoptium files remain under their own licenses. FuryBuilder does not
redistribute them. See `docs/legal/third-party-notices.md` and ADR-015.
