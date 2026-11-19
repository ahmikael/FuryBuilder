use std::collections::{BTreeMap, BTreeSet, VecDeque};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use observation::{
    render_cross_section, render_revision_diff, render_surface, BlockDelta,
    MAX_OBSERVATION_SCAN_BLOCKS, OBSERVATION_SCHEMA_VERSION,
};
use scene_model::{
    ObjectKind, ParametricBaseline, ParametricConflictPolicy, ParametricError, ParametricObject,
    ParametricObjectSummary, ParametricScene, ParametricSpec, Scene, SemanticObject,
};
use serde_json::json;
use uuid::Uuid;
use validation::{ProtectedArea, Severity, ValidationRequest};
use workspace_model::WorkspacePolicy;
use world_domain::{BlockBox, BlockPos, BlockRegistry, BlockState, ChunkPos, World, WorldInfo};
use world_operations::{
    self as operations, BlockChange, NoopObserver, Operation, OperationObserver, SectionDiff,
    StructureSiteRules,
};

use crate::dto::{
    Actor, AuditAction, AuditEntry, ChunkSnapshot, ChunkSnapshotBatch, ChunkStats, HistoryState,
    HistorySummary, Observation, ObservationMode, ObservationRequest, OperationRequest,
    OperationResult, PaletteEntry, ParametricApplyResult, ParametricConflict, ParametricInspection,
    PreviewResult, Query, QueryResult, RegionBlocks, RegionSummary, RenderSnapshot,
    SceneObjectInspection, SectionSnapshot, StructureSites, UndoTarget, ValidationCounts,
    ValidationQuery, ValidationReport,
};
use crate::error::{AppError, ErrorCode};

pub const MAX_REGION_SUMMARY_BLOCKS: u64 = 1_000_000;
pub const MAX_REGION_READ_BLOCKS: u64 = 65_536;
pub const MAX_IDEMPOTENCY_ENTRIES: usize = 256;
pub const MAX_RENDER_RADIUS: u32 = 6;
pub const MAX_RENDER_CHUNKS: usize = 256;
pub const DEFAULT_MAX_HISTORY: usize = 100;
pub const MAX_PARAMETRIC_CONFLICTS_REPORTED: usize = 32;
const MAX_PROJECT_ID: usize = 128;
const MAX_KEY_LENGTH: usize = 128;

#[derive(Debug, Clone)]
struct HistoryEntry {
    project_id: String,
    operation_id: String,
    actor: Actor,
    timestamp_ms: u128,
    revision_before: u64,
    revision_after: u64,
    changed_blocks: u64,
    parameter_summary: serde_json::Value,
    linked_object_id: Option<String>,
    layer_id: Option<String>,
    /// Dimension and exact operation extent, kept so an access policy can bound undo
    /// without exposing section payloads.
    extent: Option<(String, BlockBox)>,
    diffs: Vec<SectionDiff>,
}

#[derive(Debug, Clone)]
struct StoredOutcome {
    fingerprint: String,
    result: OperationResult,
}

/// Application facade shared by the UI and MCP. Owns the authoritative world, history,
/// idempotency records and audit trail.
#[derive(Debug)]
pub struct WorldService {
    registry: BlockRegistry,
    world: World,
    history: Vec<HistoryEntry>,
    redo: Vec<HistoryEntry>,
    idempotency: BTreeMap<String, StoredOutcome>,
    idempotency_order: VecDeque<String>,
    audit: Vec<AuditEntry>,
    workspace: WorkspacePolicy,
    scene: Scene,
    parametric: ParametricScene,
    writes_enabled: bool,
    allow_unknown_targets: bool,
    max_history: usize,
}

impl Default for WorldService {
    fn default() -> Self {
        Self::new()
    }
}

impl WorldService {
    pub fn new() -> Self {
        Self {
            registry: BlockRegistry::target_1_21_4(),
            world: World::new_empty(),
            history: Vec::new(),
            redo: Vec::new(),
            idempotency: BTreeMap::new(),
            idempotency_order: VecDeque::new(),
            audit: Vec::new(),
            workspace: WorkspacePolicy::default(),
            scene: Scene::default(),
            parametric: ParametricScene::default(),
            writes_enabled: true,
            allow_unknown_targets: false,
            max_history: DEFAULT_MAX_HISTORY,
        }
    }

    pub fn with_max_history(mut self, max_history: usize) -> Self {
        self.max_history = max_history.max(1);
        self
    }

    /// Builds a service around an already loaded world. History, redo, idempotency and
    /// audit start empty: they belong to a live session, not to the saved project.
    pub fn from_world(world: World) -> Self {
        Self {
            registry: BlockRegistry::target_1_21_4(),
            world,
            history: Vec::new(),
            redo: Vec::new(),
            idempotency: BTreeMap::new(),
            idempotency_order: VecDeque::new(),
            audit: Vec::new(),
            workspace: WorkspacePolicy::default(),
            scene: Scene::default(),
            parametric: ParametricScene::default(),
            writes_enabled: true,
            allow_unknown_targets: false,
            max_history: DEFAULT_MAX_HISTORY,
        }
    }

    pub fn world(&self) -> &World {
        &self.world
    }

    pub fn registry(&self) -> &BlockRegistry {
        &self.registry
    }

    /// Returns the persisted workspace policy currently enforced by this service.
    pub fn workspace_policy(&self) -> WorkspacePolicy {
        self.workspace.clone()
    }

    /// Validates a workspace policy against this world's dimensions without changing
    /// the service. Used by save/open paths that need to reject inconsistent metadata.
    pub fn validate_workspace_policy(&self, policy: &WorkspacePolicy) -> Result<(), AppError> {
        policy.validate()?;
        for region in policy.masks.iter().chain(policy.protected_regions.iter()) {
            self.world.dimension(&region.dimension)?;
        }
        Ok(())
    }

    /// Replaces the editing policy. Policy changes do not bump the world revision or
    /// enter block history; they are project metadata.
    pub fn set_workspace_policy(&mut self, policy: WorkspacePolicy) -> Result<(), AppError> {
        self.validate_workspace_policy(&policy)?;
        self.workspace = policy;
        Ok(())
    }

    /// Returns the persisted semantic catalog. It is project metadata, never voxel data.
    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    /// Validates a scene against this world's dimensions without changing the service.
    pub fn validate_scene(&self, scene: &Scene) -> Result<(), AppError> {
        scene.validate()?;
        for object in scene.objects() {
            self.world.dimension(&object.dimension)?;
        }
        Ok(())
    }

    /// Replaces the semantic catalog. Like the workspace policy, this does not bump the
    /// world revision or enter block history.
    pub fn set_scene(&mut self, scene: Scene) -> Result<(), AppError> {
        self.validate_scene(&scene)?;
        self.scene = scene;
        Ok(())
    }

    /// Adds one semantic object. Only metadata changes; blocks are untouched.
    #[allow(clippy::too_many_arguments)]
    pub fn add_scene_object(
        &mut self,
        name: &str,
        kind: ObjectKind,
        dimension: &str,
        bounds: BlockBox,
        parent_id: Option<String>,
        tags: BTreeMap<String, String>,
        note: Option<String>,
    ) -> Result<SemanticObject, AppError> {
        self.world.dimension(dimension)?;
        Ok(self
            .scene
            .push(name, kind, dimension, bounds, parent_id, tags, note)?
            .clone())
    }

