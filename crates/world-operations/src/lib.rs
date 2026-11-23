//! World operations: selections, fills, replace, batched edits and section diffs.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use world_domain::{
    BiomeState, BlockBox, BlockPos, BlockState, BoundsError, ChunkPos, Section, World, WorldError,
};
use world_generation::{
    generate_heightmap, generate_route, scatter_roll, tree_height, HeightmapParams, RoutePoint,
};

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
/// Widest generated road or river cross-section.
pub const MAX_ROUTE_WIDTH: u32 = world_generation::MAX_ROUTE_WIDTH;
/// Largest lateral wander accepted by the route generators.
pub const MAX_ROUTE_MEANDER: u32 = world_generation::MAX_ROUTE_MEANDER;
/// Largest number of air layers generated above a road.
pub const MAX_ROAD_CLEARANCE: u32 = 8;
/// Deepest generated river channel.
pub const MAX_RIVER_DEPTH: u32 = 16;
/// Largest minimum spacing between generated trees.
pub const MAX_TREE_SPACING: u32 = 32;
/// Largest structure footprint edge accepted by the placement rules.
pub const MAX_STRUCTURE_FOOTPRINT: u32 = 32;
/// Largest structure clearance checked above the surface.
pub const MAX_STRUCTURE_CLEARANCE: u32 = 32;
/// Largest surface height difference accepted by the structure rules.
pub const MAX_STRUCTURE_SLOPE: u32 = 32;
/// Largest number of structure sites returned by one query.
pub const MAX_STRUCTURE_SITES: usize = 64;
/// Hard bound for the structure-site block checks in one query.
pub const MAX_STRUCTURE_SITE_CHECKS: u64 = 2_000_000;
/// Biome cells cover 4x4x4 blocks in modern Java worlds.
const BIOME_CELL_SIZE: i32 = 4;
const PLANT_SALT: u64 = 0x51A7_7E12;
const TREE_SALT: u64 = 0x7AEE_5EED;

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
    #[error("invalid local generator: {0}")]
    InvalidGenerator(String),
    #[error("invalid biome operation: {0}")]
    InvalidBiome(String),
    #[error("structure site scan would check {requested} candidates, limit is {limit}")]
    TooManySiteCandidates { requested: u64, limit: u64 },
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
    /// Deterministic local road between two XZ positions. Only blocks inside the
    /// selection are written.
    GenerateRoad {
        dimension: String,
        bounds: BlockBox,
        from: BlockPos,
        to: BlockPos,
        #[serde(default = "default_route_width")]
        width: u32,
        #[serde(default = "default_route_meander")]
        meander: u32,
        #[serde(default)]
        seed: u64,
        #[serde(default = "default_road_clearance")]
        clearance: u32,
        surface: BlockState,
    },
    /// Deterministic local river channel between two XZ positions.
    GenerateRiver {
        dimension: String,
        bounds: BlockBox,
        from: BlockPos,
        to: BlockPos,
        #[serde(default = "default_route_width")]
        width: u32,
        #[serde(default = "default_route_meander")]
        meander: u32,
        #[serde(default)]
        seed: u64,
        #[serde(default = "default_river_depth")]
        depth: u32,
        water: BlockState,
        bed: BlockState,
    },
    /// Deterministic scatter of plants and small trees on existing surfaces.
    GenerateVegetation {
        dimension: String,
        bounds: BlockBox,
        #[serde(default)]
        seed: u64,
        #[serde(default = "default_ground_cover", rename = "groundCover")]
        ground_cover: u32,
        #[serde(default = "default_tree_chance", rename = "treeChance")]
        tree_chance: u32,
        #[serde(default = "default_tree_spacing", rename = "treeSpacing")]
        tree_spacing: u32,
        plant: BlockState,
        trunk: BlockState,
        leaves: BlockState,
    },
    /// Paints the 4x4x4 biome cells covering the selection. Requires imported or generated
    /// biome data; it never invents a biome for a world that has none.
    SetBiome {
        dimension: String,
        bounds: BlockBox,
        biome: BiomeState,
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

fn default_route_width() -> u32 {
    3
}

fn default_route_meander() -> u32 {
    4
}

fn default_road_clearance() -> u32 {
    2
}

fn default_river_depth() -> u32 {
    3
}

fn default_ground_cover() -> u32 {
    35
}

fn default_tree_chance() -> u32 {
    2
}

fn default_tree_spacing() -> u32 {
    5
}

impl Operation {
    pub fn dimension(&self) -> &str {
        match self {
            Operation::SetBlock { dimension, .. }
            | Operation::Batch { dimension, .. }
            | Operation::FillCuboid { dimension, .. }
            | Operation::Replace { dimension, .. }
            | Operation::TerrainBrush { dimension, .. }
            | Operation::GenerateHeightmap { dimension, .. }
            | Operation::GenerateRoad { dimension, .. }
            | Operation::GenerateRiver { dimension, .. }
            | Operation::GenerateVegetation { dimension, .. }
            | Operation::SetBiome { dimension, .. } => dimension,
        }
    }

    /// Upper bound of blocks the operation may inspect.
    pub fn affected_blocks(&self) -> u64 {
        match self {
            Operation::SetBlock { .. } => 1,
            Operation::Batch { changes, .. } => changes.len() as u64,
            Operation::FillCuboid { bounds, .. }
            | Operation::Replace { bounds, .. }
            | Operation::TerrainBrush { bounds, .. }
            | Operation::GenerateRoad { bounds, .. }
            | Operation::GenerateRiver { bounds, .. }
            | Operation::GenerateVegetation { bounds, .. }
            | Operation::SetBiome { bounds, .. } => bounds.volume(),
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
            | Operation::GenerateHeightmap { bounds, .. }
            | Operation::GenerateRoad { bounds, .. }
            | Operation::GenerateRiver { bounds, .. }
            | Operation::GenerateVegetation { bounds, .. }
            | Operation::SetBiome { bounds, .. } => Some(*bounds),
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
        Operation::SetBiome { bounds, .. } => {
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
        Operation::GenerateRoad {
            dimension,
            bounds,
            from,
            to,
            width,
            meander,
            seed,
            clearance,
            surface,
        } => {
            check(bounds.min)?;
            check(bounds.max)?;
            validate_route(&RouteRequest {
                dimension,
                bounds: *bounds,
                from: *from,
                to: *to,
                width: *width,
                meander: *meander,
                seed: *seed,
            })?;
            if *clearance > MAX_ROAD_CLEARANCE {
                return Err(OperationError::InvalidGenerator(format!(
                    "road clearance must be 0..={MAX_ROAD_CLEARANCE}, got {clearance}"
                )));
            }
            if surface.is_air() {
                return Err(OperationError::InvalidGenerator(
                    "road surface must not be minecraft:air".to_owned(),
                ));
            }
        }
        Operation::GenerateRiver {
            dimension,
            bounds,
            from,
            to,
            width,
            meander,
            seed,
            depth,
            water,
            bed,
        } => {
            check(bounds.min)?;
            check(bounds.max)?;
            validate_route(&RouteRequest {
                dimension,
                bounds: *bounds,
                from: *from,
                to: *to,
                width: *width,
                meander: *meander,
                seed: *seed,
            })?;
            if !(1..=MAX_RIVER_DEPTH).contains(depth) {
                return Err(OperationError::InvalidGenerator(format!(
                    "river depth must be 1..={MAX_RIVER_DEPTH}, got {depth}"
                )));
            }
            if water.is_air() || bed.is_air() {
                return Err(OperationError::InvalidGenerator(
                    "river water and bed must not be minecraft:air".to_owned(),
                ));
            }
        }
        Operation::GenerateVegetation {
            dimension,
            bounds,
            seed,
            ground_cover,
            tree_chance,
            tree_spacing,
            plant,
            trunk,
            leaves,
        } => {
            check(bounds.min)?;
            check(bounds.max)?;
            validate_vegetation(&VegetationRequest {
                dimension,
                bounds: *bounds,
                seed: *seed,
                ground_cover: *ground_cover,
                tree_chance: *tree_chance,
                tree_spacing: *tree_spacing,
                plant,
                trunk,
                leaves,
            })?;
        }
    }
    Ok(())
}

struct RouteRequest<'a> {
    dimension: &'a str,
    bounds: BlockBox,
    from: BlockPos,
    to: BlockPos,
    width: u32,
    meander: u32,
    seed: u64,
}

fn validate_route(request: &RouteRequest<'_>) -> Result<(), OperationError> {
    if !(1..=MAX_ROUTE_WIDTH).contains(&request.width) {
        return Err(OperationError::InvalidGenerator(format!(
            "route width must be 1..={MAX_ROUTE_WIDTH}, got {}",
            request.width
        )));
    }
    if request.meander > MAX_ROUTE_MEANDER {
        return Err(OperationError::InvalidGenerator(format!(
            "route meander must be 0..={MAX_ROUTE_MEANDER}, got {}",
            request.meander
        )));
    }
    if !route_xz_inside(request.bounds, request.from) {
        return Err(OperationError::InvalidGenerator(format!(
            "route start {:?} is outside the selection XZ bounds {:?}..{:?}",
            request.from, request.bounds.min, request.bounds.max
        )));
    }
    if !route_xz_inside(request.bounds, request.to) {
        return Err(OperationError::InvalidGenerator(format!(
            "route end {:?} is outside the selection XZ bounds {:?}..{:?}",
            request.to, request.bounds.min, request.bounds.max
        )));
    }
    Ok(())
}

fn route_xz_inside(bounds: BlockBox, pos: BlockPos) -> bool {
    pos.x >= bounds.min.x && pos.x <= bounds.max.x && pos.z >= bounds.min.z && pos.z <= bounds.max.z
}

struct VegetationRequest<'a> {
    dimension: &'a str,
    bounds: BlockBox,
    seed: u64,
    ground_cover: u32,
    tree_chance: u32,
    tree_spacing: u32,
    plant: &'a BlockState,
    trunk: &'a BlockState,
    leaves: &'a BlockState,
}

fn validate_vegetation(request: &VegetationRequest<'_>) -> Result<(), OperationError> {
    if request.ground_cover > 100 {
        return Err(OperationError::InvalidGenerator(format!(
            "groundCover must be 0..=100, got {}",
            request.ground_cover
        )));
    }
    if request.tree_chance > 100 {
        return Err(OperationError::InvalidGenerator(format!(
            "treeChance must be 0..=100, got {}",
            request.tree_chance
        )));
    }
    if !(1..=MAX_TREE_SPACING).contains(&request.tree_spacing) {
        return Err(OperationError::InvalidGenerator(format!(
            "treeSpacing must be 1..={MAX_TREE_SPACING}, got {}",
            request.tree_spacing
        )));
    }
    if request.ground_cover == 0 && request.tree_chance == 0 {
        return Err(OperationError::InvalidGenerator(
            "at least one of groundCover or treeChance must be above zero".to_owned(),
        ));
    }
    if request.plant.is_air() || request.trunk.is_air() || request.leaves.is_air() {
        return Err(OperationError::InvalidGenerator(
            "vegetation materials must not be minecraft:air".to_owned(),
        ));
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
                if done == total || done.is_multiple_of(1024) {
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
                if done == total || done.is_multiple_of(4096) {
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
                if done == total || done.is_multiple_of(4096) {
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
        Operation::GenerateRoad {
            dimension,
            bounds,
            from,
            to,
            width,
            meander,
            seed,
            clearance,
            surface,
        } => {
            return plan_road(
                world,
                RouteRequest {
                    dimension,
                    bounds: *bounds,
                    from: *from,
                    to: *to,
                    width: *width,
                    meander: *meander,
                    seed: *seed,
                },
                *clearance,
                surface,
                observer,
            );
        }
        Operation::GenerateRiver {
            dimension,
            bounds,
            from,
            to,
            width,
            meander,
            seed,
            depth,
            water,
            bed,
        } => {
            return plan_river(
                world,
                RouteRequest {
                    dimension,
                    bounds: *bounds,
                    from: *from,
                    to: *to,
                    width: *width,
                    meander: *meander,
                    seed: *seed,
                },
                *depth,
                water,
                bed,
                observer,
            );
        }
        Operation::GenerateVegetation {
            dimension,
            bounds,
            seed,
            ground_cover,
            tree_chance,
            tree_spacing,
            plant,
            trunk,
            leaves,
        } => {
            return plan_vegetation(
                world,
                VegetationRequest {
                    dimension,
                    bounds: *bounds,
                    seed: *seed,
                    ground_cover: *ground_cover,
                    tree_chance: *tree_chance,
                    tree_spacing: *tree_spacing,
                    plant,
                    trunk,
                    leaves,
                },
                observer,
            );
        }
        Operation::SetBiome { .. } => {
            return Err(OperationError::InvalidBiome(
                "biome operations use the dedicated biome planning path".to_owned(),
            ));
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
            if done == total || done.is_multiple_of(64) {
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
            if done == total || done.is_multiple_of(256) {
                report(observer, OperationPhase::Planning, done, total)?;
            }
        }
    }
    Ok(PlannedChanges {
        changes,
        warnings: Vec::new(),
    })
}

/// Deterministic local road. The surface follows the topmost non-air block of each
/// column inside the selection; the road never bridges a fluid or builds from the
/// selection floor. Reapplying the same road does not stack a second layer.
fn plan_road(
    world: &World,
    request: RouteRequest<'_>,
    clearance: u32,
    surface: &BlockState,
    observer: &dyn OperationObserver,
) -> Result<PlannedChanges, OperationError> {
    let route = generate_route(
        RoutePoint::new(request.from.x, request.from.z),
        RoutePoint::new(request.to.x, request.to.z),
        request.width,
        request.meander,
        request.seed,
    )
    .map_err(|error| OperationError::InvalidGenerator(error.to_string()))?;
    let total = route.cells.len().max(1) as u64;
    report(observer, OperationPhase::Planning, 0, total)?;

    let air = BlockState::default();
    let mut desired: BTreeMap<BlockPos, BlockState> = BTreeMap::new();
    let mut clipped = 0usize;
    let mut missing = 0usize;
    let mut fluid = 0usize;
    let mut done = 0u64;
    for (x, z) in route.cells {
        done += 1;
        if done == total || done.is_multiple_of(128) {
            report(observer, OperationPhase::Planning, done, total)?;
        }
        if x < request.bounds.min.x
            || x > request.bounds.max.x
            || z < request.bounds.min.z
            || z > request.bounds.max.z
        {
            clipped += 1;
            continue;
        }
        let Some((surface_y, top)) =
            topmost_surface(world, request.dimension, request.bounds, x, z)?
        else {
            missing += 1;
            continue;
        };
        if is_fluid(&top) {
            fluid += 1;
            continue;
        }
        let road_y = if top == *surface {
            surface_y
        } else {
            surface_y + 1
        };
        if road_y < request.bounds.min.y || road_y > request.bounds.max.y {
            clipped += 1;
            continue;
        }
        desired.insert(BlockPos::new(x, road_y, z), surface.clone());
        let clear_to = (road_y + clearance as i32).min(request.bounds.max.y);
        for y in (road_y + 1)..=clear_to {
            desired.insert(BlockPos::new(x, y, z), air.clone());
        }
    }

    let mut warnings = Vec::new();
    if clipped > 0 {
        warnings.push(format!(
            "{clipped} route column(s) were outside the selection or could not fit its Y range"
        ));
    }
    if missing > 0 {
        warnings.push(format!(
            "{missing} route column(s) had no terrain surface and were skipped"
        ));
    }
    if fluid > 0 {
        warnings.push(format!(
            "{fluid} route column(s) rested on a fluid and were skipped"
        ));
    }
    let changes = finalize_changes(world, request.dimension, desired)?;
    Ok(PlannedChanges { changes, warnings })
}

/// Deterministic river channel. It carves down from the topmost solid surface and fills
/// the channel with water over a bed. Columns already covered by fluid are skipped, so a
/// repeated application does not deepen the same river.
fn plan_river(
    world: &World,
    request: RouteRequest<'_>,
    depth: u32,
    water: &BlockState,
    bed: &BlockState,
    observer: &dyn OperationObserver,
) -> Result<PlannedChanges, OperationError> {
    let route = generate_route(
        RoutePoint::new(request.from.x, request.from.z),
        RoutePoint::new(request.to.x, request.to.z),
        request.width,
        request.meander,
        request.seed,
    )
    .map_err(|error| OperationError::InvalidGenerator(error.to_string()))?;
    let total = route.cells.len().max(1) as u64;
    report(observer, OperationPhase::Planning, 0, total)?;

    let mut desired: BTreeMap<BlockPos, BlockState> = BTreeMap::new();
    let mut clipped = 0usize;
    let mut missing = 0usize;
    let mut fluid = 0usize;
    let mut done = 0u64;
    for (x, z) in route.cells {
        done += 1;
        if done == total || done.is_multiple_of(128) {
            report(observer, OperationPhase::Planning, done, total)?;
        }
        if x < request.bounds.min.x
            || x > request.bounds.max.x
            || z < request.bounds.min.z
            || z > request.bounds.max.z
        {
            clipped += 1;
            continue;
        }
        let Some((surface_y, top)) =
            topmost_surface(world, request.dimension, request.bounds, x, z)?
        else {
            missing += 1;
            continue;
        };
        if is_fluid(&top) {
            fluid += 1;
            continue;
        }
        let depth = i32::try_from(depth).unwrap_or(i32::MAX);
        let water_bottom = (surface_y - depth + 1).max(request.bounds.min.y);
        let bed_y = water_bottom - 1;
        if bed_y >= request.bounds.min.y {
            desired.insert(BlockPos::new(x, bed_y, z), bed.clone());
        }
        for y in water_bottom..=surface_y {
            desired.insert(BlockPos::new(x, y, z), water.clone());
        }
    }

    let mut warnings = Vec::new();
    if clipped > 0 {
        warnings.push(format!(
            "{clipped} river column(s) were outside the selection"
        ));
    }
    if missing > 0 {
        warnings.push(format!(
            "{missing} river column(s) had no terrain surface and were skipped"
        ));
    }
    if fluid > 0 {
        warnings.push(format!(
            "{fluid} river column(s) already contained fluid and were left unchanged"
        ));
    }
    let changes = finalize_changes(world, request.dimension, desired)?;
    Ok(PlannedChanges { changes, warnings })
}

/// Deterministic vegetation scatter. Plants and trees are placed only into existing air;
/// trees require a fully free trunk/canopy volume and respect a minimum spacing. A tree
/// that would overwrite a block or another planned feature is skipped instead of clipping.
fn plan_vegetation(
    world: &World,
    request: VegetationRequest<'_>,
    observer: &dyn OperationObserver,
) -> Result<PlannedChanges, OperationError> {
    let width = (i64::from(request.bounds.max.x) - i64::from(request.bounds.min.x) + 1).max(0);
    let depth = (i64::from(request.bounds.max.z) - i64::from(request.bounds.min.z) + 1).max(0);
    let total = (width * depth) as u64;
    report(observer, OperationPhase::Planning, 0, total)?;

    let mut desired: BTreeMap<BlockPos, BlockState> = BTreeMap::new();
    let mut columns: Vec<(i32, i32, i32)> = Vec::new();
    let mut accepted_trees: Vec<(i32, i32)> = Vec::new();
    let mut empty = 0usize;
    let mut fluid = 0usize;
    let mut existing_vegetation = 0usize;
    let mut rejected_spacing = 0usize;
    let mut rejected_clearance = 0usize;
    let mut trees = 0usize;
    let mut done = 0u64;

    for z in request.bounds.min.z..=request.bounds.max.z {
        for x in request.bounds.min.x..=request.bounds.max.x {
            done += 1;
            if done == total || done.is_multiple_of(256) {
                report(observer, OperationPhase::Planning, done, total)?;
            }
            let Some((surface_y, top)) =
                topmost_surface(world, request.dimension, request.bounds, x, z)?
            else {
                empty += 1;
                continue;
            };
            if is_fluid(&top) {
                fluid += 1;
                continue;
            }
            if is_existing_vegetation(&top) {
                existing_vegetation += 1;
                continue;
            }
            columns.push((x, z, surface_y));

            if request.tree_chance == 0
                || scatter_roll(request.seed, TREE_SALT, x, z) >= request.tree_chance
            {
                continue;
            }
            let spacing = request.tree_spacing as i32;
            if accepted_trees
                .iter()
                .any(|(tree_x, tree_z)| (x - tree_x).abs().max((z - tree_z).abs()) < spacing)
            {
                rejected_spacing += 1;
                continue;
            }

            let height = tree_height(request.seed, x, z) as i32;
            let top_y = surface_y + height;
            let radius = 2i32;
            if x - radius < request.bounds.min.x
                || x + radius > request.bounds.max.x
                || z - radius < request.bounds.min.z
                || z + radius > request.bounds.max.z
                || top_y > request.bounds.max.y
            {
                rejected_clearance += 1;
                continue;
            }

            let mut tree_positions: BTreeMap<BlockPos, BlockState> = BTreeMap::new();
            for y in (surface_y + 1)..=top_y {
                tree_positions.insert(BlockPos::new(x, y, z), request.trunk.clone());
            }
            for (layer, layer_radius) in [(0i32, 1i32), (-1, 2), (-2, 2)] {
                let y = top_y + layer;
                for offset_x in -layer_radius..=layer_radius {
                    for offset_z in -layer_radius..=layer_radius {
                        if offset_x == 0 && offset_z == 0 {
                            continue;
                        }
                        tree_positions.insert(
                            BlockPos::new(x + offset_x, y, z + offset_z),
                            request.leaves.clone(),
                        );
                    }
                }
            }

            let mut blocked = false;
            for pos in tree_positions.keys() {
                if desired.contains_key(pos) || !world.get_block(request.dimension, *pos)?.is_air()
                {
                    blocked = true;
                    break;
                }
            }
            if blocked {
                rejected_clearance += 1;
                continue;
            }
            for (pos, state) in tree_positions {
                desired.insert(pos, state);
            }
            accepted_trees.push((x, z));
            trees += 1;
        }
    }

    let mut plants = 0usize;
    for (x, z, surface_y) in columns {
        if request.ground_cover == 0
            || scatter_roll(request.seed, PLANT_SALT, x, z) >= request.ground_cover
        {
            continue;
        }
        let plant_y = surface_y + 1;
        if plant_y > request.bounds.max.y {
            continue;
        }
        let pos = BlockPos::new(x, plant_y, z);
        if desired.contains_key(&pos) {
            continue;
        }
        if !world.get_block(request.dimension, pos)?.is_air() {
            continue;
        }
        desired.insert(pos, request.plant.clone());
        plants += 1;
    }

    let mut warnings = Vec::new();
    if empty > 0 {
        warnings.push(format!(
            "{empty} column(s) had no terrain surface and were skipped"
        ));
    }
    if fluid > 0 {
        warnings.push(format!(
            "{fluid} column(s) were covered by fluid and were skipped"
        ));
    }
    if existing_vegetation > 0 {
        warnings.push(format!(
            "{existing_vegetation} column(s) already had leaves or a trunk and were skipped"
        ));
    }
    if rejected_spacing > 0 {
        warnings.push(format!(
            "{rejected_spacing} tree(s) were skipped to keep the configured spacing"
        ));
    }
    if rejected_clearance > 0 {
        warnings.push(format!(
            "{rejected_clearance} tree(s) were skipped because their canopy did not fit or was obstructed"
        ));
    }
    let changes = finalize_changes(world, request.dimension, desired)?;
    warnings.push(format!(
        "{trees} tree(s) and {plants} plant(s) planned inside the selection"
    ));
    Ok(PlannedChanges { changes, warnings })
}

fn topmost_surface(
    world: &World,
    dimension: &str,
    bounds: BlockBox,
    x: i32,
    z: i32,
) -> Result<Option<(i32, BlockState)>, OperationError> {
    for y in (bounds.min.y..=bounds.max.y).rev() {
        let state = world.get_block(dimension, BlockPos::new(x, y, z))?;
        if !state.is_air() {
            return Ok(Some((y, state)));
        }
    }
    Ok(None)
}

fn is_fluid(state: &BlockState) -> bool {
    matches!(state.name(), "minecraft:water" | "minecraft:lava")
}

fn is_existing_vegetation(state: &BlockState) -> bool {
    let name = state.name();
    name.ends_with("_leaves")
        || name.ends_with("_log")
        || name.ends_with("_wood")
        || name == "minecraft:short_grass"
        || name == "minecraft:tall_grass"
        || name == "minecraft:fern"
        || name == "minecraft:large_fern"
}

fn finalize_changes(
    world: &World,
    dimension: &str,
    desired: BTreeMap<BlockPos, BlockState>,
) -> Result<Vec<BlockChange>, OperationError> {
    let mut changes = Vec::with_capacity(desired.len());
    for (pos, state) in desired {
        if world.get_block(dimension, pos)? != state {
            changes.push(BlockChange { pos, state });
        }
    }
    Ok(changes)
}

/// Rules for a deterministic, greedy structure-site search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StructureSiteRules {
    pub width: u32,
    pub depth: u32,
    pub step: u32,
    pub max_slope: u32,
    pub clearance: u32,
    pub limit: usize,
}

/// One valid site. `origin` is the minimum XZ corner at the highest surface plus one;
/// the caller can use the min/max surface heights to adapt the structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructureSite {
    pub origin: BlockPos,
    pub min_surface_y: i32,
    pub max_surface_y: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructureSiteSearch {
    pub sites: Vec<StructureSite>,
    pub scanned_candidates: u64,
    pub rejected_candidates: u64,
}

/// Finds bounded flat, clear and non-overlapping structure sites. The rules are a
/// deterministic heuristic, not a claim about vanilla structure generation.
pub fn find_structure_sites(
    world: &World,
    dimension: &str,
    bounds: BlockBox,
    rules: StructureSiteRules,
) -> Result<StructureSiteSearch, OperationError> {
    world.dimension(dimension)?;
    if !(1..=MAX_STRUCTURE_FOOTPRINT).contains(&rules.width)
        || !(1..=MAX_STRUCTURE_FOOTPRINT).contains(&rules.depth)
    {
        return Err(OperationError::InvalidGenerator(format!(
            "structure width and depth must be 1..={MAX_STRUCTURE_FOOTPRINT}"
        )));
    }
    if !(1..=64).contains(&rules.step) {
        return Err(OperationError::InvalidGenerator(format!(
            "structure step must be 1..=64, got {}",
            rules.step
        )));
    }
    if rules.max_slope > MAX_STRUCTURE_SLOPE {
        return Err(OperationError::InvalidGenerator(format!(
            "structure maxSlope must be 0..={MAX_STRUCTURE_SLOPE}, got {}",
            rules.max_slope
        )));
    }
    if rules.clearance > MAX_STRUCTURE_CLEARANCE {
        return Err(OperationError::InvalidGenerator(format!(
            "structure clearance must be 0..={MAX_STRUCTURE_CLEARANCE}, got {}",
            rules.clearance
        )));
    }
    if rules.limit == 0 || rules.limit > MAX_STRUCTURE_SITES {
        return Err(OperationError::InvalidGenerator(format!(
            "structure limit must be 1..={MAX_STRUCTURE_SITES}, got {}",
            rules.limit
        )));
    }

    let bounds_width = (i64::from(bounds.max.x) - i64::from(bounds.min.x) + 1).max(0) as u64;
    let bounds_depth = (i64::from(bounds.max.z) - i64::from(bounds.min.z) + 1).max(0) as u64;
    if u64::from(rules.width) > bounds_width || u64::from(rules.depth) > bounds_depth {
        return Ok(StructureSiteSearch {
            sites: Vec::new(),
            scanned_candidates: 0,
            rejected_candidates: 0,
        });
    }
    let step = u64::from(rules.step);
    let candidates_x = (bounds_width - u64::from(rules.width)) / step + 1;
    let candidates_z = (bounds_depth - u64::from(rules.depth)) / step + 1;
    let candidates = candidates_x.saturating_mul(candidates_z);
    let checks = candidates
        .saturating_mul(u64::from(rules.width))
        .saturating_mul(u64::from(rules.depth))
        .saturating_mul(u64::from(rules.clearance) + 1);
    if checks > MAX_STRUCTURE_SITE_CHECKS {
        return Err(OperationError::TooManySiteCandidates {
            requested: checks,
            limit: MAX_STRUCTURE_SITE_CHECKS,
        });
    }

    let last_x = bounds.max.x - rules.width as i32 + 1;
    let last_z = bounds.max.z - rules.depth as i32 + 1;
    let mut sites = Vec::new();
    let mut scanned = 0u64;
    let mut rejected = 0u64;
    'outer: for z in (bounds.min.z..=last_z).step_by(rules.step as usize) {
        for x in (bounds.min.x..=last_x).step_by(rules.step as usize) {
            scanned += 1;
            if sites.iter().any(|site: &StructureSite| {
                x < site.origin.x + rules.width as i32
                    && x + rules.width as i32 > site.origin.x
                    && z < site.origin.z + rules.depth as i32
                    && z + rules.depth as i32 > site.origin.z
            }) {
                rejected += 1;
                continue;
            }
            let mut min_surface = i32::MAX;
            let mut max_surface = i32::MIN;
            let mut valid = true;
            'columns: for offset_z in 0..rules.depth as i32 {
                for offset_x in 0..rules.width as i32 {
                    let column_x = x + offset_x;
                    let column_z = z + offset_z;
                    let Some((surface_y, top)) =
                        topmost_surface(world, dimension, bounds, column_x, column_z)?
                    else {
                        valid = false;
                        break 'columns;
                    };
                    if is_fluid(&top) {
                        valid = false;
                        break 'columns;
                    }
                    min_surface = min_surface.min(surface_y);
                    max_surface = max_surface.max(surface_y);
                    for offset_y in 1..=rules.clearance as i32 {
                        let check_y = surface_y + offset_y;
                        if check_y > bounds.max.y
                            || !world
                                .get_block(dimension, BlockPos::new(column_x, check_y, column_z))?
                                .is_air()
                        {
                            valid = false;
                            break 'columns;
                        }
                    }
                }
            }
            if !valid || max_surface - min_surface > rules.max_slope as i32 {
                rejected += 1;
                continue;
            }
            sites.push(StructureSite {
                origin: BlockPos::new(x, max_surface + 1, z),
                min_surface_y: min_surface,
                max_surface_y: max_surface,
            });
            if sites.len() >= rules.limit {
                break 'outer;
            }
        }
    }
    Ok(StructureSiteSearch {
        sites,
        scanned_candidates: scanned,
        rejected_candidates: rejected,
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

#[derive(Debug, Clone, Copy)]
struct BiomeCell {
    x: i32,
    y: i32,
    z: i32,
}

impl BiomeCell {
    fn representative(self) -> BlockPos {
        BlockPos::new(
            self.x * BIOME_CELL_SIZE,
            self.y * BIOME_CELL_SIZE,
            self.z * BIOME_CELL_SIZE,
        )
    }
}

struct PlannedBiomes {
    cells: Vec<BiomeCell>,
    changed_blocks: u64,
    warnings: Vec<String>,
}

/// Plans a biome paint cell by cell. Imported worlds always carry biome data; an empty or
/// partially loaded world is refused instead of silently inventing a default biome.
fn plan_biomes(
    world: &World,
    dimension: &str,
    bounds: BlockBox,
    biome: &BiomeState,
    observer: &dyn OperationObserver,
) -> Result<PlannedBiomes, OperationError> {
    let min_cell_x = bounds.min.x.div_euclid(BIOME_CELL_SIZE);
    let max_cell_x = bounds.max.x.div_euclid(BIOME_CELL_SIZE);
    let min_cell_y = bounds.min.y.div_euclid(BIOME_CELL_SIZE);
    let max_cell_y = bounds.max.y.div_euclid(BIOME_CELL_SIZE);
    let min_cell_z = bounds.min.z.div_euclid(BIOME_CELL_SIZE);
    let max_cell_z = bounds.max.z.div_euclid(BIOME_CELL_SIZE);
    let total = (i64::from(max_cell_x) - i64::from(min_cell_x) + 1).max(0) as u64
        * (i64::from(max_cell_y) - i64::from(min_cell_y) + 1).max(0) as u64
        * (i64::from(max_cell_z) - i64::from(min_cell_z) + 1).max(0) as u64;
    report(observer, OperationPhase::Planning, 0, total)?;

    let mut cells = Vec::new();
    let mut changed_blocks = 0u64;
    let mut done = 0u64;
    for cell_y in min_cell_y..=max_cell_y {
        for cell_z in min_cell_z..=max_cell_z {
            for cell_x in min_cell_x..=max_cell_x {
                let cell = BiomeCell {
                    x: cell_x,
                    y: cell_y,
                    z: cell_z,
                };
                let current = world.get_biome(dimension, cell.representative())?;
                let Some(current) = current else {
                    return Err(OperationError::InvalidBiome(format!(
                        "biome data is missing at {dimension} {:?}; import a Java world or use a project with biome data before painting biomes",
                        cell.representative()
                    )));
                };
                if current != *biome {
                    let x0 = bounds.min.x.max(cell_x * BIOME_CELL_SIZE);
                    let x1 = bounds
                        .max
                        .x
                        .min(cell_x * BIOME_CELL_SIZE + BIOME_CELL_SIZE - 1);
                    let y0 = bounds.min.y.max(cell_y * BIOME_CELL_SIZE);
                    let y1 = bounds
                        .max
                        .y
                        .min(cell_y * BIOME_CELL_SIZE + BIOME_CELL_SIZE - 1);
                    let z0 = bounds.min.z.max(cell_z * BIOME_CELL_SIZE);
                    let z1 = bounds
                        .max
                        .z
                        .min(cell_z * BIOME_CELL_SIZE + BIOME_CELL_SIZE - 1);
                    let blocks = (i64::from(x1) - i64::from(x0) + 1).max(0) as u64
                        * (i64::from(y1) - i64::from(y0) + 1).max(0) as u64
                        * (i64::from(z1) - i64::from(z0) + 1).max(0) as u64;
                    changed_blocks = changed_blocks.saturating_add(blocks);
                    cells.push(cell);
                }
                done += 1;
                if done == total || done.is_multiple_of(64) {
                    report(observer, OperationPhase::Planning, done, total)?;
                }
            }
        }
    }

    Ok(PlannedBiomes {
        cells,
        changed_blocks,
        warnings: Vec::new(),
    })
}

fn apply_biomes(
    world: &mut World,
    dimension: &str,
    biome: &BiomeState,
    planned: &PlannedBiomes,
    observer: &dyn OperationObserver,
) -> Result<AppliedOperation, OperationError> {
    if planned.cells.is_empty() {
        return Ok(AppliedOperation {
            dimension: dimension.to_owned(),
            changed_blocks: 0,
            dirty_chunks: Vec::new(),
            diffs: Vec::new(),
            warnings: planned.warnings.clone(),
        });
    }

    let mut touched: BTreeSet<(ChunkPos, i32)> = BTreeSet::new();
    let mut dirty_chunks = BTreeSet::new();
    for cell in &planned.cells {
        let pos = cell.representative();
        touched.insert((pos.chunk_pos(), pos.section_y()));
        dirty_chunks.insert(pos.chunk_pos());
    }
    let before: Vec<((ChunkPos, i32), Option<Section>)> = touched
        .iter()
        .map(|(chunk, section_y)| {
            (
                (*chunk, *section_y),
                world
                    .section(dimension, *chunk, *section_y)
                    .ok()
                    .flatten()
                    .cloned(),
            )
        })
        .collect();

    let total = planned.cells.len() as u64;
    report(observer, OperationPhase::Applying, 0, total)?;
    for (index, cell) in planned.cells.iter().enumerate() {
        let write = world
            .set_biome(dimension, cell.representative(), biome)
            .map_err(OperationError::World)?;
        if write.is_none() {
            restore_sections(world, dimension, &before);
            return Err(OperationError::InvalidBiome(format!(
                "biome data disappeared at {dimension} {:?} while applying the operation",
                cell.representative()
            )));
        }
        let done = index as u64 + 1;
        if done == total || done.is_multiple_of(64) {
            if let Err(error) = report(observer, OperationPhase::Applying, done, total) {
                restore_sections(world, dimension, &before);
                return Err(error);
            }
        }
    }

    let mut diffs = Vec::new();
    for ((chunk, section_y), before_section) in before {
        let after_section = world.section(dimension, chunk, section_y)?.cloned();
        if before_section != after_section {
            diffs.push(SectionDiff {
                dimension: dimension.to_owned(),
                chunk,
                section_y,
                before: before_section,
                after: after_section,
            });
        }
    }

    Ok(AppliedOperation {
        dimension: dimension.to_owned(),
        changed_blocks: planned.changed_blocks,
        dirty_chunks: dirty_chunks.into_iter().collect(),
        diffs,
        warnings: planned.warnings.clone(),
    })
}

pub fn preview(world: &World, operation: &Operation) -> Result<Preview, OperationError> {
    prevalidate(world, operation)?;
    if let Operation::SetBiome {
        dimension,
        bounds,
        biome,
    } = operation
    {
        let planned = plan_biomes(world, dimension, *bounds, biome, &NoopObserver)?;
        let mut chunks = BTreeSet::new();
        for cell in &planned.cells {
            chunks.insert(cell.representative().chunk_pos());
        }
        return Ok(Preview {
            affected_blocks: operation.affected_blocks(),
            changed_blocks: planned.changed_blocks,
            dirty_chunks: chunks.into_iter().collect(),
            warnings: planned.warnings,
        });
    }
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
    if let Operation::SetBiome {
        dimension,
        bounds,
        biome,
    } = operation
    {
        let planned = plan_biomes(world, dimension, *bounds, biome, observer)?;
        return apply_biomes(world, dimension, biome, &planned, observer);
    }
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
        if done == total || done.is_multiple_of(1024) {
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

    fn biome(name: &str) -> BiomeState {
        name.parse().expect("valid biome")
    }

    fn world_with_biomes() -> World {
        let mut world = World::new_empty();
        world
            .install_biomes(
                DIM,
                ChunkPos::new(0, 0),
                0,
                Some(world_domain::BiomeSection::uniform(biome(
                    "minecraft:plains",
                ))),
            )
            .unwrap();
        world
    }

    #[test]
    fn biome_paint_updates_covering_cells_and_reverts_exactly() {
        let mut world = world_with_biomes();
        let bounds = BlockBox::new(BlockPos::new(0, 0, 0), BlockPos::new(7, 7, 7)).unwrap();
        let operation = Operation::SetBiome {
            dimension: DIM.into(),
            bounds,
            biome: biome("minecraft:forest"),
        };
        let previewed = preview(&world, &operation).unwrap();
        assert_eq!(previewed.affected_blocks, 512);
        assert_eq!(previewed.changed_blocks, 512);
        assert_eq!(previewed.dirty_chunks, vec![ChunkPos::new(0, 0)]);

        let applied = apply(&mut world, &operation).unwrap();
        assert_eq!(applied.changed_blocks, 512);
        assert_eq!(
            world.get_biome(DIM, BlockPos::new(0, 0, 0)).unwrap(),
            Some(biome("minecraft:forest"))
        );
        assert_eq!(
            world.get_biome(DIM, BlockPos::new(7, 7, 7)).unwrap(),
            Some(biome("minecraft:forest"))
        );
        assert_eq!(
            world.get_biome(DIM, BlockPos::new(8, 8, 8)).unwrap(),
            Some(biome("minecraft:plains")),
            "a cell outside the selection must stay untouched"
        );

        for diff in applied.diffs.iter().rev() {
            diff.revert(&mut world).unwrap();
        }
        assert_eq!(
            world.get_biome(DIM, BlockPos::new(0, 0, 0)).unwrap(),
            Some(biome("minecraft:plains"))
        );
        assert_eq!(
            world.get_biome(DIM, BlockPos::new(7, 7, 7)).unwrap(),
            Some(biome("minecraft:plains"))
        );

        for diff in &applied.diffs {
            diff.reapply(&mut world).unwrap();
        }
        assert_eq!(
            world.get_biome(DIM, BlockPos::new(0, 0, 0)).unwrap(),
            Some(biome("minecraft:forest"))
        );
    }

    #[test]
    fn biome_paint_counts_only_changed_cells_and_refuses_missing_data() {
        let mut world = world_with_biomes();
        world
            .set_biome(DIM, BlockPos::new(0, 0, 0), &biome("minecraft:forest"))
            .unwrap()
            .expect("biome data");
        let bounds = BlockBox::new(BlockPos::new(0, 0, 0), BlockPos::new(7, 7, 7)).unwrap();
        let operation = Operation::SetBiome {
            dimension: DIM.into(),
            bounds,
            biome: biome("minecraft:forest"),
        };
        let previewed = preview(&world, &operation).unwrap();
        assert_eq!(previewed.changed_blocks, 448);

        let empty = World::new_empty();
        let error = preview(&empty, &operation).unwrap_err();
        assert!(matches!(error, OperationError::InvalidBiome(_)));
        assert!(error.to_string().contains("biome data is missing"));
    }

    #[test]
    fn road_follows_the_surface_clears_above_and_is_repeatable() {
        let mut world = World::new_empty();
        for x in 0..=4 {
            fill_column(&mut world, x, 0, 60, 64, "minecraft:stone");
        }
        let bounds = BlockBox::new(BlockPos::new(0, 60, 0), BlockPos::new(4, 70, 0)).unwrap();
        let operation = Operation::GenerateRoad {
            dimension: DIM.into(),
            bounds,
            from: BlockPos::new(0, 64, 0),
            to: BlockPos::new(4, 64, 0),
            width: 1,
            meander: 0,
            seed: 0x5EED,
            clearance: 2,
            surface: state("minecraft:gold_block"),
        };
        let applied = apply(&mut world, &operation).unwrap();
        assert_eq!(applied.changed_blocks, 5);
        for x in 0..=4 {
            assert_eq!(
                world.get_block(DIM, BlockPos::new(x, 64, 0)).unwrap(),
                state("minecraft:stone")
            );
            assert_eq!(
                world.get_block(DIM, BlockPos::new(x, 65, 0)).unwrap(),
                state("minecraft:gold_block")
            );
            assert_eq!(
                world.get_block(DIM, BlockPos::new(x, 66, 0)).unwrap(),
                state("minecraft:air")
            );
            assert_eq!(
                world.get_block(DIM, BlockPos::new(x, 67, 0)).unwrap(),
                state("minecraft:air")
            );
        }
        let again = apply(&mut world, &operation).unwrap();
        assert_eq!(again.changed_blocks, 0, "the same road must not stack");
    }

    #[test]
    fn river_carves_a_bed_and_water_without_deepening_on_reapply() {
        let mut world = World::new_empty();
        for x in 0..=4 {
            fill_column(&mut world, x, 0, 60, 64, "minecraft:stone");
        }
        let bounds = BlockBox::new(BlockPos::new(0, 60, 0), BlockPos::new(4, 70, 0)).unwrap();
        let operation = Operation::GenerateRiver {
            dimension: DIM.into(),
            bounds,
            from: BlockPos::new(0, 64, 0),
            to: BlockPos::new(4, 64, 0),
            width: 1,
            meander: 0,
            seed: 0x5EED,
            depth: 3,
            water: state("minecraft:water[level=0]"),
            bed: state("minecraft:sand"),
        };
        let applied = apply(&mut world, &operation).unwrap();
        assert_eq!(applied.changed_blocks, 20);
        for x in 0..=4 {
            assert_eq!(
                world.get_block(DIM, BlockPos::new(x, 60, 0)).unwrap(),
                state("minecraft:stone")
            );
            assert_eq!(
                world.get_block(DIM, BlockPos::new(x, 61, 0)).unwrap(),
                state("minecraft:sand")
            );
            for y in 62..=64 {
                assert_eq!(
                    world.get_block(DIM, BlockPos::new(x, y, 0)).unwrap(),
                    state("minecraft:water[level=0]")
                );
            }
            assert_eq!(
                world.get_block(DIM, BlockPos::new(x, 65, 0)).unwrap(),
                state("minecraft:air")
            );
        }
        let again = apply(&mut world, &operation).unwrap();
        assert_eq!(again.changed_blocks, 0);
        assert!(again
            .warnings
            .iter()
            .any(|warning| warning.contains("fluid")));
    }

    #[test]
    fn vegetation_is_deterministic_and_only_uses_free_air() {
        let mut world = World::new_empty();
        for x in 0..16 {
            for z in 0..16 {
                fill_column(&mut world, x, z, 60, 64, "minecraft:grass_block");
            }
        }
        let bounds = BlockBox::new(BlockPos::new(0, 60, 0), BlockPos::new(15, 80, 15)).unwrap();
        let operation = Operation::GenerateVegetation {
            dimension: DIM.into(),
            bounds,
            seed: 0x5EED,
            ground_cover: 100,
            tree_chance: 100,
            tree_spacing: 5,
            plant: state("minecraft:short_grass"),
            trunk: state("minecraft:oak_log"),
            leaves: state("minecraft:oak_leaves"),
        };
        let first = preview(&world, &operation).unwrap();
        let second = preview(&world, &operation).unwrap();
        assert_eq!(first.changed_blocks, second.changed_blocks);
        assert!(first.changed_blocks > 64);
        let applied = apply(&mut world, &operation).unwrap();
        assert_eq!(applied.changed_blocks, first.changed_blocks);

        let mut logs = 0;
        let mut leaves = 0;
        let mut plants = 0;
        for x in 0..16 {
            for z in 0..16 {
                for y in 65..=80 {
                    let block = world.get_block(DIM, BlockPos::new(x, y, z)).unwrap();
                    match block.name() {
                        "minecraft:oak_log" => logs += 1,
                        "minecraft:oak_leaves" => leaves += 1,
                        "minecraft:short_grass" => plants += 1,
                        _ => {}
                    }
                }
            }
        }
        assert!(logs > 0, "the scatter must place trees");
        assert!(leaves > 0, "the scatter must place tree leaves");
        assert!(plants > 0, "the scatter must place ground cover");
        assert!(
            applied.diffs.iter().all(|diff| diff.dimension == DIM),
            "all diffs stay in the selected dimension"
        );
    }

    #[test]
    fn structure_sites_require_flat_clear_ground_and_do_not_overlap() {
        let mut world = World::new_empty();
        for x in 0..16 {
            for z in 0..16 {
                fill_column(&mut world, x, z, 60, 64, "minecraft:stone");
            }
        }
        let bounds = BlockBox::new(BlockPos::new(0, 60, 0), BlockPos::new(15, 70, 15)).unwrap();
        let rules = StructureSiteRules {
            width: 3,
            depth: 3,
            step: 3,
            max_slope: 0,
            clearance: 2,
            limit: 4,
        };
        let search = find_structure_sites(&world, DIM, bounds, rules).unwrap();
        assert_eq!(search.sites.len(), 4);
        assert!((4..=25).contains(&search.scanned_candidates));
        assert!(search.sites.iter().all(|site| site.origin.y == 65));
        assert!(search
            .sites
            .iter()
            .all(|site| site.min_surface_y == 64 && site.max_surface_y == 64));
        assert!(search.sites.windows(2).all(|pair| {
            let first = pair[0];
            let second = pair[1];
            first.origin.x + 3 <= second.origin.x
                || second.origin.x + 3 <= first.origin.x
                || first.origin.z + 3 <= second.origin.z
                || second.origin.z + 3 <= first.origin.z
        }));

        let blocked_bounds =
            BlockBox::new(BlockPos::new(0, 60, 0), BlockPos::new(2, 70, 2)).unwrap();
        world
            .set_block(DIM, BlockPos::new(1, 65, 1), &state("minecraft:stone"))
            .unwrap();
        let blocked = find_structure_sites(
            &world,
            DIM,
            blocked_bounds,
            StructureSiteRules { limit: 1, ..rules },
        )
        .unwrap();
        assert!(blocked.sites.is_empty());
        assert_eq!(blocked.rejected_candidates, 1);
    }

    #[test]
    fn route_generators_reject_a_position_outside_the_selection() {
        let world = World::new_empty();
        let bounds = BlockBox::new(BlockPos::new(0, 60, 0), BlockPos::new(3, 70, 3)).unwrap();
        let operation = Operation::GenerateRoad {
            dimension: DIM.into(),
            bounds,
            from: BlockPos::new(10, 64, 0),
            to: BlockPos::new(0, 64, 0),
            width: 1,
            meander: 0,
            seed: 0,
            clearance: 1,
            surface: state("minecraft:stone_bricks"),
        };
        let error = preview(&world, &operation).unwrap_err();
        assert!(matches!(error, OperationError::InvalidGenerator(_)));
        assert!(error.to_string().contains("outside the selection"));
    }
}
