import { beforeEach, describe, expect, it, vi } from "vitest";
import type { OpenedProjectPayload, ProjectSummary, SessionStatus } from "@/project/types";
import { defaultWorkspacePolicy } from "@/project/types";
import { defaultScene } from "@/project/scene-types";
import type { StructureInsertOutcome } from "@/project/structure-types";
import { useEditorStore } from "@/state/editor-store";
import { useHistoryStore } from "@/state/history-store";
import { useWorkspaceStore } from "@/state/workspace-store";
import { isJobCancelled, useToolStore } from "@/state/tool-store";
import { clearToasts, getToasts } from "@/test-utils/toast-log";
import { useWorldStore } from "@/state/world-store";
import { viewportBus } from "@/features/viewport/viewport-bus";

const api = vi.hoisted(() => ({
  list: vi.fn(),
  status: vi.fn(),
  create: vi.fn(),
  open: vi.fn(),
  save: vi.fn(),
  autosave: vi.fn(),
  checkpoint: vi.fn(),
  checkpoints: vi.fn(),
  openCheckpoint: vi.fn(),
  close: vi.fn(),
  rename: vi.fn(),
}));

const worldApi = vi.hoisted(() => ({
  info: vi.fn(),
  history: vi.fn(),
}));

const structureApi = vi.hoisted(() => ({
  importProject: vi.fn(),
  insert: vi.fn(),
}));

vi.mock("@/lib/project-api", () => ({
  fetchProjects: api.list,
  fetchProjectStatus: api.status,
  createProject: api.create,
  openProject: api.open,
  saveProject: api.save,
  autosaveProject: api.autosave,
  checkpointProject: api.checkpoint,
  fetchCheckpoints: api.checkpoints,
  openCheckpoint: api.openCheckpoint,
  closeProject: api.close,
  renameProject: api.rename,
}));

vi.mock("@/lib/world-api", () => ({
  fetchWorldInfo: worldApi.info,
  fetchHistoryState: worldApi.history,
}));

vi.mock("@/lib/structure-api", () => ({
  importStructureProject: structureApi.importProject,
  insertStructure: structureApi.insert,
}));

const { currentEditorState, projectAllowsEditor, projectIsDirty, useProjectStore } =
  await import("@/state/project-store");

const workspace = defaultWorkspacePolicy();
const scene = defaultScene();

const summary: ProjectSummary = {
  folder: "My Castle.furybuilder",
  projectId: "project-1",
  name: "My Castle",
  creationMode: "flat",
  targetMinecraftVersion: "1.21.4",
  targetDataVersion: 4189,
  worldRevision: 4,
  updatedAtMs: 1_700_000_000_000,
  lastSavedAtMs: 1_700_000_000_000,
  lastSaveKind: "manual",
  bytes: 8192,
  schemaVersion: 2,
  recoverable: false,
};

function session(overrides: Partial<SessionStatus> = {}): SessionStatus {
  return {
    folder: summary.folder,
    name: summary.name,
    savedRevision: 4,
    autosavedRevision: null,
    revision: 4,
    dirty: false,
    editorState: { selectedDimension: "minecraft:overworld", loadRadius: 2 },
    availableCheckpoints: 0,
    ...overrides,
  };
}

function payload(overrides: Partial<OpenedProjectPayload> = {}): OpenedProjectPayload {
  return {
    detail: {
      summary,
      editorState: { selectedDimension: "minecraft:overworld", loadRadius: 2 },
      source: { kind: "generated", immutable: true },
      createdAtMs: summary.updatedAtMs,
      warnings: [],
    },
    info: {
      targetMinecraftVersion: "1.21.4",
      targetDataVersion: 4189,
      revision: 4,
      dimensions: [],
      chunkCount: 9,
      sectionCount: 12,
      blockCount: 4096,
    },
    recovery: null,
    session: session(),
    ...overrides,
  };
}