    /// Removes a leaf semantic object. Objects with children are refused.
    pub fn remove_scene_object(&mut self, id: &str) -> Result<SemanticObject, AppError> {
        Ok(self.scene.remove(id)?)
    }

    /// Returns the persisted parametric catalog (recipes plus rollback baselines).
    pub fn parametric_scene(&self) -> &ParametricScene {
        &self.parametric
    }

    pub fn parametric_summaries(&self) -> Result<Vec<ParametricObjectSummary>, AppError> {
        Ok(self.parametric.summaries()?)
    }

    /// Validates a parametric catalog against this world's dimensions without changing the
    /// service. Opening or saving a project calls this fail-closed.
    pub fn validate_parametric_scene(&self, scene: &ParametricScene) -> Result<(), AppError> {
        scene.validate()?;
        for object in scene.objects() {
            let dimension = self.world.dimension(&object.dimension)?;
            let bounds = object.bounds()?;
            if !dimension.contains_y(bounds.min.y) || !dimension.contains_y(bounds.max.y) {
                return Err(AppError::new(
                    ErrorCode::OutOfBounds,
                    format!(
                        "parametric object `{}` spans Y {}..{} outside dimension `{}`",
                        object.id, bounds.min.y, bounds.max.y, object.dimension
                    ),
                ));
            }
        }
        Ok(())
    }

    /// Replaces the parametric catalog. Like the semantic scene, this is metadata and does
    /// not bump the revision or enter block history.
    pub fn set_parametric_scene(&mut self, scene: ParametricScene) -> Result<(), AppError> {
        self.validate_parametric_scene(&scene)?;
        self.parametric = scene;
        Ok(())
    }

    /// Generates the object overlay into the world as one reversible Batch operation and
    /// records the object recipe. The baseline capture happens before the write, so a
    /// rollback can restore terrain and other pre-existing blocks.
    pub fn create_parametric_object(
        &mut self,
        project_id: &str,
        actor: Actor,
        spec: ParametricSpec,
    ) -> Result<ParametricApplyResult, AppError> {
        spec.validate()?;
        self.world.dimension(&spec.dimension)?;
        let id = self.parametric.next_id(&spec.name)?;
        let mut object = ParametricObject {
            id: id.clone(),
            name: spec.name.trim().to_owned(),
            generator: spec.generator,
            generator_version: spec.generator_version,
            dimension: spec.dimension,
            origin: spec.origin,
            rotation: spec.rotation,
            params: spec.params,
            layer_id: spec.layer_id,
            applied_revision: self.world.revision(),
            last_operation_id: None,
            baseline: ParametricBaseline::default(),
        };
        object.validate_shape()?;
        let generated = generated_map(&object)?;
        let mut baseline: BTreeMap<BlockPos, BlockState> = BTreeMap::new();
        for pos in generated.keys() {
            baseline.insert(*pos, self.world.get_block(&object.dimension, *pos)?);
        }
        object.baseline = ParametricBaseline::encode(&baseline)?;
        object.validate()?;

        // Validate catalog limits before mutating the world.
        let mut catalog = self.parametric.clone();
        catalog.insert(object.clone())?;

        let changes: Vec<BlockChange> = generated
            .into_iter()
            .map(|(pos, state)| BlockChange { pos, state })
            .collect();
        if changes.is_empty() {
            return Err(AppError::invalid_argument(
                "parametric object generated no blocks",
            ));
        }
        let result = self.execute(OperationRequest {
            project_id: project_id.to_owned(),
            operation: Operation::Batch {
                dimension: object.dimension.clone(),
                changes,
            },
            expected_revision: None,
            idempotency_key: None,
            actor,
            linked_object_id: Some(id.clone()),
            layer_id: object.layer_id.clone(),
        })?;
        object.applied_revision = result.world_revision;
        object.last_operation_id = Some(result.operation_id.clone());
        catalog.replace(object.clone())?;
        self.parametric = catalog;
        Ok(ParametricApplyResult {
            object: object.summary()?,
            operation: Some(result),
            conflicts: Vec::new(),
        })
    }

    /// Replaces a recipe or transform. The old overlay is checked for manual edits; without
    /// `Override` any mismatch is reported and nothing is written. When accepted, the old
    /// overlay is restored to its captured baseline and the new overlay is written in one
    /// reversible Batch operation.
    pub fn update_parametric_object(
        &mut self,
        project_id: &str,
        actor: Actor,
        id: &str,
        spec: ParametricSpec,
        policy: ParametricConflictPolicy,
    ) -> Result<ParametricApplyResult, AppError> {
        spec.validate()?;
        let old = self
            .parametric
            .get(id)
            .cloned()
            .ok_or_else(|| ParametricError::NotFound(id.to_owned()))?;
        if old.dimension != spec.dimension {
            return Err(AppError::invalid_argument(
                "a parametric object cannot change dimension; bake it and create a new object",
            ));
        }
        let mut updated = ParametricObject {
            id: old.id.clone(),
            name: spec.name.trim().to_owned(),
            generator: spec.generator,
            generator_version: spec.generator_version,
            dimension: old.dimension.clone(),
            origin: spec.origin,
            rotation: spec.rotation,
            params: spec.params,
            layer_id: spec.layer_id,
            applied_revision: old.applied_revision,
            last_operation_id: old.last_operation_id.clone(),
            baseline: ParametricBaseline::default(),
        };
        updated.validate_shape()?;
        self.world.dimension(&updated.dimension)?;

        let old_generated = generated_map(&old)?;
        let new_generated = generated_map(&updated)?;
        let old_baseline = old.baseline.decode()?;
        let mut new_baseline: BTreeMap<BlockPos, BlockState> = BTreeMap::new();
        for pos in new_generated.keys() {
            let state = match old_baseline.get(pos) {
                Some(state) => state.clone(),
                None => self.world.get_block(&updated.dimension, *pos)?,
            };
            new_baseline.insert(*pos, state);
        }
        updated.baseline = ParametricBaseline::encode(&new_baseline)?;
        updated.validate()?;

        let conflicts = self.parametric_conflicts(&old)?;
        if !conflicts.is_empty() && policy == ParametricConflictPolicy::Fail {
            return Err(parametric_conflict_error(&conflicts));
        }

        let mut changes: BTreeMap<BlockPos, BlockState> = BTreeMap::new();
        for pos in old_generated.keys() {
            if !new_generated.contains_key(pos) {
                let state = old_baseline.get(pos).ok_or_else(|| {
                    AppError::internal("parametric baseline is missing an old overlay position")
                })?;
                changes.insert(*pos, state.clone());
            }
        }
        for (pos, state) in &new_generated {
            changes.insert(*pos, state.clone());
        }

        let mut catalog = self.parametric.clone();
        catalog.replace(updated.clone())?;

        let operation = if changes.is_empty() {
            None
        } else {
            let changes: Vec<BlockChange> = changes
                .into_iter()
                .map(|(pos, state)| BlockChange { pos, state })
                .collect();
            let result = self.execute(OperationRequest {
                project_id: project_id.to_owned(),
                operation: Operation::Batch {
                    dimension: updated.dimension.clone(),
                    changes,
                },
                expected_revision: None,
                idempotency_key: None,
                actor,
                linked_object_id: Some(updated.id.clone()),
                layer_id: updated.layer_id.clone(),
            })?;
            updated.applied_revision = result.world_revision;
            updated.last_operation_id = Some(result.operation_id.clone());
            catalog.replace(updated.clone())?;
            Some(result)
        };
        self.parametric = catalog;
        Ok(ParametricApplyResult {
            object: updated.summary()?,
            operation,
            conflicts,
        })
    }

