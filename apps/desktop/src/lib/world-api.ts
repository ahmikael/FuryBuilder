import { invokeCommand } from "@/lib/ipc";
import type { WorldInfo } from "@/lib/contracts";
import type { BlockState, ChunkPos } from "@/renderer/types";
import type {
  BlockDefinition,
  BoxBounds,
  EditorOperation,
  HistoryState,
  HistorySummary,
  OperationResult,
  PreviewResult,
  RegionBlocks,
} from "@/editor/types";

/** Single local project for the MVP; project management arrives in M5. */
export const UI_PROJECT_ID = "local-project";

export function newIdempotencyKey(): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    return crypto.randomUUID();
  }
  return `ui-${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

export function fetchBlockCatalog(): Promise<BlockDefinition[]> {
  return invokeCommand<BlockDefinition[]>("block_catalog");
}

export function fetchWorldInfo(): Promise<WorldInfo> {
  return invokeCommand<WorldInfo>("world_info");
}

export function previewOperation(operation: EditorOperation): Promise<PreviewResult> {
  return invokeCommand<PreviewResult>("preview_operation", { operation });
}

export function applyOperation(
  operation: EditorOperation,
  expectedRevision?: number,
  idempotencyKey: string = newIdempotencyKey(),
): Promise<OperationResult> {
  return invokeCommand<OperationResult>("apply_operation", {
    operation,
    expectedRevision: expectedRevision ?? null,
    idempotencyKey,
  });
}

export function undoOperation(operationId?: string): Promise<OperationResult> {
  return invokeCommand<OperationResult>("undo_operation", {
    operationId: operationId ?? null,
  });
}

export function redoOperation(): Promise<OperationResult> {
  return invokeCommand<OperationResult>("redo_operation");
}

export function fetchHistoryState(): Promise<HistoryState> {
  return invokeCommand<HistoryState>("history_state");
}

export function fetchHistory(): Promise<HistorySummary[]> {
  return invokeCommand<HistorySummary[]>("operation_history");
}

export function readRegion(dimension: string, bounds: BoxBounds): Promise<RegionBlocks> {
  return invokeCommand<RegionBlocks>("read_region", { dimension, bounds });
}

export interface BlockRead {
  dimension: string;
  pos: { x: number; y: number; z: number };
  state: BlockState;
}

export function readBlock(
  dimension: string,
  pos: { x: number; y: number; z: number },
): Promise<BlockState> {
  return invokeCommand<BlockState>("get_block", { dimension, pos });
}

/** Reads the biome covering a block; `null` means the source has no biome data. */
export function readBiome(
  dimension: string,
  pos: { x: number; y: number; z: number },
): Promise<string | null> {
  return invokeCommand<string | null>("get_biome", { dimension, pos });
}

export function dirtyChunkList(result: { dirtyChunks: ChunkPos[] }): ChunkPos[] {
  return result.dirtyChunks;
}
