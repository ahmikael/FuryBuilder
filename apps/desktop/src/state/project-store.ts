import { create } from "zustand";
import { normalizeError } from "@/lib/ipc";
import { presentError } from "@/lib/error-messages";
import {
  analyzeJavaWorld,
  autosaveProject,
  cancelProjectPrepare,
  checkpointProject,
  closeProject,
  createProject,
  type CreateProjectOptions,
  fetchCheckpoints,
  fetchProjectStatus,
  fetchProjects,
  importGeneratedWorld as importGeneratedWorldApi,
  importJavaWorld,
  openCheckpoint,
  openProject,
  type PrepareProgress,
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
  ConfirmedImportVersion,
  CreationMode,
  EditorState,
  ImportStrategy,
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

/** Invalidates any autosave that was started for the previous session. */
let autosaveGeneration = 0;
let autosaveInFlight: Promise<boolean> | null = null;

/**
 * Stops the next autosave and waits for one already running. Closing, switching or
 * replacing a project must never race an autosave that still writes the old folder.
 */
async function settlePendingAutosave(): Promise<void> {
  autosaveGeneration += 1;
  const pending = autosaveInFlight;
  if (pending) await pending.catch(() => false);
}

/**
 * Clears every in-memory trace of the session: renderer-independent world state,
 * editor tools, jobs, selection, clipboard, layers, scene and undo history. The
 * viewport renderer disposes itself when the editor shell unmounts.
 */
function teardownSessionState(): void {
  const tools = useToolStore.getState();
  if (tools.job) {
    tools.requestCancel();
    tools.finishJob(tools.job.id);
  }
  tools.resetTransient();
  tools.setSelection(null);
  tools.setClipboard(null);
  tools.markSaved();
  useWorldStore.getState().reset();
  useWorkspaceStore.getState().reset();
  useSceneStore.getState().reset();
  useParametricStore.getState().reset();
  useHistoryStore.getState().reset();
}

/** The single app-level state of the open project. */
export type ProjectPhase = "closed" | "preparing" | "ready" | "saving" | "error";

/** Tabs of the project manager dialog. */
export type ManagerTab = "projects" | "create" | "checkpoints" | "exchange" | "settings";

export const MANAGER_TABS: readonly ManagerTab[] = [
  "projects",
  "create",
  "checkpoints",
  "exchange",
  "settings",
];

export function isManagerTab(value: string): value is ManagerTab {
  return (MANAGER_TABS as readonly string[]).includes(value);
}

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
  prepareJobId: string | null;
  prepareProgress: PrepareProgress[];
  prepareCancelling: boolean;
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
  managerTab: ManagerTab;
  managerCreateMode: CreationMode;
  bootstrapped: boolean;
  setManagerOpen: (open: boolean) => void;
  setManagerTab: (tab: ManagerTab) => void;
  setManagerCreateMode: (mode: CreationMode) => void;
  openManager: (tab?: ManagerTab, mode?: CreationMode) => void;
  bootstrap: () => Promise<void>;
  beginPreparing: (message: string) => void;
  markReady: () => void;
  beginSaving: () => void;
  fail: (error: unknown) => void;
  clearFailure: () => void;
  applyPrepareProgress: (progress: PrepareProgress) => void;
  cancelPreparation: () => Promise<boolean>;
  refreshList: () => Promise<void>;
  refreshSession: () => Promise<void>;
  resume: () => Promise<void>;
  create: (
    name: string,
    mode: CreationMode,
    folder?: string,
    options?: CreateProjectOptions,
  ) => Promise<boolean>;
  analyzeWorld: (source: string) => Promise<JavaWorldAnalysis | null>;
  clearWorldAnalysis: () => void;
  importWorld: (
    name: string,
    source: string,
    strategy: ImportStrategy,
    confirmedVersion: ConfirmedImportVersion,
    projectFolder?: string,
  ) => Promise<boolean>;
  importGeneratedWorld: (name: string, runId: string, projectFolder?: string) => Promise<boolean>;
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
  prepareJobId: null as string | null,
  prepareProgress: [] as PrepareProgress[],
  prepareCancelling: false,
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
  managerTab: "projects" as ManagerTab,
  managerCreateMode: "empty" as CreationMode,
  bootstrapped: false,
};

function preparingState(message: string, jobId: string | null = null) {
  return {
    phase: "preparing" as const,
    prepareMessage: message,
    prepareJobId: jobId,
    prepareProgress: [] as PrepareProgress[],
    prepareCancelling: false,
    loading: true,
    failure: null,
    error: null,
  };
}

