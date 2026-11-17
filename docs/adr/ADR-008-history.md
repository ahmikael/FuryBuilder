# ADR-008: History model

- **Status:** Accepted
- **Date:** 2026-11-17

## Decision

- Undo/redo stores **reversible diffs of modified sections**, not full world copies.
- Each accepted transaction records: operation ID, actor, source (ui/mcp/system), world
  revision before/after, affected chunks/sections, per-section before/after payload or
  compressed diff, timestamp, parameters and linked object IDs.
- Journal files live under `changes/`; checkpoints are periodic full snapshots of touched
  sections for faster recovery.
- Disk budget is configurable; the oldest undo entries are pruned first, never checkpoints
  needed for crash recovery.
- Redo is invalidated by a new transaction after undo.
- Export is blocked while a transaction is uncommitted.

## Consequences

- Undo of a 10k-block batch remains bounded by touched sections, not by world size.
- Crash recovery replays/validates the last published checkpoint and ignores incomplete
  journal tails.
- History is project-native data and is not exported to `.nbt`/`.schem`.
