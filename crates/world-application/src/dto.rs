use scene_model::SemanticObject;
use serde::{Deserialize, Serialize};
use workspace_model::WorkspacePolicy;
use world_domain::{BiomeState, BlockBox, BlockPos, BlockState, ChunkPos, WorldInfo};
use world_operations::{Operation, StructureSite};

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
    /// Edit layer for this operation. Omitted means the active workspace layer.
    #[serde(default)]
    pub layer_id: Option<String>,
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

/// The operation a read-only undo check would target. It contains no history payloads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoTarget {
    pub operation_id: String,
    pub dimension: String,
    pub bounds: BlockBox,
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

/// Metadata plus bounded world statistics for one semantic object. Inspection never
/// changes blocks or the scene; it reports the palette of the object's current bounds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneObjectInspection {
    pub object: SemanticObject,
    pub volume: u64,
    pub non_air_blocks: u64,
    pub palette: Vec<PaletteEntry>,
    pub children: Vec<SemanticObject>,
}

/// Sparse region payload for copy/paste. Air positions are implied, so a mostly empty
/// volume stays small. Bounded by `MAX_REGION_READ_BLOCKS`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegionBlocks {
    pub dimension: String,
    pub bounds: BlockBox,
    pub volume: u64,
    pub blocks: Vec<world_operations::BlockChange>,
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

/// Result of the bounded structure-site rule check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructureSites {
    pub dimension: String,
    pub bounds: BlockBox,
    pub sites: Vec<StructureSite>,
    pub scanned_candidates: u64,
    pub rejected_candidates: u64,
    pub limit: u32,
}

/// One section transferred to the renderer. `data` is empty when `palette` has one entry;
/// otherwise it contains little-endian `u32` halves of the packed `u64` words
/// (`[lo0, hi0, lo1, hi1, ...]`) so JavaScript never loses integer precision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SectionSnapshot {
    pub section_y: i32,
    pub palette: Vec<BlockState>,
    pub data: Vec<u32>,
    pub bits_per_index: u8,
    pub non_air: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChunkSnapshot {
    pub chunk: ChunkPos,
    pub loaded: bool,
    pub sections: Vec<SectionSnapshot>,
}

/// Snapshot of explicitly requested chunks, used to refresh dirty chunks after an edit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChunkSnapshotBatch {
    pub dimension: String,
    pub revision: u64,
    pub chunks: Vec<ChunkSnapshot>,
}

/// Bounded render data for a set of chunks. This is a derived view; it never mutates
/// the world and carries the authoritative revision it was taken from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderSnapshot {
    pub dimension: String,
    pub revision: u64,
    pub center: ChunkPos,
    pub radius: u32,
    pub chunks: Vec<ChunkSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Query {
    WorldInfo,
    GetBlock {
        dimension: String,
        pos: BlockPos,
    },
    GetBiome {
        dimension: String,
        pos: BlockPos,
    },
    RegionSummary {
        dimension: String,
        bounds: BlockBox,
    },
    RegionBlocks {
        dimension: String,
        bounds: BlockBox,
    },
    ChunkStats {
        dimension: String,
        chunk: ChunkPos,
    },
    FindStructureSites {
        dimension: String,
        bounds: BlockBox,
        width: u32,
        depth: u32,
        step: u32,
        #[serde(rename = "maxSlope")]
        max_slope: u32,
        clearance: u32,
        limit: u32,
    },
    WorkspacePolicy,
    SceneObjects,
    InspectSceneObject {
        #[serde(rename = "objectId")]
        object_id: String,
    },
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
    Biome {
        dimension: String,
        pos: BlockPos,
        biome: Option<BiomeState>,
    },
    RegionSummary(RegionSummary),
    RegionBlocks(RegionBlocks),
    ChunkStats(ChunkStats),
    StructureSites(StructureSites),
    WorkspacePolicy(WorkspacePolicy),
    SceneObjects(Vec<SemanticObject>),
    SceneObject(SceneObjectInspection),
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
    pub layer_id: Option<String>,
}

/// Bounded editor history state for undo/redo buttons and the history panel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryState {
    pub can_undo: bool,
    pub can_redo: bool,
    pub revision: u64,
    pub entries: Vec<HistorySummary>,
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
    pub layer_id: Option<String>,
}
