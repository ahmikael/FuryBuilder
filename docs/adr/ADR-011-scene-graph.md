# ADR-011: Scene Graph, parametric objects and voxel core boundary

- **Status:** Accepted
- **Date:** 2026-11-17

## Decision

- Block states in `world-domain` are the primary, authoritative representation.
  Parametric objects are an **optional overlay layer** with their own IDs, transforms,
  parameters, generator version and dependency graph.
- A parametric object compiles deterministically into voxel changes through the Operation
  API. Object state lives in project metadata, never in Minecraft NBT.
- Compiled results are cached as voxel overlays; changing parameters recomputes only the
  affected region and marks affected chunks dirty.
- Manual edits on top of an object are tracked. On rebuild the user must choose:
  `override` (keep manual block), `detach/bake` (object becomes plain blocks), or
  `rebuild` (discard manual edits after explicit confirmation). Silent destruction is
  forbidden.
- Moving/rotating an object rotates **block states** (stairs, doors, logs) as part of the
  generator output, not only geometry.
- Export bakes objects into block states; native history, hierarchy and generator
  metadata stay in the `.furybuilder` project.

## Consequences

- `world-domain` does not depend on scene graph code; parametric objects live in
  `world-application` (or a dedicated crate) and call operations.
- Undo works on the resulting voxel transaction plus object metadata change.
- Imported plain builds never become parametric automatically.
