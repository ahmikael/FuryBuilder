import { create } from "zustand";
import { normalizeError } from "@/lib/ipc";
import { presentError } from "@/lib/error-messages";
import {
  analyzeJavaWorld,
  autosaveProject,
  checkpointProject,
  closeProject,
  createProject,
  fetchCheckpoints,
  fetchProjectStatus,
  fetchProjects,
  importGeneratedWorld as importGeneratedWorldApi,
  importJavaWorld,
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
  JavaWorldAnalysis,
  OpenedProjectPayload,
  ProjectSummary,
  RecoveryInfo,
  SaveOutcome,
  SessionStatus,
} from "@/project/types";
import { defaultWorkspacePolicy } from "@/project/types";
import { defaultScene } from "@/project/scene-types";
import type { StructureImportProjectArgs, StructureInsertArgs } from "@/project/structure-types";
import { useEditorStore } from "@/state/editor-store";
import { useHistoryStore } from "@/state/history-store";
import { pushToast } from "@/state/toast-store";
import { useToolStore } from "@/state/tool-store";
import { useWorldStore } from "@/state/world-store";
import { useWorkspaceStore } from "@/state/workspace-store";
import { useSceneStore } from "@/state/scene-store";
import { useParametricStore } from "@/state/parametric-store";
import { reloadViewport } from "@/features/viewport/viewport-bus";

/** The single app-level state of the open project. */
export type ProjectPhase = "closed" | "preparing" | "ready" | "saving" | "error";

/** Actionable lifecycle failure shown by the welcome/editor error state. */
export interface ProjectFailure {
  code: string;
  message: string;
  hint: string | null;
}

interface ProjectUiState {
  projects: ProjectSummary[];
  warnings: string[];
  current: SessionStatus | null;
  phase: ProjectPhase;
  prepareMessage: string | null;
  failure: ProjectFailure | null;
  recovery: RecoveryInfo | null;
  checkpoints: CheckpointMeta[];
  lastSavedAtMs: number | null;
  lastSave: SaveOutcome | null;
  worldAnalysis: JavaWorldAnalysis | null;
  analysisLoading: boolean;
  loading: boolean;
  error: string | null;
  managerOpen: boolean;
  setManagerOpen: (open: boolean) => void;
  beginPreparing: (message: string) => void;
  markReady: () => void;
  beginSaving: () => void;
  fail: (error: unknown) => void;
  clearFailure: () => void;
  refreshList: () => Promise<void>;
  refreshSession: () => Promise<void>;
  resume: () => Promise<void>;
  create: (name: string, mode: CreationMode) => Promise<boolean>;
  analyzeWorld: (folder: string) => Promise<JavaWorldAnalysis | null>;
  clearWorldAnalysis: () => void;
  importWorld: (name: string, folder: string) => Promise<boolean>;
  importGeneratedWorld: (name: string, runId: string) => Promise<boolean>;
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
  phase: "closed" as ProjectPhase,
  prepareMessage: null as string | null,
  failure: null as ProjectFailure | null,
  recovery: null as RecoveryInfo | null,
  checkpoints: [] as CheckpointMeta[],
  lastSavedAtMs: null as number | null,
  lastSave: null as SaveOutcome | null,
  worldAnalysis: null as JavaWorldAnalysis | null,
  analysisLoading: false,
  loading: false,
  error: null as string | null,
  managerOpen: false,
};

function preparingState(message: string) {
  return {
    phase: "preparing" as const,
    prepareMessage: message,
    loading: true,
    failure: null,
    error: null,
  };
}

function readyState() {
  return {
    phase: "ready" as const,
    prepareMessage: null,
    loading: false,
    failure: null,
    error: null,
  };
}

function savingState() {
  return {
    phase: "saving" as const,
    prepareMessage: "Saving the project…",
    loading: true,
    failure: null,
    error: null,
  };
}

function failureState(error: unknown) {
  const presented = presentError(error);
  return {
    phase: "error" as const,
    prepareMessage: null,
    loading: false,
    failure: presented,
    error: presented.message,
  };
}

function closedState() {
  return {
    phase: "closed" as const,
    prepareMessage: null,
    loading: false,
    failure: null,
    error: null,
  };
}

export function currentEditorState(): EditorState {
  return {
    selectedDimension: useEditorStore.getState().selectedDimension,
    loadRadius: useWorldStore.getState().loadRadius,
    workspace: useWorkspaceStore.getState().policy,
    scene: useSceneStore.getState().scene,
  };
}