    /// Detaches a parametric object while keeping the blocks it produced in the world.
    /// The recipe, hierarchy and baseline are forgotten; the voxels become ordinary edits.
    pub fn bake_parametric_object(&mut self, id: &str) -> Result<ParametricApplyResult, AppError> {
        let removed = self.parametric.remove(id)?;
        Ok(ParametricApplyResult {
            object: removed.summary()?,
            operation: None,
            conflicts: Vec::new(),
        })
    }

    /// Restores every block captured before the object was first applied and removes the
    /// recipe. Manual edits inside the old overlay are reported unless `force` is set.
    pub fn rollback_parametric_object(
        &mut self,
        project_id: &str,
        actor: Actor,
        id: &str,
        force: bool,
    ) -> Result<ParametricApplyResult, AppError> {
        let object = self
            .parametric
            .get(id)
            .cloned()
            .ok_or_else(|| ParametricError::NotFound(id.to_owned()))?;
        let conflicts = self.parametric_conflicts(&object)?;
        if !conflicts.is_empty() && !force {
            return Err(parametric_conflict_error(&conflicts));
        }
        let baseline = object.baseline.decode()?;
        let changes: Vec<BlockChange> = baseline
            .into_iter()
            .map(|(pos, state)| BlockChange { pos, state })
            .collect();
        if changes.is_empty() {
            self.parametric.remove(id)?;
            return Ok(ParametricApplyResult {
                object: object.summary()?,
                operation: None,
                conflicts,
            });
        }
        let result = self.execute(OperationRequest {
            project_id: project_id.to_owned(),
            operation: Operation::Batch {
                dimension: object.dimension.clone(),
                changes,
            },
            expected_revision: None,
            idempotency_key: None,
            actor,
            linked_object_id: Some(object.id.clone()),
            layer_id: object.layer_id.clone(),
        })?;
        self.parametric.remove(id)?;
        Ok(ParametricApplyResult {
            object: object.summary()?,
            operation: Some(result),
            conflicts,
        })
    }

    pub fn inspect_parametric_object(&self, id: &str) -> Result<ParametricInspection, AppError> {
        let object = self
            .parametric
            .get(id)
            .cloned()
            .ok_or_else(|| ParametricError::NotFound(id.to_owned()))?;
        let conflicts = self.parametric_conflicts(&object)?;
        let conflict_count = conflicts.len() as u64;
        let conflicts = conflicts
            .into_iter()
            .take(MAX_PARAMETRIC_CONFLICTS_REPORTED)
            .collect();
        Ok(ParametricInspection {
            object: object.summary()?,
            generated_blocks: object.baseline.len() as u64,
            conflict_count,
            conflicts,
        })
    }

    fn parametric_conflicts(
        &self,
        object: &ParametricObject,
    ) -> Result<Vec<ParametricConflict>, AppError> {
        let generated = generated_map(object)?;
        let mut conflicts = Vec::new();
        for (pos, expected) in generated {
            let found = self.world.get_block(&object.dimension, pos)?;
            if found != expected {
                conflicts.push(ParametricConflict {
                    pos,
                    expected,
                    found,
                });
            }
        }
        Ok(conflicts)
    }

    fn enforce_workspace(
        &self,
        requested_layer: Option<&str>,
        operation: &Operation,
    ) -> Result<(), AppError> {
        self.workspace.check_operation(
            requested_layer,
            operation.dimension(),
            &operation
                .bounds()
                .ok_or_else(|| AppError::invalid_argument("operation has no block extent"))?,
        )?;
        Ok(())
    }

    pub fn writes_enabled(&self) -> bool {
        self.writes_enabled
    }

    /// Toggles the write policy. MCP connections start read-only and must be granted
    /// write access explicitly by the user (ADR-013).
    pub fn set_writes_enabled(&mut self, enabled: bool) {
        self.writes_enabled = enabled;
    }

    pub fn set_allow_unknown_targets(&mut self, allow: bool) {
        self.allow_unknown_targets = allow;
    }

    pub fn world_info(&self) -> WorldInfo {
        self.world.info()
    }

    /// Bounded section snapshots for the renderer. Missing chunks are returned with
    /// `loaded: false` so the viewport can show holes and request them later.
    pub fn render_snapshot(
        &self,
        dimension: &str,
        center: ChunkPos,
        radius: u32,
    ) -> Result<RenderSnapshot, AppError> {
        if radius > MAX_RENDER_RADIUS {
            return Err(AppError::new(
                ErrorCode::LimitExceeded,
                format!("render radius {radius} exceeds limit {MAX_RENDER_RADIUS}"),
            ));
        }
        let dim = self.world.dimension(dimension)?;
        let mut chunks = Vec::new();
        for dx in -(radius as i32)..=(radius as i32) {
            for dz in -(radius as i32)..=(radius as i32) {
                chunks.push(snapshot_chunk(
                    dim,
                    ChunkPos::new(center.x + dx, center.z + dz),
                ));
            }
        }
        Ok(RenderSnapshot {
            dimension: dimension.to_owned(),
            revision: self.world.revision(),
            center,
            radius,
            chunks,
        })
    }

    /// Snapshots an explicit set of chunks, bounded to `MAX_RENDER_CHUNKS`. Used after an
    /// edit to refresh only dirty chunks and their neighbours.
    pub fn render_chunks(
        &self,
        dimension: &str,
        chunks: &[ChunkPos],
    ) -> Result<ChunkSnapshotBatch, AppError> {
        if chunks.len() > MAX_RENDER_CHUNKS {
            return Err(AppError::new(
                ErrorCode::LimitExceeded,
                format!(
                    "render request contains {} chunks, limit is {MAX_RENDER_CHUNKS}",
                    chunks.len()
                ),
            ));
        }
        let dim = self.world.dimension(dimension)?;
        let mut seen = BTreeSet::new();
        let chunks = chunks
            .iter()
            .filter(|pos| seen.insert(**pos))
            .map(|pos| snapshot_chunk(dim, *pos))
            .collect();
        Ok(ChunkSnapshotBatch {
            dimension: dimension.to_owned(),
            revision: self.world.revision(),
            chunks,
        })
    }