function insertOutcome(): StructureInsertOutcome {
  return {
    preview: {
      format: "java_structure",
      dataVersion: 4189,
      targetDataVersion: 4189,
      targetMinecraftVersion: "1.21.4",
      size: [2, 2, 2],
      volume: 8,
      paletteSize: 2,
      palette: [{ state: "minecraft:stone", count: 2 }],
      storedBlocks: 2,
      nonAirBlocks: 2,
      entities: 0,
      blockEntities: 0,
      unknownTags: [],
      warnings: [],
      compatible: true,
    },
    operation: {
      operationId: "op-structure",
      worldRevision: 5,
      changedBlocks: 2,
      dirtyChunks: [{ x: 0, z: 0 }],
      warnings: [],
      replayed: false,
    },
    info: { ...payload().info, revision: 5, blockCount: 4098 },
    session: session({ savedRevision: 4, revision: 5, dirty: true }),
  };
}

function saveOutcome() {
  return {
    project: payload().detail,
    checksum: "a".repeat(64),
    snapshotFile: "data/world-00000000000000000004.fwl",
    bytesWritten: 8192,
    backupCreated: true,
    prunedSnapshots: 0,
  };
}

function resetStores(): void {
  useWorldStore.setState({
    status: "ready",
    source: "core",
    error: null,
    revision: 0,
    blockCount: 0,
    chunkCount: 0,
    sectionCount: 0,
    dimension: "minecraft:overworld",
    loadRadius: 1,
    selection: null,
  });
  useEditorStore.setState({ selectedDimension: "minecraft:overworld" });
  useToolStore.setState({
    selection: null,
    anchor: null,
    hover: null,
    ghost: null,
    clipboard: null,
    job: null,
    syncState: "modified",
    lastOperation: null,
  });
  useHistoryStore.setState({ canUndo: false, canRedo: false, revision: 0, entries: [] });
  clearToasts();
  useProjectStore.setState({
    projects: [],
    warnings: [],
    current: null,
    phase: "closed",
    prepareMessage: null,
    failure: null,
    recovery: null,
    checkpoints: [],
    lastSavedAtMs: null,
    lastSave: null,
    loading: false,
    error: null,
    managerOpen: false,
    managerTab: "projects",
    managerCreateMode: "empty",
    bootstrapped: false,
  });
}

beforeEach(() => {
  api.list.mockReset().mockResolvedValue({ projects: [summary], warnings: [] });
  api.status.mockReset().mockResolvedValue(session());
  api.create.mockReset().mockResolvedValue(payload());
  api.open.mockReset().mockResolvedValue(payload());
  api.save.mockReset().mockResolvedValue(saveOutcome());
  api.autosave.mockReset().mockResolvedValue(5);
  api.checkpoint.mockReset().mockResolvedValue([]);
  api.checkpoints.mockReset().mockResolvedValue([]);
  api.openCheckpoint.mockReset().mockResolvedValue({});
  api.close.mockReset().mockResolvedValue(session({ folder: null, name: null }));
  api.rename.mockReset().mockResolvedValue(payload().detail);
  worldApi.info.mockReset().mockResolvedValue(payload().info);
  worldApi.history.mockReset().mockResolvedValue({
    canUndo: false,
    canRedo: false,
    revision: 0,
    entries: [],
  });
  structureApi.importProject.mockReset().mockResolvedValue(payload());
  structureApi.insert.mockReset().mockResolvedValue(insertOutcome());
  resetStores();
});

