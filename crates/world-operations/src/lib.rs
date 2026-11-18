//! World operations: selections, fills, replace, batched edits and section diffs.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use world_domain::{
    BlockBox, BlockPos, BlockState, BoundsError, ChunkPos, Section, World, WorldError,
};
use world_generation::{generate_heightmap, HeightmapParams};

pub const MAX_OPERATION_BLOCKS: u64 = 1_000_000;
/// Largest allowed raise/lower/noise/smooth/erode step per brush application.
pub const MAX_TERRAIN_BRUSH_AMOUNT: u32 = 16;
/// Smallest terrace step in blocks.
pub const MIN_TERRACE_STEP: u32 = 2;
/// Largest terrace step in blocks.
pub const MAX_TERRACE_STEP: u32 = 16;
/// Largest supported heightmap noise cell size.
pub const MAX_HEIGHTMAP_SCALE: u32 = world_generation::MAX_HEIGHTMAP_SCALE;
/// Largest supported heightmap fractal octave count.
pub const MAX_HEIGHTMAP_OCTAVES: u32 = world_generation::MAX_HEIGHTMAP_OCTAVES;
/// Largest supported heightmap amplitude above the base surface.
pub const MAX_HEIGHTMAP_AMPLITUDE: u32 = world_generation::MAX_HEIGHTMAP_AMPLITUDE;
/// Largest supported number of surface layers in a generated heightmap patch.
pub const MAX_HEIGHTMAP_SURFACE_DEPTH: u32 = 32;

/// Deterministic surface brush. Each brush changes the topmost non-air block of every
/// non-empty column inside the selection; it never invents or scales a whole world.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerrainBrushKind {
    /// Add `amount` layers of `material` above each surface.
    Raise,
    /// Remove up to `amount` blocks above each surface.
    Lower,
    /// Move each surface toward the rounded 8-neighbour average by at most `amount`.
    Smooth,
    /// Move each surface to one explicit or median target height.
    Flatten,
    /// Offset each surface by deterministic signed noise in `-amount..=amount`.
    Noise,
    /// Lower peaks toward the rounded 4-neighbour average by at most `amount`.
    Erode,
    /// Snap each surface to the nearest multiple of `step`.
    Terrace,
}

impl TerrainBrushKind {
    pub fn label(self) -> &'static str {
        match self {
            TerrainBrushKind::Raise => "raise",
            TerrainBrushKind::Lower => "lower",
            TerrainBrushKind::Smooth => "smooth",
            TerrainBrushKind::Flatten => "flatten",
            TerrainBrushKind::Noise => "noise",
            TerrainBrushKind::Erode => "erode",
            TerrainBrushKind::Terrace => "terrace",
        }
    }
}

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
    #[error("invalid terrain brush: {0}")]
    InvalidBrush(String),
    #[error("invalid heightmap generator: {0}")]
    InvalidHeightmap(String),
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
    /// Deterministic terrain surface brush over an explicit selection.
    TerrainBrush {
        dimension: String,
        bounds: BlockBox,
        kind: TerrainBrushKind,
        material: BlockState,
        #[serde(default = "default_brush_amount")]
        amount: u32,
        #[serde(default)]
        seed: u64,
        #[serde(default = "default_terrace_step")]
        step: u32,
        #[serde(default, rename = "targetY")]
        target_y: Option<i32>,
    },
    /// Deterministic local heightmap patch. This is a bounded construction tool, not a
    /// whole-world generator and it makes no vanilla-parity claim.
    GenerateHeightmap {
        dimension: String,
        bounds: BlockBox,
        #[serde(default)]
        seed: u64,
        #[serde(default = "default_heightmap_scale")]
        scale: u32,
        #[serde(default = "default_heightmap_octaves")]
        octaves: u32,
        #[serde(default = "default_heightmap_amplitude")]
        amplitude: u32,
        #[serde(rename = "baseY")]
        base_y: i32,
        #[serde(default = "default_surface_depth", rename = "surfaceDepth")]
        surface_depth: u32,
        surface: BlockState,
        filler: BlockState,
    },
}

fn default_brush_amount() -> u32 {
    1
}

fn default_terrace_step() -> u32 {
    4
}

fn default_heightmap_scale() -> u32 {
    8
}

fn default_heightmap_octaves() -> u32 {
    3
}

fn default_heightmap_amplitude() -> u32 {
    8
}

fn default_surface_depth() -> u32 {
    1
}

impl Operation {
    pub fn dimension(&self) -> &str {
        match self {
            Operation::SetBlock { dimension, .. }
            | Operation::Batch { dimension, .. }
            | Operation::FillCuboid { dimension, .. }
            | Operation::Replace { dimension, .. }
            | Operation::TerrainBrush { dimension, .. }
            | Operation::GenerateHeightmap { dimension, .. } => dimension,
        }
    }