    /// Deterministic spatial observation at the current authoritative revision. The image
    /// is a derived view and never mutates the world. `revision_diff` reconstructs the
    /// prior state from retained section history and fails closed when the requested
    /// revision is no longer reconstructable.
    pub fn observe(&self, request: &ObservationRequest) -> Result<Observation, AppError> {
        let dimension = self.world.dimension(&request.dimension)?;
        let revision = self.world.revision();
        let rendered = match request.mode {
            ObservationMode::TopDown
            | ObservationMode::Heightmap
            | ObservationMode::MaterialMap
            | ObservationMode::BiomeMap => {
                if request.slice.is_some() {
                    return Err(AppError::invalid_argument(
                        "slice is only valid for the cross_section mode",
                    ));
                }
                if request.from_revision.is_some() {
                    return Err(AppError::invalid_argument(
                        "fromRevision is only valid for the revision_diff mode",
                    ));
                }
                render_surface(dimension, request.bounds, request.mode)?
            }
            ObservationMode::CrossSection => {
                if request.from_revision.is_some() {
                    return Err(AppError::invalid_argument(
                        "fromRevision is only valid for the revision_diff mode",
                    ));
                }
                let slice = request.slice.ok_or_else(|| {
                    AppError::invalid_argument("cross_section requires a slice plane")
                })?;
                render_cross_section(dimension, request.bounds, slice)?
            }
            ObservationMode::RevisionDiff => {
                if request.slice.is_some() {
                    return Err(AppError::invalid_argument(
                        "slice is only valid for the cross_section mode",
                    ));
                }
                let from_revision = request.from_revision.ok_or_else(|| {
                    AppError::invalid_argument("revision_diff requires fromRevision")
                })?;
                let deltas =
                    self.revision_deltas(&request.dimension, request.bounds, from_revision)?;
                render_revision_diff(request.bounds, &deltas)?
            }
        };

        let mut notes = vec![
            "Image is a deterministic derived map rendered from the authoritative Rust world."
                .to_owned(),
            format!(
                "Same dimension, bounds, mode and worldRevision {revision} produce the same image."
            ),
            "Use get_block/get_region for exact positions; a colour map is a summary.".to_owned(),
            "Absent or unloaded chunks read as air in the world model; this is not a texture guess."
                .to_owned(),
        ];
        match request.mode {
            ObservationMode::TopDown
            | ObservationMode::Heightmap
            | ObservationMode::MaterialMap
            | ObservationMode::BiomeMap => notes.push(
                "Each pixel summarises one X/Z column; the Y range of bounds is scanned for the top non-air block."
                    .to_owned(),
            ),
            ObservationMode::CrossSection => notes.push(
                "Each pixel is one block on the requested orthogonal plane; X/Z planes use row 0 for the highest Y."
                    .to_owned(),
            ),
            ObservationMode::RevisionDiff => notes.push(
                "The image is a top-down projection of block-level changes between fromRevision and the current revision; counts are exact for the bounds."
                    .to_owned(),
            ),
        }

        Ok(Observation {
            schema_version: OBSERVATION_SCHEMA_VERSION,
            mode: request.mode,
            dimension: request.dimension.clone(),
            world_revision: revision,
            bounds: request.bounds,
            width: rendered.width,
            height: rendered.height,
            blocks_per_pixel: 1,
            image: format!("data:image/png;base64,{}", BASE64.encode(&rendered.png)),
            image_bytes: rendered.png.len(),
            legend: rendered.legend,
            legend_truncated: rendered.legend_truncated,
            partial: false,
            warnings: rendered.warnings,
            notes,
            scanned_blocks: rendered.scanned_blocks,
            sampled_units: rendered.sampled_units,
            non_air_units: rendered.non_air_units,
            counts: rendered.counts,
            from_revision: request.from_revision,
            slice: request.slice,
        })
    }

    /// Runs bounded, explainable validators at the current authoritative revision. The
    /// report is read-only; suggestions inside findings are ordinary operations that still
    /// require preview and the confirmed transaction path.
    pub fn validate_region(&self, query: &ValidationQuery) -> Result<ValidationReport, AppError> {
        let dimension = self.world.dimension(&query.dimension)?;
        let protected_regions = self
            .workspace
            .protected_regions
            .iter()
            .filter(|region| {
                region.dimension == query.dimension && region.bounds.intersects(&query.bounds)
            })
            .map(|region| ProtectedArea {
                id: region.id.clone(),
                name: region.name.clone(),
                bounds: region.bounds,
            })
            .collect();
        let request = ValidationRequest {
            bounds: query.bounds,
            rules: query.rules.clone(),
            allowed_bounds: query.allowed_bounds,
            path: query.path.clone(),
            max_surface_materials: query.max_surface_materials,
            floating_fix: query.floating_fix,
            support_state: None,
            protected_regions,
        };
        let outcome = validation::validate(dimension, &request)?;
        let counts =
            outcome
                .findings
                .iter()
                .fold(ValidationCounts::default(), |mut counts, finding| {
                    match finding.severity {
                        Severity::Error => counts.error += 1,
                        Severity::Warning => counts.warning += 1,
                        Severity::Info => counts.info += 1,
                    }
                    counts
                });
        let revision = self.world.revision();
        let mut notes = vec![
            "Validation is read-only and reports findings at the current authoritative revision."
                .to_owned(),
            format!(
                "The same dimension, bounds and rules at worldRevision {revision} produce the same findings."
            ),
            "Floating-block support is a heuristic; Minecraft does not simulate structure stability."
                .to_owned(),
            "Suggested fixes are ordinary operations and must be previewed and applied through the confirmed transaction path."
                .to_owned(),
            "Absent chunks read as air in the world model; a finding is not proof of unloaded data."
                .to_owned(),
        ];
        if query.rules.is_empty() {
            notes.push(
                "Default rules ran; out_of_bounds and path_continuity are included only when their inputs are supplied."
                    .to_owned(),
            );
        }
        Ok(ValidationReport {
            schema_version: validation::VALIDATION_SCHEMA_VERSION,
            dimension: query.dimension.clone(),
            world_revision: revision,
            bounds: query.bounds,
            rules: outcome.rules,
            findings: outcome.findings,
            counts,
            truncated: outcome.truncated,
            scanned_blocks: outcome.scanned_blocks,
            warnings: outcome.warnings,
            notes,
        })
    }

    /// Reconstructs a bounded block-level diff from the retained section history. The
    /// reconstruction is only accepted when every revision transition between the
    /// requested and current revision is still represented in the live history.
    fn revision_deltas(
        &self,
        dimension: &str,
        bounds: BlockBox,
        from_revision: u64,
    ) -> Result<BTreeMap<BlockPos, BlockDelta>, AppError> {
        let current = self.world.revision();
        if from_revision > current {
            return Err(AppError::invalid_argument(format!(
                "fromRevision {from_revision} is newer than the current revision {current}"
            )));
        }
        let dim = self.world.dimension(dimension)?;
        if !dim.contains_y(bounds.min.y) || !dim.contains_y(bounds.max.y) {
            return Err(AppError::new(
                ErrorCode::OutOfBounds,
                format!(
                    "observation Y range {}..={} is outside dimension `{dimension}` ({}..={})",
                    bounds.min.y,
                    bounds.max.y,
                    dim.min_y(),
                    dim.max_y()
                ),
            ));
        }
        let volume = bounds.volume();
        if volume > MAX_OBSERVATION_SCAN_BLOCKS {
            return Err(AppError::new(
                ErrorCode::LimitExceeded,
                format!(
                    "observation compares {volume} positions, limit is {MAX_OBSERVATION_SCAN_BLOCKS}"
                ),
            ));
        }
        if from_revision == current {
            return Ok(BTreeMap::new());
        }

        let earliest = self.min_reconstructable_revision();
        if from_revision < earliest {
            return Err(AppError::new(
                ErrorCode::NoHistory,
                format!(
                    "revision {from_revision} is older than retained history (earliest reconstructable revision is {earliest})"
                ),
            )
            .with_details(json!({
                "fromRevision": from_revision,
                "earliestRevision": earliest
            })));
        }

        // Walk history backwards and collect the state before every touched position in
        // the requested box. Reverse chronological order means the newest `before`
        // snapshot wins for positions touched by several operations.
        let mut previous: BTreeMap<BlockPos, BlockState> = BTreeMap::new();
        for entry in self.history.iter().rev() {
            if entry.revision_after <= from_revision {
                break;
            }
            for diff in &entry.diffs {
                if diff.dimension != dimension {
                    continue;
                }
                for local_y in 0..16i32 {
                    for local_z in 0..16i32 {
                        for local_x in 0..16i32 {
                            let pos = BlockPos::new(
                                diff.chunk.x * 16 + local_x,
                                diff.section_y * 16 + local_y,
                                diff.chunk.z * 16 + local_z,
                            );
                            if !bounds.contains(pos) {
                                continue;
                            }
                            let state = diff
                                .before
                                .as_ref()
                                .and_then(|section| {
                                    section.get(local_x as u8, local_y as u8, local_z as u8)
                                })
                                .cloned()
                                .unwrap_or_default();
                            previous.insert(pos, state);
                        }
                    }
                }
            }
        }

        let mut deltas = BTreeMap::new();
        for pos in bounds.iter() {
            let current_state = dim
                .get_block(pos)
                .map_err(|error| AppError::internal(error.to_string()))?;
            let previous_state = previous
                .get(&pos)
                .cloned()
                .unwrap_or_else(|| current_state.clone());
            if previous_state == current_state {
                continue;
            }
            let delta = if previous_state.is_air() {
                BlockDelta::Added
            } else if current_state.is_air() {
                BlockDelta::Removed
            } else {
                BlockDelta::Changed
            };
            deltas.insert(pos, delta);
        }
        Ok(deltas)
    }

