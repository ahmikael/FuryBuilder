import { beforeEach, describe, expect, it, vi } from "vitest";
import type { OpenedProjectPayload, ProjectSummary, SessionStatus } from "@/project/types";
import { defaultWorkspacePolicy } from "@/project/types";
import type { StructureInsertOutcome } from "@/project/structure-types";
import { useEditorStore } from "@/state/editor-store";
import { useHistoryStore } from "@/state/history-store";
import { useToastStore, useToolStore } from "@/state/tool-store";
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

const { currentEditorState, projectIsDirty, useProjectStore } =
  await import("@/state/project-store");

const workspace = defaultWorkspacePolicy();

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
  schemaVersion: 1,
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
  useToastStore.setState({ toasts: [] });
  useProjectStore.setState({
    projects: [],
    warnings: [],
    current: null,
    recovery: null,
    checkpoints: [],
    lastSavedAtMs: null,
    lastSave: null,
    loading: false,
    error: null,
    managerOpen: false,
  });
}

beforeEach(() => {
  api.list.mockReset().mockResolvedValue({ projects: [summary], warnings: [] });
  api.status.mockReset().mockResolvedValue(session());
  api.create.mockReset().mockResolvedValue(payload());
  api.open.mockReset().mockResolvedValue(payload());
  api.save.mockReset().mockResolvedValue({
    project: payload().detail,
    checksum: "a".repeat(64),
    snapshotFile: "data/world-00000000000000000004.fwl",
    bytesWritten: 8192,
    backupCreated: true,
    prunedSnapshots: 0,
  });
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
      expect(api.create).toHaveBeenCalledWith("My Castle", "flat", {
        selectedDimension: "minecraft:overworld",
        loadRadius: 1,
        workspace,
      });
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

  it("refuses to switch projects with unsaved changes unless discarding", async () => {
    useProjectStore.setState({ current: session(), projects: [summary] });
    useWorldStore.setState({ revision: 7 });
    expect(projectIsDirty(useProjectStore.getState().current, 7)).toBe(true);

    const opened = await useProjectStore.getState().open("Other.furybuilder");
    expect(opened).toBe(false);
    expect(api.open).not.toHaveBeenCalled();
    expect(useToastStore.getState().toasts.at(-1)?.title).toBe("Unsaved changes");

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
        },
      }),
    );
    await useProjectStore.getState().open(summary.folder);
    expect(useProjectStore.getState().recovery?.recoveredRevision).toBe(6);
    expect(useToastStore.getState().toasts.at(-1)).toMatchObject({ kind: "warning" });
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
    });
    const current = useProjectStore.getState().current!;
    expect(current.savedRevision).toBe(4);
    expect(current.autosavedRevision).toBeNull();
    expect(useToolStore.getState().syncState).toBe("pristine");
    expect(useProjectStore.getState().lastSave?.checksum).toHaveLength(64);
    expect(useToastStore.getState().toasts.at(-1)?.title).toBe("Project saved");
  });

  it("does not save without an open project", async () => {
    const saved = await useProjectStore.getState().save();
    expect(saved).toBe(false);
    expect(api.save).not.toHaveBeenCalled();
    expect(useToastStore.getState().toasts[0]?.title).toBe("No project open");
  });

  it("autosaves only when the world revision is unsaved", async () => {
    useProjectStore.setState({ current: session(), projects: [summary] });
    useWorldStore.setState({ revision: 4 });
    expect(await useProjectStore.getState().autosave()).toBe(false);
    expect(api.autosave).not.toHaveBeenCalled();

    useWorldStore.setState({ revision: 6 });
    expect(await useProjectStore.getState().autosave("edit")).toBe(true);
    expect(api.autosave).toHaveBeenCalledWith(
      { selectedDimension: "minecraft:overworld", loadRadius: 1, workspace },
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
    });
  });
});

describe("close, checkpoints and rename", () => {
  it("closes without saving and resets the UI session", async () => {
    useProjectStore.setState({ current: session(), projects: [summary] });
    const reload = vi.fn();
    viewportBus.addEventListener("reload-world", reload);
    try {
      const closed = await useProjectStore.getState().close(false);
      expect(closed).toBe(true);
      expect(api.close).toHaveBeenCalledWith(false, {
        selectedDimension: "minecraft:overworld",
        loadRadius: 1,
        workspace,
      });
      expect(useProjectStore.getState().current).toBeNull();
      expect(useWorldStore.getState().revision).toBe(0);
      expect(reload).toHaveBeenCalledTimes(1);
    } finally {
      viewportBus.removeEventListener("reload-world", reload);
    }
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
      expect(useToastStore.getState().toasts.at(-1)?.title).toBe("Restored checkpoint r9");
    } finally {
      viewportBus.removeEventListener("reload-world", reload);
    }
  });

  it("creates checkpoints and exposes the refreshed list", async () => {
    useProjectStore.setState({ current: session(), projects: [summary] });
    api.checkpoint.mockResolvedValue([
      { revision: 4, checksum: "c".repeat(64), bytes: 100, savedAtMs: 1, reason: "manual" },
    ]);
    const created = await useProjectStore.getState().createCheckpoint("manual");
    expect(created).toBe(true);
    expect(api.checkpoint).toHaveBeenCalledWith("manual");
    expect(useProjectStore.getState().checkpoints).toHaveLength(1);
    expect(useToastStore.getState().toasts.at(-1)?.title).toBe("Checkpoint created");
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
      expect(useToastStore.getState().toasts.at(-1)?.title).toContain("Imported");
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
      expect(useToastStore.getState().toasts.at(-1)?.title).toBe("Structure inserted");
    } finally {
      viewportBus.removeEventListener("reload-world", reload);
    }
  });
});
