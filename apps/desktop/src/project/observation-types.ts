import type { Bounds3 } from "@/renderer/types";

export type ObservationMode =
  | "top_down"
  | "heightmap"
  | "material_map"
  | "biome_map"
  | "cross_section"
  | "revision_diff";

export type ObservationSliceAxis = "x" | "y" | "z";

export interface ObservationSlice {
  axis: ObservationSliceAxis;
  coordinate: number;
}

export interface ObservationLegendEntry {
  key: string;
  label: string;
  color: string;
  count: number;
}

export interface ObservationCounts {
  added: number;
  removed: number;
  changed: number;
}

export interface ObservationRequest {
  dimension: string;
  bounds: Bounds3;
  mode: ObservationMode;
  slice?: ObservationSlice;
  fromRevision?: number;
}

/** Deterministic derived map plus its authoritative revision and legend. */
export interface Observation {
  schemaVersion: number;
  mode: ObservationMode;
  dimension: string;
  worldRevision: number;
  bounds: Bounds3;
  width: number;
  height: number;
  blocksPerPixel: number;
  /** `data:image/png;base64,...` */
  image: string;
  imageBytes: number;
  legend: ObservationLegendEntry[];
  legendTruncated: boolean;
  partial: boolean;
  warnings: string[];
  notes: string[];
  scannedBlocks: number;
  sampledUnits: number;
  nonAirUnits: number;
  counts: ObservationCounts;
  fromRevision?: number | null;
  slice?: ObservationSlice | null;
}

export const OBSERVATION_MODES: {
  value: ObservationMode;
  label: string;
  description: string;
}[] = [
  {
    value: "top_down",
    label: "Top-down",
    description: "Surface materials shaded by elevation.",
  },
  {
    value: "heightmap",
    label: "Heightmap",
    description: "Five elevation bands for the top non-air block.",
  },
  {
    value: "material_map",
    label: "Material map",
    description: "Top non-air block colour per column.",
  },
  {
    value: "biome_map",
    label: "Biome map",
    description: "Biome at the surface column; unknown data is explicit.",
  },
  {
    value: "cross_section",
    label: "Cross-section",
    description: "One orthogonal plane through the selected bounds.",
  },
  {
    value: "revision_diff",
    label: "Revision diff",
    description: "Added, removed and changed blocks projected top-down.",
  },
];

export function observationModeLabel(mode: ObservationMode): string {
  return OBSERVATION_MODES.find((entry) => entry.value === mode)?.label ?? mode;
}

export function observationBoundsLabel(bounds: Bounds3): string {
  const { min, max } = bounds;
  return `(${min.x},${min.y},${min.z})..(${max.x},${max.y},${max.z})`;
}