    /// Earliest revision whose state can be rebuilt by reverting retained history.
    fn min_reconstructable_revision(&self) -> u64 {
        let mut minimum = self.world.revision();
        for entry in self.history.iter().rev() {
            if entry.revision_after != minimum {
                break;
            }
            minimum = entry.revision_before;
        }
        minimum
    }

    /// Seeds a small deterministic demo scene for the viewport milestone. This is
    /// demo content, not a Minecraft world and not a substitute for Vanilla World.
    pub fn load_demo_world(&mut self) -> Result<WorldInfo, AppError> {
        self.world = World::new_empty();
        self.history.clear();
        self.redo.clear();
        self.idempotency.clear();
        self.idempotency_order.clear();
        self.audit.clear();

        const DIMENSION: &str = "minecraft:overworld";
        {
            let mut place = |x: i32, y: i32, z: i32, name: &str| -> Result<(), AppError> {
                let state: world_domain::BlockState = name
                    .parse()
                    .map_err(|error| AppError::internal(format!("invalid demo state: {error}")))?;
                self.world
                    .set_block(DIMENSION, world_domain::BlockPos::new(x, y, z), &state)?;
                Ok(())
            };

            for chunk_x in -1..=1 {
                for chunk_z in -1..=1 {
                    for local_x in 0..16 {
                        for local_z in 0..16 {
                            let x = chunk_x * 16 + local_x;
                            let z = chunk_z * 16 + local_z;
                            for y in -64..61 {
                                place(x, y, z, "minecraft:stone")?;
                            }
                            for y in 61..64 {
                                place(x, y, z, "minecraft:dirt")?;
                            }
                            place(x, 64, z, "minecraft:grass_block")?;
                        }
                    }
                }
            }

            // Small pond.
            for x in 10..=15 {
                for z in 4..=10 {
                    place(x, 64, z, "minecraft:water")?;
                    place(x, 63, z, "minecraft:water")?;
                    place(x, 62, z, "minecraft:water")?;
                }
            }

            // House floor and walls.
            for x in 2..=10 {
                for z in -8..=0 {
                    place(x, 65, z, "minecraft:stone_bricks")?;
                }
            }
            for y in 66..=69 {
                for x in 2..=10 {
                    place(x, y, -8, "minecraft:oak_planks")?;
                    place(x, y, 0, "minecraft:oak_planks")?;
                }
                for z in -8..=0 {
                    place(2, y, z, "minecraft:oak_planks")?;
                    place(10, y, z, "minecraft:oak_planks")?;
                }
            }
            // Door opening and glass windows.
            for y in 66..=67 {
                place(6, y, -8, "minecraft:air")?;
                place(6, y, 0, "minecraft:air")?;
            }
            for x in [4, 8] {
                place(x, 67, -8, "minecraft:glass")?;
                place(x, 67, 0, "minecraft:glass")?;
            }
            for z in [-4, -6] {
                place(2, 67, z, "minecraft:glass")?;
                place(10, 67, z, "minecraft:glass")?;
            }
            // Flat roof with a stair border.
            for x in 1..=11 {
                for z in -9..=1 {
                    place(x, 70, z, "minecraft:dark_oak_planks")?;
                }
            }
            for x in 1..=11 {
                place(x, 71, -9, "minecraft:oak_stairs")?;
                place(x, 71, 1, "minecraft:oak_stairs")?;
            }
            for z in -9..=1 {
                place(1, 71, z, "minecraft:oak_stairs")?;
                place(11, 71, z, "minecraft:oak_stairs")?;
            }
            // Interior light sources.
            for (x, z) in [(4, -6), (8, -2)] {
                place(x, 66, z, "minecraft:torch")?;
                place(x, 70, z, "minecraft:sea_lantern")?;
            }

            // Trees.
            for (tree_x, tree_z) in [(-6, -6), (-6, 6), (12, -10)] {
                for y in 65..=67 {
                    place(tree_x, y, tree_z, "minecraft:oak_log")?;
                }
                for dx in -2i32..=2 {
                    for dz in -2i32..=2 {
                        for dy in 0..=1 {
                            if dx.abs() == 2 && dz.abs() == 2 {
                                continue;
                            }
                            place(tree_x + dx, 68 + dy, tree_z + dz, "minecraft:oak_leaves")?;
                        }
                    }
                }
            }
        }

        self.world.bump_revision();
        Ok(self.world.info())
    }

    pub fn history(&self) -> Vec<HistorySummary> {
        self.history
            .iter()
            .map(|entry| HistorySummary {
                project_id: entry.project_id.clone(),
                operation_id: entry.operation_id.clone(),
                actor_id: entry.actor.id.clone(),
                source: entry.actor.source,
                revision_before: entry.revision_before,
                revision_after: entry.revision_after,
                changed_blocks: entry.changed_blocks,
                timestamp_ms: entry.timestamp_ms,
                layer_id: entry.layer_id.clone(),
            })
            .collect()
    }

    pub fn history_state(&self) -> HistoryState {
        HistoryState {
            can_undo: !self.history.is_empty(),
            can_redo: !self.redo.is_empty(),
            revision: self.world.revision(),
            entries: self.history(),
        }
    }

    pub fn audit_log(&self) -> &[AuditEntry] {
        &self.audit
    }

