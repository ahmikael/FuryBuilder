# ADR-012: Build DSL, Observation and Validation

- **Status:** Accepted
- **Date:** 2026-11-17

## Decision

### Build DSL

- The DSL is a **typed JSON/AST schema**, not an interpreter and never arbitrary
  JavaScript/Python supplied by an AI.
- Input flows through: schema validation → block resolution for the target version →
  deterministic generator → bounds/budget checks → preview/diff → policy approval →
  normal world transaction.
- Every recipe has `kind`, `version`, typed parameters, an output bounds declaration and
  a block budget. Identical input + generator version produces identical output.
- Generators rotate/mirror block states correctly. Unknown/unsupported block states are
  rejected with a diagnostic, not silently dropped.
- Composition is supported (house → block → district) by generating child recipes into
  the same operation batch.
- DSL complements, never replaces, exact block-level operations.

### Observation

- Every observation output is tied to `project_id`, dimension, bounds/camera, `world_revision`,
  and reports completeness/`partial`.
- Modes: `viewport`, `top_down`, `heightmap`, `material_map`, `biome_map`,
  `cross_section`, `revision_diff`. Extended modes are added after `capture_viewport`.
- An image is never presented as an exact block query; structured data accompanies maps.

### Validation

- Findings are structured: `rule_id`, `severity`, bounds/positions, explanation,
  `world_revision`, confidence, suggested fix.
- Rules are explicit and bounded. Heuristics (support, paths) are labelled as heuristics;
  they never claim full Minecraft simulation.
- Fixes go through preview and transactions. Agents have an attempt budget.

## Consequences

- Schemas live in `packages/contracts` (TS) and are mirrored by Serde types (Rust), with
  contract tests.
- DSL execution is isolated in `world-generation`; no shell/network/file access.
- Validation never mutates the world directly.
