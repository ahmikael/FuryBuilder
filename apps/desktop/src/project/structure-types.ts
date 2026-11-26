import type { OperationResult } from "@/editor/types";
import type { WorldInfo } from "@/lib/contracts";
import type { Bounds3 } from "@/renderer/types";
import type { EditorState, SessionStatus } from "@/project/types";
import type { VersionDimension } from "@/project/version-types";

export interface StructurePaletteCount {
  state: string;
  count: number;
}

/** Preview returned by `structure_import_preview` before any world is modified. */
export interface StructureImportPreview {
  format: string;
  dataVersion: number;
  targetDataVersion: number | null;
  targetMinecraftVersion: string | null;
  dimensions: VersionDimension[];
  size: [number, number, number];
  volume: number;
  paletteSize: number;
  palette: StructurePaletteCount[];
  storedBlocks: number;
  nonAirBlocks: number;
  entities: number;
  blockEntities: number;
  unknownTags: string[];
  warnings: string[];
  compatible: boolean;
}

export interface StructureInsertOutcome {
  preview: StructureImportPreview;
  operation: OperationResult | null;
  info: WorldInfo;
  session: SessionStatus;
}

export interface StructureExportPreview {
  format: string;
  dataVersion: number;
  targetDataVersion: number;
  targetMinecraftVersion: string;
  dimension: string;
  bounds: Bounds3;
  size: [number, number, number];
  volume: number;
  paletteSize: number;
  palette: StructurePaletteCount[];
  nonAirBlocks: number;
  storedBlocks: number;
  bytes: number;
  checksum: string;
  fileName: string;
  relativePath: string;
  warnings: string[];
  compatible: boolean;
}

export interface ExportOutcome {
  relativePath: string;
  fileName: string;
  bytes: number;
  checksum: string;
  createdAtMs: number;
}

export interface StructureExportResult {
  outcome: ExportOutcome;
  verified: boolean;
  size: [number, number, number];
  nonAirBlocks: number;
  dataVersion: number;
  targetDataVersion: number;
  targetMinecraftVersion: string;
}

export interface StructureImportProjectArgs {
  name: string;
  sourceFileName: string;
  payload: string;
  dimension: string;
  origin: [number, number, number];
  includeAir: boolean;
  editorState: EditorState;
}

export interface StructureInsertArgs {
  payload: string;
  dimension: string;
  origin: [number, number, number];
  includeAir: boolean;
}

export const STRUCTURE_DIMENSIONS = [
  { id: "minecraft:overworld", label: "Overworld" },
  { id: "minecraft:the_nether", label: "Nether" },
  { id: "minecraft:the_end", label: "End" },
] as const;

export function shortBlockState(state: string): string {
  const bracket = state.indexOf("[");
  const name = (bracket >= 0 ? state.slice(0, bracket) : state).replace("minecraft:", "");
  return bracket >= 0 ? `${name} […]` : name;
}