    pub fn can_undo(&self) -> bool {
        !self.history.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn preview(
        &self,
        project_id: &str,
        operation: &Operation,
    ) -> Result<PreviewResult, AppError> {
        self.validate_project(project_id)?;
        self.validate_target_states(operation)?;
        self.enforce_workspace(None, operation)?;
        let preview = operations::preview(&self.world, operation)?;
        Ok(PreviewResult {
            affected_blocks: preview.affected_blocks,
            changed_blocks: preview.changed_blocks,
            dirty_chunks: preview.dirty_chunks,
            warnings: preview.warnings,
        })
    }

    pub fn execute(&mut self, request: OperationRequest) -> Result<OperationResult, AppError> {
        self.execute_with_observer(request, &NoopObserver)
    }

    /// Executes an operation while reporting progress and checking cancellation. The
    /// operation layer restores touched sections if cancellation arrives during writing,
    /// so a cancelled job leaves the authoritative world unchanged.
    pub fn execute_with_observer(
        &mut self,
        request: OperationRequest,
        observer: &dyn OperationObserver,
    ) -> Result<OperationResult, AppError> {
        self.validate_project(&request.project_id)?;
        self.validate_actor(&request.actor)?;
        if !self.writes_enabled {
            return Err(AppError::new(
                ErrorCode::PermissionDenied,
                "writes are disabled for this project; enable edit access explicitly",
            ));
        }

        let fingerprint = self.fingerprint(&request)?;
        if let Some(key) = &request.idempotency_key {
            self.validate_key(key)?;
            if let Some(stored) = self.idempotency.get(key) {
                if stored.fingerprint != fingerprint {
                    return Err(AppError::new(
                        ErrorCode::Conflict,
                        "idempotency key was already used with a different request",
                    )
                    .with_details(json!({ "idempotencyKey": key })));
                }
                let mut result = stored.result.clone();
                result.replayed = true;
                return Ok(result);
            }
        }

        if let Some(expected) = request.expected_revision {
            if expected != self.world.revision() {
                return Err(AppError::new(
                    ErrorCode::RevisionMismatch,
                    format!(
                        "expected revision {expected}, current revision is {}",
                        self.world.revision()
                    ),
                )
                .with_details(json!({
                    "expected": expected,
                    "current": self.world.revision()
                })));
            }
        }

        self.validate_target_states(&request.operation)?;
        self.enforce_workspace(request.layer_id.as_deref(), &request.operation)?;
        let layer_id = match request.layer_id.clone() {
            Some(id) => Some(id),
            None => self.workspace.active_layer_id.clone(),
        };
        let timestamp_ms = now_ms()?;
        let revision_before = self.world.revision();
        let extent = request
            .operation
            .bounds()
            .map(|bounds| (request.operation.dimension().to_owned(), bounds));
        let applied =
            operations::apply_with_observer(&mut self.world, &request.operation, observer)?;
        let revision_after = if applied.changed_blocks > 0 {
            self.world.bump_revision()
        } else {
            revision_before
        };

        let operation_id = new_operation_id("op");
        let result = OperationResult {
            operation_id: operation_id.clone(),
            world_revision: revision_after,
            changed_blocks: applied.changed_blocks,
            dirty_chunks: applied.dirty_chunks.clone(),
            warnings: applied.warnings.clone(),
            replayed: false,
        };

        if applied.changed_blocks > 0 {
            self.history.push(HistoryEntry {
                project_id: request.project_id.clone(),
                operation_id: operation_id.clone(),
                actor: request.actor.clone(),
                timestamp_ms,
                revision_before,
                revision_after,
                changed_blocks: applied.changed_blocks,
                parameter_summary: summarize_operation(&request.operation),
                linked_object_id: request.linked_object_id.clone(),
                layer_id: layer_id.clone(),
                extent,
                diffs: applied.diffs,
            });
            if self.history.len() > self.max_history {
                self.history.remove(0);
            }
            self.redo.clear();
        }

        self.audit.push(AuditEntry {
            project_id: request.project_id.clone(),
            operation_id: operation_id.clone(),
            actor_id: request.actor.id.clone(),
            source: request.actor.source,
            action: AuditAction::Apply,
            revision_before,
            revision_after,
            changed_blocks: applied.changed_blocks,
            timestamp_ms,
            parameter_summary: summarize_operation(&request.operation),
            linked_object_id: request.linked_object_id.clone(),
            layer_id,
        });

        if let Some(key) = &request.idempotency_key {
            self.remember_idempotent(key.clone(), fingerprint, result.clone());
        }

        Ok(result)
    }

    pub fn undo(
        &mut self,
        project_id: &str,
        actor: Actor,
        operation_id: Option<&str>,
    ) -> Result<OperationResult, AppError> {
        self.validate_project(project_id)?;
        self.validate_actor(&actor)?;
        if !self.writes_enabled {
            return Err(AppError::new(
                ErrorCode::PermissionDenied,
                "writes are disabled for this project",
            ));
        }
        self.undo_index(operation_id)?;
        let entry = self.history.pop().expect("index checked");
        for diff in entry.diffs.iter().rev() {
            diff.revert(&mut self.world)?;
        }
        let revision_before = self.world.revision();
        let revision_after = self.world.bump_revision();
        let timestamp_ms = now_ms()?;
        let dirty_chunks = dirty_chunks(&entry.diffs);
        let operation_id = new_operation_id("undo");

        self.audit.push(AuditEntry {
            project_id: project_id.to_owned(),
            operation_id: operation_id.clone(),
            actor_id: actor.id.clone(),
            source: actor.source,
            action: AuditAction::Undo,
            revision_before,
            revision_after,
            changed_blocks: entry.changed_blocks,
            timestamp_ms,
            parameter_summary: json!({
                "undoneOperationId": entry.operation_id,
                "summary": entry.parameter_summary
            }),
            linked_object_id: entry.linked_object_id.clone(),
            layer_id: entry.layer_id.clone(),
        });

        let result = OperationResult {
            operation_id,
            world_revision: revision_after,
            changed_blocks: entry.changed_blocks,
            dirty_chunks,
            warnings: Vec::new(),
            replayed: false,
        };
        self.redo.push(entry);
        Ok(result)
    }

    /// Resolves the operation that `undo` would affect without mutating anything.
    /// The MCP access policy uses this to keep an undo inside the same region as the
    /// original edit; `undo` itself repeats the latest-operation check.
    pub fn undo_target(&self, operation_id: Option<&str>) -> Result<UndoTarget, AppError> {
        let index = self.undo_index(operation_id)?;
        let entry = &self.history[index];
        let (dimension, bounds) = entry
            .extent
            .clone()
            .ok_or_else(|| AppError::internal("history entry has no recorded operation extent"))?;
        Ok(UndoTarget {
            operation_id: entry.operation_id.clone(),
            dimension,
            bounds,
        })
    }

    fn undo_index(&self, operation_id: Option<&str>) -> Result<usize, AppError> {
        let index = match operation_id {
            Some(id) => self
                .history
                .iter()
                .rposition(|entry| entry.operation_id == id)
                .ok_or_else(|| {
                    AppError::new(ErrorCode::NoHistory, format!("unknown operation `{id}`"))
                })?,
            None => self.history.len().checked_sub(1).ok_or_else(|| {
                AppError::new(ErrorCode::NoHistory, "there is no operation to undo")
            })?,
        };
        if index + 1 != self.history.len() {
            return Err(AppError::new(
                ErrorCode::Conflict,
                "cannot undo an operation that is not the latest; newer operations exist",
            ));
        }
        Ok(index)
    }

    pub fn redo(&mut self, project_id: &str, actor: Actor) -> Result<OperationResult, AppError> {
        self.validate_project(project_id)?;
        self.validate_actor(&actor)?;
        if !self.writes_enabled {
            return Err(AppError::new(
                ErrorCode::PermissionDenied,
                "writes are disabled for this project",
            ));
        }
        let entry = self
            .redo
            .pop()
            .ok_or_else(|| AppError::new(ErrorCode::NoHistory, "there is no operation to redo"))?;
        for diff in &entry.diffs {
            diff.reapply(&mut self.world)?;
        }
        let revision_before = self.world.revision();
        let revision_after = self.world.bump_revision();
        let timestamp_ms = now_ms()?;
        let dirty_chunks = dirty_chunks(&entry.diffs);
        let operation_id = new_operation_id("redo");

        self.audit.push(AuditEntry {
            project_id: project_id.to_owned(),
            operation_id: operation_id.clone(),
            actor_id: actor.id.clone(),
            source: actor.source,
            action: AuditAction::Redo,
            revision_before,
            revision_after,
            changed_blocks: entry.changed_blocks,
            timestamp_ms,
            parameter_summary: json!({
                "redoneOperationId": entry.operation_id,
                "summary": entry.parameter_summary
            }),
            linked_object_id: entry.linked_object_id.clone(),
            layer_id: entry.layer_id.clone(),
        });

        let result = OperationResult {
            operation_id,
            world_revision: revision_after,
            changed_blocks: entry.changed_blocks,
            dirty_chunks,
            warnings: Vec::new(),
            replayed: false,
        };
        self.history.push(entry);
        if self.history.len() > self.max_history {
            self.history.remove(0);
        }
        Ok(result)
    }

    pub fn query(&self, query: Query) -> Result<QueryResult, AppError> {
        match query {
            Query::WorldInfo => Ok(QueryResult::WorldInfo(self.world_info())),
            Query::GetBlock { dimension, pos } => {
                let state = self.world.get_block(&dimension, pos)?;
                Ok(QueryResult::Block {
                    dimension,
                    pos,
                    state,
                })
            }
            Query::GetBiome { dimension, pos } => {
                let biome = self.world.get_biome(&dimension, pos)?;
                Ok(QueryResult::Biome {
                    dimension,
                    pos,
                    biome,
                })
            }
            Query::RegionSummary { dimension, bounds } => Ok(QueryResult::RegionSummary(
                self.region_summary(&dimension, bounds)?,
            )),
            Query::RegionBlocks { dimension, bounds } => {
                let volume = bounds.volume();
                if volume > MAX_REGION_READ_BLOCKS {
                    return Err(AppError::new(
                        ErrorCode::LimitExceeded,
                        format!(
                            "region read affects {volume} blocks, limit is {MAX_REGION_READ_BLOCKS}"
                        ),
                    ));
                }
                let mut blocks = Vec::new();
                for pos in bounds.iter() {
                    let state = self.world.get_block(&dimension, pos)?;
                    if !state.is_air() {
                        blocks.push(BlockChange { pos, state });
                    }
                }
                Ok(QueryResult::RegionBlocks(RegionBlocks {
                    dimension,
                    bounds,
                    volume,
                    blocks,
                }))
            }
            Query::ChunkStats { dimension, chunk } => {
                let dim = self.world.dimension(&dimension)?;
                let loaded = dim.chunk(chunk);
                Ok(QueryResult::ChunkStats(ChunkStats {
                    dimension,
                    chunk,
                    loaded: loaded.is_some(),
                    sections: loaded.map(|c| c.sections().len()).unwrap_or(0),
                    blocks: loaded.map(|c| c.block_count()).unwrap_or(0),
                }))
            }
            Query::FindStructureSites {
                dimension,
                bounds,
                width,
                depth,
                step,
                max_slope,
                clearance,
                limit,
            } => {
                let search = operations::find_structure_sites(
                    self.world(),
                    &dimension,
                    bounds,
                    StructureSiteRules {
                        width,
                        depth,
                        step,
                        max_slope,
                        clearance,
                        limit: limit as usize,
                    },
                )?;
                Ok(QueryResult::StructureSites(StructureSites {
                    dimension,
                    bounds,
                    sites: search.sites,
                    scanned_candidates: search.scanned_candidates,
                    rejected_candidates: search.rejected_candidates,
                    limit,
                }))
            }
            Query::WorkspacePolicy => Ok(QueryResult::WorkspacePolicy(self.workspace.clone())),
            Query::SceneObjects => Ok(QueryResult::SceneObjects(self.scene.objects().to_vec())),
            Query::InspectSceneObject { object_id } => {
                let object = self.scene.get(&object_id).cloned().ok_or_else(|| {
                    AppError::new(
                        ErrorCode::NotFound,
                        format!("semantic object `{object_id}` was not found"),
                    )
                })?;
                let summary = self.region_summary(&object.dimension, object.bounds)?;
                let children = self
                    .scene
                    .children(&object_id)
                    .into_iter()
                    .cloned()
                    .collect();
                Ok(QueryResult::SceneObject(SceneObjectInspection {
                    object,
                    volume: summary.blocks,
                    non_air_blocks: summary.non_air_blocks,
                    palette: summary.palette,
                    children,
                }))
            }
            Query::ParametricObjects => {
                Ok(QueryResult::ParametricObjects(self.parametric_summaries()?))
            }
            Query::InspectParametricObject { object_id } => Ok(QueryResult::ParametricObject(
                self.inspect_parametric_object(&object_id)?,
            )),
        }
    }

    fn region_summary(&self, dimension: &str, bounds: BlockBox) -> Result<RegionSummary, AppError> {
        let volume = bounds.volume();
        if volume > MAX_REGION_SUMMARY_BLOCKS {
            return Err(AppError::new(
                ErrorCode::LimitExceeded,
                format!(
                    "region summary affects {volume} blocks, limit is {MAX_REGION_SUMMARY_BLOCKS}"
                ),
            ));
        }
        let mut palette: BTreeMap<String, u64> = BTreeMap::new();
        let mut non_air_blocks = 0u64;
        for pos in bounds.iter() {
            let state = self.world.get_block(dimension, pos)?;
            if !state.is_air() {
                non_air_blocks += 1;
            }
            *palette.entry(state.to_string()).or_default() += 1;
        }
        let mut palette: Vec<PaletteEntry> = palette
            .into_iter()
            .map(|(state, count)| PaletteEntry { state, count })
            .collect();
        palette.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.state.cmp(&b.state)));
        Ok(RegionSummary {
            dimension: dimension.to_owned(),
            bounds,
            blocks: volume,
            non_air_blocks,
            palette,
        })
    }

    fn validate_project(&self, project_id: &str) -> Result<(), AppError> {
        if project_id.trim().is_empty() {
            return Err(AppError::invalid_argument("project_id must not be empty"));
        }
        if project_id.len() > MAX_PROJECT_ID {
            return Err(AppError::invalid_argument(format!(
                "project_id must be at most {MAX_PROJECT_ID} characters"
            )));
        }
        Ok(())
    }

    fn validate_actor(&self, actor: &Actor) -> Result<(), AppError> {
        if actor.id.trim().is_empty() {
            return Err(AppError::invalid_argument("actor id must not be empty"));
        }
        if actor.id.len() > MAX_PROJECT_ID {
            return Err(AppError::invalid_argument("actor id is too long"));
        }
        Ok(())
    }

    fn validate_key(&self, key: &str) -> Result<(), AppError> {
        if key.trim().is_empty() || key.len() > MAX_KEY_LENGTH {
            return Err(AppError::invalid_argument(format!(
                "idempotency_key must be 1..={MAX_KEY_LENGTH} characters"
            )));
        }
        Ok(())
    }

    fn validate_target_states(&self, operation: &Operation) -> Result<(), AppError> {
        if self.allow_unknown_targets {
            return Ok(());
        }
        match operation {
            Operation::SetBlock { state, .. } => self.registry.validate(state)?,
            Operation::Batch { changes, .. } => {
                for change in changes {
                    self.registry.validate(&change.state)?;
                }
            }
            Operation::FillCuboid { state, .. } => self.registry.validate(state)?,
            Operation::Replace { to, .. } => {
                self.registry.validate(to)?;
                // `from` is a predicate; accept any syntactically valid state, including
                // imported unknown block states.
            }
            Operation::TerrainBrush { material, .. } => self.registry.validate(material)?,
            Operation::GenerateHeightmap {
                surface, filler, ..
            } => {
                self.registry.validate(surface)?;
                self.registry.validate(filler)?;
            }
            Operation::GenerateRoad { surface, .. } => self.registry.validate(surface)?,
            Operation::GenerateRiver { water, bed, .. } => {
                self.registry.validate(water)?;
                self.registry.validate(bed)?;
            }
            Operation::GenerateVegetation {
                plant,
                trunk,
                leaves,
                ..
            } => {
                self.registry.validate(plant)?;
                self.registry.validate(trunk)?;
                self.registry.validate(leaves)?;
            }
            // `BiomeState` is validated by its own deserializer; there is no block registry
            // entry to check and no biomes are fabricated here.
            Operation::SetBiome { .. } => {}
        }
        Ok(())
    }

    fn fingerprint(&self, request: &OperationRequest) -> Result<String, AppError> {
        let value = json!({
            "projectId": request.project_id,
            "operation": request.operation,
            "actorId": request.actor.id,
            "actorSource": request.actor.source,
            "linkedObjectId": request.linked_object_id,
        });
        serde_json::to_string(&value)
            .map_err(|error| AppError::internal(format!("cannot fingerprint request: {error}")))
    }

    fn remember_idempotent(&mut self, key: String, fingerprint: String, result: OperationResult) {
        if self.idempotency.len() >= MAX_IDEMPOTENCY_ENTRIES {
            if let Some(oldest) = self.idempotency_order.pop_front() {
                self.idempotency.remove(&oldest);
            }
        }
        self.idempotency_order.push_back(key.clone());
        self.idempotency.insert(
            key,
            StoredOutcome {
                fingerprint,
                result,
            },
        );
    }
}

