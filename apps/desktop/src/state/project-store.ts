import { create } from "zustand";
import { normalizeError } from "@/lib/ipc";
import {
  autosaveProject,
  checkpointProject,
  closeProject,
  createProject,
  fetchCheckpoints,
  fetchProjectStatus,
  fetchProjects,
  openCheckpoint,
  openProject,
  renameProject,
  saveProject,
} from "@/lib/project-api";
import {
  importStructureProject as importStructureProjectApi,
  insertStructure as insertStructureApi,
} from "@/lib/structure-api";
import { fetchWorldInfo as fetchProjectWorldInfo } from "@/lib/world-api";
import type {
  CheckpointMeta,
  CreationMode,
  EditorState,
  OpenedProjectPayload,
  ProjectSummary,
  RecoveryInfo,
  SaveOutcome,
  SessionStatus,
} from "@/project/types";
import type { StructureImportProjectArgs, StructureInsertArgs } from "@/project/structure-types";
import { useEditorStore } from "@/state/editor-store";
import { useHistoryStore } from "@/state/history-store";
import { pushToast, useToolStore } from "@/state/tool-store";
import { useWorldStore } from "@/state/world-store";
import { reloadViewport } from "@/features/viewport/viewport-bus";

interface ProjectUiState {
  projects: ProjectSummary[];
  warnings: string[];
  current: SessionStatus | null;
  recovery: RecoveryInfo | null;
  checkpoints: CheckpointMeta[];
  lastSavedAtMs: number | null;
  lastSave: SaveOutcome | null;
  loading: boolean;
  error: string | null;
  managerOpen: boolean;
  setManagerOpen: (open: boolean) => void;
  refreshList: () => Promise<void>;
  refreshSession: () => Promise<void>;
  resume: () => Promise<void>;
  create: (name: string, mode: CreationMode) => Promise<boolean>;
  importStructure: (args: StructureImportProjectArgs) => Promise<boolean>;
  insertStructure: (args: StructureInsertArgs) => Promise<boolean>;
  open: (folder: string, options?: { discardCurrent?: boolean }) => Promise<boolean>;
  save: () => Promise<boolean>;
  autosave: (reason?: string) => Promise<boolean>;
  createCheckpoint: (reason: string) => Promise<boolean>;
  loadCheckpoints: (folder?: string) => Promise<void>;
  restoreCheckpoint: (revision: number) => Promise<boolean>;
  close: (save: boolean) => Promise<boolean>;
  rename: (folder: string, name: string) => Promise<boolean>;
  dismissRecovery: () => void;
  discardRecovery: () => Promise<boolean>;
  reset: () => void;
}

const initial = {
  projects: [] as ProjectSummary[],
  warnings: [] as string[],
  current: null as SessionStatus | null,
  recovery: null as RecoveryInfo | null,
  checkpoints: [] as CheckpointMeta[],
  lastSavedAtMs: null as number | null,
  lastSave: null as SaveOutcome | null,
  loading: false,
  error: null as string | null,
  managerOpen: false,
};

export function currentEditorState(): EditorState {
  return {
    selectedDimension: useEditorStore.getState().selectedDimension,
    loadRadius: useWorldStore.getState().loadRadius,
  };
}

function reportError(error: unknown, title: string): void {
  const normalized = normalizeError(error);
  pushToast({
    kind: "error",
    title,
    description: `${normalized.message}${normalized.code !== "unknown" ? ` [${normalized.code}]` : ""}`,
  });
}

function applyPayload(payload: OpenedProjectPayload): void {
  const { detail, info, recovery, session } = payload;
  useProjectStore.setState({
    current: session,
    recovery,
    lastSavedAtMs: detail.summary.lastSavedAtMs,
    lastSave: null,
    loading: false,
    error: null,
    checkpoints: [],
  });
  const editorState = detail.editorState;
  useEditorStore.getState().setSelectedDimension(editorState.selectedDimension);
  useWorldStore.getState().setLoadRadius(editorState.loadRadius);
  useWorldStore.getState().applySummary({
    source: "core",
    dimension: editorState.selectedDimension,
    revision: info.revision,
    blockCount: info.blockCount,
    sectionCount: info.sectionCount,
    chunkCount: info.chunkCount,
  });
  useToolStore.getState().resetTransient();
  useToolStore.getState().setSelection(null);
  useToolStore.getState().setClipboard(null);
  useToolStore.getState().markSaved();
  void useHistoryStore.getState().refresh();
  reloadViewport();
}