describe("project activation", () => {
  it("applies editor state, world summary and reloads the viewport on create", async () => {
    const reload = vi.fn();
    viewportBus.addEventListener("reload-world", reload);
    try {
      const created = await useProjectStore.getState().create("My Castle", "flat");
      expect(created).toBe(true);
      expect(api.create).toHaveBeenCalledWith(
        "My Castle",
        "flat",
        {
          selectedDimension: "minecraft:overworld",
          loadRadius: 1,
          workspace,
          scene,
        },
        undefined,
      );
      expect(useProjectStore.getState().current?.folder).toBe(summary.folder);
      expect(useWorldStore.getState().loadRadius).toBe(2);
      expect(useWorldStore.getState().revision).toBe(4);
      expect(useWorldStore.getState().blockCount).toBe(4096);
      expect(useEditorStore.getState().selectedDimension).toBe("minecraft:overworld");
      expect(useToolStore.getState().syncState).toBe("pristine");
      expect(reload).toHaveBeenCalledTimes(1);
      expect(api.list).toHaveBeenCalled();
      await Promise.resolve();
      expect(worldApi.history).toHaveBeenCalled();
    } finally {
      viewportBus.removeEventListener("reload-world", reload);
    }
  });

  it("forwards the verified profile and flat preset to the creator", async () => {
    const flat = {
      baseY: 70,
      layers: [{ block: "minecraft:stone", thickness: 2 }],
      biome: "minecraft:desert",
    };
    const created = await useProjectStore
      .getState()
      .create("My Castle", "flat", "Custom.furybuilder", {
        profileId: "java-classic-test",
        flat,
      });

    expect(created).toBe(true);
    expect(api.create).toHaveBeenCalledWith(
      "My Castle",
      "flat",
      {
        selectedDimension: "minecraft:overworld",
        loadRadius: 1,
        workspace,
        scene,
      },
      "Custom.furybuilder",
      { profileId: "java-classic-test", flat },
    );
  });

  it("refuses to switch projects with unsaved changes unless discarding", async () => {
    useProjectStore.setState({ current: session(), projects: [summary] });
    useWorldStore.setState({ revision: 7 });
    expect(projectIsDirty(useProjectStore.getState().current, 7)).toBe(true);

    const opened = await useProjectStore.getState().open("Other.furybuilder");
    expect(opened).toBe(false);
    expect(api.open).not.toHaveBeenCalled();
    expect(getToasts().at(-1)?.title).toBe("Unsaved changes");

    const forced = await useProjectStore
      .getState()
      .open("Other.furybuilder", { discardCurrent: true });
    expect(forced).toBe(true);
    expect(api.open).toHaveBeenCalledWith("Other.furybuilder");
  });

  it("surfaces verified recovery with a warning toast", async () => {
    api.open.mockResolvedValue(
      payload({
        recovery: {
          reason: "the previous session ended without closing the project",
          recoveredRevision: 6,
          recoveredAtMs: 1_700_000_100_000,
          fromBackup: false,
          available: true,
          autosaveRevision: 6,
        },
      }),
    );
    await useProjectStore.getState().open(summary.folder);
    expect(useProjectStore.getState().recovery?.recoveredRevision).toBe(6);
    expect(getToasts().at(-1)).toMatchObject({ kind: "warning" });
  });

  it("does not claim a damaged autosave was restored", async () => {
    api.open.mockResolvedValue(
      payload({
        recovery: {
          reason: "The crash autosave does not match its recorded checksum.",
          recoveredRevision: 4,
          recoveredAtMs: 1_700_000_100_000,
          fromBackup: false,
          available: false,
          autosaveRevision: 7,
        },
      }),
    );
    await useProjectStore.getState().open(summary.folder);
    const toast = getToasts().at(-1);
    expect(toast?.kind).toBe("warning");
    expect(toast?.title).toContain("recovery warning");
    expect(toast?.description).toContain("Opened saved revision 4");
    expect(toast?.description).not.toContain("restored from the autosave");
  });
});

