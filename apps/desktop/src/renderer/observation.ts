import { blockColor } from "@/renderer/assets";
import type { CameraPose, ChunkPos } from "@/renderer/types";
import type { RenderWorld } from "@/renderer/world";

export const OBSERVATION_SCHEMA_VERSION = 1;
export const RENDERER_SCHEMA_VERSION = 1;
export const MAX_LEGEND_ENTRIES = 16;

export interface ObservationLegendEntry {
  state: string;
  count: number;
  color: string;
}

export interface CaptureMetadata {
  schemaVersion: number;
  mode: "viewport";
  dimension: string;
  worldRevision: number;
  camera: CameraPose;
  resolution: [number, number];
  renderer: "three-webgl2";
  rendererSchema: number;
  legend: ObservationLegendEntry[];
  legendTruncated: boolean;
  requestedChunks: ChunkPos[];
  loadedChunks: ChunkPos[];
  missingChunks: ChunkPos[];
  partial: boolean;
  capturedAt: string;
  notes: string[];
}

export interface CaptureResult {
  dataUrl: string;
  metadata: CaptureMetadata;
}

/**
 * Builds the Observation metadata for a viewport capture. Reproducibility contract:
 * the same project, `worldRevision`, camera pose, resolution and renderer schema must
 * describe the same view. Anti-aliasing/GPU differences are explicitly out of scope.
 */
export function buildCaptureMetadata(
  world: RenderWorld,
  camera: CameraPose,
  resolution: [number, number],
  requestedChunks: ChunkPos[],
  capturedAt: Date = new Date(),
): CaptureMetadata {
  const loadedChunks = world.loadedChunks();
  const missingChunks = world.missingChunks(requestedChunks);
  const counts = world.blockCountsByState();
  const sorted = [...counts.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
  const legend = sorted.slice(0, MAX_LEGEND_ENTRIES).map(([state, count]) => ({
    state,
    count,
    color: blockColor(state),
  }));
  return {
    schemaVersion: OBSERVATION_SCHEMA_VERSION,
    mode: "viewport",
    dimension: world.dimension,
    worldRevision: world.revision,
    camera,
    resolution,
    renderer: "three-webgl2",
    rendererSchema: RENDERER_SCHEMA_VERSION,
    legend,
    legendTruncated: sorted.length > MAX_LEGEND_ENTRIES,
    requestedChunks,
    loadedChunks,
    missingChunks,
    partial: missingChunks.length > 0,
    capturedAt: capturedAt.toISOString(),
    notes: [
      "Image is a derived view; authoritative block data lives in the Rust world core.",
      "Textures are original FuryBuilder placeholders, not Minecraft assets.",
      "Reproduce with the same worldRevision, camera pose, resolution and renderer schema.",
      "GPU anti-aliasing and driver differences are not modelled by the metadata.",
      "partial=true means requested chunks were not loaded when the image was captured.",
    ],
  };
}
