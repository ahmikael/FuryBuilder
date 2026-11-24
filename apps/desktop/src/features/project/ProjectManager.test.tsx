import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type * as IpcModule from "@/lib/ipc";
import { ProjectManager } from "@/features/project/ProjectManager";
import type { ComponentStepState } from "@/features/project/ProjectComponentsStep";
import type { JavaWorldAnalysis, ProjectSummary } from "@/project/types";
import type { VersionCatalogReport } from "@/project/version-types";
import type { PrepareProgress } from "@/lib/project-api";
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
  importGenerated: vi.fn(),
  cancelPrepare: vi.fn(),
  listenPrepare: vi.fn(),
  defaults: vi.fn(),
  versions: vi.fn(),
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
  importGeneratedWorld: api.importGenerated,
  cancelProjectPrepare: api.cancelPrepare,
  listenProjectPrepare: api.listenPrepare,
  fetchProjectDefaults: api.defaults,
}));

vi.mock("@/lib/ipc", async (importOriginal) => {
  const actual = await importOriginal<typeof IpcModule>();
  return { ...actual, isTauriRuntime: () => true };
});

vi.mock("@/lib/version-api", () => ({
  fetchVersionCatalog: api.versions,
}));

vi.mock("@/features/project/VanillaWorldWizard", () => ({
  VanillaWorldWizard: ({
    onOpenWorld,
  }: {
    onOpenWorld: (runId: string, name: string) => Promise<boolean>;
  }) => (
    <button type="button" onClick={() => void onOpenWorld("completed-run", "Generated world")}>
      Open as project
    </button>
  ),
}));

