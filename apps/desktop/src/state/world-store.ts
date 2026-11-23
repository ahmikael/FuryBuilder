import { create } from "zustand";
import type { CameraPose, ChunkPos, RenderMetrics } from "@/renderer/types";
import type { BlockSelection } from "@/renderer/types";
import type { CaptureMetadata } from "@/renderer/observation";

export type WorldSource = "core" | "preview";
export type WorldStatus = "idle" | "loading" | "ready" | "error";

export interface WorldSummary {
  dimension: string;
  revision: number;
  blockCount: number;
  sectionCount: number;
  chunkCount: number;
}

interface WorldUiState extends WorldSummary {
  status: WorldStatus;
  source: WorldSource | null;
  error: string | null;
  loadRadius: number;
  selection: BlockSelection | null;
  metrics: RenderMetrics | null;
  cameraPose: CameraPose | null;
  lastCapture: CaptureMetadata | null;
  gridVisible: boolean;
  chunkBordersVisible: boolean;
  setStatus: (status: WorldStatus, error?: string | null) => void;
  applySummary: (summary: Partial<WorldSummary> & { source?: WorldSource }) => void;
  setLoadRadius: (radius: number) => void;
  setSelection: (selection: BlockSelection | null) => void;
  setMetrics: (metrics: RenderMetrics) => void;
  setCameraPose: (pose: CameraPose) => void;
  setLastCapture: (capture: CaptureMetadata | null) => void;
  setGridVisible: (visible: boolean) => void;
  setChunkBordersVisible: (visible: boolean) => void;
  reset: () => void;
}

export const MIN_LOAD_RADIUS = 1;
export const MAX_LOAD_RADIUS = 4;

export function clampLoadRadius(radius: number): number {
  if (!Number.isFinite(radius)) return 1;
  return Math.min(MAX_LOAD_RADIUS, Math.max(MIN_LOAD_RADIUS, Math.round(radius)));
}

export const useWorldStore = create<WorldUiState>()((set) => ({
  status: "idle",
  source: null,
  error: null,
  dimension: "minecraft:overworld",
  revision: 0,
  blockCount: 0,
  sectionCount: 0,
  chunkCount: 0,
  loadRadius: 1,
  selection: null,
  metrics: null,
  cameraPose: null,
  lastCapture: null,
  gridVisible: true,
  chunkBordersVisible: true,
  setStatus: (status, error = null) => set({ status, error }),
  applySummary: (summary) => set(summary),
  setLoadRadius: (radius) => set({ loadRadius: clampLoadRadius(radius) }),
  setSelection: (selection) => set({ selection }),
  setMetrics: (metrics) => set({ metrics }),
  setCameraPose: (cameraPose) => set({ cameraPose }),
  setLastCapture: (lastCapture) => set({ lastCapture }),
  setGridVisible: (gridVisible) => set({ gridVisible }),
  setChunkBordersVisible: (chunkBordersVisible) => set({ chunkBordersVisible }),
  reset: () =>
    set({
      status: "idle",
      source: null,
      error: null,
      dimension: "minecraft:overworld",
      revision: 0,
      blockCount: 0,
      sectionCount: 0,
      chunkCount: 0,
      loadRadius: 1,
      selection: null,
      metrics: null,
      cameraPose: null,
      lastCapture: null,
      gridVisible: true,
      chunkBordersVisible: true,
    }),
}));

export function chunkPosFromBlock(pos: { x: number; z: number }): ChunkPos {
  return { x: Math.floor(pos.x / 16), z: Math.floor(pos.z / 16) };
}