    /// Upper bound of blocks the operation may inspect.
    pub fn affected_blocks(&self) -> u64 {
        match self {
            Operation::SetBlock { .. } => 1,
            Operation::Batch { changes, .. } => changes.len() as u64,
            Operation::FillCuboid { bounds, .. }
            | Operation::Replace { bounds, .. }
            | Operation::TerrainBrush { bounds, .. } => bounds.volume(),
            Operation::GenerateHeightmap { bounds, base_y, .. } => {
                let columns = (i64::from(bounds.max.x) - i64::from(bounds.min.x) + 1).max(0) as u64
                    * (i64::from(bounds.max.z) - i64::from(bounds.min.z) + 1).max(0) as u64;
                let layers = (i64::from(bounds.max.y) - i64::from(*base_y) + 1).max(0) as u64;
                columns.saturating_mul(layers)
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
            Operation::FillCuboid { bounds, .. }
            | Operation::Replace { bounds, .. }
            | Operation::TerrainBrush { bounds, .. }
            | Operation::GenerateHeightmap { bounds, .. } => Some(*bounds),
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
        Operation::TerrainBrush {
            bounds,
            kind,
            material,
            amount,
            step,
            target_y,
            ..
        } => {
            check(bounds.min)?;
            check(bounds.max)?;
            validate_terrain_brush(*bounds, *kind, material, *amount, *step, *target_y)?;
        }
        Operation::GenerateHeightmap {
            dimension,
            bounds,
            seed,
            scale,
            octaves,
            amplitude,
            base_y,
            surface_depth,
            surface,
            filler,
        } => {
            check(bounds.min)?;
            check(bounds.max)?;
            validate_heightmap(&HeightmapRequest {
                dimension,
                bounds: *bounds,
                seed: *seed,
                scale: *scale,
                octaves: *octaves,
                amplitude: *amplitude,
                base_y: *base_y,
                surface_depth: *surface_depth,
                surface,
                filler,
            })?;
        }
    }
    Ok(())
}

struct HeightmapRequest<'a> {
    dimension: &'a str,
    bounds: BlockBox,
    seed: u64,
    scale: u32,
    octaves: u32,
    amplitude: u32,
    base_y: i32,
    surface_depth: u32,
    surface: &'a BlockState,
    filler: &'a BlockState,
}

fn validate_heightmap(request: &HeightmapRequest<'_>) -> Result<(), OperationError> {
    if !(1..=MAX_HEIGHTMAP_SCALE).contains(&request.scale) {
        return Err(OperationError::InvalidHeightmap(format!(
            "scale must be 1..={MAX_HEIGHTMAP_SCALE}, got {}",
            request.scale
        )));
    }
    if !(1..=MAX_HEIGHTMAP_OCTAVES).contains(&request.octaves) {
        return Err(OperationError::InvalidHeightmap(format!(
            "octaves must be 1..={MAX_HEIGHTMAP_OCTAVES}, got {}",
            request.octaves
        )));
    }
    if !(1..=MAX_HEIGHTMAP_AMPLITUDE).contains(&request.amplitude) {
        return Err(OperationError::InvalidHeightmap(format!(
            "amplitude must be 1..={MAX_HEIGHTMAP_AMPLITUDE}, got {}",
            request.amplitude
        )));
    }
    if !(1..=MAX_HEIGHTMAP_SURFACE_DEPTH).contains(&request.surface_depth) {
        return Err(OperationError::InvalidHeightmap(format!(
            "surfaceDepth must be 1..={MAX_HEIGHTMAP_SURFACE_DEPTH}, got {}",
            request.surface_depth
        )));
    }
    if request.surface.is_air() || request.filler.is_air() {
        return Err(OperationError::InvalidHeightmap(
            "surface and filler must not be minecraft:air".to_owned(),
        ));
    }
    if request.base_y < request.bounds.min.y || request.base_y > request.bounds.max.y {
        return Err(OperationError::InvalidHeightmap(format!(
            "baseY {} is outside the selection Y range {}..={}",
            request.base_y, request.bounds.min.y, request.bounds.max.y
        )));
    }
    let max_height = i64::from(request.base_y) + i64::from(request.amplitude);
    if max_height > i64::from(request.bounds.max.y) {
        return Err(OperationError::InvalidHeightmap(format!(
            "baseY {} plus amplitude {} exceeds the selection maxY {}",
            request.base_y, request.amplitude, request.bounds.max.y
        )));
    }
    Ok(())
}

fn validate_terrain_brush(
    bounds: BlockBox,
    kind: TerrainBrushKind,
    material: &BlockState,
    amount: u32,
    step: u32,
    target_y: Option<i32>,
) -> Result<(), OperationError> {
    if !(1..=MAX_TERRAIN_BRUSH_AMOUNT).contains(&amount) {
        return Err(OperationError::InvalidBrush(format!(
            "amount must be 1..={MAX_TERRAIN_BRUSH_AMOUNT}, got {amount}"
        )));
    }
    if !(MIN_TERRACE_STEP..=MAX_TERRACE_STEP).contains(&step) {
        return Err(OperationError::InvalidBrush(format!(
            "terrace step must be {MIN_TERRACE_STEP}..={MAX_TERRACE_STEP}, got {step}"
        )));
    }
    if material.is_air() {
        return Err(OperationError::InvalidBrush(
            "material must not be minecraft:air".to_owned(),
        ));
    }
    if let Some(target_y) = target_y {
        if kind != TerrainBrushKind::Flatten {
            return Err(OperationError::InvalidBrush(
                "targetY is only accepted by the flatten brush".to_owned(),
            ));
        }
        if target_y < bounds.min.y || target_y > bounds.max.y {
            return Err(OperationError::InvalidBrush(format!(
                "targetY {target_y} is outside the selection Y range {}..={}",
                bounds.min.y, bounds.max.y
            )));
        }
    }
    Ok(())
}

struct PlannedChanges {
    changes: Vec<BlockChange>,
    warnings: Vec<String>,
}

fn target_changes(
    world: &World,
    operation: &Operation,
    observer: &dyn OperationObserver,
) -> Result<PlannedChanges, OperationError> {
    let changes = match operation {
        Operation::SetBlock {
            dimension,
            pos,
            state,
        } => {
            report(observer, OperationPhase::Planning, 0, 1)?;
            let current = world.get_block(dimension, *pos)?;
            report(observer, OperationPhase::Planning, 1, 1)?;
            if current == *state {
                Vec::new()
            } else {
                vec![BlockChange {
                    pos: *pos,
                    state: state.clone(),
                }]
            }
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
            result
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
            result
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
            result
        }
        Operation::TerrainBrush {
            dimension,
            bounds,
            kind,
            material,
            amount,
            seed,
            step,
            target_y,
        } => {
            return plan_terrain_brush(
                world,
                TerrainBrushRequest {
                    dimension,
                    bounds: *bounds,
                    kind: *kind,
                    material,
                    amount: *amount,
                    seed: *seed,
                    step: *step,
                    target_y: *target_y,
                },
                observer,
            );
        }
        Operation::GenerateHeightmap {
            dimension,
            bounds,
            seed,
            scale,
            octaves,
            amplitude,
            base_y,
            surface_depth,
            surface,
            filler,
        } => {
            return plan_heightmap(
                world,
                HeightmapRequest {
                    dimension,
                    bounds: *bounds,
                    seed: *seed,
                    scale: *scale,
                    octaves: *octaves,
                    amplitude: *amplitude,
                    base_y: *base_y,
                    surface_depth: *surface_depth,
                    surface,
                    filler,
                },
                observer,
            );
        }
    };
    Ok(PlannedChanges {
        changes,
        warnings: Vec::new(),
    })
}

struct TerrainBrushRequest<'a> {
    dimension: &'a str,
    bounds: BlockBox,
    kind: TerrainBrushKind,
    material: &'a BlockState,
    amount: u32,
    seed: u64,
    step: u32,
    target_y: Option<i32>,
}

