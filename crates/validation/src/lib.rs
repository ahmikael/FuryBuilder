//! Bounded, explainable validation of buildings in the authoritative voxel world.
//!
//! The validators are read-only heuristics and structural checks, never a Minecraft
//! simulation. Every finding carries a stable rule id, severity, confidence, coordinates,
//! the revision is attached by the application layer, and simple problems include a
//! suggested [`Operation`] that must still go through preview and a confirmed transaction.
//! All scans are bounded before any block is read and never mutate the world.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use world_domain::{BlockBox, BlockPos, BlockRegistry, BlockState, Dimension};
use world_operations::{BlockChange, Operation};

/// Version of the finding schema and rule semantics.
pub const VALIDATION_SCHEMA_VERSION: u32 = 1;
/// Largest number of block reads one validation run may perform.
pub const MAX_VALIDATION_BLOCKS: u64 = 2_000_000;
/// Largest number of findings returned by one run.
pub const MAX_FINDINGS: usize = 128;
/// Largest number of coordinates embedded in one finding.
pub const MAX_FINDING_POSITIONS: usize = 64;
/// Largest accepted surface-material guideline.
pub const MAX_SURFACE_MATERIALS: usize = 64;
/// Largest accepted path length in sampled points.
pub const MAX_PATH_STEPS: u32 = 512;
/// Largest accepted path width.
pub const MAX_PATH_WIDTH: u32 = 16;
/// Default number of distinct surface materials before a palette finding is emitted.
pub const DEFAULT_SURFACE_MATERIALS: usize = 32;

const NEIGHBORS: [(i32, i32, i32); 6] = [
    (1, 0, 0),
    (-1, 0, 0),
    (0, 1, 0),
    (0, -1, 0),
    (0, 0, 1),
    (0, 0, -1),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleId {
    FloatingBlocks,
    OutOfBounds,
    PathContinuity,
    Palette,
    ExportCompatibility,
    BlockStateOrientation,
    ProtectedRegion,
}

impl RuleId {
    pub fn label(self) -> &'static str {
        match self {
            Self::FloatingBlocks => "floating_blocks",
            Self::OutOfBounds => "out_of_bounds",
            Self::PathContinuity => "path_continuity",
            Self::Palette => "palette",
            Self::ExportCompatibility => "export_compatibility",
            Self::BlockStateOrientation => "block_state_orientation",
            Self::ProtectedRegion => "protected_region",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// A structural fact derived from coordinates or an exact catalog check.
    Certain,
    /// A bounded approximation; Minecraft does not simulate this rule.
    Heuristic,
}

/// A suggested repair as an ordinary reversible operation. Applying it is never implicit:
/// the caller must preview it and pass the normal permission/transaction checks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestedFix {
    pub description: String,
    pub operation: Operation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub rule_id: RuleId,
    pub severity: Severity,
    pub confidence: Confidence,
    pub message: String,
    pub bounds: BlockBox,
    /// Bounded sample of coordinates; `position_count` is the exact total.
    pub positions: Vec<BlockPos>,
    pub position_count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_fix: Option<SuggestedFix>,
}

/// A protected workspace region supplied by the application layer. Clients cannot inject
/// or override these through this crate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtectedArea {
    pub id: String,
    pub name: String,
    pub bounds: BlockBox,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PathPoint {
    pub x: i32,
    pub z: i32,
}

/// Straight walkable corridor check at an explicit surface level.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PathSpec {
    pub from: PathPoint,
    pub to: PathPoint,
    pub base_y: i32,
    pub width: u32,
    pub material: BlockState,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FloatingFix {
    None,
    Remove,
    #[default]
    Support,
}

/// Internal request built by the application facade; it contains server-side context that
/// is not exposed to MCP clients.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationRequest {
    pub bounds: BlockBox,
    pub rules: Vec<RuleId>,
    pub allowed_bounds: Option<BlockBox>,
    pub path: Option<PathSpec>,
    pub max_surface_materials: Option<usize>,
    pub floating_fix: FloatingFix,
    pub support_state: Option<BlockState>,
    pub protected_regions: Vec<ProtectedArea>,
}

impl ValidationRequest {
    pub fn new(bounds: BlockBox) -> Self {
        Self {
            bounds,
            rules: Vec::new(),
            allowed_bounds: None,
            path: None,
            max_surface_materials: None,
            floating_fix: FloatingFix::default(),
            support_state: None,
            protected_regions: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationOutcome {
    pub findings: Vec<Finding>,
    pub rules: Vec<RuleId>,
    pub scanned_blocks: u64,
    pub truncated: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ValidationError {
    #[error("validation bounds cover {requested} blocks, limit is {limit}")]
    TooManyBlocks { requested: u64, limit: u64 },
    #[error(
        "validation Y range {min}..={max} is outside dimension `{dimension}` ({dimension_min}..={dimension_max})"
    )]
    OutOfBounds {
        dimension: String,
        min: i32,
        max: i32,
        dimension_min: i32,
        dimension_max: i32,
    },
    #[error("path is {points} points long, limit is {limit}")]
    PathTooLong { points: u64, limit: u32 },
    #[error("path width {width} is outside 1..={limit}")]
    InvalidWidth { width: u32, limit: u32 },
    #[error("path continuity requires a path")]
    MissingPath,
    #[error("out_of_bounds requires allowedBounds")]
    MissingAllowedBounds,
    #[error("path material must not be air")]
    InvalidPathMaterial,
    #[error("surface material guideline {requested} is outside 1..={limit}")]
    InvalidMaterialLimit { requested: usize, limit: usize },
    #[error("path coordinates overflow the block position range")]
    PathOverflow,
    #[error("world read failed: {0}")]
    World(String),
}

impl ValidationError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::TooManyBlocks { .. }
            | Self::PathTooLong { .. }
            | Self::InvalidMaterialLimit { .. } => "limit_exceeded",
            Self::OutOfBounds { .. } => "out_of_bounds",
            Self::MissingPath
            | Self::MissingAllowedBounds
            | Self::InvalidWidth { .. }
            | Self::PathOverflow
            | Self::InvalidPathMaterial => "invalid_argument",
            Self::World(_) => "internal",
        }
    }
}