fn summarize_operation(operation: &Operation) -> serde_json::Value {
    match operation {
        Operation::SetBlock {
            dimension,
            pos,
            state,
        } => json!({
            "type": "set_block",
            "dimension": dimension,
            "pos": pos,
            "state": state.to_string()
        }),
        Operation::Batch { dimension, changes } => json!({
            "type": "batch",
            "dimension": dimension,
            "changes": changes.len()
        }),
        Operation::FillCuboid {
            dimension,
            bounds,
            state,
        } => json!({
            "type": "fill_cuboid",
            "dimension": dimension,
            "bounds": bounds,
            "state": state.to_string()
        }),
        Operation::Replace {
            dimension,
            bounds,
            from,
            to,
        } => json!({
            "type": "replace",
            "dimension": dimension,
            "bounds": bounds,
            "from": from.to_string(),
            "to": to.to_string()
        }),
        Operation::TerrainBrush {
            dimension,
            bounds,
            kind,
            material,
            amount,
            seed,
            step,
            target_y,
        } => json!({
            "type": "terrain_brush",
            "dimension": dimension,
            "bounds": bounds,
            "kind": kind.label(),
            "material": material.to_string(),
            "amount": amount,
            "seed": seed,
            "step": step,
            "targetY": target_y
        }),
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
        } => json!({
            "type": "generate_heightmap",
            "dimension": dimension,
            "bounds": bounds,
            "seed": seed,
            "scale": scale,
            "octaves": octaves,
            "amplitude": amplitude,
            "baseY": base_y,
            "surfaceDepth": surface_depth,
            "surface": surface.to_string(),
            "filler": filler.to_string()
        }),
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
        } => json!({
            "type": "generate_road",
            "dimension": dimension,
            "bounds": bounds,
            "from": from,
            "to": to,
            "width": width,
            "meander": meander,
            "seed": seed,
            "clearance": clearance,
            "surface": surface.to_string()
        }),
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
        } => json!({
            "type": "generate_river",
            "dimension": dimension,
            "bounds": bounds,
            "from": from,
            "to": to,
            "width": width,
            "meander": meander,
            "seed": seed,
            "depth": depth,
            "water": water.to_string(),
            "bed": bed.to_string()
        }),
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
        } => json!({
            "type": "generate_vegetation",
            "dimension": dimension,
            "bounds": bounds,
            "seed": seed,
            "groundCover": ground_cover,
            "treeChance": tree_chance,
            "treeSpacing": tree_spacing,
            "plant": plant.to_string(),
            "trunk": trunk.to_string(),
            "leaves": leaves.to_string()
        }),
        Operation::SetBiome {
            dimension,
            bounds,
            biome,
        } => json!({
            "type": "set_biome",
            "dimension": dimension,
            "bounds": bounds,
            "biome": biome.to_string()
        }),
    }
}

