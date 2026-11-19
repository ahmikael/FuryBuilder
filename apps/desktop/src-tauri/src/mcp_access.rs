//! MCP access policy and per-write confirmation.
//!
//! The default is read-only. `edit_in_selection` additionally restricts every write to one
//! dimension and one inclusive box; `full_project` drops the box but keeps hard block
//! limits. The desktop UI is the only place that can change the policy or approve a write.

use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use world_application::{AppError, ErrorCode, Operation, Selection};
use world_domain::BlockBox;

pub const MAX_MCP_BLOCKS: u64 = 1_000_000;
pub const MAX_MCP_BATCH_CHANGES: usize = 8192;
pub const MAX_PENDING_CONFIRMATIONS: usize = 8;
pub const MAX_SELECTION_DIMENSION: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpAccessMode {
    ReadOnly,
    EditInSelection,
    FullProject,
}

#[derive(Debug, Clone)]
pub struct McpAccessPolicy {
    pub mode: McpAccessMode,
    pub selection: Option<Selection>,
    pub max_blocks: u64,
    pub max_batch_changes: usize,
    pub require_confirmation: bool,
}

impl Default for McpAccessPolicy {
    fn default() -> Self {
        Self {
            mode: McpAccessMode::ReadOnly,
            selection: None,
            max_blocks: 50_000,
            max_batch_changes: 4096,
            require_confirmation: true,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpAccessInput {
    pub mode: McpAccessMode,
    #[serde(default)]
    pub selection: Option<Selection>,
    #[serde(default)]
    pub max_blocks: Option<u64>,
    #[serde(default)]
    pub max_batch_changes: Option<usize>,
    #[serde(default = "confirmation_default")]
    pub require_confirmation: bool,
}

fn confirmation_default() -> bool {
    true
}

#[derive(Debug, Default)]
pub struct McpAccessState {
    policy: Mutex<McpAccessPolicy>,
}

impl McpAccessState {
    pub fn policy(&self) -> McpAccessPolicy {
        self.policy
            .lock()
            .map(|policy| policy.clone())
            .unwrap_or_default()
    }

    pub fn summary(&self) -> Value {
        let policy = self.policy();
        json!({
            "mode": policy.mode,
            "selection": policy.selection,
            "maxBlocks": policy.max_blocks,
            "maxBatchChanges": policy.max_batch_changes,
            "requireConfirmation": policy.require_confirmation,
            "writesEnabled": policy.mode != McpAccessMode::ReadOnly,
        })
    }

    /// Replaces the policy after validating every bound. A bad policy is rejected instead
    /// of being silently clamped, so the UI can show the user what is wrong.
    pub fn update(&self, input: McpAccessInput) -> Result<Value, AppError> {
        let current = self.policy();
        let max_blocks = input.max_blocks.unwrap_or(current.max_blocks);
        if !(1..=MAX_MCP_BLOCKS).contains(&max_blocks) {
            return Err(AppError::new(
                ErrorCode::LimitExceeded,
                format!("max_blocks must be between 1 and {MAX_MCP_BLOCKS}"),
            ));
        }
        let max_batch_changes = input.max_batch_changes.unwrap_or(current.max_batch_changes);
        if max_batch_changes == 0
            || max_batch_changes > MAX_MCP_BATCH_CHANGES
            || max_batch_changes as u64 > max_blocks
        {
            return Err(AppError::invalid_argument(format!(
                "max_batch_changes must be 1..={MAX_MCP_BATCH_CHANGES} and no larger than max_blocks"
            )));
        }
        let selection = match input.mode {
            McpAccessMode::ReadOnly | McpAccessMode::FullProject => None,
            McpAccessMode::EditInSelection => {
                let selection = input.selection.ok_or_else(|| {
                    AppError::invalid_argument(
                        "edit_in_selection requires a dimension and selection bounds",
                    )
                })?;
                if selection.dimension.trim().is_empty() {
                    return Err(AppError::invalid_argument(
                        "selection dimension must not be empty",
                    ));
                }
                if selection.dimension.len() > MAX_SELECTION_DIMENSION {
                    return Err(AppError::new(
                        ErrorCode::LimitExceeded,
                        format!(
                            "selection dimension must be at most {MAX_SELECTION_DIMENSION} characters"
                        ),
                    ));
                }
                if selection.dimension.chars().any(char::is_control) {
                    return Err(AppError::invalid_argument(
                        "selection dimension must not contain control characters",
                    ));
                }
                if selection.bounds.volume() > max_blocks {
                    return Err(AppError::new(
                        ErrorCode::LimitExceeded,
                        format!(
                            "selection contains {} blocks, limit is {max_blocks}",
                            selection.bounds.volume()
                        ),
                    ));
                }
                Some(selection)
            }
        };
        let policy = McpAccessPolicy {
            mode: input.mode,
            selection,
            max_blocks,
            max_batch_changes,
            require_confirmation: input.require_confirmation,
        };
        {
            let mut stored = self
                .policy
                .lock()
                .map_err(|_| AppError::internal("MCP access policy lock poisoned"))?;
            *stored = policy;
        }
        Ok(self.summary())
    }

    pub fn require_confirmation(&self) -> bool {
        self.policy().require_confirmation
    }

    pub fn mode(&self) -> McpAccessMode {
        self.policy().mode
    }

    /// Rejects an undo/apply before looking up history, so read-only mode always produces
    /// a permission error rather than an unrelated "no history" result.
    pub fn require_editing(&self, operation: &str) -> Result<(), AppError> {
        if self.policy().mode == McpAccessMode::ReadOnly {
            return Err(read_only_error(operation));
        }
        Ok(())
    }

    /// Checks one operation against the current mode, box and block limits.
    pub fn authorize_operation(&self, operation: &Operation) -> Result<(), AppError> {
        let policy = self.policy();
        if policy.mode == McpAccessMode::ReadOnly {
            return Err(read_only_error("apply_operation"));
        }
        if operation.affected_blocks() > policy.max_blocks {
            return Err(AppError::new(
                ErrorCode::LimitExceeded,
                format!(
                    "operation affects {} blocks, MCP limit is {}",
                    operation.affected_blocks(),
                    policy.max_blocks
                ),
            )
            .with_details(json!({
                "affectedBlocks": operation.affected_blocks(),
                "maxBlocks": policy.max_blocks,
            })));
        }
        if let Operation::Batch { changes, .. } = operation {
            if changes.len() > policy.max_batch_changes {
                return Err(AppError::new(
                    ErrorCode::LimitExceeded,
                    format!(
                        "batch has {} changes, MCP limit is {}",
                        changes.len(),
                        policy.max_batch_changes
                    ),
                ));
            }
        }

        if policy.mode == McpAccessMode::EditInSelection {
            let selection = policy.selection.as_ref().ok_or_else(|| {
                AppError::new(
                    ErrorCode::PermissionDenied,
                    "edit_in_selection has no active selection",
                )
            })?;
            if operation.dimension() != selection.dimension {
                return Err(outside_selection_error(
                    operation.dimension(),
                    &selection.dimension,
                ));
            }
            match operation {
                Operation::SetBlock { pos, .. } => {
                    if !selection.contains(*pos) {
                        return Err(outside_selection_error(
                            operation.dimension(),
                            &selection.dimension,
                        ));
                    }
                }
                Operation::Batch { changes, .. } => {
                    if let Some(change) = changes
                        .iter()
                        .find(|change| !selection.contains(change.pos))
                    {
                        return Err(AppError::new(
                            ErrorCode::PermissionDenied,
                            format!(
                                "block {}:{}:{} is outside the MCP selection",
                                change.pos.x, change.pos.y, change.pos.z
                            ),
                        ));
                    }
                }
                Operation::FillCuboid { bounds, .. }
                | Operation::Replace { bounds, .. }
                | Operation::TerrainBrush { bounds, .. }
                | Operation::GenerateHeightmap { bounds, .. }
                | Operation::GenerateRoad { bounds, .. }
                | Operation::GenerateRiver { bounds, .. }
                | Operation::GenerateVegetation { bounds, .. }
                | Operation::SetBiome { bounds, .. } => {
                    if !box_contains(&selection.bounds, bounds) {
                        return Err(AppError::new(
                            ErrorCode::PermissionDenied,
                            "operation bounds are outside the MCP selection",
                        )
                        .with_details(json!({
                            "selection": selection.bounds,
                            "operation": bounds,
                        })));
                    }
                }
            }
        }
        Ok(())
    }

    /// Undo is allowed only when the operation's actual extent is inside the policy.
    pub fn authorize_undo(&self, dimension: &str, bounds: BlockBox) -> Result<(), AppError> {
        let policy = self.policy();
        if policy.mode == McpAccessMode::ReadOnly {
            return Err(read_only_error("undo_operation"));
        }
        if bounds.volume() > policy.max_blocks {
            return Err(AppError::new(
                ErrorCode::LimitExceeded,
                format!(
                    "operation covers {} blocks, MCP limit is {}",
                    bounds.volume(),
                    policy.max_blocks
                ),
            ));
        }
        if policy.mode == McpAccessMode::EditInSelection {
            let selection = policy.selection.as_ref().ok_or_else(|| {
                AppError::new(
                    ErrorCode::PermissionDenied,
                    "edit_in_selection has no active selection",
                )
            })?;
            if dimension != selection.dimension || !box_contains(&selection.bounds, &bounds) {
                return Err(outside_selection_error(dimension, &selection.dimension));
            }
        }
        Ok(())
    }
}

fn read_only_error(operation: &str) -> AppError {
    AppError::new(
        ErrorCode::PermissionDenied,
        "MCP access is read-only; enable edit access explicitly in FuryBuilder",
    )
    .with_details(json!({
        "mcpAccessMode": "read_only",
        "operation": operation,
    }))
}

fn outside_selection_error(actual_dimension: &str, expected_dimension: &str) -> AppError {
    AppError::new(
        ErrorCode::PermissionDenied,
        "operation is outside the MCP selection",
    )
    .with_details(json!({
        "mcpAccessMode": "edit_in_selection",
        "requestedDimension": actual_dimension,
        "selectionDimension": expected_dimension,
    }))
}

fn box_contains(outer: &BlockBox, inner: &BlockBox) -> bool {
    outer.contains(inner.min) && outer.contains(inner.max)
}

#[derive(Debug)]
struct PendingConfirmation {
    request_id: String,
    summary: Value,
    expires_at_ms: u64,
    sender: mpsc::Sender<bool>,
}

/// Pending approvals shown in the editor. Only the Tauri UI can complete one.
#[derive(Debug, Default)]
pub struct McpConfirmationBridge {
    pending: Mutex<HashMap<String, PendingConfirmation>>,
}

impl McpConfirmationBridge {
    pub fn open(
        &self,
        request_id: &str,
        summary: Value,
        expires_at_ms: u64,
    ) -> Result<mpsc::Receiver<bool>, AppError> {
        let mut pending = self
            .pending
            .lock()
            .map_err(|_| AppError::internal("confirmation bridge lock poisoned"))?;
        if pending.len() >= MAX_PENDING_CONFIRMATIONS {
            return Err(AppError::new(
                ErrorCode::LimitExceeded,
                "too many MCP confirmations are already pending",
            ));
        }
        let (sender, receiver) = mpsc::channel();
        pending.insert(
            request_id.to_owned(),
            PendingConfirmation {
                request_id: request_id.to_owned(),
                summary,
                expires_at_ms,
                sender,
            },
        );
        Ok(receiver)
    }

    pub fn complete(&self, request_id: &str, approved: bool) -> bool {
        let sender = self
            .pending
            .lock()
            .ok()
            .and_then(|mut pending| pending.remove(request_id));
        sender.is_some_and(|entry| entry.sender.send(approved).is_ok())
    }

    pub fn cancel(&self, request_id: &str) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.remove(request_id);
        }
    }

    pub fn pending_summaries(&self) -> Vec<Value> {
        self.pending
            .lock()
            .map(|pending| {
                pending
                    .values()
                    .map(|entry| {
                        json!({
                            "requestId": entry.request_id,
                            "summary": entry.summary,
                            "expiresAtMs": entry.expires_at_ms,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn count(&self) -> usize {
        self.pending
            .lock()
            .map(|pending| pending.len())
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use world_application::BlockChange;
    use world_domain::{BlockPos, BlockState};

    fn state() -> McpAccessState {
        McpAccessState::default()
    }

    fn selection() -> Selection {
        Selection {
            dimension: "minecraft:overworld".into(),
            bounds: BlockBox::new(BlockPos::new(0, 60, 0), BlockPos::new(15, 75, 15)).unwrap(),
        }
    }

    fn block(name: &str) -> BlockState {
        BlockState::new(name, BTreeMap::new()).unwrap()
    }

    fn set(pos: BlockPos) -> Operation {
        Operation::SetBlock {
            dimension: "minecraft:overworld".into(),
            pos,
            state: block("minecraft:stone"),
        }
    }

    fn enable(
        state: &McpAccessState,
        mode: McpAccessMode,
        selection: Option<Selection>,
        max_blocks: u64,
        max_batch_changes: usize,
        require_confirmation: bool,
    ) {
        state
            .update(McpAccessInput {
                mode,
                selection,
                max_blocks: Some(max_blocks),
                max_batch_changes: Some(max_batch_changes),
                require_confirmation,
            })
            .expect("policy");
    }

    #[test]
    fn default_policy_is_read_only_and_rejects_writes() {
        let state = state();
        assert_eq!(state.summary()["mode"], "read_only");
        assert_eq!(state.summary()["writesEnabled"], false);
        assert_eq!(
            state.authorize_operation(&set(BlockPos::new(1, 64, 1))),
            Err(read_only_error("apply_operation"))
        );
        assert_eq!(
            state.require_editing("undo_operation"),
            Err(read_only_error("undo_operation"))
        );
    }

    #[test]
    fn edit_in_selection_requires_and_enforces_the_box() {
        let state = state();
        assert!(state
            .update(McpAccessInput {
                mode: McpAccessMode::EditInSelection,
                selection: None,
                max_blocks: Some(10_000),
                max_batch_changes: Some(100),
                require_confirmation: true,
            })
            .is_err());

        enable(
            &state,
            McpAccessMode::EditInSelection,
            Some(selection()),
            10_000,
            100,
            true,
        );
        assert!(state
            .authorize_operation(&set(BlockPos::new(5, 64, 5)))
            .is_ok());
        assert!(state
            .authorize_operation(&set(BlockPos::new(50, 64, 5)))
            .is_err());
        assert!(state
            .authorize_operation(&Operation::SetBlock {
                dimension: "minecraft:the_nether".into(),
                pos: BlockPos::new(5, 64, 5),
                state: block("minecraft:stone"),
            })
            .is_err());
        assert!(state
            .authorize_operation(&Operation::FillCuboid {
                dimension: "minecraft:overworld".into(),
                bounds: BlockBox::new(BlockPos::new(0, 60, 0), BlockPos::new(15, 75, 15)).unwrap(),
                state: block("minecraft:stone"),
            })
            .is_ok());
        assert!(state
            .authorize_operation(&Operation::FillCuboid {
                dimension: "minecraft:overworld".into(),
                bounds: BlockBox::new(BlockPos::new(0, 60, 0), BlockPos::new(16, 75, 15)).unwrap(),
                state: block("minecraft:stone"),
            })
            .is_err());
    }

    #[test]
    fn limits_apply_to_operations_and_batches() {
        let state = state();
        enable(&state, McpAccessMode::FullProject, None, 4, 4, false);
        assert!(state
            .authorize_operation(&set(BlockPos::new(1, 64, 1)))
            .is_ok());
        assert!(state
            .authorize_operation(&Operation::Batch {
                dimension: "minecraft:overworld".into(),
                changes: (0..5)
                    .map(|x| BlockChange {
                        pos: BlockPos::new(x, 64, 0),
                        state: block("minecraft:stone"),
                    })
                    .collect(),
            })
            .is_err());
        assert!(state
            .authorize_operation(&set(BlockPos::new(1, 64, 1)))
            .is_ok());
    }

    #[test]
    fn undo_is_bounded_to_the_selection_extent() {
        let state = state();
        enable(
            &state,
            McpAccessMode::EditInSelection,
            Some(selection()),
            10_000,
            100,
            true,
        );
        assert!(state
            .authorize_undo(
                "minecraft:overworld",
                BlockBox::new(BlockPos::new(4, 64, 4), BlockPos::new(8, 70, 8)).unwrap(),
            )
            .is_ok());
        assert!(state
            .authorize_undo(
                "minecraft:overworld",
                BlockBox::new(BlockPos::new(4, 64, 4), BlockPos::new(40, 70, 40)).unwrap(),
            )
            .is_err());
        assert!(state
            .authorize_undo(
                "minecraft:the_nether",
                BlockBox::single(BlockPos::new(4, 64, 4)),
            )
            .is_err());
    }

    #[test]
    fn selection_dimension_is_bounded_and_clean() {
        let state = state();
        for dimension in [
            "x".repeat(MAX_SELECTION_DIMENSION + 1),
            "minecraft:over\nworld".to_owned(),
        ] {
            let result = state.update(McpAccessInput {
                mode: McpAccessMode::EditInSelection,
                selection: Some(Selection {
                    dimension,
                    bounds: BlockBox::single(BlockPos::new(0, 64, 0)),
                }),
                max_blocks: Some(10_000),
                max_batch_changes: Some(100),
                require_confirmation: true,
            });
            assert!(result.is_err());
        }
    }

    #[test]
    fn confirmation_bridge_accepts_exactly_one_answer() {
        let bridge = McpConfirmationBridge::default();
        let receiver = bridge
            .open("request-1", json!({ "kind": "apply" }), 1_000_000)
            .expect("open");
        assert_eq!(bridge.count(), 1);
        let pending = bridge.pending_summaries();
        assert_eq!(pending[0]["requestId"], "request-1");
        assert_eq!(pending[0]["summary"]["kind"], "apply");
        assert!(pending[0]["expiresAtMs"].as_u64().is_some());
        assert!(bridge.complete("request-1", true));
        assert!(receiver.recv().expect("answer"));
        assert_eq!(bridge.count(), 0);
        assert!(!bridge.complete("request-1", true));
    }
}