function newPrepareJobId(): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    return crypto.randomUUID();
  }
  return `prepare-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

function readyState() {
  return {
    phase: "ready" as const,
    prepareMessage: null,
    prepareJobId: null,
    prepareProgress: [] as PrepareProgress[],
    prepareCancelling: false,
    loading: false,
    failure: null,
    error: null,
  };
}

function savingState() {
  return {
    phase: "saving" as const,
    prepareMessage: "Saving the project…",
    prepareJobId: null,
    prepareCancelling: false,
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
    prepareJobId: null,
    prepareCancelling: false,
    loading: false,
    failure: presented,
    error: presented.message,
  };
}

function closedState() {
  return {
    phase: "closed" as const,
    prepareMessage: null,
    prepareJobId: null,
    prepareProgress: [] as PrepareProgress[],
    prepareCancelling: false,
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
  const { blockStateWarning, detail, info, recovery, session } = payload;
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
  if (blockStateWarning) {
    pushToast({
      kind: "warning",
      title: "Unsupported block states preserved",
      description: blockStateWarning,
    });
  }
}

export const useProjectStore = create<ProjectUiState>()((set, get) => ({
  ...initial,

  setManagerOpen: (managerOpen) => set({ managerOpen }),

  setManagerTab: (managerTab) => set({ managerTab }),

  setManagerCreateMode: (managerCreateMode) => set({ managerCreateMode }),

  openManager: (managerTab = "projects", managerCreateMode = "empty") =>
    set({ managerOpen: true, managerTab, managerCreateMode }),

  bootstrap: async () => {
    if (get().bootstrapped) return;
    set({ bootstrapped: true });
    await get().resume();
  },

  beginPreparing: (prepareMessage) => set(preparingState(prepareMessage)),

  markReady: () => set(readyState()),

  beginSaving: () => set(savingState()),

  fail: (error) => set(failureState(error)),

  clearFailure: () =>
    set((state) => ({
      failure: null,
      error: null,
      prepareProgress: [],
      phase: state.current?.folder ? "ready" : "closed",
    })),

  applyPrepareProgress: (progress) =>
    set((state) => {
      if (!state.prepareJobId || progress.jobId !== state.prepareJobId) return {};
      return {
        prepareProgress: [
          ...state.prepareProgress.filter((entry) => entry.stage !== progress.stage),
          progress,
        ],
      };
    }),

  cancelPreparation: async () => {
    const jobId = get().prepareJobId;
    if (!jobId || get().prepareCancelling) return false;
    set({ prepareCancelling: true, prepareMessage: "Cancelling preparation…" });
    try {
      const accepted = await cancelProjectPrepare(jobId);
      if (!accepted && get().prepareJobId === jobId) {
        set({ prepareMessage: "Finishing preparation; it can no longer be cancelled." });
      }
      return accepted;
    } catch (error) {
      if (get().prepareJobId === jobId) {
        set({ prepareCancelling: false, prepareMessage: "Preparing the project…" });
        reportError(error, "Could not cancel preparation");
      }
      return false;
    }
  },

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

  create: async (name, mode, folder, options) => {
    const jobId = newPrepareJobId();
    set(preparingState("Creating the project…", jobId));
    try {
      const payload = options
        ? await createProject(
            jobId,
            name.trim(),
            mode,
            currentEditorState(),
            folder?.trim() || undefined,
            options,
          )
        : await createProject(
            jobId,
            name.trim(),
            mode,
            currentEditorState(),
            folder?.trim() || undefined,
          );
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
      if (presentError(error).code === "cancelled") {
        set(get().current?.folder ? readyState() : closedState());
        pushToast({
          kind: "info",
          title: "Preparation cancelled",
          description: "Downloaded components stay in the cache. Adjust the review and try again.",
        });
        return false;
      }
      set(failureState(error));
      reportError(error, "Project creation failed");
      return false;
    }
  },

  analyzeWorld: async (source) => {
    const trimmed = source.trim();
    if (!trimmed) {
      pushToast({
        kind: "info",
        title: "Select a world source",
        description: "Choose a world folder or .zip archive that contains level.dat.",
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

  importWorld: async (name, source, strategy, confirmedVersion, projectFolder) => {
    const trimmed = source.trim();
    if (!trimmed) return false;
    const analysis = get().worldAnalysis;
    const jobId = newPrepareJobId();
    set(preparingState("Importing the world…", jobId));
    try {
      const payload = await importJavaWorld(
        jobId,
        name.trim(),
        trimmed,
        strategy,
        confirmedVersion,
        currentEditorState(),
        projectFolder?.trim() || undefined,
      );
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
      if (presentError(error).code === "cancelled") {
        set(get().current?.folder ? readyState() : closedState());
        pushToast({
          kind: "info",
          title: "Import cancelled",
          description:
            "The source was not changed and downloaded components stay in the cache. Try again when ready.",
        });
        return false;
      }
      set(failureState(error));
      reportError(error, "World import failed");
      return false;
    }
  },

  importGeneratedWorld: async (name, runId, projectFolder) => {
    if (!runId.trim()) return false;
    const jobId = newPrepareJobId();
    set(preparingState("Importing the generated world…", jobId));
    try {
      const payload = await importGeneratedWorldApi(
        jobId,
        name.trim(),
        runId,
        currentEditorState(),
        projectFolder?.trim() || undefined,
      );
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
      if (presentError(error).code === "cancelled") {
        set(get().current?.folder ? readyState() : closedState());
        pushToast({
          kind: "info",
          title: "Import cancelled",
          description:
            "The generated run and downloaded components stay in place. Import it again when ready.",
        });
        return false;
      }
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
    if (state.phase !== "ready" || !state.current?.folder) return false;
    if (autosaveInFlight) return false;
    const revision = useWorldStore.getState().revision;
    if (!state.current.dirty && revision === state.current.savedRevision) return false;
    if (state.current.autosavedRevision === revision) return false;
    const generation = autosaveGeneration;
    const task = (async () => {
      try {
        const savedRevision = await autosaveProject(currentEditorState(), reason);
        if (generation !== autosaveGeneration) return false;
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
        if (generation === autosaveGeneration) reportError(error, "Autosave failed");
        return false;
      }
    })();
    autosaveInFlight = task;
    try {
      return await task;
    } finally {
      if (autosaveInFlight === task) autosaveInFlight = null;
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
    await settlePendingAutosave();
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
      teardownSessionState();
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
    await settlePendingAutosave();
    try {
      await closeProject(false, currentEditorState());
      set({ current: null, recovery: null, ...closedState() });
      teardownSessionState();
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

/**
 * True when the editor and renderer may mount. A project session must exist, and the
 * lifecycle must be past `closed`/`preparing`; saving and a recoverable error keep the
 * already-open world visible.
 */
export function projectAllowsEditor(phase: ProjectPhase, session: SessionStatus | null): boolean {
  return Boolean(session?.folder) && phase !== "closed" && phase !== "preparing";
}