pub fn validate(
    dimension: &Dimension,
    request: &ValidationRequest,
) -> Result<ValidationOutcome, ValidationError> {
    let registry = BlockRegistry::default();
    validate_with_registry(dimension, request, &registry)
}

/// Runs validation against an explicit project catalog. The default entry point keeps the
/// curated quick-pick registry for callers that have no project catalog.
pub fn validate_with_registry(
    dimension: &Dimension,
    request: &ValidationRequest,
    registry: &BlockRegistry,
) -> Result<ValidationOutcome, ValidationError> {
    let requested = request.bounds.volume();
    if requested > MAX_VALIDATION_BLOCKS {
        return Err(ValidationError::TooManyBlocks {
            requested,
            limit: MAX_VALIDATION_BLOCKS,
        });
    }
    check_dimension_y(dimension, request.bounds)?;

    let explicit = !request.rules.is_empty();
    let mut rules = if explicit {
        request.rules.clone()
    } else {
        default_rules()
    };
    rules.sort();
    rules.dedup();

    if explicit {
        if rules.contains(&RuleId::OutOfBounds) && request.allowed_bounds.is_none() {
            return Err(ValidationError::MissingAllowedBounds);
        }
        if rules.contains(&RuleId::PathContinuity) && request.path.is_none() {
            return Err(ValidationError::MissingPath);
        }
    } else {
        // Optional inputs are opt-in: a default run reports which rules it actually ran
        // instead of pretending to check a path or build envelope that was never supplied.
        if request.allowed_bounds.is_none() {
            rules.retain(|rule| *rule != RuleId::OutOfBounds);
        }
        if request.path.is_none() {
            rules.retain(|rule| *rule != RuleId::PathContinuity);
        }
    }

    let material_limit = match request.max_surface_materials {
        Some(limit) if limit == 0 || limit > MAX_SURFACE_MATERIALS => {
            return Err(ValidationError::InvalidMaterialLimit {
                requested: limit,
                limit: MAX_SURFACE_MATERIALS,
            });
        }
        Some(limit) => limit,
        None => DEFAULT_SURFACE_MATERIALS,
    };

    if let Some(path) = &request.path {
        validate_path_input(dimension, path)?;
    }

    let mut sink = FindingSink::default();
    let mut warnings = Vec::new();
    let mut scanned_blocks = 0u64;

    let need_full_scan = rules.iter().any(|rule| {
        matches!(
            rule,
            RuleId::FloatingBlocks | RuleId::ExportCompatibility | RuleId::BlockStateOrientation
        )
    });
    let mut states: BTreeMap<String, StateSample> = BTreeMap::new();
    let mut floating = BTreeSet::new();
    let mut floating_bottom_skipped = 0u64;

    if need_full_scan {
        for pos in request.bounds.iter() {
            let state = read_block(dimension, pos)?;
            scanned_blocks += 1;
            if !state.is_implicit_air()
                && (rules.contains(&RuleId::ExportCompatibility)
                    || rules.contains(&RuleId::BlockStateOrientation))
            {
                let key = state.to_string();
                let sample = states.entry(key).or_insert_with(|| StateSample {
                    state: state.clone(),
                    count: 0,
                    positions: Vec::new(),
                });
                sample.count += 1;
                if sample.positions.len() < MAX_FINDING_POSITIONS {
                    sample.positions.push(pos);
                }
            }

            if rules.contains(&RuleId::FloatingBlocks) && is_structural(state.name()) {
                if pos.y <= request.bounds.min.y {
                    floating_bottom_skipped += 1;
                } else {
                    let below = read_block(dimension, BlockPos::new(pos.x, pos.y - 1, pos.z))?;
                    scanned_blocks += 1;
                    if below.is_air() {
                        floating.insert(pos);
                    }
                }
            }
        }
    }

    if rules.contains(&RuleId::FloatingBlocks) {
        if floating_bottom_skipped > 0 {
            warnings.push(format!(
                "{floating_bottom_skipped} block(s) at the minimum scan Y were treated as supported because the layer below is outside the scan."
            ));
        }
        for cluster in clusters(&floating) {
            let count = cluster.len() as u64;
            let bound = cluster_bounds(&cluster);
            let positions: Vec<BlockPos> = cluster
                .iter()
                .take(MAX_FINDING_POSITIONS)
                .copied()
                .collect();
            let suggested_fix = floating_fix(dimension, request, &cluster);
            sink.push(Finding {
                rule_id: RuleId::FloatingBlocks,
                severity: Severity::Warning,
                confidence: Confidence::Heuristic,
                message: format!(
                    "{count} structural block(s) have air directly below. This is a support heuristic, not a Minecraft stability simulation."
                ),
                bounds: bound,
                positions,
                position_count: count,
                suggested_fix,
            });
        }
    }

    if rules.contains(&RuleId::OutOfBounds) {
        if let Some(allowed) = request.allowed_bounds {
            let mut outside = BTreeSet::new();
            for pos in request.bounds.iter() {
                let state = read_block(dimension, pos)?;
                scanned_blocks += 1;
                if !state.is_air() && !allowed.contains(pos) {
                    outside.insert(pos);
                }
            }
            for cluster in clusters(&outside) {
                let count = cluster.len() as u64;
                let bound = cluster_bounds(&cluster);
                let positions: Vec<BlockPos> = cluster
                    .iter()
                    .take(MAX_FINDING_POSITIONS)
                    .copied()
                    .collect();
                let suggested_fix =
                    remove_cluster(dimension, &cluster, "outside the allowed bounds");
                sink.push(Finding {
                    rule_id: RuleId::OutOfBounds,
                    severity: Severity::Error,
                    confidence: Confidence::Certain,
                    message: format!(
                        "{count} non-air block(s) are outside the declared allowed build bounds."
                    ),
                    bounds: bound,
                    positions,
                    position_count: count,
                    suggested_fix,
                });
            }
        }
    }

    if rules.contains(&RuleId::PathContinuity) {
        if let Some(path) = &request.path {
            validate_path(dimension, path, &mut sink, &mut scanned_blocks)?;
        }
    }

    if rules.contains(&RuleId::Palette) {
        let mut surfaces: BTreeMap<String, (u64, Option<BlockPos>)> = BTreeMap::new();
        for x in request.bounds.min.x..=request.bounds.max.x {
            for z in request.bounds.min.z..=request.bounds.max.z {
                for y in (request.bounds.min.y..=request.bounds.max.y).rev() {
                    let pos = BlockPos::new(x, y, z);
                    let state = read_block(dimension, pos)?;
                    scanned_blocks += 1;
                    if !state.is_air() {
                        let entry = surfaces
                            .entry(state.name().to_owned())
                            .or_insert((0, Some(pos)));
                        entry.0 += 1;
                        break;
                    }
                }
            }
        }
        if surfaces.len() > material_limit {
            let positions: Vec<BlockPos> = surfaces
                .values()
                .filter_map(|(_, pos)| *pos)
                .take(MAX_FINDING_POSITIONS)
                .collect();
            let bound = positions
                .first()
                .map(|first| {
                    positions
                        .iter()
                        .skip(1)
                        .fold(BlockBox::single(*first), |accumulated, pos| {
                            accumulated.union(&BlockBox::single(*pos))
                        })
                })
                .unwrap_or(request.bounds);
            sink.push(Finding {
                rule_id: RuleId::Palette,
                severity: Severity::Info,
                confidence: Confidence::Certain,
                message: format!(
                    "Surface columns use {} distinct block names, above the guideline of {material_limit}. Review palette consistency.",
                    surfaces.len()
                ),
                bounds: bound,
                positions,
                position_count: surfaces.len() as u64,
                suggested_fix: None,
            });
        }
    }

    if rules.contains(&RuleId::ExportCompatibility) {
        for (key, sample) in &states {
            let unknown = !registry.is_known(sample.state.name());
            let invalid = if unknown {
                None
            } else {
                registry.validate(&sample.state).err()
            };
            let reason = if unknown {
                Some(format!(
                    "`{key}` is not present in the active write registry"
                ))
            } else {
                invalid.map(|error| format!("`{key}` has unsupported properties: {error}"))
            };
            if let Some(reason) = reason {
                sink.push(Finding {
                    rule_id: RuleId::ExportCompatibility,
                    severity: Severity::Error,
                    confidence: Confidence::Certain,
                    message: format!(
                        "{reason}. Existing blocks are left unchanged; the state cannot be placed as a new block and may not be supported by the exported world's target."
                    ),
                    bounds: positions_bounds(&sample.positions).unwrap_or(request.bounds),
                    positions: sample.positions.clone(),
                    position_count: sample.count,
                    suggested_fix: None,
                });
            }
        }
    }

    if rules.contains(&RuleId::BlockStateOrientation) {
        for sample in states.values() {
            if !registry.is_known(sample.state.name()) {
                continue;
            }
            if let Some(issue) = orientation_issue(&sample.state) {
                sink.push(Finding {
                    rule_id: RuleId::BlockStateOrientation,
                    severity: Severity::Warning,
                    confidence: Confidence::Certain,
                    message: format!("`{}` {issue}.", sample.state),
                    bounds: positions_bounds(&sample.positions).unwrap_or(request.bounds),
                    positions: sample.positions.clone(),
                    position_count: sample.count,
                    suggested_fix: None,
                });
            }
        }
    }

    if rules.contains(&RuleId::ProtectedRegion) {
        for area in &request.protected_regions {
            if let Some(intersection) = area.bounds.intersection(&request.bounds) {
                sink.push(Finding {
                    rule_id: RuleId::ProtectedRegion,
                    severity: Severity::Error,
                    confidence: Confidence::Certain,
                    message: format!(
                        "The validation region intersects protected region `{}` ({}). Writes there are rejected by the workspace policy.",
                        area.name, area.id
                    ),
                    bounds: intersection,
                    positions: Vec::new(),
                    position_count: 0,
                    suggested_fix: None,
                });
            }
        }
    }

    let truncated = sink.truncated;
    if truncated {
        warnings.push(format!(
            "Findings were truncated at {MAX_FINDINGS}; narrow the bounds to inspect the rest."
        ));
    }
    let findings = sink.finish();

    Ok(ValidationOutcome {
        findings,
        rules,
        scanned_blocks,
        truncated,
        warnings,
    })
}