describe("project lifecycle", () => {
  it("allows the editor only for a session past preparing", () => {
    expect(projectAllowsEditor("ready", session())).toBe(true);
    expect(projectAllowsEditor("saving", session())).toBe(true);
    expect(projectAllowsEditor("error", session())).toBe(true);
    expect(projectAllowsEditor("closed", session())).toBe(false);
    expect(projectAllowsEditor("preparing", session())).toBe(false);
    expect(projectAllowsEditor("ready", session({ folder: null, name: null }))).toBe(false);
    expect(projectAllowsEditor("ready", null)).toBe(false);
  });

  it("reports preparing while opening and ready only after the payload is applied", async () => {
    let resolveOpen!: (value: OpenedProjectPayload) => void;
    api.open.mockReturnValue(
      new Promise<OpenedProjectPayload>((resolve) => {
        resolveOpen = resolve;
      }),
    );

    const opening = useProjectStore.getState().open(summary.folder, { discardCurrent: true });
    expect(useProjectStore.getState().phase).toBe("preparing");
    expect(useProjectStore.getState().prepareMessage).toContain("Opening");
    expect(useProjectStore.getState().loading).toBe(true);
    expect(useProjectStore.getState().current).toBeNull();

    resolveOpen(payload());
    expect(await opening).toBe(true);
    const state = useProjectStore.getState();
    expect(state.phase).toBe("ready");
    expect(state.prepareMessage).toBeNull();
    expect(state.failure).toBeNull();
    expect(state.current?.folder).toBe(summary.folder);
  });

  it("enters a retryable error state when creation fails and recovers on retry", async () => {
    api.create.mockRejectedValue({ code: "disk_space", message: "the volume is full" });

    const created = await useProjectStore.getState().create("My Castle", "flat");
    expect(created).toBe(false);
    const failed = useProjectStore.getState();
    expect(failed.phase).toBe("error");
    expect(failed.current).toBeNull();
    expect(failed.loading).toBe(false);
    expect(failed.prepareMessage).toBeNull();
    expect(failed.failure).toMatchObject({
      code: "disk_space",
      message: "the volume is full",
    });
    expect(failed.error).toBe("the volume is full");

    api.create.mockResolvedValue(payload());
    expect(await useProjectStore.getState().create("My Castle", "flat")).toBe(true);
    expect(useProjectStore.getState().phase).toBe("ready");
    expect(useProjectStore.getState().failure).toBeNull();
  });

  it("marks the open project saving without unmounting it", async () => {
    useProjectStore.setState({ current: session(), phase: "ready" });
    let resolveSave!: (value: ReturnType<typeof saveOutcome>) => void;
    api.save.mockReturnValue(
      new Promise<ReturnType<typeof saveOutcome>>((resolve) => {
        resolveSave = resolve;
      }),
    );

    const saving = useProjectStore.getState().save();
    expect(useProjectStore.getState().phase).toBe("saving");
    expect(useProjectStore.getState().loading).toBe(true);
    expect(useProjectStore.getState().current?.folder).toBe(summary.folder);

    resolveSave(saveOutcome());
    expect(await saving).toBe(true);
    expect(useProjectStore.getState().phase).toBe("ready");
    expect(useProjectStore.getState().loading).toBe(false);
  });

  it("keeps the project open when saving fails and clears the failure explicitly", async () => {
    useProjectStore.setState({ current: session(), phase: "ready" });
    api.save.mockRejectedValue({ code: "conflict", message: "the project changed" });

    expect(await useProjectStore.getState().save()).toBe(false);
    const failed = useProjectStore.getState();
    expect(failed.phase).toBe("error");
    expect(failed.current?.folder).toBe(summary.folder);
    expect(failed.failure?.code).toBe("conflict");

    useProjectStore.getState().clearFailure();
    expect(useProjectStore.getState().phase).toBe("ready");
    expect(useProjectStore.getState().failure).toBeNull();
    expect(useProjectStore.getState().current?.folder).toBe(summary.folder);
  });

  it("returns to closed after a close and derives the session phase on refresh", async () => {
    useProjectStore.setState({ current: session(), phase: "ready" });
    expect(await useProjectStore.getState().close(false)).toBe(true);
    expect(useProjectStore.getState().phase).toBe("closed");
    expect(useProjectStore.getState().current).toBeNull();

    api.status.mockResolvedValue(session());
    await useProjectStore.getState().refreshSession();
    expect(useProjectStore.getState().phase).toBe("ready");

    api.status.mockResolvedValue(session({ folder: null, name: null }));
    await useProjectStore.getState().refreshSession();
    expect(useProjectStore.getState().phase).toBe("closed");
  });
  it("tears down session state and waits for an in-flight autosave before closing", async () => {
    useProjectStore.setState({ current: session(), phase: "ready" });
    useWorldStore.setState({
      revision: 7,
      blockCount: 128,
      status: "ready",
      source: "core",
      selection: {
        dimension: "minecraft:overworld",
        pos: { x: 1, y: 64, z: 1 },
        state: { name: "minecraft:stone", properties: {} },
        face: [0, 1, 0],
      },
      metrics: {
        fps: 60,
        frameMs: 16,
        drawCalls: 4,
        triangles: 100,
        geometries: 2,
        textures: 3,
        loadedChunks: 9,
        renderedSections: 27,
        meshCount: 9,
      },
    });
    const job = useToolStore.getState().beginJob("Large paste", true);
    useToolStore.getState().setClipboard({
      dimension: "minecraft:overworld",
      size: [1, 1, 1],
      blocks: [
        {
          pos: { x: 0, y: 0, z: 0 },
          state: { name: "minecraft:stone", properties: {} },
        },
      ],
    });
    useWorkspaceStore.setState({
      policy: {
        ...workspace,
        layers: [
          ...workspace.layers,
          { id: "detail", name: "Detail", visible: false, locked: true },
        ],
        activeLayerId: "detail",
      },
      loading: true,
      saving: true,
      error: "stale error",
    });
    useHistoryStore.setState({ canUndo: true, canRedo: true, revision: 7 });

    let resolveAutosave!: (value: number) => void;
    api.autosave.mockReturnValue(
      new Promise<number>((resolve) => {
        resolveAutosave = resolve;
      }),
    );
    const autosaving = useProjectStore.getState().autosave();
    expect(api.autosave).toHaveBeenCalledTimes(1);

    const closing = useProjectStore.getState().close(false);
    expect(useProjectStore.getState().phase).toBe("preparing");
    expect(api.close).not.toHaveBeenCalled();

    resolveAutosave(7);
    expect(await autosaving).toBe(false);
    expect(await closing).toBe(true);
    expect(api.close).toHaveBeenCalledWith(false, expect.anything());

    const world = useWorldStore.getState();
    expect(world).toMatchObject({
      status: "idle",
      source: null,
      revision: 0,
      blockCount: 0,
      sectionCount: 0,
      chunkCount: 0,
      loadRadius: 1,
      selection: null,
      metrics: null,
      cameraPose: null,
      lastCapture: null,
    });
    expect(useToolStore.getState().job).toBeNull();
    expect(useToolStore.getState().selection).toBeNull();
    expect(useToolStore.getState().clipboard).toBeNull();
    expect(useToolStore.getState().lastOperation).toBeNull();
    expect(useWorkspaceStore.getState()).toMatchObject({
      policy: defaultWorkspacePolicy(),
      loading: false,
      saving: false,
      error: null,
    });
    expect(useHistoryStore.getState()).toMatchObject({
      canUndo: false,
      canRedo: false,
      revision: 0,
      entries: [],
    });
    expect(useProjectStore.getState().phase).toBe("closed");
    expect(useProjectStore.getState().current).toBeNull();
    expect(job).toBeGreaterThan(0);
    expect(isJobCancelled(job)).toBe(true);
  });

  it("never autosaves after the project is closed", async () => {
    useProjectStore.setState({ current: session(), phase: "closed" });
    expect(await useProjectStore.getState().autosave()).toBe(false);
    expect(api.autosave).not.toHaveBeenCalled();
  });
});

