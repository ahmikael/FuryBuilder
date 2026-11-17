# ADR-003: Where meshing runs

- **Status:** Accepted (initial)
- **Date:** 2026-11-17

## Decision

Initial meshing runs in the **TypeScript renderer** (main thread first, then a Web Worker
pool if profiling shows need). The Rust core produces compact section snapshots and
dirty-chunk events; it does not generate triangles in the first release.

Why: the first viewport needs fast iteration and simple debugging; IPC volume for mesh
data would likely exceed the cost of local meshing for MVP-sized scenes. This choice is
revisited only with measurements (section snapshot transfer vs meshing time, worker
startup, GC pauses) recorded in `docs/testing/perf-runs.md`.

## Consequences

- Renderer receives palette + packed indices, not per-block JSON.
- Mesh building is scheduled with a frame-time budget and cancellation.
- Moving meshing to Rust remains possible behind the same snapshot contract.