/// Plans a deterministic surface brush. Heights are read once, so smoothing and erosion do
/// not depend on the order in which columns are written.
fn plan_terrain_brush(
    world: &World,
    request: TerrainBrushRequest<'_>,
    observer: &dyn OperationObserver,
) -> Result<PlannedChanges, OperationError> {
    let TerrainBrushRequest {
        dimension,
        bounds,
        kind,
        material,
        amount,
        seed,
        step,
        target_y,
    } = request;
    let width = (bounds.max.x - bounds.min.x + 1) as usize;
    let depth = (bounds.max.z - bounds.min.z + 1) as usize;
    let columns = width
        .checked_mul(depth)
        .ok_or_else(|| OperationError::InvalidBrush("brush area overflow".to_owned()))?;
    let total = columns as u64;
    report(observer, OperationPhase::Planning, 0, total)?;

    // `None` means the column has no non-air block inside the selection. Such columns are
    // skipped: a terrain brush adjusts existing terrain and never builds unsupported
    // pillars from the selection floor.
    let mut heights = vec![None; columns];
    let mut skipped = 0usize;
    let mut done = 0u64;
    for (z_index, z) in (bounds.min.z..=bounds.max.z).enumerate() {
        for (x_index, x) in (bounds.min.x..=bounds.max.x).enumerate() {
            let mut surface = None;
            for y in (bounds.min.y..=bounds.max.y).rev() {
                let state = world.get_block(dimension, BlockPos::new(x, y, z))?;
                if !state.is_air() {
                    surface = Some(y);
                    break;
                }
            }
            if surface.is_none() {
                skipped += 1;
            }
            heights[z_index * width + x_index] = surface;
            done += 1;
            if done == total || done % 64 == 0 {
                report(observer, OperationPhase::Planning, done, total)?;
            }
        }
    }

    let mut warnings = Vec::new();
    if skipped > 0 {
        warnings.push(format!(
            "{skipped} empty column(s) had no terrain surface and were skipped"
        ));
    }
    let mut changes = Vec::new();
    if skipped == columns {
        return Ok(PlannedChanges { changes, warnings });
    }

    let index = |x: i32, z: i32| -> usize {
        ((z - bounds.min.z) as usize) * width + ((x - bounds.min.x) as usize)
    };
    let flatten_target = if kind == TerrainBrushKind::Flatten {
        match target_y {
            Some(height) => Some(height),
            None => median_height(&heights),
        }
    } else {
        None
    };
    if kind == TerrainBrushKind::Flatten && flatten_target.is_none() {
        warnings.push("the flatten brush found no surface height to target".to_owned());
        return Ok(PlannedChanges { changes, warnings });
    }

    for z in bounds.min.z..=bounds.max.z {
        for x in bounds.min.x..=bounds.max.x {
            let Some(current) = heights[index(x, z)] else {
                continue;
            };
            let target = match kind {
                TerrainBrushKind::Raise => current + amount as i32,
                TerrainBrushKind::Lower => current - amount as i32,
                TerrainBrushKind::Smooth => {
                    let average = neighbor_average(
                        &heights,
                        width,
                        depth,
                        x - bounds.min.x,
                        z - bounds.min.z,
                        false,
                    );
                    match average {
                        Some(average) => {
                            current + (average - current).clamp(-(amount as i32), amount as i32)
                        }
                        None => current,
                    }
                }
                TerrainBrushKind::Flatten => flatten_target.expect("checked above"),
                TerrainBrushKind::Noise => current + noise_offset(seed, x, z, amount),
                TerrainBrushKind::Erode => {
                    let average = neighbor_average(
                        &heights,
                        width,
                        depth,
                        x - bounds.min.x,
                        z - bounds.min.z,
                        true,
                    );
                    match average {
                        Some(average) if current > average => {
                            current - (current - average).min(amount as i32)
                        }
                        _ => current,
                    }
                }
                TerrainBrushKind::Terrace => terrace_height(current, step),
            };
            let target = target.clamp(bounds.min.y, bounds.max.y);
            write_column_surface(
                world,
                dimension,
                &mut changes,
                ColumnEdit {
                    x,
                    z,
                    current,
                    target,
                },
                material,
            )?;
        }
    }

    Ok(PlannedChanges { changes, warnings })
}

