# ADR-010: Deployment target

- **Status:** Accepted
- **Date:** 2026-11-17

## Decision

- Ship a **Windows desktop first** (Tauri 2, MSI/NSIS installer, WebView2 runtime check).
- macOS/Linux are built and tested only after Windows stability; WebView differences are
  treated as a separate compatibility track.
- A browser/WASM build is a future product decision, not an MVP deliverable. Domain
  crates stay free of Tauri dependencies so this remains feasible.

## Consequences

- CI builds Windows; cross-platform jobs are added later.
- Native dialogs, file paths and process management are abstracted behind ports.
- Release artifacts are versioned and checked; no Minecraft binaries are bundled.
