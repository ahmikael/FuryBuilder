import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type * as IpcModule from "@/lib/ipc";
import { ProjectManager } from "@/features/project/ProjectManager";
import type { JavaWorldAnalysis, ProjectSummary } from "@/project/types";
import { defaultWorkspacePolicy } from "@/project/types";
import { defaultScene } from "@/project/scene-types";
import { useProjectStore } from "@/state/project-store";
import { useToolStore } from "@/state/tool-store";
import { clearToasts } from "@/test-utils/toast-log";
import { useWorldStore } from "@/state/world-store";

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
  analyze: vi.fn(),
  importWorld: vi.fn(),
}));

const worldApi = vi.hoisted(() => ({
  info: vi.fn(),
  history: vi.fn(),
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
  analyzeJavaWorld: api.analyze,
  importJavaWorld: api.importWorld,
}));

vi.mock("@/lib/ipc", async (importOriginal) => {
  const actual = await importOriginal<typeof IpcModule>();
  return { ...actual, isTauriRuntime: () => true };
});

vi.mock("@/lib/world-api", () => ({
  fetchWorldInfo: worldApi.info,
  fetchHistoryState: worldApi.history,
}));

const first: ProjectSummary = {
  folder: "First.furybuilder",
  projectId: "p1",
  name: "First Castle",
  creationMode: "flat",
  targetMinecraftVersion: "1.21.4",
  targetDataVersion: 4189,
  worldRevision: 4,
  updatedAtMs: 1_700_000_000_000,
  lastSavedAtMs: 1_700_000_000_000,
  lastSaveKind: "manual",
  bytes: 2048,
  schemaVersion: 1,
  recoverable: false,
};

const second: ProjectSummary = {
  folder: "Second.furybuilder",
  projectId: "p2",
  name: "Second Base",
  creationMode: "empty",
  targetMinecraftVersion: "1.21.4",
  targetDataVersion: 4189,
  worldRevision: 1,
  updatedAtMs: 1_700_000_100_000,
  lastSavedAtMs: 1_700_000_100_000,
  lastSaveKind: "manual",
  bytes: 512,
  schemaVersion: 1,
  recoverable: true,
};

const worldAnalysis: JavaWorldAnalysis = {
  root: "C:\\saves\\World",
  layout: "classic_anvil",
  selectionKind: "world_folder",
  completeSave: true,
  supported: true,
  dataVersion: 4189,
  versionId: 4189,
  versionName: "1.21.4",
  levelName: "Import test",
  declaredDimensions: ["minecraft:overworld"],
  dimensions: [
    {
      id: "minecraft:overworld",
      kind: "overworld",
      minY: -64,
      height: 384,
      regionFiles: 4,
      chunks: 841,
    },
  ],
  regions: 4,
  chunks: 841,
  blockEntities: 3,
  entities: 2,
  warnings: [
    "the selected world is opened read-only and is never modified",
    "only block states become editable in this milestone",
  ],
  blockers: [],
  supportMatrix: {
    schemaVersion: 1,
    versions: [
      {
        id: "java-1.21.4-classic",
        minecraftVersions: "1.21.4",
        dataVersions: "4189",
        layout: "classic_anvil",
        status: "supported",
        read: "supported_subset",
        edit: "block_states_and_biomes",
        write: "verified_block_states_and_biomes",
        passthrough: "byte_for_byte",
        note: "supported target",
      },
    ],
    categories: [
      {
        id: "block_states",
        label: "Block states and palettes",
        read: "supported",
        edit: "supported",
        write: "verified",
        passthrough: "byte_for_byte",
        note: "only block states",
      },
    ],
  },
};

function opened(summary: ProjectSummary) {
  return {
    detail: {
      summary,
      editorState: { selectedDimension: "minecraft:overworld", loadRadius: 1 },
      source: { kind: "generated" as const, immutable: true },
      createdAtMs: summary.updatedAtMs,
      warnings: [],
    },
    info: {
      targetMinecraftVersion: "1.21.4",
      targetDataVersion: 4189,
      revision: summary.worldRevision,
      dimensions: [],
      chunkCount: 1,
      sectionCount: 2,
      blockCount: 300,
    },
    recovery: null,
    session: {
      folder: summary.folder,
      name: summary.name,
      savedRevision: summary.worldRevision,
      autosavedRevision: null,
      revision: summary.worldRevision,
      dirty: false,
      editorState: { selectedDimension: "minecraft:overworld", loadRadius: 1 },
      availableCheckpoints: 0,
    },
  };
}