/// Plans a bounded local heightmap patch. It fills from `base_y` to the generated surface,
/// places `surface` on the top `surface_depth` layers, and clears the remaining selection
/// above the new surface. Blocks below `base_y` are not touched.
fn plan_heightmap(
    world: &World,
    request: HeightmapRequest<'_>,
    observer: &dyn OperationObserver,
) -> Result<PlannedChanges, OperationError> {
    let width = (request.bounds.max.x - request.bounds.min.x + 1) as u32;
    let depth = (request.bounds.max.z - request.bounds.min.z + 1) as u32;
    let patch = generate_heightmap(
        width,
        depth,
        request.bounds.min.x,
        request.bounds.min.z,
        HeightmapParams {
            seed: request.seed,
            scale: request.scale,
            octaves: request.octaves,
            amplitude: request.amplitude,
        },
    )
    .map_err(|error| OperationError::InvalidHeightmap(error.to_string()))?;
    let total = u64::from(width) * u64::from(depth);
    report(observer, OperationPhase::Planning, 0, total)?;

    let air = BlockState::default();
    let mut changes = Vec::new();
    let mut done = 0u64;
    for z_local in 0..depth {
        let z = request.bounds.min.z + z_local as i32;
        for x_local in 0..width {
            let x = request.bounds.min.x + x_local as i32;
            let offset = patch
                .offset(x_local, z_local)
                .expect("the patch matches the requested footprint");
            let height = request.base_y + offset as i32;
            let layers = request
                .surface_depth
                .min((height - request.base_y + 1) as u32) as i32;
            let surface_start = height - layers + 1;
            for y in request.base_y..=height {
                let desired = if y >= surface_start {
                    request.surface
                } else {
                    request.filler
                };
                let pos = BlockPos::new(x, y, z);
                if world.get_block(request.dimension, pos)? != *desired {
                    changes.push(BlockChange {
                        pos,
                        state: desired.clone(),
                    });
                }
            }
            for y in (height + 1)..=request.bounds.max.y {
                let pos = BlockPos::new(x, y, z);
                if world.get_block(request.dimension, pos)? != air {
                    changes.push(BlockChange {
                        pos,
                        state: air.clone(),
                    });
                }
            }
            done += 1;
            if done == total || done % 256 == 0 {
                report(observer, OperationPhase::Planning, done, total)?;
            }
        }
    }
    Ok(PlannedChanges {
        changes,
        warnings: Vec::new(),
    })
}

fn median_height(heights: &[Option<i32>]) -> Option<i32> {
    let mut values: Vec<i32> = heights.iter().flatten().copied().collect();
    if values.is_empty() {
        return None;
    }
    values.sort_unstable();
    Some(values[(values.len() - 1) / 2])
}

/// Rounded average of existing 8-neighbours (smooth) or 4-neighbours (erode). Out-of-box
/// neighbours are ignored; the result never depends on iteration order.
fn neighbor_average(
    heights: &[Option<i32>],
    width: usize,
    depth: usize,
    x: i32,
    z: i32,
    cardinal_only: bool,
) -> Option<i32> {
    let mut sum: i64 = 0;
    let mut count: i64 = 0;
    for dz in -1i32..=1 {
        for dx in -1i32..=1 {
            if dx == 0 && dz == 0 {
                continue;
            }
            if cardinal_only && dx != 0 && dz != 0 {
                continue;
            }
            let nx = x + dx;
            let nz = z + dz;
            if nx < 0 || nz < 0 || nx as usize >= width || nz as usize >= depth {
                continue;
            }
            if let Some(value) = heights[nz as usize * width + nx as usize] {
                sum += i64::from(value);
                count += 1;
            }
        }
    }
    if count == 0 {
        return None;
    }
    let rounded = if sum >= 0 {
        (sum + count / 2) / count
    } else {
        -((-sum + count / 2) / count)
    };
    i32::try_from(rounded).ok()
}

fn terrace_height(height: i32, step: u32) -> i32 {
    let step = step as i32;
    let remainder = height.rem_euclid(step);
    let base = height - remainder;
    if remainder * 2 >= step {
        base + step
    } else {
        base
    }
}

/// Stable signed noise in `-amount..=amount` for one column. SplitMix64 is used instead of
/// the standard library hasher so the result is identical across processes and platforms.
fn noise_offset(seed: u64, x: i32, z: i32, amount: u32) -> i32 {
    let mixed = splitmix64(
        seed ^ splitmix64(
            (i64::from(x) as u64)
                .rotate_left(17)
                .wrapping_add((i64::from(z) as u64).rotate_left(41)),
        ),
    );
    let span = u64::from(amount) * 2 + 1;
    (mixed % span) as i32 - amount as i32
}

