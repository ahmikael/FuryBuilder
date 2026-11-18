//! World operations: selections, fills, replace, batched edits and section diffs.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use world_domain::{
    BlockBox, BlockPos, BlockState, BoundsError, ChunkPos, Section, World, WorldError,
};

pub const MAX_OPERATION_BLOCKS: u64 = 1_000_000;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum OperationError {
    #[error("unknown dimension `{0}`")]
    UnknownDimension(String),
    #[error("selection bounds are invalid: {0}")]
    Bounds(#[from] BoundsError),
    #[error("operation affects {requested} blocks, limit is {limit}")]
    TooManyBlocks { requested: u64, limit: u64 },
    #[error("operation at {dimension} {pos:?} is outside the world: {source}")]
    OutOfBounds {
        dimension: String,
        pos: BlockPos,
        #[source]
        source: WorldError,
    },
    #[error("batch operation is empty")]
    EmptyBatch,
    #[error("operation was cancelled before it could be committed")]
    Cancelled,
    #[error(transparent)]
    World(#[from] WorldError),
}

/// Phase reported by a cancellable operation. The application layer maps these phases to
/// a 0–100 job percentage; the domain keeps them separate so planning and writing are not
/// confused with each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationPhase {
    Validating,
    Planning,
    Applying,
}

/// Receives progress from a long operation and decides whether it should continue.
/// Implementations must be cheap: `progress` is called for every bounded batch of work.
pub trait OperationObserver {
    fn progress(&self, phase: OperationPhase, processed: u64, total: u64);
    fn is_cancelled(&self) -> bool;
}

/// Default observer for synchronous operations.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopObserver;

impl OperationObserver for NoopObserver {
    fn progress(&self, _phase: OperationPhase, _processed: u64, _total: u64) {}
    fn is_cancelled(&self) -> bool {
        false
    }
}

fn report(
    observer: &dyn OperationObserver,
    phase: OperationPhase,
    processed: u64,
    total: u64,
) -> Result<(), OperationError> {
    observer.progress(phase, processed, total);
    if observer.is_cancelled() {
        return Err(OperationError::Cancelled);
    }
    Ok(())
}

/// A dimension-scoped region selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Selection {
    pub dimension: String,
    pub bounds: BlockBox,
}

impl Selection {
    pub fn new(dimension: impl Into<String>, bounds: BlockBox) -> Result<Self, OperationError> {
        if bounds.volume() > MAX_OPERATION_BLOCKS {
            return Err(OperationError::TooManyBlocks {
                requested: bounds.volume(),
                limit: MAX_OPERATION_BLOCKS,
            });
        }
        Ok(Self {
            dimension: dimension.into(),
            bounds,
        })
    }

    pub fn volume(&self) -> u64 {
        self.bounds.volume()
    }

    pub fn contains(&self, pos: BlockPos) -> bool {
        self.bounds.contains(pos)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockChange {
    pub pos: BlockPos,
    pub state: BlockState,
}

/// Typed operations accepted by the command API. This enum is versioned with the API;
/// breaking shape changes require a migration note.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Operation {
    SetBlock {
        dimension: String,
        pos: BlockPos,
        state: BlockState,
    },
    Batch {
        dimension: String,
        changes: Vec<BlockChange>,
    },
    FillCuboid {
        dimension: String,
        bounds: BlockBox,
        state: BlockState,
    },
    Replace {
        dimension: String,
        bounds: BlockBox,
        from: BlockState,
        to: BlockState,
    },
}

impl Operation {
    pub fn dimension(&self) -> &str {
        match self {
            Operation::SetBlock { dimension, .. }
            | Operation::Batch { dimension, .. }
            | Operation::FillCuboid { dimension, .. }
            | Operation::Replace { dimension, .. } => dimension,
        }
    }

    /// Upper bound of blocks the operation may inspect.
    pub fn affected_blocks(&self) -> u64 {
        match self {
            Operation::SetBlock { .. } => 1,
            Operation::Batch { changes, .. } => changes.len() as u64,
            Operation::FillCuboid { bounds, .. } | Operation::Replace { bounds, .. } => {
                bounds.volume()
            }
        }
    }