fn default_rules() -> Vec<RuleId> {
    vec![
        RuleId::FloatingBlocks,
        RuleId::OutOfBounds,
        RuleId::PathContinuity,
        RuleId::Palette,
        RuleId::ExportCompatibility,
        RuleId::BlockStateOrientation,
        RuleId::ProtectedRegion,
    ]
}

fn check_dimension_y(dimension: &Dimension, bounds: BlockBox) -> Result<(), ValidationError> {
    if dimension.contains_y(bounds.min.y) && dimension.contains_y(bounds.max.y) {
        return Ok(());
    }
    Err(ValidationError::OutOfBounds {
        dimension: dimension.id().to_owned(),
        min: bounds.min.y,
        max: bounds.max.y,
        dimension_min: dimension.min_y(),
        dimension_max: dimension.max_y(),
    })
}

fn validate_path_input(dimension: &Dimension, path: &PathSpec) -> Result<(), ValidationError> {
    if path.material.is_air() {
        return Err(ValidationError::InvalidPathMaterial);
    }
    if path.width == 0 || path.width > MAX_PATH_WIDTH {
        return Err(ValidationError::InvalidWidth {
            width: path.width,
            limit: MAX_PATH_WIDTH,
        });
    }
    if !dimension.contains_y(path.base_y) {
        return Err(ValidationError::OutOfBounds {
            dimension: dimension.id().to_owned(),
            min: path.base_y,
            max: path.base_y,
            dimension_min: dimension.min_y(),
            dimension_max: dimension.max_y(),
        });
    }
    let points = path_points(path);
    if points.len() as u64 > u64::from(MAX_PATH_STEPS) {
        return Err(ValidationError::PathTooLong {
            points: points.len() as u64,
            limit: MAX_PATH_STEPS,
        });
    }
    Ok(())
}