export const useProjectStore = create<ProjectUiState>()((set, get) => ({
  ...initial,

  setManagerOpen: (managerOpen) => set({ managerOpen }),

  refreshList: async () => {
    try {
      const list = await fetchProjects();
      set({ projects: list.projects, warnings: list.warnings, error: null });
    } catch (error) {
      set({ error: normalizeError(error).message });
    }
  },

  refreshSession: async () => {
    try {
      const current = await fetchProjectStatus();
      set({
        current: current.folder ? current : null,
        loading: false,
      });
      if (!current.folder) {
        set({ recovery: null, checkpoints: [] });
      }
    } catch (error) {
      set({ error: normalizeError(error).message, loading: false });
    }
  },

  resume: async () => {
    try {
      const session = await fetchProjectStatus();
      if (!session.folder) {
        set({ current: null });
        return;
      }
      const info = await fetchProjectWorldInfo();
      set({ current: session });
      const editorState = session.editorState ?? currentEditorState();
      useEditorStore.getState().setSelectedDimension(editorState.selectedDimension);
      useWorldStore.getState().setLoadRadius(editorState.loadRadius);
      useWorldStore.getState().applySummary({
        source: "core",
        dimension: editorState.selectedDimension,
        revision: info.revision,
        blockCount: info.blockCount,
        sectionCount: info.sectionCount,
        chunkCount: info.chunkCount,
      });
      void useHistoryStore.getState().refresh();
      reloadViewport();
    } catch (error) {
      set({ error: normalizeError(error).message });
    }
  },

  create: async (name, mode) => {
    set({ loading: true });
    try {
      const payload = await createProject(name.trim(), mode, currentEditorState());
      applyPayload(payload);
      set({ managerOpen: false });
      pushToast({
        kind: "success",
        title: `Opened “${payload.detail.summary.name}”`,
        description: `${payload.detail.summary.creationMode} project · ${payload.detail.summary.folder}`,
      });
      void get().refreshList();
      return true;
    } catch (error) {
      set({ loading: false });
      reportError(error, "Project creation failed");
      return false;
    }
  },

  importStructure: async (args) => {
    set({ loading: true });
    try {
      const payload = await importStructureProjectApi(args);
      applyPayload(payload);
      set({ managerOpen: false });
      pushToast({
        kind: "success",
        title: `Imported “${payload.detail.summary.name}”`,
        description: `${args.sourceFileName} · rev ${payload.detail.summary.worldRevision} · ${payload.info.blockCount.toLocaleString()} blocks`,
      });
      void get().refreshList();
      return true;
    } catch (error) {
      set({ loading: false });
      reportError(error, "Structure import failed");
      return false;
    }
  },

  insertStructure: async (args) => {
    set({ loading: true });
    try {
      const result = await insertStructureApi(args);
      useEditorStore.getState().setSelectedDimension(args.dimension);
      useWorldStore.getState().applySummary({
        source: "core",
        dimension: args.dimension,
        revision: result.info.revision,
        blockCount: result.info.blockCount,
        sectionCount: result.info.sectionCount,
        chunkCount: result.info.chunkCount,
      });
      set({ current: result.session, loading: false, managerOpen: false });
      useToolStore.getState().markModified();
      useToolStore.getState().setSelection(null);
      void useHistoryStore.getState().refresh();
      reloadViewport();
      pushToast({
        kind: "success",
        title: "Structure inserted",
        description: result.operation
          ? `${result.operation.changedBlocks.toLocaleString()} block(s) changed · rev ${result.operation.worldRevision}`
          : "The structure contained no non-empty positions.",
      });
      return true;
    } catch (error) {
      set({ loading: false });
      reportError(error, "Structure insertion failed");
      return false;
    }
  },

  open: async (folder, options = {}) => {
    const state = get();
    if (state.current?.folder === folder) {
      set({ managerOpen: false });
      return true;
    }
    if (
      projectIsDirty(state.current, useWorldStore.getState().revision) &&
      !options.discardCurrent
    ) {
      pushToast({
        kind: "warning",
        title: "Unsaved changes",
        description: "Save or discard the current project before opening another one.",
      });
      return false;
    }
    set({ loading: true });
    try {
      const payload = await openProject(folder);
      applyPayload(payload);
      set({ managerOpen: false });
      pushToast({
        kind: payload.recovery ? "warning" : "success",
        title: payload.recovery
          ? `Recovered “${payload.detail.summary.name}”`
          : `Opened “${payload.detail.summary.name}”`,
        description: payload.recovery
          ? `${payload.recovery.reason} Revision ${payload.recovery.recoveredRevision} restored from the autosave; save to keep it.`
          : `Revision ${payload.detail.summary.worldRevision}`,
      });
      void get().refreshList();
      return true;
    } catch (error) {
      set({ loading: false });
      reportError(error, "Project open failed");
      return false;
    }
  },

  save: async () => {
    const state = get();
    if (!state.current?.folder) {
      pushToast({
        kind: "info",
        title: "No project open",
        description: "Create or open a project before saving.",
      });
      return false;
    }
    set({ loading: true });
    try {
      const outcome = await saveProject(currentEditorState());
      const revision = useWorldStore.getState().revision;
      set((stateValue) => ({
        current: stateValue.current
          ? {
              ...stateValue.current,
              savedRevision: outcome.project.summary.worldRevision,
              autosavedRevision: null,
              dirty: revision !== outcome.project.summary.worldRevision,
            }
          : stateValue.current,
        lastSavedAtMs: outcome.project.summary.lastSavedAtMs,
        lastSave: outcome,
        loading: false,
        recovery: null,
      }));
      useToolStore.getState().markSaved();
      pushToast({
        kind: "success",
        title: "Project saved",
        description: `rev ${outcome.project.summary.worldRevision} · ${outcome.bytesWritten.toLocaleString()} bytes · checksum ${outcome.checksum.slice(0, 12)}…${
          outcome.prunedSnapshots > 0 ? ` · pruned ${outcome.prunedSnapshots}` : ""
        }`,
      });
      void get().refreshList();
      void get().refreshSession();
      return true;
    } catch (error) {
      set({ loading: false });
      reportError(error, "Save failed");
      return false;
    }
  },

  autosave: async (reason = "") => {
    const state = get();
    if (!state.current?.folder) return false;
    const revision = useWorldStore.getState().revision;
    if (!state.current.dirty && revision === state.current.savedRevision) return false;
    if (state.current.autosavedRevision === revision) return false;
    try {
      const savedRevision = await autosaveProject(currentEditorState(), reason);
      set((current) => ({
        current: current.current
          ? {
              ...current.current,
              autosavedRevision: savedRevision,
              revision,
            }
          : current.current,
      }));
      return true;
    } catch (error) {
      reportError(error, "Autosave failed");
      return false;
    }
  },

  createCheckpoint: async (reason) => {
    if (!get().current?.folder) return false;
    try {
      const checkpoints = await checkpointProject(reason);
      set({ checkpoints });
      pushToast({
        kind: "success",
        title: "Checkpoint created",
        description: `${checkpoints.length} checkpoint(s) kept, newest revision ${checkpoints[0]?.revision ?? "—"}`,
      });
      void get().refreshSession();
      return true;
    } catch (error) {
      reportError(error, "Checkpoint failed");
      return false;
    }
  },

  loadCheckpoints: async (folder) => {
    try {
      const checkpoints = await fetchCheckpoints(folder);
      set({ checkpoints });
    } catch (error) {
      set({ error: normalizeError(error).message });
    }
  },

  restoreCheckpoint: async (revision) => {
    if (!get().current?.folder) return false;
    set({ loading: true });
    try {
      await openCheckpoint(revision);
      await get().refreshSession();
      set({ loading: false });
      useToolStore.getState().markModified();
      void useHistoryStore.getState().refresh();
      reloadViewport();
      pushToast({
        kind: "warning",
        title: `Restored checkpoint r${revision}`,
        description: "The checkpoint replaced the open world. Save to make it the project state.",
      });
      return true;
    } catch (error) {
      set({ loading: false });
      reportError(error, "Checkpoint restore failed");
      return false;
    }
  },

  close: async (saveBeforeClose) => {
    const state = get();
    if (!state.current?.folder) return true;
    set({ loading: true });
    try {
      await closeProject(saveBeforeClose, currentEditorState());
      set({
        current: null,
        recovery: null,
        checkpoints: [],
        lastSave: null,
        lastSavedAtMs: null,
        loading: false,
      });
      useToolStore.getState().markSaved();
      useToolStore.getState().resetTransient();
      useToolStore.getState().setSelection(null);
      useHistoryStore.getState().reset();
      useWorldStore
        .getState()
        .applySummary({ revision: 0, blockCount: 0, sectionCount: 0, chunkCount: 0 });
      useWorldStore.getState().setSelection(null);
      reloadViewport();
      pushToast({
        kind: saveBeforeClose ? "success" : "info",
        title: saveBeforeClose ? "Project saved and closed" : "Project closed",
        description: saveBeforeClose
          ? "All edits were persisted."
          : "Unsaved changes were discarded.",
      });
      void get().refreshList();
      return true;
    } catch (error) {
      set({ loading: false });
      reportError(error, "Close failed");
      return false;
    }
  },

  rename: async (folder, name) => {
    try {
      const detail = await renameProject(folder, name);
      set((state) => ({
        projects: state.projects.map((project) =>
          project.folder === folder ? detail.summary : project,
        ),
        current:
          state.current?.folder === folder
            ? { ...state.current, name: detail.summary.name }
            : state.current,
      }));
      pushToast({ kind: "success", title: "Project renamed", description: detail.summary.name });
      return true;
    } catch (error) {
      reportError(error, "Rename failed");
      return false;
    }
  },

  dismissRecovery: () => set({ recovery: null }),

  discardRecovery: async () => {
    const state = get();
    const folder = state.current?.folder;
    if (!folder) return false;
    set({ loading: true });
    try {
      await closeProject(false, currentEditorState());
      set({ current: null, recovery: null, loading: false });
      useToolStore.getState().markSaved();
      const opened = await get().open(folder, { discardCurrent: true });
      return opened;
    } catch (error) {
      set({ loading: false });
      reportError(error, "Recovery discard failed");
      return false;
    }
  },

  reset: () => set(initial),
}));

/** True while unsaved edits exist in the open project. */
export function projectIsDirty(session: SessionStatus | null, worldRevision: number): boolean {
  return Boolean(session?.folder) && worldRevision !== (session?.savedRevision ?? 0);
}
