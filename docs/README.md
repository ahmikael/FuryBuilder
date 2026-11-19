# FuryBuilder documentation

Start with the guide that matches your task. The [master plan](../FURYBUILDER_MASTER_PLAN.md)
is the authoritative roadmap; a checked item is not, by itself, a guarantee that a whole
milestone is complete.

## User guides

- [Windows installation and updates](install-windows.md) — packages, requirements,
  install/update/uninstall, user-data location and unsigned-beta warnings.
- [Getting started](getting-started.md) — install, create a project, learn the window,
  edit blocks, save/recover and export a structure.
- [Advanced workflows](advanced-workflow.md) — precise area editing, landscape tools,
  layers/masks, templates, semantic and parametric objects, Build DSL, observation,
  validation, MCP, performance and deliberate boundaries.
- [Known limitations](known-limitations.md) — the maintained example-backed list of what is
  unsupported, partial, environment-specific or pending release work.
- [MCP setup](mcp-setup.md) — connect an AI client, access modes, confirmations, common
  errors and how to turn access off.

## Architecture and design

- [Architecture decisions (ADR)](adr/README.md) — Java target, project format, meshing,
  assets, MCP runtime, renderer, formats, history, deployment, scene graph, Build DSL,
  compatibility, creation modes, dependencies and the core boundary.
- [Dependency architecture](architecture/dependencies.md)
- [World API contracts](api/world-api.md), [observation API](api/observation.md) and
  [IPC/contract notes](api/contracts.md)
- [Editor and tools](design/editor.md)
- [Project storage](design/projects.md)
- [Renderer](design/renderer.md)
- [Camera controls](design/camera-controls.md)
- [Design system](design/design-system.md)
- [Assets](design/assets.md)
- [AI workflows](design/ai-workflows.md)
- [Security boundaries](design/security.md)
- [Diagnostics](design/diagnostics.md)
- [MVP scenarios](design/mvp-scenarios.md)

## Formats

- [Format matrix](formats/format-matrix.md) — read/write/passthrough support by format.
- [Support matrix](formats/support-matrix.md) — version and data-category status.
- [Java structure `.nbt`](formats/java-structure-nbt.md)
- [Java world layouts and DataVersions](formats/java-world-layouts.md)
- [Java world selection rules](formats/java-world-selection.md)
- [Java world export](formats/java-world-export.md)
- [Java chunk derived data](formats/java-chunk-derived-data.md)
- [Java version risks](formats/java-version-risks.md)

## Assets, Java and generation

- [Minecraft assets](assets/minecraft-assets.md) — launcher discovery, verification,
  resource packs, render bundles and cache limits.
- [Java runtime](runtime/java-runtime.md)
- [Official server JAR](runtime/minecraft-server-jar.md)
- [Vanilla World generation](runtime/vanilla-generation.md)
- [Dependency sources research](research/dependency-sources.md)
- [MCP 2026-07-28 research](research/mcp-2026-07-28.md)
- [Reference overlay research](research/reference-overlays.md) — design only; the editor
  overlay is not enabled.

## Testing and evidence

- [Test matrix](testing/test-matrix.md) — requirement-to-evidence coverage.
- [Performance runs](testing/perf-runs.md) — CPU/GPU/heap profiles, budgets and the tested
  large-map envelope.
- [MCP compatibility](testing/mcp-compatibility.md)
- [Worldgen parity](testing/worldgen-parity.md)
- [Official server fixture](testing/minecraft-server-fixture.md)
- [Minecraft structure checks](testing/minecraft-structure-check.md)
- [Java 1.21.4 reference](testing/java-1.21.4-reference.md)
- [Evidence archive](testing/evidence) — recorded runs, screenshots and JSON metrics.

## Legal

- [Third-party notices](legal/third-party-notices.md) — FuryBuilder code is MIT; Mojang
  assets and JARs are not distributed with this repository.
