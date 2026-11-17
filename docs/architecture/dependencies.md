# Rust crate dependency policy

Status: initial M0 selection. Versions are pinned by `Cargo.lock` in each build.

| Crate | Purpose | License | Notes |
|---|---|---|---|
| `serde` + `serde_json` | Contracts/serialization | MIT/Apache-2.0 | Required |
| `thiserror` | Structured error enums | MIT/Apache-2.0 | Library errors |
| `uuid` | Operation/project IDs | MIT/Apache-2.0 | v4 + serde |
| `flate2` | gzip/zlib for NBT/schem | MIT/Apache-2.0 | Pure-rust backend ok |
| `sha1` | Mojang server.jar checksum | MIT/Apache-2.0 | Download verification |
| `sha2` | Adoptium SHA-256 | MIT/Apache-2.0 | Download verification |
| `tempfile` | Atomic writes/tests | MIT/Apache-2.0 | Dev/infra |
| `fontdue`/`resvg`? | Not selected | — | Renderer uses browser fonts/canvas |
| `fastnbt` | Candidate for NBT | MIT/Apache-2.0 | Evaluate API + round-trip before adoption |
| `fastanvil` | Candidate for region read | MIT/Apache-2.0 | Read-only or low-level use; never trusted for full write |

## Rules

- No crate enters a domain crate if it pulls in Tauri, async runtimes or filesystem policy
  that belongs to an adapter.
- Every new dependency is checked for license compatibility (MIT-compatible) and
  security advisories before merge.
- NBT is implemented/verified against our own round-trip fixtures first; a third-party
  parser is adopted only if it preserves unknown tags or is used strictly read-only.
- Generated types must be versioned; no duplicated hand-maintained Rust/TS contracts.