function seed(overrides: { revision?: number; recovery?: boolean } = {}): void {
  useProjectStore.setState({
    managerOpen: true,
    projects: [first, second],
    warnings: [],
    current: {
      folder: first.folder,
      name: first.name,
      savedRevision: 4,
      autosavedRevision: null,
      revision: overrides.revision ?? 4,
      dirty: (overrides.revision ?? 4) !== 4,
      editorState: { selectedDimension: "minecraft:overworld", loadRadius: 1 },
      availableCheckpoints: 0,
    },
    recovery: overrides.recovery
      ? {
          reason: "the previous session ended without closing the project",
          recoveredRevision: 6,
          recoveredAtMs: 1_700_000_200_000,
          fromBackup: false,
          available: true,
          autosaveRevision: 6,
        }
      : null,
    checkpoints: [],
    lastSave: null,
    worldAnalysis: null,
    analysisLoading: false,
    loading: false,
    error: null,
  });
  useWorldStore.setState({ revision: overrides.revision ?? 4 });
  useToolStore.setState({ syncState: "pristine" });
  clearToasts();
}

beforeEach(() => {
  api.list.mockReset().mockResolvedValue({ projects: [first, second], warnings: [] });
  api.status.mockReset().mockImplementation(async () => useProjectStore.getState().current);
  api.create.mockReset().mockResolvedValue(opened(first));
  api.open.mockReset().mockResolvedValue(opened(second));
  api.save.mockReset().mockResolvedValue({
    project: opened(first).detail,
    checksum: "a".repeat(64),
    snapshotFile: "data/world-00000000000000000004.fwl",
    bytesWritten: 2048,
    backupCreated: true,
    prunedSnapshots: 0,
  });
  api.autosave.mockReset().mockResolvedValue(5);
  api.checkpoint.mockReset().mockResolvedValue([]);
  api.checkpoints.mockReset().mockResolvedValue([]);
  api.openCheckpoint.mockReset().mockResolvedValue({});
  api.close.mockReset().mockResolvedValue({
    folder: null,
    name: null,
    savedRevision: 0,
    autosavedRevision: null,
    revision: 0,
    dirty: false,
    editorState: null,
    availableCheckpoints: 0,
  });
  api.rename.mockReset().mockImplementation(async (folder: string, name: string) => ({
    ...opened(first).detail,
    summary: { ...first, folder, name },
  }));
  api.analyze.mockReset();
  api.importWorld.mockReset();
  worldApi.info.mockReset().mockResolvedValue(opened(first).info);
  worldApi.history.mockReset().mockResolvedValue({
    canUndo: false,
    canRedo: false,
    revision: 0,
    entries: [],
  });
  seed();
});

