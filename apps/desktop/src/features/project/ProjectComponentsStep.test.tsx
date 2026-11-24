import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CatalogSnapshot } from "@/assets/types";
import type {
  ComponentInventory,
  ComponentReport,
  ComponentStatus,
  ComponentTarget,
} from "@/lib/component-types";
import type { VersionProfile } from "@/project/version-types";
import type { ServerReleaseInfo } from "@/runtime/server-types";
import type { RuntimeReleaseInfo } from "@/runtime/types";
import {
  ProjectComponentsStep,
  type ComponentStepState,
} from "@/features/project/ProjectComponentsStep";

const api = vi.hoisted(() => ({
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

vi.mock("@/lib/components-api", () => ({
  componentStatus: api.status,
  componentInventory: api.inventory,
  componentJavaRelease: api.javaRelease,
  componentAssetsRelease: api.assetsRelease,
  componentServerRelease: api.serverRelease,
  installComponent: api.install,
  cancelComponentInstall: api.cancel,
  relinkComponent: api.relink,
  selectPlaceholderComponent: api.placeholder,
  listenComponentProgress: api.listenProgress,
  listenComponentChanged: api.listenChanged,
  listenComponentError: api.listenError,
}));

vi.mock("@/lib/assets-api", () => ({
  loadAssetCatalog: api.catalog,
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: api.openDialog,
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

const targetKey = `${profile.id}|empty`;

function componentStatus(
  kind: ComponentStatus["kind"],
  overrides: Partial<ComponentStatus> = {},
): ComponentStatus {
  return { kind, required: true, state: "missing", notes: [], ...overrides };
}

const missingJava = componentStatus("java", {
  notes: ["no Java runtime is selected for this project"],
});
const missingAssets = componentStatus("assets", {
  notes: ["no Minecraft client assets are selected"],
});
const missingServer = componentStatus("server", {
  notes: ["no server JAR is selected for Vanilla World generation"],
});
const notRequiredServer = componentStatus("server", { required: false, state: "not_required" });

const readyAssets = componentStatus("assets", {
  mode: "managed",
  state: "ready",
  id: "assets-managed-test-version",
  version: profile.minecraftVersion,
  path: "C:\\cache\\assets\\client.jar",
  hash: { algorithm: "sha1", value: "b".repeat(40) },
  bytes: 54_000_000,
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
const placeholderAssets = componentStatus("assets", {
  mode: "placeholder",
  state: "unverified",
  notes: ["placeholder textures are active; there is no client JAR catalog for this project"],
});

function report(components: ComponentStatus[], overrides: Partial<ComponentReport> = {}) {
  return {
    profileId: target.profileId,
    minecraftVersion: target.minecraftVersion,
    dataVersion: target.dataVersion,
    javaMajor: target.javaMajor,
    components,
    ready: components.filter((entry) => entry.required).every((entry) => entry.state === "ready"),
    warnings: [],
    ...overrides,
  } satisfies ComponentReport;
}

const editorReady = report([missingJava, readyAssets, notRequiredServer]);
const editorMissing = report([missingJava, missingAssets, notRequiredServer]);
const editorPlaceholder = report([missingJava, placeholderAssets, notRequiredServer]);
const vanillaMissing = report([missingJava, readyAssets, missingServer]);

const emptyInventory: ComponentInventory = {
  java: { root: "C:\\cache\\java", runtimes: [], totalBytes: 0, problems: [] },
  assets: { root: "C:\\cache\\assets", clients: [], totalBytes: 0, problems: [] },
  servers: { root: "C:\\cache\\servers", servers: [], totalBytes: 0, problems: [] },
};

const inventoryWithExisting: ComponentInventory = {
  ...emptyInventory,
  java: {
    ...emptyInventory.java,
    runtimes: [
      {
        id: "java-managed-21",
        major: 21,
        path: "C:\\cache\\java",
        javaPath: "C:\\cache\\java\\bin\\java.exe",
        version: "21.0.5",
        releaseName: "jdk-21.0.5+11",
        sha256: "a".repeat(64),
        sourceUrl: "https://example.test/java.zip",
        vendor: "Eclipse Adoptium",
        os: "windows",
        arch: "x64",
        installedAt: 1_700_000_000,
        bytes: 120_000_000,
      },
    ],
  },
  servers: {
    ...emptyInventory.servers,
    servers: [
      {
        version: profile.minecraftVersion,
        path: "C:\\cache\\servers\\test-version",
        jarPath: "C:\\cache\\servers\\test-version\\server.jar",
        sha1: "c".repeat(40),
        size: 48_000_000,
        dataVersion: profile.dataVersion,
        javaMajor: 21,
        releaseTime: "2025-01-01T00:00:00+00:00",
        sourceUrl: "https://example.test/server.jar",
        versionJsonSha1: "d".repeat(40),
        installedAt: 1_700_000_000,
      },
    ],
  },
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

const assetsRelease = {
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

const serverRelease: ServerReleaseInfo = {
  minecraftVersion: profile.minecraftVersion,
  versionType: "release",
  releaseTime: "2025-01-01T00:00:00+00:00",
  manifestUrl: "https://piston-meta.mojang.com/manifest.json",
  versionJsonSha1: "d".repeat(40),
  downloadUrl: "https://piston-data.mojang.com/server.jar",
  sha1: "c".repeat(40),
  size: 48_000_000,
  javaMajor: 21,
  expectedDataVersion: profile.dataVersion,
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
  blocks: [],
  items: [],
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

function renderStep(
  overrides: Partial<{
    mode: "empty" | "flat" | "import" | "vanilla";
    desktop: boolean;
    profile: VersionProfile | null;
    disabled: boolean;
  }> = {},
) {
  const onStateChange = vi.fn();
  const view = render(
    <ProjectComponentsStep
      targetKey={targetKey}
      mode="empty"
      profile={profile}
      desktop
      onStateChange={onStateChange}
      {...overrides}
    />,
  );
  return { onStateChange, view };
}

function lastState(onStateChange: ReturnType<typeof vi.fn>): ComponentStepState {
  return onStateChange.mock.calls.at(-1)?.[0] as ComponentStepState;
}

beforeEach(() => {
  api.status.mockReset().mockResolvedValue(editorReady);
  api.inventory.mockReset().mockResolvedValue(emptyInventory);
  api.javaRelease.mockReset().mockResolvedValue(javaRelease);
  api.assetsRelease.mockReset().mockResolvedValue(assetsRelease);
  api.serverRelease.mockReset().mockResolvedValue(serverRelease);
  api.install.mockReset().mockResolvedValue({
    kind: "assets",
    mode: "managed",
    id: "assets-managed-test-version",
    version: profile.minecraftVersion,
    path: "C:\\cache\\assets\\client.jar",
    hash: { algorithm: "sha1", value: "b".repeat(40) },
    bytes: 54_000_000,
  });
  api.cancel.mockReset().mockResolvedValue("requested");
  api.relink.mockReset().mockResolvedValue(readyJava);
  api.placeholder.mockReset().mockResolvedValue(placeholderAssets);
  api.listenProgress.mockReset().mockResolvedValue(() => undefined);
  api.listenChanged.mockReset().mockResolvedValue(() => undefined);
  api.listenError.mockReset().mockResolvedValue(() => undefined);
  api.catalog.mockReset().mockResolvedValue(catalog);
  api.openDialog.mockReset().mockResolvedValue(null);
});

describe("project components step", () => {
  it("reports a ready editor setup and shows catalog counts", async () => {
    const { onStateChange } = renderStep();

    await waitFor(() =>
      expect(lastState(onStateChange)).toMatchObject({
        targetKey,
        checking: false,
        ready: true,
        errors: [],
      }),
    );
    expect(await screen.findByText("1234 blocks · 567 items")).toBeInTheDocument();
    expect(screen.getByText("java-test-version-abc123")).toBeInTheDocument();
    expect(screen.getAllByText("Minecraft client assets").length).toBeGreaterThan(0);
    expect(screen.queryByText("Official server JAR")).not.toBeInTheDocument();
    expect(api.catalog).toHaveBeenCalledTimes(1);
  });

  it("blocks the step when client assets are missing", async () => {
    api.status.mockResolvedValue(editorMissing);
    const { onStateChange } = renderStep();

    await waitFor(() =>
      expect(lastState(onStateChange)).toMatchObject({ checking: false, ready: false }),
    );
    expect(lastState(onStateChange).errors.join(" ")).toContain("Minecraft client assets");
    expect(
      await screen.findByText(/Download or choose Minecraft client assets/),
    ).toBeInTheDocument();
    expect(screen.getByText("Components still need attention")).toBeInTheDocument();
  });

  it("downloads client assets only after the explicit action and rechecks them", async () => {
    api.status.mockResolvedValueOnce(editorMissing).mockResolvedValue(editorReady);
    const user = userEvent.setup();
    const { onStateChange } = renderStep();

    const download = await screen.findByRole("button", {
      name: "Download Minecraft client JAR",
    });
    expect(api.install).not.toHaveBeenCalled();
    await user.click(download);

    await waitFor(() =>
      expect(api.install).toHaveBeenCalledWith({
        kind: "assets",
        target,
        expectedVersion: profile.minecraftVersion,
        expectedHash: { algorithm: "sha1", value: assetsRelease.sha1 },
      }),
    );
    expect(
      await screen.findByText(/Minecraft client assets is installed and verified/),
    ).toBeInTheDocument();
    await waitFor(() => expect(api.status).toHaveBeenCalledTimes(2));
    await waitFor(() =>
      expect(lastState(onStateChange)).toMatchObject({ ready: true, errors: [] }),
    );
    expect(await screen.findByText("1234 blocks · 567 items")).toBeInTheDocument();
  });

  it("accepts an explicit placeholder choice with a warning and no catalog", async () => {
    api.status.mockResolvedValueOnce(editorMissing).mockResolvedValue(editorPlaceholder);
    const user = userEvent.setup();
    const { onStateChange } = renderStep();

    await user.click(await screen.findByRole("button", { name: "Placeholder" }));
    await user.click(screen.getByRole("button", { name: "Use placeholder textures" }));

    await waitFor(() => expect(api.placeholder).toHaveBeenCalledWith(target));
    await waitFor(() =>
      expect(lastState(onStateChange)).toMatchObject({ ready: true, errors: [] }),
    );
    expect(lastState(onStateChange).warnings.join(" ")).toContain("Placeholder textures");
    expect(api.catalog).not.toHaveBeenCalled();
    expect(screen.getByText("Placeholder active")).toBeInTheDocument();
  });

  it("lists managed Java runtimes and records the selected existing path", async () => {
    api.inventory.mockResolvedValue(inventoryWithExisting);
    const user = userEvent.setup();
    const { onStateChange } = renderStep();

    await user.click(await screen.findByRole("button", { name: "Use existing" }));
    expect(await screen.findByText("Java 21.0.5")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Use Java 21.0.5" }));

    await waitFor(() =>
      expect(api.relink).toHaveBeenCalledWith("java", "C:\\cache\\java\\bin\\java.exe", target),
    );
    await waitFor(() => expect(lastState(onStateChange).checking).toBe(false));
  });

  it("requires Java and the server for Vanilla World without requiring them for editing", async () => {
    api.status.mockResolvedValue(vanillaMissing);
    const vanilla = renderStep({ mode: "vanilla" });

    await waitFor(() =>
      expect(lastState(vanilla.onStateChange)).toMatchObject({ checking: false, ready: false }),
    );
    const errors = lastState(vanilla.onStateChange).errors.join(" ");
    expect(errors).toContain("Java runtime");
    expect(errors).toContain("server JAR");
    expect(screen.getByText("Official server JAR")).toBeInTheDocument();
    vanilla.view.unmount();

    api.status.mockResolvedValue(editorMissing);
    const editor = renderStep({ mode: "empty" });
    await waitFor(() =>
      expect(lastState(editor.onStateChange)).toMatchObject({ checking: false, ready: false }),
    );
    expect(lastState(editor.onStateChange).errors.join(" ")).not.toContain("Java runtime");
    expect(screen.queryByText("Official server JAR")).not.toBeInTheDocument();
  });

  it("shows an incompatible manual Java error with an actionable hint", async () => {
    api.openDialog.mockResolvedValue("C:\\Java8\\bin\\java.exe");
    api.relink.mockRejectedValue({
      code: "invalid_argument",
      message: "Java 8 is older than the required Java 21",
    });
    const user = userEvent.setup();
    renderStep();

    await user.click(await screen.findByRole("button", { name: "Manual path" }));
    await user.click(screen.getByRole("button", { name: "Choose Java executable" }));

    expect(await screen.findByText("Component action failed")).toBeInTheDocument();
    expect(screen.getByText("Java 8 is older than the required Java 21")).toBeInTheDocument();
    expect(screen.getByText(/Check coordinates, names, selected files/)).toBeInTheDocument();
  });

  it("shows a component check error and retries the status", async () => {
    api.status
      .mockRejectedValueOnce({ code: "offline", message: "network access is disabled" })
      .mockResolvedValue(editorReady);
    const user = userEvent.setup();
    const { onStateChange } = renderStep();

    expect(await screen.findByText("Component check unavailable")).toBeInTheDocument();
    expect(screen.getByText("network access is disabled")).toBeInTheDocument();
    await waitFor(() =>
      expect(lastState(onStateChange)).toMatchObject({ checking: false, ready: false }),
    );

    await user.click(screen.getByRole("button", { name: "Retry" }));
    await waitFor(() =>
      expect(lastState(onStateChange)).toMatchObject({ checking: false, ready: true }),
    );
    expect(await screen.findByText("1234 blocks · 567 items")).toBeInTheDocument();
  });

  it("keeps a manual source selection inside the card that owns it", async () => {
    api.openDialog.mockResolvedValue("C:\\minecraft");
    api.relink.mockResolvedValue(readyAssets);
    const user = userEvent.setup();
    renderStep();

    const assetsCard = (await screen.findByText("Minecraft client assets")).closest(
      "[data-slot='card']",
    );
    expect(assetsCard).not.toBeNull();
    await user.click(screen.getByRole("button", { name: "Manual" }));
    await user.click(screen.getByRole("button", { name: "Choose .minecraft folder" }));

    await waitFor(() => expect(api.relink).toHaveBeenCalledWith("assets", "C:\\minecraft", target));
    expect(within(assetsCard as HTMLElement).getByText("Managed")).toBeInTheDocument();
  });
});