fn validate_path(
    dimension: &Dimension,
    path: &PathSpec,
    sink: &mut FindingSink,
    scanned_blocks: &mut u64,
) -> Result<(), ValidationError> {
    let mut corridor = BTreeSet::new();
    for point in path_points(path) {
        for offset in path_offsets(path.width) {
            let x = point
                .x
                .checked_add(offset.0)
                .ok_or(ValidationError::PathOverflow)?;
            let z = point
                .z
                .checked_add(offset.1)
                .ok_or(ValidationError::PathOverflow)?;
            corridor.insert((x, z));
        }
    }

    let mut gaps = BTreeSet::new();
    let mut obstructed = BTreeSet::new();
    for (x, z) in corridor {
        let pos = BlockPos::new(x, path.base_y, z);
        let floor = read_block(dimension, pos)?;
        *scanned_blocks += 1;
        if floor.is_air() {
            gaps.insert(pos);
        }
        let above_y = path.base_y.saturating_add(1);
        if dimension.contains_y(above_y) {
            let above = read_block(dimension, BlockPos::new(x, above_y, z))?;
            *scanned_blocks += 1;
            if !above.is_air() {
                obstructed.insert(pos);
            }
        }
    }

    if !gaps.is_empty() {
        let count = gaps.len() as u64;
        let positions: Vec<BlockPos> = gaps.iter().take(MAX_FINDING_POSITIONS).copied().collect();
        let suggested_fix = path_fix(dimension, path, &gaps);
        sink.push(Finding {
            rule_id: RuleId::PathContinuity,
            severity: Severity::Error,
            confidence: Confidence::Certain,
            message: format!(
                "{count} path column(s) have no block at the declared surface Y={}.",
                path.base_y
            ),
            bounds: positions_bounds(&positions).unwrap_or(BlockBox::single(BlockPos::new(
                path.from.x,
                path.base_y,
                path.from.z,
            ))),
            positions,
            position_count: count,
            suggested_fix,
        });
    }

    if !obstructed.is_empty() {
        let count = obstructed.len() as u64;
        let positions: Vec<BlockPos> = obstructed
            .iter()
            .take(MAX_FINDING_POSITIONS)
            .copied()
            .collect();
        sink.push(Finding {
            rule_id: RuleId::PathContinuity,
            severity: Severity::Warning,
            confidence: Confidence::Certain,
            message: format!(
                "{count} path column(s) are obstructed above the declared surface Y={}.",
                path.base_y
            ),
            bounds: positions_bounds(&positions).unwrap_or(BlockBox::single(BlockPos::new(
                path.from.x,
                path.base_y,
                path.from.z,
            ))),
            positions,
            position_count: count,
            suggested_fix: None,
        });
    }

    Ok(())
}

fn path_points(path: &PathSpec) -> Vec<PathPoint> {
    let dx = i64::from(path.to.x) - i64::from(path.from.x);
    let dz = i64::from(path.to.z) - i64::from(path.from.z);
    let mut x = path.from.x;
    let mut z = path.from.z;
    let step_x = if dx > 0 {
        1
    } else if dx < 0 {
        -1
    } else {
        0
    };
    let step_z = if dz > 0 {
        1
    } else if dz < 0 {
        -1
    } else {
        0
    };
    let mut err = dx.abs() - dz.abs();
    let mut points = vec![PathPoint { x, z }];
    while x != path.to.x || z != path.to.z {
        let doubled = err * 2;
        if doubled > -dz.abs() {
            err -= dz.abs();
            x += step_x;
        }
        if doubled < dx.abs() {
            err += dx.abs();
            z += step_z;
        }
        points.push(PathPoint { x, z });
        if points.len() > MAX_PATH_STEPS as usize {
            break;
        }
    }
    points
}

