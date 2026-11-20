import type { BlockPosition, BlockState, ChunkPos } from "@/renderer/types";

export interface BlockDefinition {
  name: string;
  properties: Record<string, string>;
}

export interface BlockChange {
  pos: BlockPosition;
  state: BlockState;
}

export interface BoxBounds {
  min: BlockPosition;
  max: BlockPosition;
}

/** Deterministic terrain surface brushes. They never generate a whole world. */
export type TerrainBrushKind =
  | "raise"
  | "lower"
  | "smooth"
  | "flatten"
  | "noise"
  | "erode"
  | "terrace";

/** Deterministic bounded heightmap patch. It is a local construction tool, not worldgen. */
export interface HeightmapOperation {
  type: "generate_heightmap";
  dimension: string;
  bounds: BoxBounds;
  seed: number;
  scale: number;
  octaves: number;
  amplitude: number;
  baseY: number;
  surfaceDepth: number;
  surface: BlockState;
  filler: BlockState;
}

/** Paints the 4x4x4 biome cells covering a selection. */
export interface BiomeOperation {
  type: "set_biome";
  dimension: string;
  bounds: BoxBounds;
  biome: string;
}

/** Deterministic local road that follows the existing surface inside the selection. */
export interface GenerateRoadOperation {
  type: "generate_road";
  dimension: string;
  bounds: BoxBounds;
  from: BlockPosition;
  to: BlockPosition;
  width: number;
  meander: number;
  seed: number;
  clearance: number;
  surface: BlockState;
}

/** Deterministic local river channel with a bed and water column. */
export interface GenerateRiverOperation {
  type: "generate_river";
  dimension: string;
  bounds: BoxBounds;
  from: BlockPosition;
  to: BlockPosition;
  width: number;
  meander: number;
  seed: number;
  depth: number;
  water: BlockState;
  bed: BlockState;
}

/** Deterministic scatter of ground cover and small trees on free surfaces. */
export interface GenerateVegetationOperation {
  type: "generate_vegetation";
  dimension: string;
  bounds: BoxBounds;
  seed: number;
  groundCover: number;
  treeChance: number;
  treeSpacing: number;
  plant: BlockState;
  trunk: BlockState;
  leaves: BlockState;
}

/** One bounded, flat and clear candidate for a structure footprint. */
export interface StructureSite {
  origin: BlockPosition;
  minSurfaceY: number;
  maxSurfaceY: number;
}

export interface StructureSiteQuery {
  dimension: string;
  bounds: BoxBounds;
  width: number;
  depth: number;
  step: number;
  maxSlope: number;
  clearance: number;
  limit: number;
}

export interface StructureSiteSearch {
  dimension: string;
  bounds: BoxBounds;
  sites: StructureSite[];
  scannedCandidates: number;
  rejectedCandidates: number;
  limit: number;
}

export type EditorOperation =
  | { type: "set_block"; dimension: string; pos: BlockPosition; state: BlockState }
  | { type: "batch"; dimension: string; changes: BlockChange[] }
  | { type: "fill_cuboid"; dimension: string; bounds: BoxBounds; state: BlockState }
  | { type: "replace"; dimension: string; bounds: BoxBounds; from: BlockState; to: BlockState }
  | {
      type: "terrain_brush";
      dimension: string;
      bounds: BoxBounds;
      kind: TerrainBrushKind;
      material: BlockState;
      amount: number;
      seed: number;
      step: number;
      targetY?: number | null;
    }
  | HeightmapOperation
  | BiomeOperation
  | GenerateRoadOperation
  | GenerateRiverOperation
  | GenerateVegetationOperation;

export interface PreviewResult {
  affectedBlocks: number;
  changedBlocks: number;
  dirtyChunks: ChunkPos[];
  warnings: string[];
}

export interface OperationResult {
  operationId: string;
  worldRevision: number;
  changedBlocks: number;
  dirtyChunks: ChunkPos[];
  warnings: string[];
  replayed: boolean;
}

export type ActorSource = "ui" | "mcp" | "api" | "system";

export interface HistorySummary {
  projectId: string;
  operationId: string;
  actorId: string;
  source: ActorSource;
  revisionBefore: number;
  revisionAfter: number;
  changedBlocks: number;
  timestampMs: number;
}

export interface HistoryState {
  canUndo: boolean;
  canRedo: boolean;
  revision: number;
  entries: HistorySummary[];
}

export interface RegionBlocks {
  dimension: string;
  bounds: BoxBounds;
  volume: number;
  blocks: BlockChange[];
}

export type EditorTool =
  | "select"
  | "place"
  | "remove"
  | "brush"
  | "box"
  | "fill"
  | "line"
  | "replace";

export type SavedState = "clean" | "dirty" | "saving" | "saved";

export interface ToastMessage {
  id: string;
  kind: "info" | "success" | "warning" | "error";
  title: string;
  description?: string;
}

export interface ClipboardPayload {
  dimension: string;
  /** Size of the copied bounds; paste preserves this volume. */
  size: [number, number, number];
  /** Positions are relative to the copied minimum corner. */
  blocks: BlockChange[];
}

/** Large paste staged for an explicit second confirmation and ghost preview. */
export interface PendingPaste {
  target: BlockPosition;
  bounds: BoxBounds;
  blocks: number;
}