function reportError(error: unknown, title: string): void {
  const presented = presentError(error);
  pushToast({
    kind: "error",
    title,
    description: `${presented.message}${presented.hint ? ` ${presented.hint}` : ""}${
      presented.code !== "unknown" ? ` [${presented.code}]` : ""
    }`,
  });
}

function applyPayload(payload: OpenedProjectPayload): void {
  const { detail, info, recovery, session } = payload;
  useProjectStore.setState({
    current: session,
    recovery,
    lastSavedAtMs: detail.summary.lastSavedAtMs,
    lastSave: null,
    checkpoints: [],
    worldAnalysis: null,
    analysisLoading: false,
    ...readyState(),
  });
  const editorState = detail.editorState;
  useEditorStore.getState().setSelectedDimension(editorState.selectedDimension);
  useWorldStore.getState().setLoadRadius(editorState.loadRadius);
  useWorkspaceStore.getState().reset(editorState.workspace ?? defaultWorkspacePolicy());
  useSceneStore.getState().reset(editorState.scene ?? defaultScene());
  void useParametricStore.getState().refresh();
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

  beginPreparing: (prepareMessage) => set(preparingState(prepareMessage)),

  markReady: () => set(readyState()),

  beginSaving: () => set(savingState()),

  fail: (error) => set(failureState(error)),

  clearFailure: () =>
    set((state) => ({
      failure: null,
      error: null,
      phase: state.current?.folder ? "ready" : "closed",
    })),

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
      const hasProject = Boolean(current.folder);
      set((state) => {
        const inFlight = state.phase === "preparing" || state.phase === "saving";
        return {
          current: hasProject ? current : inFlight ? state.current : null,
          loading: inFlight ? state.loading : false,
          phase: inFlight ? state.phase : hasProject ? "ready" : state.failure ? "error" : "closed",
          prepareMessage: inFlight ? state.prepareMessage : null,
          failure: hasProject ? null : state.failure,
          error: hasProject ? null : state.error,
        };
      });
      if (!hasProject) {
        set({ recovery: null, checkpoints: [] });
      }
    } catch (error) {
      set({ error: normalizeError(error).message, loading: false });
    }
  },

  resume: async () => {
    set(preparingState("Restoring the last project…"));
    try {
      const session = await fetchProjectStatus();
      if (!session.folder) {
        set({ current: null, ...closedState() });
        return;
      }
      const info = await fetchProjectWorldInfo();
      set({ current: session, ...readyState() });
      const editorState = session.editorState ?? currentEditorState();
      useEditorStore.getState().setSelectedDimension(editorState.selectedDimension);
      useWorldStore.getState().setLoadRadius(editorState.loadRadius);
      useWorkspaceStore.getState().reset(editorState.workspace ?? defaultWorkspacePolicy());
      useSceneStore.getState().reset(editorState.scene ?? defaultScene());
      void useParametricStore.getState().refresh();
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
      set(failureState(error));
    }
  },

  create: async (name, mode) => {
    set(preparingState("Creating the project…"));
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
      set(failureState(error));
      reportError(error, "Project creation failed");
      return false;
    }
  },

  analyzeWorld: async (folder) => {
    const trimmed = folder.trim();
    if (!trimmed) {
      pushToast({
        kind: "info",
        title: "Select a world folder",
        description: "Choose the folder that contains level.dat and region/.",
      });
      return null;
    }
    set({ analysisLoading: true });
    try {
      const analysis = await analyzeJavaWorld(trimmed);
      set({ worldAnalysis: analysis, analysisLoading: false, error: null });
      if (!analysis.supported) {
        pushToast({
          kind: "warning",
          title: "World cannot be imported",
          description: analysis.blockers.join(" · "),
        });
      }
      return analysis;
    } catch (error) {
      set({ worldAnalysis: null, analysisLoading: false });
      reportError(error, "World analysis failed");
      return null;
    }
  },

  clearWorldAnalysis: () => set({ worldAnalysis: null }),

  importWorld: async (name, folder) => {
    const trimmed = folder.trim();
    if (!trimmed) return false;
    const analysis = get().worldAnalysis;
    set(preparingState("Importing the world…"));
    try {
      const payload = await importJavaWorld(name.trim(), trimmed, currentEditorState());
      applyPayload(payload);
      set({ managerOpen: false });
      pushToast({
        kind: "success",
        title: `Imported “${payload.detail.summary.name}”`,
        description: `${analysis?.chunks ?? payload.info.chunkCount} chunk(s) · ${payload.info.blockCount.toLocaleString()} editable block(s) · source copy kept read-only`,
      });
      void get().refreshList();
      return true;
    } catch (error) {
      set(failureState(error));
      reportError(error, "World import failed");
      return false;
    }
  },

  importGeneratedWorld: async (name, runId) => {
    if (!runId.trim()) return false;
    set(preparingState("Importing the generated world…"));
    try {
      const payload = await importGeneratedWorldApi(name.trim(), runId, currentEditorState());
      applyPayload(payload);
      set({ managerOpen: false });
      const vanilla = payload.detail.vanilla;
      pushToast({
        kind: "success",
        title: `Imported “${payload.detail.summary.name}”`,
        description: vanilla
          ? `Minecraft ${vanilla.minecraftVersion} · seed ${vanilla.seed} · ${vanilla.chunks} chunk(s) · ${payload.info.blockCount.toLocaleString()} editable block(s) · generated base kept read-only`
          : `${payload.info.chunkCount} chunk(s) · ${payload.info.blockCount.toLocaleString()} editable block(s) · generated base kept read-only`,
      });
      void get().refreshList();
      return true;
    } catch (error) {
      set(failureState(error));
      reportError(error, "Generated world import failed");
      return false;
    }
  },

  importStructure: async (args) => {
    set(preparingState("Importing the structure…"));
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
      set(failureState(error));
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
    set(preparingState(`Opening “${folder}”…`));
    try {
      const payload = await openProject(folder);
      applyPayload(payload);
      set({ managerOpen: false });
      pushToast({
        kind: payload.recovery ? "warning" : "success",
        title: payload.recovery
          ? payload.recovery.available
            ? `Recovered "${payload.detail.summary.name}"`
            : `Opened "${payload.detail.summary.name}" with a recovery warning`
          : `Opened "${payload.detail.summary.name}"`,
        description: payload.recovery
          ? payload.recovery.available
            ? `${payload.recovery.reason} Revision ${payload.recovery.recoveredRevision} is loaded in memory; save to keep it.`
            : `${payload.recovery.reason} Opened saved revision ${payload.recovery.recoveredRevision}.`
          : `Revision ${payload.detail.summary.worldRevision}`,
      });
      void get().refreshList();
      return true;
    } catch (error) {
      set(failureState(error));
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
    set(savingState());
    try {
      const outcome = await saveProject(currentEditorState());
      const revision = useWorldStore.getState().revision;
      set((stateValue) => ({
        ...readyState(),
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
      set(failureState(error));
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
    set(preparingState(`Restoring checkpoint r${revision}…`));
    try {
      await openCheckpoint(revision);
      await get().refreshSession();
      set(readyState());
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
      set(failureState(error));
      reportError(error, "Checkpoint restore failed");
      return false;
    }
  },

  close: async (saveBeforeClose) => {
    const state = get();
    if (!state.current?.folder) {
      set(closedState());
      return true;
    }
    set(saveBeforeClose ? savingState() : preparingState("Closing the project…"));
    try {
      await closeProject(saveBeforeClose, currentEditorState());
      set({
        current: null,
        recovery: null,
        checkpoints: [],
        lastSave: null,
        lastSavedAtMs: null,
        ...closedState(),
      });
      useToolStore.getState().markSaved();
      useToolStore.getState().resetTransient();
      useWorkspaceStore.getState().reset();
      useSceneStore.getState().reset();
      useParametricStore.getState().reset();
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
      set(failureState(error));
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
    set(preparingState("Discarding the recovered autosave…"));
    try {
      await closeProject(false, currentEditorState());
      set({ current: null, recovery: null, ...closedState() });
      useToolStore.getState().markSaved();
      const opened = await get().open(folder, { discardCurrent: true });
      return opened;
    } catch (error) {
      set(failureState(error));
      reportError(error, "Recovery discard failed");
      return false;
    }
  },

  reset: () => {
    useWorkspaceStore.getState().reset();
    useSceneStore.getState().reset();
    useParametricStore.getState().reset();
    set(initial);
  },
}));

/** True while unsaved edits exist in the open project. */
export function projectIsDirty(session: SessionStatus | null, worldRevision: number): boolean {
  return Boolean(session?.folder) && worldRevision !== (session?.savedRevision ?? 0);
}