fn splitmix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut result = value;
    result = (result ^ (result >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    result = (result ^ (result >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    result ^ (result >> 31)
}

struct ColumnEdit {
    x: i32,
    z: i32,
    current: i32,
    target: i32,
}

fn write_column_surface(
    world: &World,
    dimension: &str,
    changes: &mut Vec<BlockChange>,
    edit: ColumnEdit,
    material: &BlockState,
) -> Result<(), OperationError> {
    let ColumnEdit {
        x,
        z,
        current,
        target,
    } = edit;
    if target > current {
        for y in (current + 1)..=target {
            let pos = BlockPos::new(x, y, z);
            if world.get_block(dimension, pos)? != *material {
                changes.push(BlockChange {
                    pos,
                    state: material.clone(),
                });
            }
        }
    } else if target < current {
        let air = BlockState::default();
        for y in (target + 1)..=current {
            let pos = BlockPos::new(x, y, z);
            if world.get_block(dimension, pos)? != air {
                changes.push(BlockChange {
                    pos,
                    state: air.clone(),
                });
            }
        }
    }
    Ok(())
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
    let planned = target_changes(world, operation, &NoopObserver)?;
    let mut chunks = BTreeSet::new();
    for change in &planned.changes {
        chunks.insert(change.pos.chunk_pos());
    }
    Ok(Preview {
        affected_blocks: operation.affected_blocks(),
        changed_blocks: planned.changes.len() as u64,
        dirty_chunks: chunks.into_iter().collect(),
        warnings: planned.warnings,
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
    let planned = target_changes(world, operation, observer)?;
    if planned.changes.is_empty() {
        report(observer, OperationPhase::Applying, 0, 0)?;
        return Ok(AppliedOperation {
            dimension: operation.dimension().to_owned(),
            changed_blocks: 0,
            dirty_chunks: Vec::new(),
            diffs: Vec::new(),
            warnings: planned.warnings,
        });
    }
    let changes = planned.changes;

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
        warnings: planned.warnings,
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

    const DIM: &str = "minecraft:overworld";

    fn fill_column(world: &mut World, x: i32, z: i32, min_y: i32, top_y: i32, name: &str) {
        for y in min_y..=top_y {
            world
                .set_block(DIM, BlockPos::new(x, y, z), &state(name))
                .unwrap();
        }
    }

    fn terrain_operation(
        bounds: BlockBox,
        kind: TerrainBrushKind,
        material: &str,
        amount: u32,
        seed: u64,
        step: u32,
        target_y: Option<i32>,
    ) -> Operation {
        Operation::TerrainBrush {
            dimension: DIM.into(),
            bounds,
            kind,
            material: state(material),
            amount,
            seed,
            step,
            target_y,
        }
    }

    fn air() -> BlockState {
        BlockState::default()
    }

    #[test]
    fn terrain_brush_reports_its_exact_bounds_and_scan_budget() {
        let bounds = BlockBox::new(BlockPos::new(-4, 60, 3), BlockPos::new(5, 70, 12)).unwrap();
        let operation = terrain_operation(
            bounds,
            TerrainBrushKind::Raise,
            "minecraft:dirt",
            1,
            0,
            4,
            None,
        );
        assert_eq!(operation.affected_blocks(), bounds.volume());
        assert_eq!(operation.bounds(), Some(bounds));
        assert_eq!(operation.dimension(), DIM);
    }

    #[test]
    fn raise_adds_material_above_every_surface() {
        let mut world = World::new_empty();
        for x in 0..2 {
            for z in 0..2 {
                fill_column(&mut world, x, z, 60, 64, "minecraft:stone");
            }
        }
        let bounds = BlockBox::new(BlockPos::new(0, 60, 0), BlockPos::new(1, 70, 1)).unwrap();
        let operation = terrain_operation(
            bounds,
            TerrainBrushKind::Raise,
            "minecraft:dirt",
            2,
            0,
            4,
            None,
        );
        let preview = preview(&world, &operation).unwrap();
        assert_eq!(preview.changed_blocks, 8);
        assert!(preview.warnings.is_empty());
        let applied = apply(&mut world, &operation).unwrap();
        assert_eq!(applied.changed_blocks, 8);
        for x in 0..2 {
            for z in 0..2 {
                assert_eq!(
                    world.get_block(DIM, BlockPos::new(x, 65, z)).unwrap(),
                    state("minecraft:dirt")
                );
                assert_eq!(
                    world.get_block(DIM, BlockPos::new(x, 66, z)).unwrap(),
                    state("minecraft:dirt")
                );
                assert_eq!(
                    world.get_block(DIM, BlockPos::new(x, 67, z)).unwrap(),
                    air()
                );
            }
        }
    }

    #[test]
    fn lower_removes_layers_and_clamps_to_the_selection() {
        let mut world = World::new_empty();
        fill_column(&mut world, 0, 0, 60, 64, "minecraft:stone");
        let bounds = BlockBox::new(BlockPos::new(0, 62, 0), BlockPos::new(0, 70, 0)).unwrap();
        let operation = terrain_operation(
            bounds,
            TerrainBrushKind::Lower,
            "minecraft:dirt",
            2,
            0,
            4,
            None,
        );
        let applied = apply(&mut world, &operation).unwrap();
        assert_eq!(applied.changed_blocks, 2);
        assert_eq!(
            world.get_block(DIM, BlockPos::new(0, 62, 0)).unwrap(),
            state("minecraft:stone")
        );
        assert_eq!(
            world.get_block(DIM, BlockPos::new(0, 63, 0)).unwrap(),
            air()
        );
        assert_eq!(
            world.get_block(DIM, BlockPos::new(0, 64, 0)).unwrap(),
            air()
        );

        // The selection floor is respected: a follow-up lower cannot write below y=62.
        let again = apply(&mut world, &operation).unwrap();
        assert_eq!(again.changed_blocks, 0);
        assert_eq!(
            world.get_block(DIM, BlockPos::new(0, 62, 0)).unwrap(),
            state("minecraft:stone")
        );
    }

    #[test]
    fn smooth_moves_peak_and_edge_toward_neighbours_by_amount() {
        let mut world = World::new_empty();
        fill_column(&mut world, 0, 0, 60, 64, "minecraft:stone");
        fill_column(&mut world, 1, 0, 60, 70, "minecraft:stone");
        fill_column(&mut world, 2, 0, 60, 64, "minecraft:stone");
        let bounds = BlockBox::new(BlockPos::new(0, 60, 0), BlockPos::new(2, 72, 0)).unwrap();
        let operation = terrain_operation(
            bounds,
            TerrainBrushKind::Smooth,
            "minecraft:dirt",
            4,
            0,
            4,
            None,
        );
        apply(&mut world, &operation).unwrap();

        // The peak moves four layers toward the neighbour average and edge columns rise
        // four layers toward the peak; heights are read once, so order does not matter.
        assert_eq!(
            world.get_block(DIM, BlockPos::new(1, 66, 0)).unwrap(),
            state("minecraft:stone")
        );
        assert_eq!(
            world.get_block(DIM, BlockPos::new(1, 67, 0)).unwrap(),
            air()
        );
        assert_eq!(
            world.get_block(DIM, BlockPos::new(0, 68, 0)).unwrap(),
            state("minecraft:dirt")
        );
        assert_eq!(
            world.get_block(DIM, BlockPos::new(2, 68, 0)).unwrap(),
            state("minecraft:dirt")
        );
    }

    #[test]
    fn flatten_uses_explicit_or_median_target() {
        let bounds = BlockBox::new(BlockPos::new(0, 60, 0), BlockPos::new(1, 70, 0)).unwrap();
        let mut explicit = World::new_empty();
        fill_column(&mut explicit, 0, 0, 60, 64, "minecraft:stone");
        fill_column(&mut explicit, 1, 0, 60, 68, "minecraft:stone");
        let operation = terrain_operation(
            bounds,
            TerrainBrushKind::Flatten,
            "minecraft:dirt",
            1,
            0,
            4,
            Some(66),
        );
        let applied = apply(&mut explicit, &operation).unwrap();
        assert_eq!(applied.changed_blocks, 4);
        assert_eq!(
            explicit.get_block(DIM, BlockPos::new(0, 66, 0)).unwrap(),
            state("minecraft:dirt")
        );
        assert_eq!(
            explicit.get_block(DIM, BlockPos::new(1, 66, 0)).unwrap(),
            state("minecraft:stone")
        );
        assert_eq!(
            explicit.get_block(DIM, BlockPos::new(1, 67, 0)).unwrap(),
            air()
        );

        let mut median = World::new_empty();
        fill_column(&mut median, 0, 0, 60, 64, "minecraft:stone");
        fill_column(&mut median, 1, 0, 60, 68, "minecraft:stone");
        let operation = terrain_operation(
            bounds,
            TerrainBrushKind::Flatten,
            "minecraft:dirt",
            1,
            0,
            4,
            None,
        );
        let applied = apply(&mut median, &operation).unwrap();
        assert_eq!(applied.changed_blocks, 4);
        assert_eq!(
            median.get_block(DIM, BlockPos::new(1, 64, 0)).unwrap(),
            state("minecraft:stone")
        );
        assert_eq!(
            median.get_block(DIM, BlockPos::new(1, 65, 0)).unwrap(),
            air()
        );
    }

    #[test]
    fn noise_is_stable_bounded_and_reversible() {
        let mut world = World::new_empty();
        for x in 0..8 {
            for z in 0..8 {
                fill_column(&mut world, x, z, 60, 64, "minecraft:stone");
            }
        }
        let bounds = BlockBox::new(BlockPos::new(0, 60, 0), BlockPos::new(7, 70, 7)).unwrap();
        let operation = terrain_operation(
            bounds,
            TerrainBrushKind::Noise,
            "minecraft:dirt",
            2,
            0x5EED,
            4,
            None,
        );
        let before = world.block_count();
        let applied = apply(&mut world, &operation).unwrap();
        assert!(applied.changed_blocks > 0);
        assert!(applied.changed_blocks < 64 * 4);
        for x in 0..8 {
            for z in 0..8 {
                for y in 65..=67 {
                    assert!(world.get_block(DIM, BlockPos::new(x, y, z)).is_ok());
                }
            }
        }

        // Every offset is in -2..=2 and the same input always produces the same offset.
        for x in -20..20 {
            for z in -20..20 {
                let offset = noise_offset(0x5EED, x, z, 2);
                assert!((-2..=2).contains(&offset));
                assert_eq!(offset, noise_offset(0x5EED, x, z, 2));
            }
        }
        assert!((0..64).any(|index| {
            let x = index % 8;
            let z = index / 8;
            noise_offset(0x5EED, x, z, 2) != noise_offset(0x5EED + 1, x, z, 2)
        }));

        for diff in applied.diffs.iter().rev() {
            diff.revert(&mut world).unwrap();
        }
        assert_eq!(world.block_count(), before);
    }

    #[test]
    fn erosion_only_lowers_peaks_and_terrace_snaps_heights() {
        let mut world = World::new_empty();
        fill_column(&mut world, 0, 0, 60, 64, "minecraft:stone");
        fill_column(&mut world, 1, 0, 60, 70, "minecraft:stone");
        fill_column(&mut world, 2, 0, 60, 64, "minecraft:stone");
        let bounds = BlockBox::new(BlockPos::new(0, 60, 0), BlockPos::new(2, 72, 0)).unwrap();
        let erode = terrain_operation(
            bounds,
            TerrainBrushKind::Erode,
            "minecraft:dirt",
            1,
            0,
            4,
            None,
        );
        let applied = apply(&mut world, &erode).unwrap();
        assert_eq!(applied.changed_blocks, 1);
        assert_eq!(
            world.get_block(DIM, BlockPos::new(1, 69, 0)).unwrap(),
            state("minecraft:stone")
        );
        assert_eq!(
            world.get_block(DIM, BlockPos::new(0, 64, 0)).unwrap(),
            state("minecraft:stone")
        );
        assert_eq!(
            world.get_block(DIM, BlockPos::new(2, 64, 0)).unwrap(),
            state("minecraft:stone")
        );

        let mut terraces = World::new_empty();
        for (x, height) in [65, 66, 67, 68].into_iter().enumerate() {
            fill_column(&mut terraces, x as i32, 0, 60, height, "minecraft:stone");
        }
        let bounds = BlockBox::new(BlockPos::new(0, 60, 0), BlockPos::new(3, 72, 0)).unwrap();
        let terrace = terrain_operation(
            bounds,
            TerrainBrushKind::Terrace,
            "minecraft:dirt",
            1,
            0,
            4,
            None,
        );
        apply(&mut terraces, &terrace).unwrap();
        assert_eq!(
            terraces.get_block(DIM, BlockPos::new(0, 64, 0)).unwrap(),
            state("minecraft:stone")
        );
        assert_eq!(
            terraces.get_block(DIM, BlockPos::new(0, 65, 0)).unwrap(),
            air()
        );
        assert_eq!(
            terraces.get_block(DIM, BlockPos::new(1, 68, 0)).unwrap(),
            state("minecraft:dirt")
        );
        assert_eq!(
            terraces.get_block(DIM, BlockPos::new(2, 68, 0)).unwrap(),
            state("minecraft:dirt")
        );
        assert_eq!(
            terraces.get_block(DIM, BlockPos::new(3, 68, 0)).unwrap(),
            state("minecraft:stone")
        );
    }

    #[test]
    fn empty_columns_are_skipped_with_a_warning() {
        let mut world = World::new_empty();
        fill_column(&mut world, 0, 0, 60, 64, "minecraft:stone");
        let bounds = BlockBox::new(BlockPos::new(0, 60, 0), BlockPos::new(1, 70, 0)).unwrap();
        let operation = terrain_operation(
            bounds,
            TerrainBrushKind::Raise,
            "minecraft:dirt",
            1,
            0,
            4,
            None,
        );
        let planned = preview(&world, &operation).unwrap();
        assert_eq!(planned.changed_blocks, 1);
        assert!(planned
            .warnings
            .iter()
            .any(|warning| warning.contains("1 empty column")));
        let applied = apply(&mut world, &operation).unwrap();
        assert_eq!(applied.changed_blocks, 1);
        assert!(applied
            .warnings
            .iter()
            .any(|warning| warning.contains("1 empty column")));

        let empty_world = World::new_empty();
        let empty_preview = preview(&empty_world, &operation).unwrap();
        assert_eq!(empty_preview.changed_blocks, 0);
        assert!(empty_preview
            .warnings
            .iter()
            .any(|warning| warning.contains("2 empty column")));
    }

    #[test]
    fn brush_parameters_are_validated_before_planning() {
        let bounds = BlockBox::new(BlockPos::new(0, 60, 0), BlockPos::new(1, 70, 1)).unwrap();
        let world = World::new_empty();
        for operation in [
            terrain_operation(
                bounds,
                TerrainBrushKind::Raise,
                "minecraft:dirt",
                0,
                0,
                4,
                None,
            ),
            terrain_operation(
                bounds,
                TerrainBrushKind::Terrace,
                "minecraft:dirt",
                1,
                0,
                1,
                None,
            ),
            terrain_operation(
                bounds,
                TerrainBrushKind::Raise,
                "minecraft:air",
                1,
                0,
                4,
                None,
            ),
            terrain_operation(
                bounds,
                TerrainBrushKind::Flatten,
                "minecraft:dirt",
                1,
                0,
                4,
                Some(59),
            ),
            terrain_operation(
                bounds,
                TerrainBrushKind::Raise,
                "minecraft:dirt",
                1,
                0,
                4,
                Some(64),
            ),
        ] {
            assert!(matches!(
                preview(&world, &operation),
                Err(OperationError::InvalidBrush(_))
            ));
        }
    }

    #[test]
    fn brush_targets_are_clamped_inside_the_selection() {
        let mut world = World::new_empty();
        fill_column(&mut world, 0, 0, 60, 70, "minecraft:stone");
        let bounds = BlockBox::new(BlockPos::new(0, 60, 0), BlockPos::new(0, 70, 0)).unwrap();
        let operation = terrain_operation(
            bounds,
            TerrainBrushKind::Raise,
            "minecraft:dirt",
            16,
            0,
            4,
            None,
        );
        let applied = apply(&mut world, &operation).unwrap();
        assert_eq!(applied.changed_blocks, 0);
        assert_eq!(
            world.get_block(DIM, BlockPos::new(0, 71, 0)).unwrap(),
            air()
        );
    }

    #[derive(Clone, Copy)]
    struct HeightmapSpec<'a> {
        seed: u64,
        scale: u32,
        octaves: u32,
        amplitude: u32,
        base_y: i32,
        surface_depth: u32,
        surface: &'a str,
        filler: &'a str,
    }

    impl Default for HeightmapSpec<'_> {
        fn default() -> Self {
            Self {
                seed: 0,
                scale: 8,
                octaves: 3,
                amplitude: 8,
                base_y: 60,
                surface_depth: 1,
                surface: "minecraft:grass_block",
                filler: "minecraft:dirt",
            }
        }
    }

    fn heightmap_operation(bounds: BlockBox, spec: HeightmapSpec<'_>) -> Operation {
        Operation::GenerateHeightmap {
            dimension: DIM.into(),
            bounds,
            seed: spec.seed,
            scale: spec.scale,
            octaves: spec.octaves,
            amplitude: spec.amplitude,
            base_y: spec.base_y,
            surface_depth: spec.surface_depth,
            surface: state(spec.surface),
            filler: state(spec.filler),
        }
    }

    #[test]
    fn heightmap_patch_places_surface_and_clears_above_the_base() {
        let mut world = World::new_empty();
        for y in 60..=70 {
            world
                .set_block(DIM, BlockPos::new(0, y, 0), &state("minecraft:stone"))
                .unwrap();
        }
        world
            .set_block(DIM, BlockPos::new(0, 59, 0), &state("minecraft:gold_block"))
            .unwrap();
        let bounds = BlockBox::new(BlockPos::new(0, 60, 0), BlockPos::new(0, 70, 0)).unwrap();
        let operation = heightmap_operation(
            bounds,
            HeightmapSpec {
                seed: 7,
                scale: 4,
                octaves: 2,
                amplitude: 6,
                surface_depth: 2,
                surface: "minecraft:dirt",
                filler: "minecraft:cobblestone",
                ..HeightmapSpec::default()
            },
        );
        assert_eq!(operation.dimension(), DIM);
        assert_eq!(operation.bounds(), Some(bounds));
        let patch = world_generation::generate_heightmap(
            1,
            1,
            0,
            0,
            world_generation::HeightmapParams {
                seed: 7,
                scale: 4,
                octaves: 2,
                amplitude: 6,
            },
        )
        .unwrap();
        let height = 60 + patch.offsets[0] as i32;
        apply(&mut world, &operation).unwrap();

        assert_eq!(
            world.get_block(DIM, BlockPos::new(0, 59, 0)).unwrap(),
            state("minecraft:gold_block"),
            "blocks below baseY must stay untouched"
        );
        for y in 60..=height {
            let expected = if y >= height - 1 {
                "minecraft:dirt"
            } else {
                "minecraft:cobblestone"
            };
            assert_eq!(
                world.get_block(DIM, BlockPos::new(0, y, 0)).unwrap(),
                state(expected),
                "unexpected state at y={y}, generated height {height}"
            );
        }
        for y in (height + 1)..=70 {
            assert_eq!(
                world.get_block(DIM, BlockPos::new(0, y, 0)).unwrap(),
                air(),
                "generated air missing at y={y}, generated height {height}"
            );
        }
    }

    #[test]
    fn heightmap_patch_is_deterministic_bounded_and_reversible() {
        let bounds = BlockBox::new(BlockPos::new(-3, 60, -2), BlockPos::new(4, 72, 5)).unwrap();
        let mut first = World::new_empty();
        let mut second = World::new_empty();
        for world in [&mut first, &mut second] {
            for x in -3..=4 {
                for z in -2..=5 {
                    for y in 60..=62 {
                        world
                            .set_block(DIM, BlockPos::new(x, y, z), &state("minecraft:stone"))
                            .unwrap();
                    }
                }
            }
        }
        let operation = heightmap_operation(
            bounds,
            HeightmapSpec {
                seed: 0x5EED,
                scale: 4,
                octaves: 3,
                amplitude: 7,
                ..HeightmapSpec::default()
            },
        );
        assert_eq!(operation.affected_blocks(), bounds.volume());
        let preview_first = preview(&first, &operation).unwrap();
        let preview_second = preview(&second, &operation).unwrap();
        assert_eq!(preview_first.changed_blocks, preview_second.changed_blocks);
        assert_eq!(preview_first.dirty_chunks, preview_second.dirty_chunks);
        assert!(preview_first.changed_blocks > 0);

        let before_count = first.block_count();
        let applied = apply(&mut first, &operation).unwrap();
        apply(&mut second, &operation).unwrap();
        let mut heights = BTreeSet::new();
        for x in -3..=4 {
            for z in -2..=5 {
                let mut top = None;
                for y in (60..=72).rev() {
                    if !first
                        .get_block(DIM, BlockPos::new(x, y, z))
                        .unwrap()
                        .is_air()
                    {
                        top = Some(y);
                        break;
                    }
                }
                let top = top.expect("generated column must not be empty");
                assert!((60..=67).contains(&top));
                assert_eq!(
                    first.get_block(DIM, BlockPos::new(x, top, z)).unwrap(),
                    state("minecraft:grass_block")
                );
                for y in 63..top {
                    assert_eq!(
                        first.get_block(DIM, BlockPos::new(x, y, z)).unwrap(),
                        state("minecraft:dirt")
                    );
                }
                heights.insert(top);
            }
        }
        assert!(heights.len() > 1, "noise should produce varied heights");

        for diff in applied.diffs.iter().rev() {
            diff.revert(&mut first).unwrap();
        }
        assert_eq!(first.block_count(), before_count);
        assert_eq!(
            first.get_block(DIM, BlockPos::new(0, 61, 0)).unwrap(),
            state("minecraft:stone")
        );
    }

    #[test]
    fn heightmap_parameters_are_validated_before_planning() {
        let bounds = BlockBox::new(BlockPos::new(0, 60, 0), BlockPos::new(1, 70, 1)).unwrap();
        let world = World::new_empty();
        let invalid = |spec: HeightmapSpec<'_>| {
            matches!(
                preview(&world, &heightmap_operation(bounds, spec)),
                Err(OperationError::InvalidHeightmap(_))
            )
        };
        for scale in [0, 65] {
            assert!(invalid(HeightmapSpec {
                scale,
                ..HeightmapSpec::default()
            }));
        }
        for octaves in [0, 7] {
            assert!(invalid(HeightmapSpec {
                octaves,
                ..HeightmapSpec::default()
            }));
        }
        for amplitude in [0, 257] {
            assert!(invalid(HeightmapSpec {
                amplitude,
                ..HeightmapSpec::default()
            }));
        }
        for surface_depth in [0, 33] {
            assert!(invalid(HeightmapSpec {
                surface_depth,
                ..HeightmapSpec::default()
            }));
        }
        assert!(invalid(HeightmapSpec {
            surface: "minecraft:air",
            ..HeightmapSpec::default()
        }));
        assert!(invalid(HeightmapSpec {
            filler: "minecraft:air",
            ..HeightmapSpec::default()
        }));
        for base_y in [59, 65, 71] {
            assert!(invalid(HeightmapSpec {
                base_y,
                ..HeightmapSpec::default()
            }));
        }
        assert!(preview(
            &world,
            &heightmap_operation(bounds, HeightmapSpec::default())
        )
        .is_ok());
    }
}