describe("project manager entry points and startup", () => {
  it("opens the manager on a requested tab and creation mode", () => {
    useProjectStore.getState().openManager("create", "import");
    expect(useProjectStore.getState()).toMatchObject({
      managerOpen: true,
      managerTab: "create",
      managerCreateMode: "import",
    });

    useProjectStore.getState().openManager("projects");
    expect(useProjectStore.getState()).toMatchObject({
      managerOpen: true,
      managerTab: "projects",
      managerCreateMode: "empty",
    });
  });

  it("probes the last session once and stays closed without a project", async () => {
    api.status.mockResolvedValue(session({ folder: null, name: null }));

    await useProjectStore.getState().bootstrap();
    await useProjectStore.getState().bootstrap();

    expect(api.status).toHaveBeenCalledTimes(1);
    expect(worldApi.info).not.toHaveBeenCalled();
    expect(useProjectStore.getState().phase).toBe("closed");
    expect(useProjectStore.getState().bootstrapped).toBe(true);
  });

  it("restores an existing session before the editor mounts", async () => {
    await useProjectStore.getState().bootstrap();

    expect(api.status).toHaveBeenCalledTimes(1);
    expect(worldApi.info).toHaveBeenCalledTimes(1);
    expect(useProjectStore.getState().phase).toBe("ready");
    expect(useProjectStore.getState().current?.folder).toBe(summary.folder);
  });
});

