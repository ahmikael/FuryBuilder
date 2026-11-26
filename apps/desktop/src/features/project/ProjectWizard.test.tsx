import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CatalogSnapshot } from "@/assets/types";
import type {
  ClientJarReleaseInfo,
  ComponentReport,
  ComponentStatus,
  ComponentTarget,
} from "@/lib/component-types";
import type * as IpcModule from "@/lib/ipc";
import type { VersionProfile } from "@/project/version-types";
import type { RuntimeReleaseInfo } from "@/runtime/types";
import { ProjectWizard } from "@/features/project/ProjectWizard";
import { useProjectStore } from "@/state/project-store";

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

const components = vi.hoisted(() => ({
  status: vi.fn(),
  inventory: vi.fn(),
  javaRelease: vi.fn(),
  assetsRelease: vi.fn(),
  serverRelease: vi.fn(),
  install: vi.fn(),
  cancel: vi.fn(),
  relink: vi.fn(),
  placeholder: vi.fn(),
  listenProgress: vi.fn(),
  listenChanged: vi.fn(),
  listenError: vi.fn(),
  catalog: vi.fn(),
  openDialog: vi.fn(),
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

vi.mock("@/lib/version-api", () => ({
  fetchVersionCatalog: api.versions,
}));

vi.mock("@/lib/components-api", () => ({
  componentStatus: components.status,
  componentInventory: components.inventory,
  componentJavaRelease: components.javaRelease,
  componentAssetsRelease: components.assetsRelease,
  componentServerRelease: components.serverRelease,
  installComponent: components.install,
  cancelComponentInstall: components.cancel,
  relinkComponent: components.relink,
  selectPlaceholderComponent: components.placeholder,
  listenComponentProgress: components.listenProgress,
  listenComponentChanged: components.listenChanged,
  listenComponentError: components.listenError,
}));

vi.mock("@/lib/assets-api", () => ({
  loadAssetCatalog: components.catalog,
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: components.openDialog,
}));

vi.mock("@/lib/ipc", async (importOriginal) => {
  const actual = await importOriginal<typeof IpcModule>();
  return { ...actual, isTauriRuntime: () => true };
});

vi.mock("@/features/project/VanillaWorldWizard", () => ({
  VanillaWorldWizard: () => null,
}));

const profile: VersionProfile = {
  id: "java-classic-test",
  minecraftVersion: "test-version",
  dataVersion: 12345,
  releaseType: "release",
  family: "modern",
  worldLayout: "classic-anvil",
  chunkFamily: "modern-paletted",
  biomeFormat: "palette-4x4x4",
  dimensionProfiles: [
    { id: "minecraft:overworld", kind: "overworld", minY: -64, height: 384, folder: "region" },
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
  notes: [],
};

const target: ComponentTarget = {
  profileId: profile.id,
  minecraftVersion: profile.minecraftVersion,
  dataVersion: profile.dataVersion,
  javaMajor: 21,
};

function componentStatus(
  kind: ComponentStatus["kind"],
  overrides: Partial<ComponentStatus> = {},
): ComponentStatus {
  return { kind, required: true, state: "missing", notes: [], ...overrides };
}

const missingJava = componentStatus("java");
const missingAssets = componentStatus("assets", {
  notes: ["no Minecraft client assets are selected"],
});
const readyJava = componentStatus("java", {
  mode: "managed",
  state: "ready",
  id: "java-managed-21",
  version: "21.0.5",
  path: "C:\\cache\\java\\bin\\java.exe",
  hash: { algorithm: "sha256", value: "a".repeat(64) },
  bytes: 120_000_000,
});
const readyAssets = componentStatus("assets", {
  mode: "managed",
  state: "ready",
  id: "assets-managed-test-version",
  version: profile.minecraftVersion,
  path: "C:\\cache\\assets\\client.jar",
  hash: { algorithm: "sha1", value: "b".repeat(40) },
  bytes: 54_000_000,
});
const manualAssets = componentStatus("assets", {
  ...readyAssets,
  mode: "manual",
  path: "C:\\minecraft\\versions\\test-version\\test-version.jar",
});
const placeholderAssets = componentStatus("assets", {
  mode: "placeholder",
  state: "unverified",
  notes: ["placeholder textures are active; there is no client JAR catalog for this project"],
});
const notRequiredServer = componentStatus("server", { required: false, state: "not_required" });

function report(entries: ComponentStatus[]): ComponentReport {
  return {
    profileId: target.profileId,
    minecraftVersion: target.minecraftVersion,
    dataVersion: target.dataVersion,
    javaMajor: target.javaMajor,
    components: entries,
    ready: entries.filter((entry) => entry.required).every((entry) => entry.state === "ready"),
    warnings: [],
  };
}

const missingReport = report([missingJava, missingAssets, notRequiredServer]);
const readyReport = report([readyJava, readyAssets, notRequiredServer]);
const manualReport = report([readyJava, manualAssets, notRequiredServer]);
const placeholderReport = report([missingJava, placeholderAssets, notRequiredServer]);

const emptyInventory = {
  java: { root: "C:\\cache\\java", runtimes: [], totalBytes: 0, problems: [] },
  assets: { root: "C:\\cache\\assets", clients: [], totalBytes: 0, problems: [] },
  servers: { root: "C:\\cache\\servers", servers: [], totalBytes: 0, problems: [] },
};

const javaRelease: RuntimeReleaseInfo = {
  major: 21,
  version: "21.0.5",
  releaseName: "jdk-21.0.5+11-jre",
  downloadUrl:
    "https://api.adoptium.net/v3/binary/latest/21/ga/windows/x64/jre/hotspot/normal/eclipse",
  sha256: "a".repeat(64),
  size: 120_000_000,
  vendor: "Eclipse Adoptium",
  os: "windows",
  arch: "x64",
  imageType: "jre",
  source: "adoptium",
};

const assetsRelease: ClientJarReleaseInfo = {
  minecraftVersion: profile.minecraftVersion,
  versionType: "release",
  releaseTime: "2025-01-01T00:00:00+00:00",
  versionJsonUrl: "https://piston-meta.mojang.com/version.json",
  versionJsonSha1: "d".repeat(40),
  downloadUrl: "https://piston-data.mojang.com/client.jar",
  sha1: "b".repeat(40),
  size: 54_000_000,
  source: "mojang",
};

const catalog: CatalogSnapshot = {
  schemaVersion: 1,
  catalogId: "java-test-version-abc123",
  catalogHash: "e".repeat(64),
  minecraftVersion: profile.minecraftVersion,
  dataVersion: profile.dataVersion,
  clientJar: "C:\\cache\\assets\\client.jar",
  clientJarSha1: "b".repeat(40),
  resourcePackFormat: 46,
  datapackFormat: 61,
  entries: [],
  atlases: [],
  counts: {
    blocks: 1234,
    items: 567,
    blocksWithItems: 500,
    itemOnly: 67,
    blockstateFiles: 1200,
    blockModels: 1300,
    itemModels: 500,
    itemDefinitions: 100,
    textures: 2000,
    languages: 1,
    langEntries: 3000,
    blockTags: 10,
    itemTags: 5,
    atlases: 2,
    atlasSources: 20,
  },
  warnings: [],
  cacheHit: true,
};

const versionCatalog = {
  source: "cache" as const,
  fetchedAtUnix: 1_700_000_000,
  latestRelease: profile.minecraftVersion,
  latestSnapshot: null,
  recommended: profile.minecraftVersion,
  versions: [
    {
      id: profile.minecraftVersion,
      versionType: "release" as const,
      releaseTime: "2025-01-01T00:00:00+00:00",
      latestRelease: true,
      latestSnapshot: false,
      recommended: true,
      unavailableReason: null,
      profile,
    },
  ],
};

async function openComponents(user: ReturnType<typeof userEvent.setup>): Promise<void> {
  render(<ProjectWizard />);

  await user.type(await screen.findByLabelText("Project name"), "Component World");
  await user.click(screen.getByRole("button", { name: "Next" }));
  expect((await screen.findAllByText("Full support")).length).toBeGreaterThan(0);
  await user.click(screen.getByRole("button", { name: "Next" }));
  expect(await screen.findByRole("heading", { name: "Source" })).toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Next" }));
  expect(await screen.findByRole("heading", { name: "Components" })).toBeInTheDocument();
}

beforeEach(() => {
  useProjectStore.getState().reset();
  useProjectStore.setState({
    managerOpen: true,
    managerTab: "create",
    managerCreateMode: "empty",
    current: null,
    phase: "closed",
    failure: null,
    error: null,
    prepareMessage: null,
    prepareProgress: [],
    prepareCancelling: false,
    worldAnalysis: null,
    analysisLoading: false,
  });

  api.defaults.mockReset().mockResolvedValue({
    projectsRoot: "C:\\Users\\Test\\Documents\\FuryBuilder",
    folderSuffix: ".furybuilder",
    flat: null,
  });
  api.versions.mockReset().mockResolvedValue(versionCatalog);
  api.create.mockReset();
  api.importWorld.mockReset();
  api.importGenerated.mockReset();
  api.listenPrepare.mockReset().mockResolvedValue(() => {});

  components.status.mockReset().mockResolvedValue(readyReport);
  components.inventory.mockReset().mockResolvedValue(emptyInventory);
  components.javaRelease.mockReset().mockResolvedValue(javaRelease);
  components.assetsRelease.mockReset().mockResolvedValue(assetsRelease);
  components.serverRelease.mockReset().mockResolvedValue(null);
  components.install.mockReset().mockResolvedValue(readyAssets);
  components.cancel.mockReset().mockResolvedValue("requested");
  components.relink.mockReset().mockResolvedValue(manualAssets);
  components.placeholder.mockReset().mockResolvedValue(placeholderAssets);
  components.listenProgress.mockReset().mockResolvedValue(() => undefined);
  components.listenChanged.mockReset().mockResolvedValue(() => undefined);
  components.listenError.mockReset().mockResolvedValue(() => undefined);
  components.catalog.mockReset().mockResolvedValue(catalog);
  components.openDialog.mockReset().mockResolvedValue(null);
});

describe("project wizard components integration", () => {
  it("downloads client assets only on request and advances to review", async () => {
    components.status.mockResolvedValueOnce(missingReport).mockResolvedValue(readyReport);
    const user = userEvent.setup();
    await openComponents(user);

    expect(screen.getByText("Components still need attention")).toBeInTheDocument();
    await user.click(await screen.findByRole("button", { name: "Download Minecraft client JAR" }));

    await waitFor(() =>
      expect(components.install).toHaveBeenCalledWith({
        kind: "assets",
        target,
        expectedVersion: profile.minecraftVersion,
        expectedHash: { algorithm: "sha1", value: assetsRelease.sha1 },
      }),
    );
    expect(
      await screen.findByText(/Minecraft client assets is installed and verified/),
    ).toBeInTheDocument();
    expect(await screen.findByText("1234 blocks · 567 items")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(await screen.findByRole("heading", { name: "Review" })).toBeInTheDocument();
    expect(screen.getByText("1234 blocks · 567 items")).toBeInTheDocument();
  });

  it("records a manually selected .minecraft folder and advances to review", async () => {
    components.status.mockResolvedValueOnce(missingReport).mockResolvedValue(manualReport);
    components.openDialog.mockResolvedValue("C:\\minecraft");
    const user = userEvent.setup();
    await openComponents(user);

    await user.click(await screen.findByRole("button", { name: "Manual" }));
    await user.click(screen.getByRole("button", { name: "Choose .minecraft folder" }));

    await waitFor(() =>
      expect(components.relink).toHaveBeenCalledWith("assets", "C:\\minecraft", target),
    );
    expect(
      await screen.findByText(/Minecraft client assets selected and verified/),
    ).toBeInTheDocument();
    expect(await screen.findByText("1234 blocks · 567 items")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(await screen.findByRole("heading", { name: "Review" })).toBeInTheDocument();
    expect(screen.getByText("Manual")).toBeInTheDocument();
  });

  it("accepts placeholder textures explicitly and repeats the limitation in review", async () => {
    components.status.mockResolvedValueOnce(missingReport).mockResolvedValue(placeholderReport);
    const user = userEvent.setup();
    await openComponents(user);

    await user.click(await screen.findByRole("button", { name: "Placeholder" }));
    await user.click(screen.getByRole("button", { name: "Use placeholder textures" }));

    await waitFor(() => expect(components.placeholder).toHaveBeenCalledWith(target));
    expect(await screen.findByText("Placeholder active")).toBeInTheDocument();
    expect(
      screen.getByText(/Placeholder textures are active; there is no verified block\/item catalog/),
    ).toBeInTheDocument();
    expect(components.catalog).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(await screen.findByRole("heading", { name: "Review" })).toBeInTheDocument();
    expect(screen.getByText("No client JAR or catalog")).toBeInTheDocument();
    expect(
      screen.getByText(/Placeholder textures are active; there is no verified block\/item catalog/),
    ).toBeInTheDocument();
  });

  it("keeps the components step on a check error and retries the status", async () => {
    components.status
      .mockRejectedValueOnce({ code: "offline", message: "network access is disabled" })
      .mockResolvedValue(readyReport);
    const user = userEvent.setup();
    await openComponents(user);

    expect(await screen.findByText("Component check unavailable")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(screen.getByRole("heading", { name: "Components" })).toBeInTheDocument();
    expect(
      screen.getByText(/Retry the component check: network access is disabled/),
    ).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Retry" }));
    expect(await screen.findByText("Required components are ready")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(await screen.findByRole("heading", { name: "Review" })).toBeInTheDocument();
  });

  it("retries the version catalog before allowing the next step", async () => {
    api.versions
      .mockRejectedValueOnce({ code: "offline", message: "network access is disabled" })
      .mockResolvedValue(versionCatalog);
    const user = userEvent.setup();
    render(<ProjectWizard />);

    await user.type(await screen.findByLabelText("Project name"), "Version Retry");
    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(await screen.findByText("Version catalog unavailable")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(screen.getByRole("heading", { name: "Version" })).toBeInTheDocument();
    expect(screen.getByText(/Retry loading the version catalog/)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Retry" }));
    expect((await screen.findAllByText("Full support")).length).toBeGreaterThan(0);

    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(await screen.findByRole("heading", { name: "Source" })).toBeInTheDocument();
  });
});
