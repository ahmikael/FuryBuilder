use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde_json::json;
use uuid::Uuid;
use world_domain::{BlockRegistry, ChunkPos, World, WorldInfo};
use world_operations::{self as operations, Operation, SectionDiff};

use crate::dto::{
    Actor, AuditAction, AuditEntry, ChunkStats, HistorySummary, OperationRequest, OperationResult,
    PaletteEntry, PreviewResult, Query, QueryResult, RegionSummary,
};
use crate::error::{AppError, ErrorCode};

pub const MAX_REGION_SUMMARY_BLOCKS: u64 = 1_000_000;
pub const MAX_IDEMPOTENCY_ENTRIES: usize = 256;
pub const DEFAULT_MAX_HISTORY: usize = 100;
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
            writes_enabled: true,
            allow_unknown_targets: false,
            max_history: DEFAULT_MAX_HISTORY,
        }
    }

    pub fn with_max_history(mut self, max_history: usize) -> Self {
        self.max_history = max_history.max(1);
        self
    }

    pub fn world(&self) -> &World {
        &self.world
    }

    pub fn registry(&self) -> &BlockRegistry {
        &self.registry
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
            })
            .collect()
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
        let preview = operations::preview(&self.world, operation)?;
        Ok(PreviewResult {
            affected_blocks: preview.affected_blocks,
            changed_blocks: preview.changed_blocks,
            dirty_chunks: preview.dirty_chunks,
            warnings: preview.warnings,
        })
    }

    pub fn execute(&mut self, request: OperationRequest) -> Result<OperationResult, AppError> {
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
        let timestamp_ms = now_ms()?;
        let revision_before = self.world.revision();
        let applied = operations::apply(&mut self.world, &request.operation)?;
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
            Query::RegionSummary { dimension, bounds } => {
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
                    let state = self.world.get_block(&dimension, pos)?;
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
                Ok(QueryResult::RegionSummary(RegionSummary {
                    dimension,
                    bounds,
                    blocks: volume,
                    non_air_blocks,
                    palette,
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
        }
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
    }
}

fn dirty_chunks(diffs: &[SectionDiff]) -> Vec<ChunkPos> {
    let set: BTreeSet<ChunkPos> = diffs.iter().map(|diff| diff.chunk).collect();
    set.into_iter().collect()
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
mod tests;
#[cfg(test)]
mod undo_tests;
