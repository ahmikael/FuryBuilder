use serde::{Deserialize, Serialize};
use world_domain::{BlockBox, BlockPos, BlockState, ChunkPos, WorldInfo};
use world_operations::Operation;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorSource {
    Ui,
    Mcp,
    Api,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Actor {
    pub id: String,
    pub source: ActorSource,
}

impl Actor {
    pub fn ui(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            source: ActorSource::Ui,
        }
    }

    pub fn mcp(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            source: ActorSource::Mcp,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationRequest {
    pub project_id: String,
    pub operation: Operation,
    #[serde(default)]
    pub expected_revision: Option<u64>,
    #[serde(default)]
    pub idempotency_key: Option<String>,
    pub actor: Actor,
    #[serde(default)]
    pub linked_object_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationResult {
    pub operation_id: String,
    pub world_revision: u64,
    pub changed_blocks: u64,
    pub dirty_chunks: Vec<ChunkPos>,
    pub warnings: Vec<String>,
    pub replayed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewResult {
    pub affected_blocks: u64,
    pub changed_blocks: u64,
    pub dirty_chunks: Vec<ChunkPos>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaletteEntry {
    pub state: String,
    pub count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegionSummary {
    pub dimension: String,
    pub bounds: BlockBox,
    pub blocks: u64,
    pub non_air_blocks: u64,
    pub palette: Vec<PaletteEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChunkStats {
    pub dimension: String,
    pub chunk: ChunkPos,
    pub loaded: bool,
    pub sections: usize,
    pub blocks: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Query {
    WorldInfo,
    GetBlock { dimension: String, pos: BlockPos },
    RegionSummary { dimension: String, bounds: BlockBox },
    ChunkStats { dimension: String, chunk: ChunkPos },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum QueryResult {
    WorldInfo(WorldInfo),
    Block {
        dimension: String,
        pos: BlockPos,
        state: BlockState,
    },
    RegionSummary(RegionSummary),
    ChunkStats(ChunkStats),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistorySummary {
    pub project_id: String,
    pub operation_id: String,
    pub actor_id: String,
    pub source: ActorSource,
    pub revision_before: u64,
    pub revision_after: u64,
    pub changed_blocks: u64,
    pub timestamp_ms: u128,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditAction {
    Apply,
    Undo,
    Redo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEntry {
    pub project_id: String,
    pub operation_id: String,
    pub actor_id: String,
    pub source: ActorSource,
    pub action: AuditAction,
    pub revision_before: u64,
    pub revision_after: u64,
    pub changed_blocks: u64,
    pub timestamp_ms: u128,
    pub parameter_summary: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linked_object_id: Option<String>,
}