fn path_offsets(width: u32) -> Vec<(i32, i32)> {
    let half = i32::try_from(width / 2).unwrap_or(0);
    let low = -i32::try_from((width - 1) / 2).unwrap_or(0);
    let mut offsets = Vec::new();
    for dx in low..=half {
        for dz in low..=half {
            offsets.push((dx, dz));
        }
    }
    offsets
}

fn path_fix(
    dimension: &Dimension,
    path: &PathSpec,
    gaps: &BTreeSet<BlockPos>,
) -> Option<SuggestedFix> {
    if gaps.len() > MAX_FINDING_POSITIONS {
        return None;
    }
    let changes: Vec<BlockChange> = gaps
        .iter()
        .map(|pos| BlockChange {
            pos: *pos,
            state: path.material.clone(),
        })
        .collect();
    let operation = batch_operation(dimension, changes)?;
    Some(SuggestedFix {
        description: format!(
            "Fill {} path column(s) at Y={} with {}.",
            gaps.len(),
            path.base_y,
            path.material
        ),
        operation,
    })
}

fn floating_fix(
    dimension: &Dimension,
    request: &ValidationRequest,
    cluster: &[BlockPos],
) -> Option<SuggestedFix> {
    if cluster.len() > MAX_FINDING_POSITIONS {
        return None;
    }
    match request.floating_fix {
        FloatingFix::None => None,
        FloatingFix::Remove => remove_cluster(dimension, cluster, "unsupported"),
        FloatingFix::Support => {
            let support = request
                .support_state
                .clone()
                .unwrap_or_else(|| "minecraft:stone".parse().expect("stone is a valid state"));
            let mut changes = Vec::new();
            for pos in cluster {
                let below_y = pos.y.checked_sub(1)?;
                if dimension.contains_y(below_y) {
                    changes.push(BlockChange {
                        pos: BlockPos::new(pos.x, below_y, pos.z),
                        state: support.clone(),
                    });
                }
            }
            let operation = batch_operation(dimension, changes)?;
            Some(SuggestedFix {
                description: format!(
                    "Place {support} below {} unsupported block(s) in this cluster.",
                    cluster.len()
                ),
                operation,
            })
        }
    }
}

fn remove_cluster(
    dimension: &Dimension,
    cluster: &[BlockPos],
    reason: &str,
) -> Option<SuggestedFix> {
    if cluster.len() > MAX_FINDING_POSITIONS {
        return None;
    }
    let changes: Vec<BlockChange> = cluster
        .iter()
        .map(|pos| BlockChange {
            pos: *pos,
            state: BlockState::default(),
        })
        .collect();
    let operation = batch_operation(dimension, changes)?;
    Some(SuggestedFix {
        description: format!(
            "Remove {} block(s) {reason} in this cluster.",
            cluster.len()
        ),
        operation,
    })
}

fn batch_operation(dimension: &Dimension, changes: Vec<BlockChange>) -> Option<Operation> {
    if changes.is_empty() {
        None
    } else {
        Some(Operation::Batch {
            dimension: dimension.id().to_owned(),
            changes,
        })
    }
}

fn clusters(positions: &BTreeSet<BlockPos>) -> Vec<Vec<BlockPos>> {
    let mut remaining = positions.clone();
    let mut result = Vec::new();
    while let Some(start) = remaining.iter().next().copied() {
        remaining.remove(&start);
        let mut cluster = vec![start];
        let mut index = 0;
        while index < cluster.len() {
            let current = cluster[index];
            index += 1;
            for (dx, dy, dz) in NEIGHBORS {
                let neighbor = BlockPos::new(
                    current.x.saturating_add(dx),
                    current.y.saturating_add(dy),
                    current.z.saturating_add(dz),
                );
                if remaining.remove(&neighbor) {
                    cluster.push(neighbor);
                }
            }
        }
        cluster.sort();
        result.push(cluster);
    }
    result
}

fn cluster_bounds(cluster: &[BlockPos]) -> BlockBox {
    let first = *cluster.first().expect("a cluster is never empty");
    cluster
        .iter()
        .skip(1)
        .fold(BlockBox::single(first), |accumulated, pos| {
            accumulated.union(&BlockBox::single(*pos))
        })
}

fn positions_bounds(positions: &[BlockPos]) -> Option<BlockBox> {
    let first = *positions.first()?;
    Some(
        positions
            .iter()
            .skip(1)
            .fold(BlockBox::single(first), |accumulated, pos| {
                accumulated.union(&BlockBox::single(*pos))
            }),
    )
}

fn read_block(dimension: &Dimension, pos: BlockPos) -> Result<BlockState, ValidationError> {
    dimension
        .get_block(pos)
        .map_err(|error| ValidationError::World(error.to_string()))
}

fn base_name(name: &str) -> &str {
    name.split_once(':').map_or(name, |(_, path)| path)
}