fn dirty_chunks(diffs: &[SectionDiff]) -> Vec<ChunkPos> {
    let set: BTreeSet<ChunkPos> = diffs.iter().map(|diff| diff.chunk).collect();
    set.into_iter().collect()
}

/// Builds one chunk snapshot from sparse sections. The payload is deliberately compact:
/// palette + little-endian u32 halves of packed u64 words.
fn snapshot_chunk(dim: &world_domain::Dimension, chunk_pos: ChunkPos) -> ChunkSnapshot {
    match dim.chunk(chunk_pos) {
        Some(chunk) => {
            let sections = chunk
                .sections()
                .iter()
                .filter(|(_, section)| !section.is_empty())
                .map(|(section_y, section)| SectionSnapshot {
                    section_y: *section_y,
                    palette: section.palette().to_vec(),
                    data: section
                        .packed_data()
                        .iter()
                        .flat_map(|word| [*word as u32, (*word >> 32) as u32])
                        .collect(),
                    bits_per_index: section.bits_per_index(),
                    non_air: section.non_air_count(),
                })
                .collect();
            ChunkSnapshot {
                chunk: chunk_pos,
                loaded: true,
                sections,
            }
        }
        None => ChunkSnapshot {
            chunk: chunk_pos,
            loaded: false,
            sections: Vec::new(),
        },
    }
}

fn generated_map(object: &ParametricObject) -> Result<BTreeMap<BlockPos, BlockState>, AppError> {
    Ok(object
        .generate()?
        .into_iter()
        .map(|block| (block.pos, block.state))
        .collect())
}

fn parametric_conflict_error(conflicts: &[ParametricConflict]) -> AppError {
    let count = conflicts.len();
    let sample: Vec<ParametricConflict> = conflicts
        .iter()
        .take(MAX_PARAMETRIC_CONFLICTS_REPORTED)
        .cloned()
        .collect();
    let first = conflicts.first();
    let location = first
        .map(|conflict| format!(" at {:?}", conflict.pos))
        .unwrap_or_default();
    AppError::new(
        ErrorCode::Conflict,
        format!(
            "{count} block(s) inside the parametric overlay were edited manually{location}; rebake with override, bake or roll back"
        ),
    )
    .with_details(json!({
        "count": count,
        "conflicts": sample,
    }))
}

fn now_ms() -> Result<u128, AppError> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .map_err(|error| AppError::internal(format!("system clock error: {error}")))
}

fn new_operation_id(prefix: &str) -> String {
    format!("{prefix}_{}", Uuid::new_v4().simple())
}

#[cfg(test)]
mod provenance_tests;
#[cfg(test)]
mod render_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod undo_tests;