describe("save and autosave", () => {
  it("marks the world saved with the manifest revision", async () => {
    useProjectStore.setState({ current: session(), projects: [summary] });
    useWorldStore.setState({ revision: 5 });
    const saved = await useProjectStore.getState().save();
    expect(saved).toBe(true);
    expect(api.save).toHaveBeenCalledWith({
      selectedDimension: "minecraft:overworld",
      loadRadius: 1,
      workspace,
      scene,
    });
    const current = useProjectStore.getState().current!;
    expect(current.savedRevision).toBe(4);
    expect(current.autosavedRevision).toBeNull();
    expect(useToolStore.getState().syncState).toBe("pristine");
    expect(useProjectStore.getState().lastSave?.checksum).toHaveLength(64);
    expect(getToasts().at(-1)?.title).toBe("Project saved");
  });

  it("does not save without an open project", async () => {
    const saved = await useProjectStore.getState().save();
    expect(saved).toBe(false);
    expect(api.save).not.toHaveBeenCalled();
    expect(getToasts()[0]?.title).toBe("No project open");
  });

  it("autosaves only when the world revision is unsaved", async () => {
    useProjectStore.setState({ current: session(), projects: [summary], phase: "ready" });
    useWorldStore.setState({ revision: 4 });
    expect(await useProjectStore.getState().autosave()).toBe(false);
    expect(api.autosave).not.toHaveBeenCalled();

    useWorldStore.setState({ revision: 6 });
    expect(await useProjectStore.getState().autosave("edit")).toBe(true);
    expect(api.autosave).toHaveBeenCalledWith(
      { selectedDimension: "minecraft:overworld", loadRadius: 1, workspace, scene },
      "edit",
    );
    expect(useProjectStore.getState().current?.autosavedRevision).toBe(5);
    expect(useProjectStore.getState().current?.savedRevision).toBe(4);
  });

  it("reports editor state from the world and editor stores", () => {
    useWorldStore.setState({ loadRadius: 3 });
    useEditorStore.setState({ selectedDimension: "minecraft:the_nether" });
    expect(currentEditorState()).toEqual({
      selectedDimension: "minecraft:the_nether",
      loadRadius: 3,
      workspace,
      scene,
    });
  });
});

describe("close, checkpoints and rename", () => {
  it("closes without saving and resets the UI session", async () => {
    useProjectStore.setState({ current: session(), projects: [summary] });
    const closed = await useProjectStore.getState().close(false);
    expect(closed).toBe(true);
    expect(api.close).toHaveBeenCalledWith(false, {
      selectedDimension: "minecraft:overworld",
      loadRadius: 1,
      workspace,
      scene,
    });
    expect(useProjectStore.getState().current).toBeNull();
    expect(useWorldStore.getState()).toMatchObject({
      revision: 0,
      blockCount: 0,
      chunkCount: 0,
      status: "idle",
      source: null,
      selection: null,
    });
  });

  it("restores a checkpoint as unsaved work", async () => {
    useProjectStore.setState({ current: session(), projects: [summary] });
    const reload = vi.fn();
    viewportBus.addEventListener("reload-world", reload);
    try {
      const restored = await useProjectStore.getState().restoreCheckpoint(9);
      expect(restored).toBe(true);
      expect(api.openCheckpoint).toHaveBeenCalledWith(9);
      expect(useToolStore.getState().syncState).toBe("modified");
      expect(reload).toHaveBeenCalledTimes(1);
      expect(getToasts().at(-1)?.title).toBe("Restored checkpoint r9");
    } finally {
      viewportBus.removeEventListener("reload-world", reload);
    }
  });

  it("creates checkpoints and exposes the refreshed list", async () => {
    useProjectStore.setState({ current: session(), projects: [summary] });
    api.checkpoint.mockResolvedValue([
      {
        checkpointSchemaVersion: 2,
        profileId: "java-classic-1.21.4",
        revision: 4,
        checksum: "c".repeat(64),
        bytes: 100,
        savedAtMs: 1,
        reason: "manual",
        editorState: { selectedDimension: "minecraft:overworld", loadRadius: 1 },
      },
    ]);
    const created = await useProjectStore.getState().createCheckpoint("manual");
    expect(created).toBe(true);
    expect(api.checkpoint).toHaveBeenCalledWith("manual");
    expect(useProjectStore.getState().checkpoints).toHaveLength(1);
    expect(getToasts().at(-1)?.title).toBe("Checkpoint created");
  });

  it("renames the active project in the list and session", async () => {
    useProjectStore.setState({ current: session(), projects: [summary] });
    api.rename.mockResolvedValue({
      ...payload().detail,
      summary: { ...summary, name: "Renamed" },
    });
    const renamed = await useProjectStore.getState().rename(summary.folder, "Renamed");
    expect(renamed).toBe(true);
    expect(useProjectStore.getState().current?.name).toBe("Renamed");
    expect(useProjectStore.getState().projects[0]?.name).toBe("Renamed");
  });
});