fn is_structural(name: &str) -> bool {
    let base = base_name(name);
    const SOLID: [&str; 49] = [
        "stone",
        "cobblestone",
        "mossy_cobblestone",
        "smooth_stone",
        "granite",
        "diorite",
        "andesite",
        "deepslate",
        "cobbled_deepslate",
        "calcite",
        "tuff",
        "dripstone_block",
        "dirt",
        "coarse_dirt",
        "rooted_dirt",
        "grass_block",
        "podzol",
        "mycelium",
        "mud",
        "packed_mud",
        "clay",
        "gravel",
        "sand",
        "red_sand",
        "sandstone",
        "smooth_sandstone",
        "red_sandstone",
        "bricks",
        "mud_bricks",
        "glass",
        "tinted_glass",
        "ice",
        "packed_ice",
        "blue_ice",
        "snow_block",
        "obsidian",
        "bedrock",
        "netherrack",
        "end_stone",
        "glowstone",
        "sea_lantern",
        "terracotta",
        "coal_block",
        "iron_block",
        "gold_block",
        "diamond_block",
        "emerald_block",
        "redstone_block",
        "lapis_block",
    ];
    if SOLID.contains(&base) {
        return true;
    }
    const SUFFIXES: [&str; 17] = [
        "_planks",
        "_log",
        "_wood",
        "_stem",
        "_hyphae",
        "_stone",
        "_bricks",
        "_ore",
        "_block",
        "_terracotta",
        "_concrete",
        "_concrete_powder",
        "_sandstone",
        "_stairs",
        "_slab",
        "_fence",
        "_wall",
    ];
    SUFFIXES.iter().any(|suffix| base.ends_with(suffix))
}

fn orientation_issue(state: &BlockState) -> Option<String> {
    let base = base_name(state.name());
    let properties = state.properties();
    if base.ends_with("_stairs") {
        if !is_cardinal(properties.get("facing")) {
            return Some("has a missing or non-cardinal facing".to_owned());
        }
        if !matches!(
            properties.get("half").map(String::as_str),
            Some("top" | "bottom")
        ) {
            return Some("has a missing or invalid half".to_owned());
        }
    } else if base.ends_with("_door") {
        if !is_cardinal(properties.get("facing")) {
            return Some("has a missing or non-cardinal facing".to_owned());
        }
        if !matches!(
            properties.get("half").map(String::as_str),
            Some("upper" | "lower")
        ) {
            return Some("has a missing or invalid half".to_owned());
        }
        if !matches!(
            properties.get("hinge").map(String::as_str),
            Some("left" | "right")
        ) {
            return Some("has a missing or invalid hinge".to_owned());
        }
    } else if base.ends_with("_trapdoor") {
        if !is_cardinal(properties.get("facing")) {
            return Some("has a missing or non-cardinal facing".to_owned());
        }
        if !matches!(
            properties.get("half").map(String::as_str),
            Some("top" | "bottom")
        ) {
            return Some("has a missing or invalid half".to_owned());
        }
    } else if base.ends_with("_fence_gate") || base == "wall_torch" {
        if !is_cardinal(properties.get("facing")) {
            return Some("has a missing or non-cardinal facing".to_owned());
        }
    } else if (base.ends_with("_log")
        || base.ends_with("_wood")
        || base.ends_with("_stem")
        || base.ends_with("_hyphae")
        || base.ends_with("_pillar"))
        && !matches!(
            properties.get("axis").map(String::as_str),
            Some("x" | "y" | "z")
        )
    {
        return Some("has a missing or invalid axis".to_owned());
    }
    None
}

fn is_cardinal(value: Option<&String>) -> bool {
    matches!(
        value.map(String::as_str),
        Some("north" | "south" | "east" | "west")
    )
}

struct StateSample {
    state: BlockState,
    count: u64,
    positions: Vec<BlockPos>,
}

#[derive(Default)]
struct FindingSink {
    findings: Vec<Finding>,
    truncated: bool,
}

impl FindingSink {
    fn push(&mut self, finding: Finding) {
        if self.findings.len() >= MAX_FINDINGS {
            self.truncated = true;
            return;
        }
        self.findings.push(finding);
    }