vi.mock("@/features/project/ProjectComponentsStep", async () => {
  const React = await import("react");
  return {
    ProjectComponentsStep: ({
      targetKey,
      onStateChange,
    }: {
      targetKey: string;
      onStateChange: (state: ComponentStepState) => void;
    }) => {
      React.useEffect(() => {
        onStateChange({
          targetKey,
          checking: false,
          ready: true,
          errors: [],
          warnings: [],
          summary: {
            java: {
              kind: "java",
              mode: "managed",
              state: "ready",
              version: "21.0.5",
              bytes: 120_000_000,
              hash: "a".repeat(64),
              path: "C:\\cache\\java\\bin\\java.exe",
            },
            assets: {
              kind: "assets",
              mode: "managed",
              state: "ready",
              version: "1.21.4",
              bytes: 54_200_000,
              hash: "b".repeat(40),
              path: "C:\\cache\\assets\\1.21.4\\client.jar",
            },
            server: null,
            catalog: {
              id: "java-1.21.4-test",
              hash: "e".repeat(64),
              blocks: 1234,
              items: 567,
            },
            totalBytes: 174_200_000,
          },
        });
      }, [targetKey, onStateChange]);
      return React.createElement("p", null, "Components step ready");
    },
  };
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
  schemaVersion: 2,
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
  schemaVersion: 2,
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
  sourceBytes: 48_000_000,
  sourcePath: "C:\\saves\\World",
  sourceKind: "folder",
  archive: null,
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

const flatDefaults = {
  preset: {
    baseY: 62,
    layers: [
      { block: "minecraft:stone", thickness: 1 },
      { block: "minecraft:dirt", thickness: 1 },
      { block: "minecraft:grass_block", thickness: 1 },
    ],
    biome: "minecraft:plains",
  },
  limits: { maxLayers: 8, maxLayerThickness: 16, maxTotalThickness: 64, footprint: 33 },
  blockChoices: [
    "minecraft:stone",
    "minecraft:dirt",
    "minecraft:grass_block",
    "minecraft:sand",
    "minecraft:oak_planks",
  ],
  biomeChoices: [
    "minecraft:plains",
    "minecraft:forest",
    "minecraft:desert",
    "minecraft:snowy_plains",
  ],
};

const versionCatalog: VersionCatalogReport = {
  source: "cache",
  fetchedAtUnix: 1_700_000_000,
  latestRelease: "1.21.5",
  latestSnapshot: "26.1-snapshot-3",
  recommended: "1.21.4",
  versions: [
    {
      id: "1.21.5",
      versionType: "release",
      releaseTime: "2025-03-25T10:00:00+00:00",
      latestRelease: true,
      latestSnapshot: false,
      recommended: false,
      profile: null,
      unavailableReason: "This build has no verified profile for this release yet.",
    },
    {
      id: "26.1-snapshot-3",
      versionType: "snapshot",
      releaseTime: "2025-12-10T09:00:00+00:00",
      latestRelease: false,
      latestSnapshot: true,
      recommended: false,
      profile: null,
      unavailableReason:
        "This build has no verified snapshot profile; snapshots are read-only or convertible.",
    },
    {
      id: "1.21.4",
      versionType: "release",
      releaseTime: "2024-12-03T10:12:57+00:00",
      latestRelease: false,
      latestSnapshot: false,
      recommended: true,
      unavailableReason: null,
      profile: {
        id: "java-classic-1.21.4",
        minecraftVersion: "1.21.4",
        dataVersion: 4189,
        releaseType: "release",
        family: "modern",
        worldLayout: "classic-anvil",
        chunkFamily: "modern-paletted",
        biomeFormat: "palette-4x4x4",
        dimensionProfiles: [
          {
            id: "minecraft:overworld",
            kind: "overworld",
            minY: -64,
            height: 384,
            folder: "region",
          },
        ],
        java: { minimumMajor: 21, recommendedMajor: 21 },
        resourcePackFormat: 46,
        datapackFormat: 61,
        contentCatalog: {
          registryFormat: "named",
          blockstateFormat: "json",
          itemModelFormat: "item-definitions",
          creativeCategories: true,
          dataComponents: true,
        },
        capabilities: {
          supportLevel: "full",
          read: true,
          edit: true,
          write: true,
          export: true,
          generate: true,
          assets: true,
        },
        notes: ["Block-state and biome writes are verified for classic Anvil 1.21.4."],
      },
    },
  ],
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
    phase: "ready",
    managerTab: "projects",
    managerCreateMode: "empty",
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
  api.importGenerated.mockReset();
  api.cancelPrepare.mockReset().mockResolvedValue(true);
  api.listenPrepare.mockReset().mockResolvedValue(() => {});
  api.versions.mockReset().mockResolvedValue(versionCatalog);
  api.defaults.mockReset().mockResolvedValue({
    projectsRoot: "C:\\Users\\Test\\Documents\\FuryBuilder",
    folderSuffix: ".furybuilder",
    flat: flatDefaults,
  });
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
      expect(api.create).toHaveBeenCalledWith(
        expect.any(String),
        "New World",
        "flat",
        {
          selectedDimension: "minecraft:overworld",
          loadRadius: 1,
          workspace: defaultWorkspacePolicy(),
          scene: defaultScene(),
        },
        undefined,
      ),
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
        checkpointSchemaVersion: 2,
        profileId: "java-classic-1.21.4",
        revision: 9,
        checksum: "b".repeat(64),
        bytes: 4096,
        savedAtMs: Date.now(),
        reason: "before experiment",
        editorState: { selectedDimension: "minecraft:overworld", loadRadius: 1 },
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
      expect(api.importWorld).toHaveBeenCalledWith(
        expect.any(String),
        "Imported World",
        "C:\\saves\\World",
        "importAsIs",
        {
          selectedDimension: "minecraft:overworld",
          loadRadius: 1,
          workspace: defaultWorkspacePolicy(),
          scene: defaultScene(),
        },
        undefined,
      ),
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

describe("project wizard", () => {
  function openWizard(mode: "empty" | "flat" | "import" | "vanilla" = "empty"): void {
    useProjectStore.setState({
      managerOpen: true,
      managerTab: "create",
      managerCreateMode: mode,
      current: null,
      phase: "closed",
      failure: null,
      error: null,
      worldAnalysis: null,
      analysisLoading: false,
      prepareMessage: null,
    });
  }

  it("shows steps, progress and validates the basics step", async () => {
    const user = userEvent.setup();
    openWizard();
    render(<ProjectManager />);

    expect(screen.getByRole("region", { name: "New project" })).toBeInTheDocument();
    expect(screen.getAllByText("Step 1 of 5").length).toBeGreaterThan(0);
    expect(
      screen.getByRole("progressbar", { name: "Project creation progress" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Back" })).toBeDisabled();
    expect(screen.getByRole("button", { name: /Version/ })).toBeDisabled();
    expect(screen.getByRole("button", { name: /Source/ })).toBeDisabled();
    expect(screen.getByRole("button", { name: /Components/ })).toBeDisabled();

    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(screen.getByText("Enter a project name.")).toBeInTheDocument();

    await user.type(screen.getByLabelText("Project name"), "Wizard World");
    await user.click(screen.getByRole("button", { name: "Next" }));

    expect(screen.getByRole("heading", { name: "Version" })).toBeInTheDocument();
    expect(screen.getAllByText("Step 2 of 5").length).toBeGreaterThan(0);
    expect(screen.getByRole("button", { name: /Version/ })).toHaveAttribute("aria-current", "step");
    expect((await screen.findAllByText("Full support")).length).toBeGreaterThan(0);

    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(screen.getByRole("heading", { name: "Source" })).toBeInTheDocument();
    expect(screen.getAllByText("Step 3 of 5").length).toBeGreaterThan(0);
    expect(screen.getByRole("button", { name: /Source/ })).toHaveAttribute("aria-current", "step");

    await user.click(screen.getByRole("button", { name: /Basics/ }));
    expect(screen.getByRole("heading", { name: "Basics" })).toBeInTheDocument();
    expect(screen.getByDisplayValue("Wizard World")).toBeInTheDocument();
  });

  it("browses the catalog, filters snapshots and refuses unsupported versions", async () => {
    const user = userEvent.setup();
    openWizard();
    render(<ProjectManager />);

    await user.type(screen.getByLabelText("Project name"), "Version World");
    await user.click(screen.getByRole("button", { name: "Next" }));

    const search = await screen.findByLabelText("Find a version");
    expect(
      await screen.findByText("2 versions shown, grouped by release series."),
    ).toBeInTheDocument();
    const latest = screen.getByRole("button", { name: /1\.21\.5/ });
    expect(within(latest).getByText("Latest")).toBeInTheDocument();
    expect(within(latest).getByText("No profile")).toBeInTheDocument();
    const recommended = screen.getByRole("button", { name: /1\.21\.4/ });
    expect(within(recommended).getByText("Recommended")).toBeInTheDocument();
    expect(within(recommended).getByText("Full support")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /26\.1-snapshot-3/ })).not.toBeInTheDocument();

    await user.type(search, "1.21.5");
    expect(screen.getByText("1 version shown, grouped by release series.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /1\.21\.4/ })).not.toBeInTheDocument();
    await user.clear(search);

    await user.click(screen.getByRole("checkbox", { name: "Show snapshots" }));
    const snapshot = await screen.findByRole("button", { name: /26\.1-snapshot-3/ });
    await user.click(snapshot);
    expect(screen.getByRole("heading", { name: "26.1-snapshot-3" })).toBeInTheDocument();
    expect(screen.getByText("Not selectable in this build")).toBeInTheDocument();
    expect(screen.getByText(/no verified snapshot profile/)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(
      screen.getByText(
        "This build has no verified profile for Minecraft 26.1-snapshot-3. Choose a supported version.",
      ),
    ).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Version" })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /1\.21\.4/ }));
    expect(screen.getByRole("heading", { name: "1.21.4" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(screen.getByRole("heading", { name: "Source" })).toBeInTheDocument();
  });

  it("creates a flat project from the review step with a chosen folder", async () => {
    const user = userEvent.setup();
    openWizard();
    render(<ProjectManager />);

    await waitFor(() =>
      expect(screen.getByText("C:\\Users\\Test\\Documents\\FuryBuilder")).toBeInTheDocument(),
    );
    expect(screen.getByText("Empty Project requirements")).toBeInTheDocument();

    await user.type(screen.getByLabelText("Project name"), "Wizard Flat");
    await user.type(screen.getByLabelText("Project folder"), "Custom Flat");
    await user.click(screen.getByRole("button", { name: /Flat base/ }));
    expect(screen.getByText("Flat base requirements")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(screen.getByRole("heading", { name: "Version" })).toBeInTheDocument();
    expect((await screen.findAllByText("Full support")).length).toBeGreaterThan(0);
    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(await screen.findByRole("heading", { name: "Source" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(await screen.findByText("Components step ready")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Next" }));

    expect(screen.getByRole("heading", { name: "Review" })).toBeInTheDocument();
    expect(screen.getByText("Wizard Flat")).toBeInTheDocument();
    expect(screen.getByText("Flat base")).toBeInTheDocument();
    expect(screen.getByText("Custom Flat.furybuilder")).toBeInTheDocument();
    expect(screen.getByText("1.21.4")).toBeInTheDocument();
    expect(screen.getByText("3,267 blocks · about 216.0 KiB")).toBeInTheDocument();
    expect(screen.getByText("Under a second")).toBeInTheDocument();
    expect(screen.getByText("1234 blocks · 567 items")).toBeInTheDocument();
    expect(screen.getByText("21.0.5 · 114.4 MiB · aaaaaaaaaaaa…")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Create project" }));
    await waitFor(() =>
      expect(api.create).toHaveBeenCalledWith(
        expect.any(String),
        "Wizard Flat",
        "flat",
        {
          selectedDimension: "minecraft:overworld",
          loadRadius: 1,
          workspace: defaultWorkspacePolicy(),
          scene: defaultScene(),
        },
        "Custom Flat.furybuilder",
        {
          profileId: "java-classic-1.21.4",
          flat: flatDefaults.preset,
        },
      ),
    );
    await waitFor(() => expect(useProjectStore.getState().managerOpen).toBe(false));
  });

  it("edits flat layers, height and biome before creating the project", async () => {
    const user = userEvent.setup();
    openWizard("flat");
    render(<ProjectManager />);

    await user.type(screen.getByLabelText("Project name"), "Custom Layers");
    await user.click(screen.getByRole("button", { name: "Next" }));
    expect((await screen.findAllByText("Full support")).length).toBeGreaterThan(0);
    await user.click(screen.getByRole("button", { name: "Next" }));

    expect(await screen.findByRole("heading", { name: "Source" })).toBeInTheDocument();
    expect(screen.getByText(/33×33 platform/)).toBeInTheDocument();
    expect(screen.getByText(/Y -64\.\.=319/)).toBeInTheDocument();

    const baseY = screen.getByLabelText("Bottom Y");
    await user.clear(baseY);
    await user.type(baseY, "70");

    const firstThickness = screen.getByLabelText("Layer 1 thickness");
    await user.clear(firstThickness);
    await user.type(firstThickness, "2");

    await user.click(screen.getByRole("button", { name: "Remove layer 3" }));
    await user.click(screen.getByRole("button", { name: "Remove layer 2" }));
    await user.click(screen.getByRole("button", { name: "Add layer" }));

    await user.click(screen.getByRole("combobox", { name: "Layer 2 block" }));
    await user.click(await screen.findByRole("option", { name: "Sand" }));
    await user.click(screen.getByRole("combobox", { name: "Flat source biome" }));
    await user.click(await screen.findByRole("option", { name: "Desert" }));

    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(await screen.findByText("Components step ready")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(screen.getByRole("heading", { name: "Review" })).toBeInTheDocument();
    expect(screen.getByText("Stone ×2 → Sand ×1")).toBeInTheDocument();
    expect(screen.getByText("minecraft:desert")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Create project" }));
    await waitFor(() =>
      expect(api.create).toHaveBeenCalledWith(
        expect.any(String),
        "Custom Layers",
        "flat",
        {
          selectedDimension: "minecraft:overworld",
          loadRadius: 1,
          workspace: defaultWorkspacePolicy(),
          scene: defaultScene(),
        },
        undefined,
        {
          profileId: "java-classic-1.21.4",
          flat: {
            baseY: 70,
            layers: [
              { block: "minecraft:stone", thickness: 2 },
              { block: "minecraft:sand", thickness: 1 },
            ],
            biome: "minecraft:desert",
          },
        },
      ),
    );
    await waitFor(() => expect(useProjectStore.getState().managerOpen).toBe(false));
  });

  it("refuses a flat source outside the selected version bounds", async () => {
    const user = userEvent.setup();
    openWizard("flat");
    render(<ProjectManager />);

    await user.type(screen.getByLabelText("Project name"), "Out Of Bounds");
    await user.click(screen.getByRole("button", { name: "Next" }));
    expect((await screen.findAllByText("Full support")).length).toBeGreaterThan(0);
    await user.click(screen.getByRole("button", { name: "Next" }));

    const baseY = await screen.findByLabelText("Bottom Y");
    await user.clear(baseY);
    await user.type(baseY, "319");
    await user.click(screen.getByRole("button", { name: "Next" }));

    expect(screen.getByText("Keep the flat source inside overworld Y -64..=319.")).toBeVisible();
    expect(screen.getByRole("heading", { name: "Source" })).toBeInTheDocument();
    expect(api.create).not.toHaveBeenCalled();
  });

  it("refuses an invalid project folder before the next step", async () => {
    const user = userEvent.setup();
    openWizard();
    render(<ProjectManager />);

    await user.type(screen.getByLabelText("Project name"), "Bad Folder");
    await user.type(screen.getByLabelText("Project folder"), "nested/folder");
    await user.click(screen.getByRole("button", { name: "Next" }));

    expect(
      screen.getByText("Use a folder name without path separators or reserved characters."),
    ).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Basics" })).toBeInTheDocument();
    expect(api.create).not.toHaveBeenCalled();
  });

  it("blocks import until the world is analyzed and then imports it", async () => {
    api.analyze.mockResolvedValue({ ...worldAnalysis, sourcePath: "C:\\saves\\Wizard" });
    const importedSummary: ProjectSummary = {
      ...first,
      folder: "Imported Wizard.furybuilder",
      name: "Imported Wizard",
      creationMode: "import",
    };
    api.importWorld.mockResolvedValue(opened(importedSummary));
    const user = userEvent.setup();
    openWizard("import");
    render(<ProjectManager />);

    await user.type(screen.getByLabelText("Project name"), "Imported Wizard");
    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(screen.getByRole("heading", { name: "Source" })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(screen.getByText("Choose a Java world folder or .zip archive.")).toBeInTheDocument();
    expect(screen.getByText("Analyze the world before continuing.")).toBeInTheDocument();

    await user.type(screen.getByLabelText("Java world folder"), "C:\\saves\\Wizard");
    await user.click(screen.getByRole("button", { name: "Analyze" }));
    await waitFor(() => expect(api.analyze).toHaveBeenCalledWith("C:\\saves\\Wizard"));
    expect(await screen.findByText(/DataVersion 4189/)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(screen.getByRole("heading", { name: "Version" })).toBeInTheDocument();
    expect(await screen.findByText("Detected from the world")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(await screen.findByText("Components step ready")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(screen.getByRole("heading", { name: "Review" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Create project" }));

    await waitFor(() =>
      expect(api.importWorld).toHaveBeenCalledWith(
        expect.any(String),
        "Imported Wizard",
        "C:\\saves\\Wizard",
        "importAsIs",
        {
          selectedDimension: "minecraft:overworld",
          loadRadius: 1,
          workspace: defaultWorkspacePolicy(),
          scene: defaultScene(),
        },
        undefined,
      ),
    );
    await waitFor(() => expect(useProjectStore.getState().managerOpen).toBe(false));
  });

  it("analyzes a world archive and keeps conversion unavailable", async () => {
    const archiveAnalysis: JavaWorldAnalysis = {
      ...worldAnalysis,
      sourcePath: "C:\\downloads\\Wizard.zip",
      sourceKind: "archive",
      selectionKind: "world_archive",
      archive: { entries: 4, bytes: 4096 },
    };
    api.analyze.mockResolvedValue(archiveAnalysis);
    api.importWorld.mockResolvedValue(opened({ ...first, creationMode: "import" }));
    const user = userEvent.setup();
    openWizard("import");
    render(<ProjectManager />);

    await user.type(screen.getByLabelText("Project name"), "Archived Wizard");
    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(screen.getByRole("heading", { name: "Source" })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: ".zip archive" }));
    await user.type(screen.getByLabelText("World archive (.zip)"), "C:\\downloads\\Wizard.zip");
    await user.click(screen.getByRole("button", { name: "Analyze" }));
    await waitFor(() => expect(api.analyze).toHaveBeenCalledWith("C:\\downloads\\Wizard.zip"));
    expect(await screen.findByText(/Archive: 4 file\(s\)/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Convert/ })).toBeDisabled();

    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(screen.getByRole("heading", { name: "Version" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(await screen.findByText("Components step ready")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(screen.getByRole("heading", { name: "Review" })).toBeInTheDocument();
    expect(screen.getByText(/World archive · C:\\downloads\\Wizard.zip/)).toBeInTheDocument();
    expect(screen.getByText("Import as-is")).toBeInTheDocument();
    expect(screen.getByText(/45\.8 MiB source \+ about 19\.7 MiB overlay/)).toBeInTheDocument();
    expect(screen.getByText(/841 chunk\(s\) from 4 region file\(s\)/)).toBeInTheDocument();
    expect(screen.getByText(/3 block entities and 2 entities stay/)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Create project" }));
    await waitFor(() =>
      expect(api.importWorld).toHaveBeenCalledWith(
        expect.any(String),
        "Archived Wizard",
        "C:\\downloads\\Wizard.zip",
        "importAsIs",
        expect.any(Object),
        undefined,
      ),
    );
  });

  it("keeps the folder available but hides the name for vanilla generation", async () => {
    openWizard("vanilla");
    render(<ProjectManager />);

    expect(screen.getByRole("region", { name: "New project" })).toBeInTheDocument();
    expect(screen.getAllByText("Step 1 of 4").length).toBeGreaterThan(0);
    expect(screen.queryByLabelText("Project name")).not.toBeInTheDocument();
    expect(screen.getByLabelText("Project folder")).toBeInTheDocument();
    expect(screen.getByText("Vanilla World requirements")).toBeInTheDocument();
    expect(screen.getByText(/The seed names the generated project/)).toBeInTheDocument();
  });

  it("keeps the wizard open on failure and retries the same request", async () => {
    api.create
      .mockRejectedValueOnce({ code: "disk_space", message: "Not enough free space" })
      .mockResolvedValue(opened(first));
    const user = userEvent.setup();
    openWizard();
    render(<ProjectManager />);

    await user.type(screen.getByLabelText("Project name"), "Retry World");
    await user.click(screen.getByRole("button", { name: "Next" }));
    expect((await screen.findAllByText("Full support")).length).toBeGreaterThan(0);
    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(await screen.findByRole("heading", { name: "Source" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(await screen.findByText("Components step ready")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Next" }));
    await user.click(screen.getByRole("button", { name: "Create project" }));

    const region = screen.getByRole("region", { name: "New project" });
    expect(await within(region).findByText("Not enough free space")).toBeInTheDocument();
    await user.click(within(region).getByRole("button", { name: "Retry" }));

    await waitFor(() => expect(api.create).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(useProjectStore.getState().managerOpen).toBe(false));
  });

  it("retries importing the completed vanilla run instead of creating a fresh world", async () => {
    api.importGenerated
      .mockRejectedValueOnce({ code: "disk_space", message: "Not enough free space" })
      .mockResolvedValueOnce(opened({ ...first, creationMode: "vanilla" }));
    const user = userEvent.setup();
    openWizard("vanilla");
    render(<ProjectManager />);
    await user.type(screen.getByLabelText("Project folder"), "Generated Base");
    await user.click(screen.getByRole("button", { name: "Next" }));
    expect((await screen.findAllByText("Full support")).length).toBeGreaterThan(0);
    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(await screen.findByText("Components step ready")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Next" }));
    await user.click(screen.getByRole("button", { name: "Open as project" }));

    expect(await screen.findByText("Not enough free space")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Retry" }));
    await waitFor(() => expect(useProjectStore.getState().managerOpen).toBe(false));
    expect(api.create).not.toHaveBeenCalled();
    expect(api.importGenerated).toHaveBeenCalledTimes(2);
    expect(api.importGenerated.mock.calls[1]?.slice(1)).toEqual(
      api.importGenerated.mock.calls[0]?.slice(1),
    );
    expect(api.importGenerated.mock.calls[1]?.[0]).not.toBe(api.importGenerated.mock.calls[0]?.[0]);
    expect(api.importGenerated.mock.calls[1]?.[2]).toBe("completed-run");
  });

  it("shows preparation stages and cancels without losing downloaded components", async () => {
    let emit: ((event: PrepareProgress) => void) | null = null;
    api.listenPrepare.mockImplementation(async (handler: (event: PrepareProgress) => void) => {
      emit = handler;
      return () => {};
    });
    let rejectCreate!: (reason?: unknown) => void;
    api.create.mockReturnValue(
      new Promise((_, reject) => {
        rejectCreate = reject;
      }),
    );
    const user = userEvent.setup();
    openWizard();
    render(<ProjectManager />);

    await waitFor(() => expect(emit).not.toBeNull());
    await user.type(screen.getByLabelText("Project name"), "Cancel World");
    await user.click(screen.getByRole("button", { name: "Next" }));
    expect((await screen.findAllByText("Full support")).length).toBeGreaterThan(0);
    await user.click(screen.getByRole("button", { name: "Next" }));
    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(await screen.findByText("Components step ready")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Next" }));
    await user.click(screen.getByRole("button", { name: "Create project" }));

    expect(await screen.findByText("Preparing the project")).toBeInTheDocument();
    expect(screen.getByText("Prepared components")).toBeInTheDocument();
    expect(screen.getByText("1234 blocks · 567 items")).toBeInTheDocument();
    expect(screen.getByText("Project stages")).toBeInTheDocument();
    const jobId = useProjectStore.getState().prepareJobId;
    expect(jobId).toBeTruthy();

    act(() =>
      emit?.({
        jobId: jobId!,
        stage: "metadata",
        state: "running",
        message: "Writing project metadata",
        completed: 0,
        total: 0,
      }),
    );
    expect(await screen.findByText("Writing project metadata")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Cancel preparation" }));
    expect(api.cancelPrepare).toHaveBeenCalledWith(jobId);
    expect(screen.getByText("Cancelling…")).toBeInTheDocument();

    act(() => rejectCreate({ code: "cancelled", message: "cancelled by the user" }));
    expect(await screen.findByRole("heading", { name: "Review" })).toBeInTheDocument();
    expect(screen.getByText("1234 blocks · 567 items")).toBeInTheDocument();
    expect(useProjectStore.getState().prepareJobId).toBeNull();
  });

  it("closes the full-screen wizard without creating anything", async () => {
    const user = userEvent.setup();
    openWizard();
    render(<ProjectManager />);

    await user.click(screen.getByRole("button", { name: "Cancel" }));

    expect(useProjectStore.getState().managerOpen).toBe(false);
    expect(screen.queryByRole("region", { name: "New project" })).not.toBeInTheDocument();
    expect(api.create).not.toHaveBeenCalled();
  });
});