describe("project manager", () => {
  it("lists projects with all creation modes available", async () => {
    render(<ProjectManager />);
    expect((await screen.findAllByText("First Castle")).length).toBeGreaterThan(0);
    expect(screen.getByText("Second Base")).toBeInTheDocument();
    expect(screen.getAllByText(/Flat base/).length).toBeGreaterThan(0);
    expect(screen.getAllByText(/recoverable/i).length).toBeGreaterThan(0);

    await userEvent.setup().click(screen.getByRole("tab", { name: /New project/ }));
    expect(screen.getByRole("button", { name: /Vanilla World/ })).toBeEnabled();
    expect(screen.getByRole("button", { name: /Import World/ })).toBeEnabled();
    expect(screen.getByRole("button", { name: /Empty Project/ })).toBeEnabled();
    expect(screen.getByRole("combobox", { name: "Target Java version" })).toBeDisabled();
    expect(screen.getByRole("combobox", { name: "Target Java version" })).toHaveTextContent(
      "Minecraft Java 1.21.4 · DataVersion 4189",
    );
  });

  it("creates a project with the selected mode", async () => {
    const user = userEvent.setup();
    render(<ProjectManager />);
    await user.click(screen.getByRole("tab", { name: /New project/ }));
    await user.type(screen.getByLabelText("Project name"), "New World");
    await user.click(screen.getByRole("button", { name: /Flat base/ }));
    await user.click(screen.getByRole("button", { name: "Create project" }));
    await waitFor(() =>
      expect(api.create).toHaveBeenCalledWith("New World", "flat", {
        selectedDimension: "minecraft:overworld",
        loadRadius: 1,
        workspace: defaultWorkspacePolicy(),
        scene: defaultScene(),
      }),
    );
    await waitFor(() => expect(useProjectStore.getState().managerOpen).toBe(false));
  });

  it("warns before opening another project with unsaved changes", async () => {
    seed({ revision: 7 });
    const user = userEvent.setup();
    render(<ProjectManager />);
    const row = await screen.findByTestId("project-row-Second Base");
    await user.click(within(row).getByRole("button", { name: "Open" }));
    expect(api.open).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Discard changes" }));
    await waitFor(() => expect(api.open).toHaveBeenCalledWith("Second.furybuilder"));
  });

  it("saves before opening when the user chooses Save and continue", async () => {
    seed({ revision: 7 });
    const user = userEvent.setup();
    render(<ProjectManager />);
    const row = await screen.findByTestId("project-row-Second Base");
    await user.click(within(row).getByRole("button", { name: "Open" }));
    await user.click(screen.getByRole("button", { name: "Save and continue" }));
    await waitFor(() => expect(api.save).toHaveBeenCalled());
    await waitFor(() => expect(api.open).toHaveBeenCalledWith("Second.furybuilder"));
  });

  it("shows the recovery banner and can discard to the saved snapshot", async () => {
    seed({ recovery: true });
    const user = userEvent.setup();
    render(<ProjectManager />);
    await user.click(screen.getByRole("button", { name: /Discard and reload saved/ }));
    await waitFor(() => expect(api.close).toHaveBeenCalledWith(false, expect.anything()));
    await waitFor(() => expect(api.open).toHaveBeenCalledWith(first.folder));
  });

  it("renames a project from the list", async () => {
    const user = userEvent.setup();
    render(<ProjectManager />);
    await user.click(await screen.findByRole("button", { name: "Rename Second Base" }));
    const input = screen.getByLabelText("New project name");
    await user.clear(input);
    await user.type(input, "Renamed Base");
    await user.click(screen.getByRole("button", { name: "Rename" }));
    await waitFor(() => expect(api.rename).toHaveBeenCalledWith(second.folder, "Renamed Base"));
  });

  it("lists checkpoints and restores one after confirmation", async () => {
    api.checkpoints.mockResolvedValue([
      {
        revision: 9,
        checksum: "b".repeat(64),
        bytes: 4096,
        savedAtMs: Date.now(),
        reason: "before experiment",
      },
    ]);
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
    const user = userEvent.setup();
    render(<ProjectManager />);
    await user.click(screen.getByRole("tab", { name: /Checkpoints/ }));
    expect(await screen.findByText("before experiment")).toBeInTheDocument();
    expect(screen.getByText(/rev 9/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Restore" }));
    await waitFor(() => expect(api.openCheckpoint).toHaveBeenCalledWith(9));
    confirm.mockRestore();
  });

  it("analyzes and imports a supported Java world folder", async () => {
    api.analyze.mockResolvedValue(worldAnalysis);
    const importedSummary: ProjectSummary = {
      ...first,
      folder: "Imported World.furybuilder",
      name: "Imported World",
      creationMode: "import",
    };
    api.importWorld.mockResolvedValue(opened(importedSummary));
    const user = userEvent.setup();
    render(<ProjectManager />);

    await user.click(screen.getByRole("tab", { name: /New project/ }));
    await user.click(screen.getByRole("button", { name: /Import World/ }));
    await user.type(screen.getByLabelText("Java world folder"), "C:\\saves\\World");
    await user.click(screen.getByRole("button", { name: "Analyze" }));
    await waitFor(() => expect(api.analyze).toHaveBeenCalledWith("C:\\saves\\World"));
    expect(screen.getAllByText(/DataVersion 4189/).length).toBeGreaterThan(0);
    expect(screen.getAllByText(/4 region\(s\) · 841 chunk\(s\)/).length).toBeGreaterThan(0);
    expect(screen.getByText(/classic_anvil/)).toBeInTheDocument();
    expect(
      screen.getByText("Read / edit / write / passthrough support matrix"),
    ).toBeInTheDocument();
    await user.click(screen.getByText("Read / edit / write / passthrough support matrix"));
    expect(await screen.findByText("Block states and palettes")).toBeInTheDocument();
    expect(screen.getByText("1.21.4 · supported")).toBeInTheDocument();

    await user.type(screen.getByLabelText("Project name"), "Imported World");
    await user.click(screen.getByRole("button", { name: "Import world" }));
    await waitFor(() =>
      expect(api.importWorld).toHaveBeenCalledWith("Imported World", "C:\\saves\\World", {
        selectedDimension: "minecraft:overworld",
        loadRadius: 1,
        workspace: defaultWorkspacePolicy(),
        scene: defaultScene(),
      }),
    );
    await waitFor(() => expect(useProjectStore.getState().managerOpen).toBe(false));
  });

  it("shows blockers and keeps import disabled for an unsupported world", async () => {
    api.analyze.mockResolvedValue({
      ...worldAnalysis,
      supported: false,
      dataVersion: 4325,
      blockers: [
        "DataVersion 4325 is not supported; this build edits 4189 (Minecraft Java 1.21.4)",
      ],
    });
    const user = userEvent.setup();
    render(<ProjectManager />);

    await user.click(screen.getByRole("tab", { name: /New project/ }));
    await user.click(screen.getByRole("button", { name: /Import World/ }));
    await user.type(screen.getByLabelText("Java world folder"), "C:\\saves\\Newer");
    await user.type(screen.getByLabelText("Project name"), "Newer World");
    await user.click(screen.getByRole("button", { name: "Analyze" }));
    expect(await screen.findByText(/DataVersion 4325 is not supported/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Import world" })).toBeDisabled();
    expect(api.importWorld).not.toHaveBeenCalled();
  });
});