    fn finish(mut self) -> Vec<Finding> {
        self.findings.sort_by(|left, right| {
            right
                .severity
                .cmp(&left.severity)
                .then_with(|| left.rule_id.cmp(&right.rule_id))
                .then_with(|| left.bounds.min.cmp(&right.bounds.min))
                .then_with(|| left.message.cmp(&right.message))
        });
        self.findings
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(value: &str) -> BlockState {
        value.parse().expect("valid block state")
    }

    fn bounds(min: (i32, i32, i32), max: (i32, i32, i32)) -> BlockBox {
        BlockBox::new(
            BlockPos::new(min.0, min.1, min.2),
            BlockPos::new(max.0, max.1, max.2),
        )
        .expect("valid bounds")
    }

    fn place(dimension: &mut Dimension, pos: (i32, i32, i32), value: &str) {
        dimension
            .set_block(BlockPos::new(pos.0, pos.1, pos.2), &state(value))
            .expect("block write");
    }

    #[test]
    fn floating_clusters_are_grouped_and_offer_a_support_fix() {
        let mut dimension = Dimension::overworld();
        place(&mut dimension, (0, 64, 0), "minecraft:stone");
        place(&mut dimension, (1, 64, 0), "minecraft:stone");
        place(&mut dimension, (3, 64, 0), "minecraft:stone");
        place(&mut dimension, (3, 63, 0), "minecraft:stone");

        let mut request = ValidationRequest::new(bounds((0, 63, 0), (3, 64, 0)));
        request.rules = vec![RuleId::FloatingBlocks];
        let outcome = validate(&dimension, &request).expect("validation");
        assert_eq!(outcome.findings.len(), 1);
        let finding = &outcome.findings[0];
        assert_eq!(finding.rule_id, RuleId::FloatingBlocks);
        assert_eq!(finding.severity, Severity::Warning);
        assert_eq!(finding.confidence, Confidence::Heuristic);
        assert_eq!(finding.position_count, 2);
        assert_eq!(finding.bounds, bounds((0, 64, 0), (1, 64, 0)));
        let fix = finding.suggested_fix.as_ref().expect("support fix");
        match &fix.operation {
            Operation::Batch { changes, .. } => {
                assert_eq!(changes.len(), 2);
                assert!(changes
                    .iter()
                    .any(|change| change.pos == BlockPos::new(0, 63, 0)));
            }
            other => panic!("expected a batch fix, got {other:?}"),
        }
        assert!(fix.description.contains("Place minecraft:stone below 2"));

        request.floating_fix = FloatingFix::Remove;
        let removed = validate(&dimension, &request).expect("remove validation");
        let fix = removed.findings[0]
            .suggested_fix
            .as_ref()
            .expect("remove fix");
        match &fix.operation {
            Operation::Batch { changes, .. } => {
                assert!(changes.iter().all(|change| change.state.is_air()));
            }
            other => panic!("expected a batch fix, got {other:?}"),
        }
    }

    #[test]
    fn out_of_bounds_is_exact_and_fails_closed_without_an_envelope() {
        let mut dimension = Dimension::overworld();
        place(&mut dimension, (0, 64, 0), "minecraft:stone");
        place(&mut dimension, (1, 64, 0), "minecraft:dirt");
        place(&mut dimension, (2, 64, 0), "minecraft:sand");
        let scan = bounds((0, 64, 0), (2, 64, 0));

        let mut request = ValidationRequest::new(scan);
        request.rules = vec![RuleId::OutOfBounds];
        assert_eq!(
            validate(&dimension, &request).unwrap_err(),
            ValidationError::MissingAllowedBounds
        );

        request.allowed_bounds = Some(bounds((0, 64, 0), (0, 64, 0)));
        let outcome = validate(&dimension, &request).expect("validation");
        assert_eq!(outcome.findings.len(), 1);
        let finding = &outcome.findings[0];
        assert_eq!(finding.severity, Severity::Error);
        assert_eq!(finding.confidence, Confidence::Certain);
        assert_eq!(finding.position_count, 2);
        assert_eq!(finding.bounds, bounds((1, 64, 0), (2, 64, 0)));
        let fix = finding.suggested_fix.as_ref().expect("remove fix");
        match &fix.operation {
            Operation::Batch { changes, .. } => assert_eq!(changes.len(), 2),
            other => panic!("expected a batch fix, got {other:?}"),
        }
    }

    #[test]
    fn path_continuity_reports_gaps_and_obstructions() {
        let mut dimension = Dimension::overworld();
        place(&mut dimension, (0, 64, 0), "minecraft:stone");
        place(&mut dimension, (2, 64, 0), "minecraft:stone");
        place(&mut dimension, (2, 65, 0), "minecraft:oak_fence");

        let path = PathSpec {
            from: PathPoint { x: 0, z: 0 },
            to: PathPoint { x: 2, z: 0 },
            base_y: 64,
            width: 1,
            material: state("minecraft:dirt"),
        };
        let mut request = ValidationRequest::new(bounds((0, 64, 0), (2, 65, 0)));
        request.rules = vec![RuleId::PathContinuity];
        request.path = Some(path);
        let outcome = validate(&dimension, &request).expect("validation");
        assert_eq!(outcome.findings.len(), 2);
        let gap = outcome
            .findings
            .iter()
            .find(|finding| finding.severity == Severity::Error)
            .expect("gap finding");
        assert_eq!(gap.position_count, 1);
        assert_eq!(gap.positions, vec![BlockPos::new(1, 64, 0)]);
        let fix = gap.suggested_fix.as_ref().expect("path fix");
        assert!(fix.description.contains("Fill 1 path column"));

        let obstruction = outcome
            .findings
            .iter()
            .find(|finding| finding.severity == Severity::Warning)
            .expect("obstruction finding");
        assert_eq!(obstruction.positions, vec![BlockPos::new(2, 64, 0)]);

        let mut broken = ValidationRequest::new(bounds((0, 64, 0), (2, 65, 0)));
        broken.rules = vec![RuleId::PathContinuity];
        assert_eq!(
            validate(&dimension, &broken).unwrap_err(),
            ValidationError::MissingPath
        );
    }

    #[test]
    fn palette_rule_uses_the_configured_surface_guideline() {
        let mut dimension = Dimension::overworld();
        place(&mut dimension, (0, 64, 0), "minecraft:stone");
        place(&mut dimension, (1, 64, 0), "minecraft:dirt");
        place(&mut dimension, (2, 64, 0), "minecraft:sand");

        let mut request = ValidationRequest::new(bounds((0, 64, 0), (2, 64, 0)));
        request.rules = vec![RuleId::Palette];
        request.max_surface_materials = Some(2);
        let outcome = validate(&dimension, &request).expect("validation");
        assert_eq!(outcome.findings.len(), 1);
        let finding = &outcome.findings[0];
        assert_eq!(finding.rule_id, RuleId::Palette);
        assert_eq!(finding.severity, Severity::Info);
        assert_eq!(finding.position_count, 3);
        assert!(finding.message.contains("3 distinct block names"));

        request.max_surface_materials = Some(MAX_SURFACE_MATERIALS + 1);
        assert!(matches!(
            validate(&dimension, &request),
            Err(ValidationError::InvalidMaterialLimit { .. })
        ));
    }

    #[test]
    fn export_and_orientation_rules_are_catalog_based() {
        let mut dimension = Dimension::overworld();
        place(&mut dimension, (0, 64, 0), "minecraft:test_block_0");
        place(
            &mut dimension,
            (1, 64, 0),
            "minecraft:oak_stairs[facing=sideways,half=bottom]",
        );
        place(&mut dimension, (2, 64, 0), "minecraft:stone[bad=1]");

        let mut request = ValidationRequest::new(bounds((0, 64, 0), (2, 64, 0)));
        request.rules = vec![RuleId::ExportCompatibility, RuleId::BlockStateOrientation];
        let outcome = validate(&dimension, &request).expect("validation");
        let export: Vec<&Finding> = outcome
            .findings
            .iter()
            .filter(|finding| finding.rule_id == RuleId::ExportCompatibility)
            .collect();
        assert_eq!(export.len(), 2);
        assert!(export
            .iter()
            .any(|finding| finding.message.contains("minecraft:test_block_0")));
        assert!(export
            .iter()
            .any(|finding| finding.message.contains("unsupported properties")));
        let orientation = outcome
            .findings
            .iter()
            .find(|finding| finding.rule_id == RuleId::BlockStateOrientation)
            .expect("orientation finding");
        assert!(orientation.message.contains("non-cardinal facing"));
        assert_eq!(orientation.position_count, 1);
    }

    #[test]
    fn protected_regions_are_reported_by_the_core_side_context() {
        let dimension = Dimension::overworld();
        let mut request = ValidationRequest::new(bounds((0, 64, 0), (4, 70, 4)));
        request.rules = vec![RuleId::ProtectedRegion];
        request.protected_regions.push(ProtectedArea {
            id: "spawn".to_owned(),
            name: "Spawn".to_owned(),
            bounds: bounds((2, 66, 2), (8, 72, 8)),
        });
        let outcome = validate(&dimension, &request).expect("validation");
        assert_eq!(outcome.findings.len(), 1);
        let finding = &outcome.findings[0];
        assert_eq!(finding.rule_id, RuleId::ProtectedRegion);
        assert_eq!(finding.severity, Severity::Error);
        assert_eq!(finding.bounds, bounds((2, 66, 2), (4, 70, 4)));
        assert!(finding.message.contains("Spawn"));
        assert!(finding.suggested_fix.is_none());
    }

    #[test]
    fn limits_and_inputs_fail_closed_before_scanning() {
        let dimension = Dimension::overworld();
        let huge = bounds((0, 0, 0), (130, 129, 129));
        let request = ValidationRequest::new(huge);
        assert!(matches!(
            validate(&dimension, &request),
            Err(ValidationError::TooManyBlocks { .. })
        ));

        let scan = bounds((0, 64, 0), (0, 64, 0));
        let mut request = ValidationRequest::new(scan);
        request.rules = vec![RuleId::PathContinuity];
        request.path = Some(PathSpec {
            from: PathPoint { x: 0, z: 0 },
            to: PathPoint { x: 600, z: 0 },
            base_y: 64,
            width: 1,
            material: state("minecraft:dirt"),
        });
        assert!(matches!(
            validate(&dimension, &request),
            Err(ValidationError::PathTooLong { .. })
        ));

        request.path = Some(PathSpec {
            from: PathPoint { x: 0, z: 0 },
            to: PathPoint { x: 2, z: 0 },
            base_y: 64,
            width: MAX_PATH_WIDTH + 1,
            material: state("minecraft:dirt"),
        });
        assert!(matches!(
            validate(&dimension, &request),
            Err(ValidationError::InvalidWidth { .. })
        ));

        request.path = Some(PathSpec {
            from: PathPoint { x: 0, z: 0 },
            to: PathPoint { x: 2, z: 0 },
            base_y: 64,
            width: 1,
            material: state("minecraft:air"),
        });
        assert_eq!(
            validate(&dimension, &request).unwrap_err(),
            ValidationError::InvalidPathMaterial
        );
    }

    #[test]
    fn findings_are_deterministic_and_serialize_with_stable_keys() {
        let mut dimension = Dimension::overworld();
        place(&mut dimension, (0, 64, 0), "minecraft:stone");
        let mut request = ValidationRequest::new(bounds((0, 63, 0), (0, 64, 0)));
        request.rules = vec![RuleId::FloatingBlocks];
        let first = validate(&dimension, &request).expect("first");
        let second = validate(&dimension, &request).expect("second");
        assert_eq!(first, second);

        let json = serde_json::to_value(&first.findings[0]).expect("serializable");
        assert_eq!(json["ruleId"], "floating_blocks");
        assert_eq!(json["severity"], "warning");
        assert_eq!(json["confidence"], "heuristic");
        assert_eq!(json["positionCount"], 1);
        assert_eq!(json["suggestedFix"]["operation"]["type"], "batch");
        assert!(json["bounds"]["min"]["x"].is_number());
    }
}