    /// Exact inclusive extent of the operation, or `None` for an empty batch. Access
    /// policies use this to keep an undo inside the same region as the original edit.
    pub fn bounds(&self) -> Option<BlockBox> {
        match self {
            Operation::SetBlock { pos, .. } => Some(BlockBox::single(*pos)),
            Operation::Batch { changes, .. } => {
                let first = changes.first()?;
                let mut bounds = BlockBox::single(first.pos);
                for change in &changes[1..] {
                    bounds = bounds.union(&BlockBox::single(change.pos));
                }
                Some(bounds)
            }
            Operation::FillCuboid { bounds, .. } | Operation::Replace { bounds, .. } => {
                Some(*bounds)
            }
        }
    }

    pub fn exceeds_limit(&self) -> Option<OperationError> {
        let requested = self.affected_blocks();
        (requested > MAX_OPERATION_BLOCKS).then_some(OperationError::TooManyBlocks {
            requested,
            limit: MAX_OPERATION_BLOCKS,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub affected_blocks: u64,
    pub changed_blocks: u64,
    pub dirty_chunks: Vec<ChunkPos>,
    pub warnings: Vec<String>,
}

/// Before/after snapshot of one section touched by an operation. This is the undo unit;
/// storing section payloads avoids copying the whole world and keeps history bounded by
/// the number of touched sections.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionDiff {
    pub dimension: String,
    pub chunk: ChunkPos,
    pub section_y: i32,
    pub before: Option<Section>,
    pub after: Option<Section>,
}

impl SectionDiff {
    pub fn revert(&self, world: &mut World) -> Result<(), WorldError> {
        world.install_section(
            &self.dimension,
            self.chunk,
            self.section_y,
            self.before.clone(),
        )
    }

    pub fn reapply(&self, world: &mut World) -> Result<(), WorldError> {
        world.install_section(
            &self.dimension,
            self.chunk,
            self.section_y,
            self.after.clone(),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedOperation {
    pub dimension: String,
    pub changed_blocks: u64,
    pub dirty_chunks: Vec<ChunkPos>,
    pub diffs: Vec<SectionDiff>,
    pub warnings: Vec<String>,
}

fn prevalidate(world: &World, operation: &Operation) -> Result<(), OperationError> {
    if let Some(error) = operation.exceeds_limit() {
        return Err(error);
    }
    let dimension = world.dimension(operation.dimension())?;
    let check = |pos: BlockPos| -> Result<(), OperationError> {
        if dimension.contains_y(pos.y) {
            return Ok(());
        }
        Err(OperationError::OutOfBounds {
            dimension: operation.dimension().to_owned(),
            pos,
            source: WorldError::Dimension(world_domain::DimensionError::OutOfBounds {
                dimension: operation.dimension().to_owned(),
                y: pos.y,
                min: dimension.min_y(),
                max: dimension.max_y(),
            }),
        })
    };
    match operation {
        Operation::SetBlock { pos, .. } => check(*pos)?,
        Operation::Batch { changes, .. } => {
            for change in changes {
                check(change.pos)?;
            }
        }
        Operation::FillCuboid { bounds, .. } | Operation::Replace { bounds, .. } => {
            check(bounds.min)?;
            check(bounds.max)?;
        }
    }
    Ok(())
}

fn target_changes(
    world: &World,
    operation: &Operation,
    observer: &dyn OperationObserver,
) -> Result<Vec<BlockChange>, OperationError> {
    match operation {
        Operation::SetBlock {
            dimension,
            pos,
            state,
        } => {
            report(observer, OperationPhase::Planning, 0, 1)?;
            let current = world.get_block(dimension, *pos)?;
            report(observer, OperationPhase::Planning, 1, 1)?;
            Ok(if current == *state {
                Vec::new()
            } else {
                vec![BlockChange {
                    pos: *pos,
                    state: state.clone(),
                }]
            })
        }
        Operation::Batch { dimension, changes } => {
            if changes.is_empty() {
                return Err(OperationError::EmptyBatch);
            }
            let total = changes.len() as u64;
            report(observer, OperationPhase::Planning, 0, total)?;
            let mut deduped: BTreeMap<BlockPos, BlockState> = BTreeMap::new();
            for (index, change) in changes.iter().enumerate() {
                deduped.insert(change.pos, change.state.clone());
                let done = index as u64 + 1;
                if done == total || done % 1024 == 0 {
                    report(observer, OperationPhase::Planning, done, total)?;
                }
            }
            let mut result = Vec::new();
            for (pos, state) in deduped {
                if world.get_block(dimension, pos)? != state {
                    result.push(BlockChange { pos, state });
                }
            }
            Ok(result)
        }
        Operation::FillCuboid {
            dimension,
            bounds,
            state,
        } => {
            let total = bounds.volume().max(1);
            report(observer, OperationPhase::Planning, 0, total)?;
            let mut result = Vec::new();
            let mut done = 0;
            for pos in bounds.iter() {
                if world.get_block(dimension, pos)? != *state {
                    result.push(BlockChange {
                        pos,
                        state: state.clone(),
                    });
                }
                done += 1;
                if done == total || done % 4096 == 0 {
                    report(observer, OperationPhase::Planning, done, total)?;
                }
            }
            Ok(result)
        }
        Operation::Replace {
            dimension,
            bounds,
            from,
            to,
        } => {
            let total = bounds.volume().max(1);
            report(observer, OperationPhase::Planning, 0, total)?;
            let mut result = Vec::new();
            let mut done = 0;
            for pos in bounds.iter() {
                if world.get_block(dimension, pos)? == *from {
                    result.push(BlockChange {
                        pos,
                        state: to.clone(),
                    });
                }
                done += 1;
                if done == total || done % 4096 == 0 {
                    report(observer, OperationPhase::Planning, done, total)?;
                }
            }
            Ok(result)
        }
    }
}

fn restore_sections(
    world: &mut World,
    dimension: &str,
    before: &[((ChunkPos, i32), Option<Section>)],
) {
    for ((chunk, section_y), section) in before {
        let _ = world.install_section(dimension, *chunk, *section_y, section.clone());
    }
}

pub fn preview(world: &World, operation: &Operation) -> Result<Preview, OperationError> {
    prevalidate(world, operation)?;
    let changes = target_changes(world, operation, &NoopObserver)?;
    let mut chunks = BTreeSet::new();
    for change in &changes {
        chunks.insert(change.pos.chunk_pos());
    }
    Ok(Preview {
        affected_blocks: operation.affected_blocks(),
        changed_blocks: changes.len() as u64,
        dirty_chunks: chunks.into_iter().collect(),
        warnings: Vec::new(),
    })
}

/// Applies an operation atomically. All positions and limits are validated before the
/// first write; if a write fails unexpectedly, touched sections are restored.
pub fn apply(world: &mut World, operation: &Operation) -> Result<AppliedOperation, OperationError> {
    apply_with_observer(world, operation, &NoopObserver)
}

/// Applies an operation atomically while reporting progress and honouring cancellation.
/// Cancellation before the write phase touches nothing; cancellation during the write
/// phase restores every section captured before the first write.
pub fn apply_with_observer(
    world: &mut World,
    operation: &Operation,
    observer: &dyn OperationObserver,
) -> Result<AppliedOperation, OperationError> {
    report(observer, OperationPhase::Validating, 0, 1)?;
    prevalidate(world, operation)?;
    report(observer, OperationPhase::Validating, 1, 1)?;
    let changes = target_changes(world, operation, observer)?;
    if changes.is_empty() {
        report(observer, OperationPhase::Applying, 0, 0)?;
        return Ok(AppliedOperation {
            dimension: operation.dimension().to_owned(),
            changed_blocks: 0,
            dirty_chunks: Vec::new(),
            diffs: Vec::new(),
            warnings: Vec::new(),
        });
    }

    let mut touched: BTreeSet<(ChunkPos, i32)> = BTreeSet::new();
    for change in &changes {
        touched.insert((change.pos.chunk_pos(), change.pos.section_y()));
    }

    let before: Vec<((ChunkPos, i32), Option<Section>)> = touched
        .iter()
        .map(|(chunk, section_y)| {
            (
                (*chunk, *section_y),
                world
                    .section(operation.dimension(), *chunk, *section_y)
                    .ok()
                    .flatten()
                    .cloned(),
            )
        })
        .collect();

    let total = changes.len() as u64;
    report(observer, OperationPhase::Applying, 0, total)?;
    let mut changed_blocks = 0u64;
    let mut dirty_chunks = BTreeSet::new();
    for (index, change) in changes.iter().enumerate() {
        match world.set_block(operation.dimension(), change.pos, &change.state) {
            Ok(write) => {
                if write.changed {
                    changed_blocks += 1;
                    dirty_chunks.insert(change.pos.chunk_pos());
                }
            }
            Err(error) => {
                restore_sections(world, operation.dimension(), &before);
                return Err(OperationError::OutOfBounds {
                    dimension: operation.dimension().to_owned(),
                    pos: change.pos,
                    source: error,
                });
            }
        }
        let done = index as u64 + 1;
        if done == total || done % 1024 == 0 {
            if let Err(error) = report(observer, OperationPhase::Applying, done, total) {
                restore_sections(world, operation.dimension(), &before);
                return Err(error);
            }
        }
    }

    let mut diffs = Vec::new();
    for ((chunk, section_y), before_section) in before {
        let after_section = world
            .section(operation.dimension(), chunk, section_y)?
            .cloned();
        if before_section != after_section {
            diffs.push(SectionDiff {
                dimension: operation.dimension().to_owned(),
                chunk,
                section_y,
                before: before_section,
                after: after_section,
            });
        }
    }

    Ok(AppliedOperation {
        dimension: operation.dimension().to_owned(),
        changed_blocks,
        dirty_chunks: dirty_chunks.into_iter().collect(),
        diffs,
        warnings: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(name: &str) -> BlockState {
        name.parse().expect("valid state")
    }

    fn world_with_stone() -> World {
        let mut world = World::new_empty();
        world
            .set_block(
                "minecraft:overworld",
                BlockPos::new(0, 64, 0),
                &state("minecraft:stone"),
            )
            .unwrap();
        world
    }

    #[test]
    fn operation_bounds_cover_sparse_batches_and_single_blocks() {
        let single = Operation::SetBlock {
            dimension: "minecraft:overworld".into(),
            pos: BlockPos::new(4, 64, -2),
            state: state("minecraft:stone"),
        };
        assert_eq!(
            single.bounds(),
            Some(BlockBox::single(BlockPos::new(4, 64, -2)))
        );

        let batch = Operation::Batch {
            dimension: "minecraft:overworld".into(),
            changes: vec![
                BlockChange {
                    pos: BlockPos::new(-1, 60, 7),
                    state: state("minecraft:stone"),
                },
                BlockChange {
                    pos: BlockPos::new(5, 72, -3),
                    state: state("minecraft:glass"),
                },
            ],
        };
        assert_eq!(
            batch.bounds(),
            Some(BlockBox::new(BlockPos::new(-1, 60, -3), BlockPos::new(5, 72, 7)).unwrap())
        );
        assert_eq!(
            Operation::Batch {
                dimension: "minecraft:overworld".into(),
                changes: Vec::new(),
            }
            .bounds(),
            None
        );
    }

    #[test]
    fn preview_does_not_mutate_the_world() {
        let world = world_with_stone();
        let op = Operation::SetBlock {
            dimension: "minecraft:overworld".into(),
            pos: BlockPos::new(1, 64, 0),
            state: state("minecraft:glass"),
        };
        let preview = preview(&world, &op).unwrap();
        assert_eq!(preview.affected_blocks, 1);
        assert_eq!(preview.changed_blocks, 1);
        assert_eq!(world.block_count(), 1);
        assert_eq!(world.revision(), 0);
    }

    struct RecordingObserver {
        seen: std::sync::Mutex<Vec<(OperationPhase, u64, u64)>>,
        cancel_once_applying: bool,
    }

    impl RecordingObserver {
        fn new(cancel_once_applying: bool) -> Self {
            Self {
                seen: std::sync::Mutex::new(Vec::new()),
                cancel_once_applying,
            }
        }

        fn seen(&self) -> Vec<(OperationPhase, u64, u64)> {
            self.seen.lock().expect("observer lock").clone()
        }
    }

    impl OperationObserver for RecordingObserver {
        fn progress(&self, phase: OperationPhase, processed: u64, total: u64) {
            self.seen
                .lock()
                .expect("observer lock")
                .push((phase, processed, total));
        }

        fn is_cancelled(&self) -> bool {
            self.cancel_once_applying
                && self.seen().iter().any(|(phase, processed, _)| {
                    *phase == OperationPhase::Applying && *processed > 0
                })
        }
    }

    #[test]
    fn observer_reports_phases_and_cancellation_rolls_back_touched_sections() {
        let mut world = world_with_stone();
        let bounds = BlockBox::new(BlockPos::new(0, 64, 0), BlockPos::new(15, 79, 15)).unwrap();
        let op = Operation::FillCuboid {
            dimension: "minecraft:overworld".into(),
            bounds,
            state: state("minecraft:dirt"),
        };
        let observer = RecordingObserver::new(true);
        let result = apply_with_observer(&mut world, &op, &observer);
        assert_eq!(result.unwrap_err(), OperationError::Cancelled);
        assert_eq!(world.block_count(), 1, "cancelled write must roll back");
        assert_eq!(world.revision(), 0);

        let seen = observer.seen();
        assert!(seen
            .iter()
            .any(|(phase, _, _)| *phase == OperationPhase::Validating));
        assert!(seen
            .iter()
            .any(|(phase, _, _)| *phase == OperationPhase::Planning));
        assert!(seen
            .iter()
            .any(|(phase, processed, _)| *phase == OperationPhase::Applying && *processed > 0));
        assert_eq!(seen.last().expect("progress").0, OperationPhase::Applying);

        let observer = RecordingObserver::new(false);
        let applied = apply_with_observer(&mut world, &op, &observer).expect("apply");
        assert_eq!(applied.changed_blocks, 4096);
        assert_eq!(world.block_count(), 4096);
        let seen = observer.seen();
        assert!(seen.iter().any(
            |(phase, processed, total)| *phase == OperationPhase::Applying
                && *processed == *total
                && *total == 4096
        ));
    }

    #[test]
    fn fill_counts_only_actual_changes() {
        let mut world = world_with_stone();
        let bounds = BlockBox::new(BlockPos::new(0, 64, 0), BlockPos::new(1, 64, 1)).unwrap();
        let op = Operation::FillCuboid {
            dimension: "minecraft:overworld".into(),
            bounds,
            state: state("minecraft:stone"),
        };
        let preview = preview(&world, &op).unwrap();
        assert_eq!(preview.affected_blocks, 4);
        assert_eq!(preview.changed_blocks, 3);
        let applied = apply(&mut world, &op).unwrap();
        assert_eq!(applied.changed_blocks, 3);
        assert_eq!(world.block_count(), 4);
        assert_eq!(applied.dirty_chunks, vec![ChunkPos::new(0, 0)]);
    }

    #[test]
    fn replace_only_matches_source_state() {
        let mut world = World::new_empty();
        world
            .set_block(
                "minecraft:overworld",
                BlockPos::new(0, 64, 0),
                &state("minecraft:stone"),
            )
            .unwrap();
        world
            .set_block(
                "minecraft:overworld",
                BlockPos::new(1, 64, 0),
                &state("minecraft:dirt"),
            )
            .unwrap();
        let bounds = BlockBox::new(BlockPos::new(0, 64, 0), BlockPos::new(2, 64, 0)).unwrap();
        let op = Operation::Replace {
            dimension: "minecraft:overworld".into(),
            bounds,
            from: state("minecraft:stone"),
            to: state("minecraft:gold_block"),
        };
        let applied = apply(&mut world, &op).unwrap();
        assert_eq!(applied.changed_blocks, 1);
        assert_eq!(
            world
                .get_block("minecraft:overworld", BlockPos::new(0, 64, 0))
                .unwrap(),
            state("minecraft:gold_block")
        );
        assert_eq!(
            world
                .get_block("minecraft:overworld", BlockPos::new(1, 64, 0))
                .unwrap(),
            state("minecraft:dirt")
        );
    }

    #[test]
    fn negative_and_chunk_boundary_edits_are_consistent() {
        let mut world = World::new_empty();
        let bounds = BlockBox::new(BlockPos::new(-1, -64, -1), BlockPos::new(16, -63, 16)).unwrap();
        let op = Operation::FillCuboid {
            dimension: "minecraft:overworld".into(),
            bounds,
            state: state("minecraft:deepslate"),
        };
        let applied = apply(&mut world, &op).unwrap();
        assert_eq!(applied.changed_blocks, 18 * 2 * 18);
        assert_eq!(applied.dirty_chunks.len(), 9);
        assert_eq!(
            world
                .get_block("minecraft:overworld", BlockPos::new(-1, -64, -1))
                .unwrap(),
            state("minecraft:deepslate")
        );
        assert_eq!(
            world
                .get_block("minecraft:overworld", BlockPos::new(16, -63, 16))
                .unwrap(),
            state("minecraft:deepslate")
        );
    }

    #[test]
    fn out_of_bounds_fill_writes_nothing() {
        let mut world = World::new_empty();
        let bounds = BlockBox::new(BlockPos::new(0, 319, 0), BlockPos::new(1, 320, 1)).unwrap();
        let op = Operation::FillCuboid {
            dimension: "minecraft:overworld".into(),
            bounds,
            state: state("minecraft:stone"),
        };
        let error = apply(&mut world, &op).unwrap_err();
        assert!(matches!(error, OperationError::OutOfBounds { .. }));
        assert_eq!(world.block_count(), 0);
    }

    #[test]
    fn batch_deduplicates_positions_and_reports_unique_count() {
        let mut world = World::new_empty();
        let op = Operation::Batch {
            dimension: "minecraft:overworld".into(),
            changes: vec![
                BlockChange {
                    pos: BlockPos::new(0, 64, 0),
                    state: state("minecraft:stone"),
                },
                BlockChange {
                    pos: BlockPos::new(0, 64, 0),
                    state: state("minecraft:oak_planks"),
                },
                BlockChange {
                    pos: BlockPos::new(1, 64, 0),
                    state: state("minecraft:stone"),
                },
            ],
        };
        let applied = apply(&mut world, &op).unwrap();
        assert_eq!(applied.changed_blocks, 2);
        assert_eq!(
            world
                .get_block("minecraft:overworld", BlockPos::new(0, 64, 0))
                .unwrap(),
            state("minecraft:oak_planks")
        );
    }

    #[test]
    fn empty_batch_is_rejected() {
        let mut world = World::new_empty();
        let op = Operation::Batch {
            dimension: "minecraft:overworld".into(),
            changes: Vec::new(),
        };
        assert_eq!(
            apply(&mut world, &op).unwrap_err(),
            OperationError::EmptyBatch
        );
    }

    #[test]
    fn section_diffs_revert_and_reapply_exactly() {
        let mut world = world_with_stone();
        let bounds = BlockBox::new(BlockPos::new(-2, 62, -2), BlockPos::new(2, 66, 2)).unwrap();
        let op = Operation::FillCuboid {
            dimension: "minecraft:overworld".into(),
            bounds,
            state: state("minecraft:stone_bricks"),
        };
        let before_blocks = world.block_count();
        let applied = apply(&mut world, &op).unwrap();
        assert_eq!(applied.changed_blocks, 125);
        assert_eq!(world.block_count(), 125);
        let after_blocks = world.block_count();
        for diff in &applied.diffs {
            diff.revert(&mut world).unwrap();
        }
        assert_eq!(world.block_count(), before_blocks);
        assert_eq!(
            world
                .get_block("minecraft:overworld", BlockPos::new(0, 64, 0))
                .unwrap(),
            state("minecraft:stone")
        );
        for diff in &applied.diffs {
            diff.reapply(&mut world).unwrap();
        }
        assert_eq!(world.block_count(), after_blocks);
        assert_eq!(
            world
                .get_block("minecraft:overworld", BlockPos::new(0, 64, 0))
                .unwrap(),
            state("minecraft:stone_bricks")
        );
    }

    #[test]
    fn operation_limit_is_enforced_before_writes() {
        let mut world = World::new_empty();
        let bounds = BlockBox::new(BlockPos::new(0, 0, 0), BlockPos::new(100, 100, 100)).unwrap();
        let op = Operation::FillCuboid {
            dimension: "minecraft:overworld".into(),
            bounds,
            state: state("minecraft:stone"),
        };
        assert!(matches!(
            apply(&mut world, &op),
            Err(OperationError::TooManyBlocks { .. })
        ));
        assert_eq!(world.block_count(), 0);
    }

    #[test]
    fn selection_enforces_limits_and_containment() {
        let bounds = BlockBox::new(BlockPos::new(-2, 60, -2), BlockPos::new(2, 64, 2)).unwrap();
        let selection = Selection::new("minecraft:overworld", bounds).unwrap();
        assert_eq!(selection.volume(), 125);
        assert!(selection.contains(BlockPos::new(0, 64, 0)));
        assert!(!selection.contains(BlockPos::new(3, 64, 0)));
        assert!(!selection.contains(BlockPos::new(0, 65, 0)));

        let huge = BlockBox::new(BlockPos::new(0, 0, 0), BlockPos::new(1000, 1000, 1000)).unwrap();
        assert!(matches!(
            Selection::new("minecraft:overworld", huge),
            Err(OperationError::TooManyBlocks { .. })
        ));
    }
}
