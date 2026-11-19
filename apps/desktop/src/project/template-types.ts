import type { Bounds3 } from "@/renderer/types";
import type { StructureImportPreview } from "@/project/structure-types";

export type TemplateSource = "selection" | "import";

/** One verified Java .nbt template stored in the project-local library. */
export interface TemplateEntry {
  id: string;
  name: string;
  file: string;
  bytes: number;
  sha256: string;
  dataVersion: number;
  size: [number, number, number];
  nonAirBlocks: number;
  paletteSize: number;
  source: TemplateSource;
  createdAtMs: number;
}

export interface TemplateInsertPreview {
  entry: TemplateEntry;
  preview: StructureImportPreview;
  bounds: Bounds3;
  affectedBlocks: number;
  changedBlocks: number;
  warnings: string[];
}

export interface TemplatePreviewArgs {
  templateId: string;
  dimension: string;
  origin: [number, number, number];
  includeAir: boolean;
}

export interface TemplateInsertArgs extends TemplatePreviewArgs {
  layerId?: string | null;
}
