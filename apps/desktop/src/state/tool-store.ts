import { create } from "zustand";
import type { ClipboardPayload, EditorTool, PendingPaste } from "@/editor/types";
import { MAX_BRUSH_RADIUS, MIN_BRUSH_RADIUS, type GhostPreview } from "@/editor/session";
import type { BlockPosition, BlockState } from "@/renderer/types";
import type { RayHit } from "@/renderer/raycast";

export interface EditorJob {
  id: number;
  label: string;
  cancellable: boolean;
  cancelRequested: boolean;
  startedAt: number;
  /** Integer percentage from a real backend job, or null for indeterminate work. */
  progress: number | null;
}

interface ToolState {
  tool: EditorTool;
  brushRadius: number;
  selectedBlock: BlockState;
  replaceFrom: BlockState | null;
  anchor: BlockPosition | null;
  selection: { min: BlockPosition; max: BlockPosition } | null;
  hover: RayHit | null;
  ghost: GhostPreview | null;
  clipboard: ClipboardPayload | null;
  pendingPaste: PendingPaste | null;
  job: EditorJob | null;
  syncState: "pristine" | "modified";
  lastOperation: {
    operationId: string;
    changedBlocks: number;
    revision: number;
    warnings: string[];
  } | null;
  setTool: (tool: EditorTool) => void;
  setBrushRadius: (radius: number) => void;
  setSelectedBlock: (state: BlockState) => void;
  setReplaceFrom: (state: BlockState | null) => void;
  setAnchor: (anchor: BlockPosition | null) => void;
  setSelection: (selection: { min: BlockPosition; max: BlockPosition } | null) => void;
  setHover: (hover: RayHit | null, ghost: GhostPreview | null) => void;
  setGhost: (ghost: GhostPreview | null) => void;
  setClipboard: (clipboard: ClipboardPayload | null) => void;
  setPendingPaste: (pendingPaste: PendingPaste | null) => void;
  beginJob: (label: string, cancellable?: boolean, progress?: number | null) => number;
  setJobProgress: (id: number, progress: number | null) => void;
  requestCancel: () => void;
  finishJob: (id: number) => void;
  markModified: () => void;
  markSaved: () => void;
  setLastOperation: (operation: {
    operationId: string;
    changedBlocks: number;
    revision: number;
    warnings: string[];
  }) => void;
  resetTransient: () => void;
}

export function clampBrushRadius(radius: number): number {
  if (!Number.isFinite(radius)) return MIN_BRUSH_RADIUS;
  return Math.min(MAX_BRUSH_RADIUS, Math.max(MIN_BRUSH_RADIUS, Math.round(radius)));
}

let nextJobId = 1;

export const useToolStore = create<ToolState>()((set) => ({
  tool: "place",
  brushRadius: 1,
  selectedBlock: { name: "minecraft:stone_bricks", properties: {} },
  replaceFrom: null,
  anchor: null,
  selection: null,
  hover: null,
  ghost: null,
  clipboard: null,
  pendingPaste: null,
  job: null,
  syncState: "pristine",
  lastOperation: null,
  setTool: (tool) =>
    set({
      tool,
      // Switching tools must not leave a dangling first corner from another tool.
      anchor: null,
      ghost: null,
      hover: null,
      pendingPaste: null,
    }),
  setBrushRadius: (radius) => set({ brushRadius: clampBrushRadius(radius) }),
  setSelectedBlock: (selectedBlock) => set({ selectedBlock }),
  setReplaceFrom: (replaceFrom) => set({ replaceFrom }),
  setAnchor: (anchor) =>
    set((state) => ({
      anchor,
      pendingPaste: null,
      ghost: state.pendingPaste ? null : state.ghost,
    })),
  setSelection: (selection) =>
    set((state) => ({
      selection,
      pendingPaste: null,
      ghost: state.pendingPaste ? null : state.ghost,
    })),
  setHover: (hover, ghost) => set({ hover, ghost }),
  setGhost: (ghost) => set({ ghost }),
  setClipboard: (clipboard) => set({ clipboard, pendingPaste: null }),
  setPendingPaste: (pendingPaste) => set({ pendingPaste }),
  beginJob: (label, cancellable = false, progress = null) => {
    const id = nextJobId++;
    set({
      job: {
        id,
        label,
        cancellable,
        cancelRequested: false,
        startedAt: Date.now(),
        progress: progress === null ? null : Math.min(100, Math.max(0, Math.round(progress))),
      },
    });
    return id;
  },
  setJobProgress: (id, progress) =>
    set((state) => {
      if (!state.job || state.job.id !== id) return state;
      const value = progress === null ? null : Math.min(100, Math.max(0, Math.round(progress)));
      return { job: { ...state.job, progress: value } };
    }),
  requestCancel: () =>
    set((state) => (state.job ? { job: { ...state.job, cancelRequested: true } } : state)),
  finishJob: (id) => set((state) => (state.job && state.job.id === id ? { job: null } : state)),
  markModified: () => set({ syncState: "modified" }),
  markSaved: () => set({ syncState: "pristine" }),
  setLastOperation: (lastOperation) => set({ lastOperation, syncState: "modified" }),
  resetTransient: () => set({ anchor: null, hover: null, ghost: null, pendingPaste: null }),
}));

export function isJobCancelled(id: number): boolean {
  const job = useToolStore.getState().job;
  return !job || job.id !== id || job.cancelRequested;
}