describe("structure exchange actions", () => {
  it("imports a structure as a new project and adopts its editor state", async () => {
    const reload = vi.fn();
    viewportBus.addEventListener("reload-world", reload);
    try {
      structureApi.importProject.mockResolvedValue({
        ...payload(),
        detail: {
          ...payload().detail,
          source: { kind: "imported", immutable: true, reference: "castle.nbt" },
          editorState: { selectedDimension: "minecraft:the_nether", loadRadius: 3 },
        },
        session: session({
          editorState: { selectedDimension: "minecraft:the_nether", loadRadius: 3 },
        }),
      });
      const args = {
        name: "Imported",
        sourceFileName: "castle.nbt",
        payload: "cGF5bG9hZA==",
        dimension: "minecraft:the_nether",
        origin: [0, 40, 0] as [number, number, number],
        includeAir: false,
        editorState: { selectedDimension: "minecraft:the_nether", loadRadius: 3 },
      };
      const imported = await useProjectStore.getState().importStructure(args);
      expect(imported).toBe(true);
      expect(structureApi.importProject).toHaveBeenCalledWith(args);
      expect(useProjectStore.getState().current?.folder).toBe(summary.folder);
      expect(useProjectStore.getState().managerOpen).toBe(false);
      expect(useEditorStore.getState().selectedDimension).toBe("minecraft:the_nether");
      expect(useWorldStore.getState().revision).toBe(4);
      expect(reload).toHaveBeenCalledTimes(1);
      expect(getToasts().at(-1)?.title).toContain("Imported");
      expect(api.list).toHaveBeenCalled();
    } finally {
      viewportBus.removeEventListener("reload-world", reload);
    }
  });

  it("inserts a structure into the open project as unsaved work", async () => {
    useProjectStore.setState({ current: session(), projects: [summary], managerOpen: true });
    const reload = vi.fn();
    viewportBus.addEventListener("reload-world", reload);
    try {
      const inserted = await useProjectStore.getState().insertStructure({
        payload: "cGF5bG9hZA==",
        dimension: "minecraft:overworld",
        origin: [0, 64, 0],
        includeAir: true,
      });
      expect(inserted).toBe(true);
      expect(structureApi.insert).toHaveBeenCalledWith({
        payload: "cGF5bG9hZA==",
        dimension: "minecraft:overworld",
        origin: [0, 64, 0],
        includeAir: true,
      });
      expect(useWorldStore.getState().revision).toBe(5);
      expect(useWorldStore.getState().blockCount).toBe(4098);
      expect(useProjectStore.getState().current?.revision).toBe(5);
      expect(useProjectStore.getState().current?.savedRevision).toBe(4);
      expect(useProjectStore.getState().current?.dirty).toBe(true);
      expect(useProjectStore.getState().managerOpen).toBe(false);
      expect(useToolStore.getState().syncState).toBe("modified");
      expect(reload).toHaveBeenCalledTimes(1);
      expect(getToasts().at(-1)?.title).toBe("Structure inserted");
    } finally {
      viewportBus.removeEventListener("reload-world", reload);
    }
  });
});
