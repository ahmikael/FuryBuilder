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
  "raise" | "lower" | "smooth" | "flatten" | "noise" | "erode" | "terrace";

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
  | BiomeOperation;

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
  "select" | "place" | "remove" | "brush" | "box" | "fill" | "line" | "replace";

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
