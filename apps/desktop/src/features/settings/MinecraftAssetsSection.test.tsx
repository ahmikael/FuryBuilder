import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  AssetDiscoveryReport,
  AssetPathReport,
  ResourcePackReport,
  SavedAssetSource,
  VersionReport,
} from "@/assets/types";
import { MinecraftAssetsSection } from "@/features/settings/MinecraftAssetsSection";

const api = vi.hoisted(() => ({
  discover: vi.fn(),
  inspect: vi.fn(),
  saved: vi.fn(),
  save: vi.fn(),
  clear: vi.fn(),
  savePacks: vi.fn(),
  clearPacks: vi.fn(),
}));

const dialog = vi.hoisted(() => ({ open: vi.fn() }));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: dialog.open,
}));

vi.mock("@/lib/assets-api", () => ({
  discoverMinecraftAssets: api.discover,
  inspectAssetPath: api.inspect,
  savedMinecraftAssets: api.saved,
  saveMinecraftAssets: api.save,
  clearMinecraftAssets: api.clear,
  saveMinecraftPackStack: api.savePacks,
  clearMinecraftPackStack: api.clearPacks,
}));

const targetVersion: VersionReport = {
  id: "1.21.4",
  versionType: "release",
  releaseTime: "2024-12-03T10:12:57+00:00",
  javaMajor: 21,
  inheritsFrom: null,
  metadataPath: "C:\\mc\\versions\\1.21.4\\1.21.4.json",
  metadataPresent: true,
  jarPath: "C:\\mc\\versions\\1.21.4\\1.21.4.jar",
  jarPresent: true,
  jarBytes: 28335587,
  expectedSha1: "a7e5a6024bfd3cd614625aa05629adf760020304",
  actualSha1: "a7e5a6024bfd3cd614625aa05629adf760020304",
  checksumVerified: true,
  internalVersionId: "1.21.4",
  worldVersion: 4189,
  resourcePackFormat: 46,
  javaVersionFromJar: 21,
  assetIndex: "19",
  assetIndexPresent: true,
  assetIndexChecksumVerified: true,
  status: "compatible",
  notes: [],
};

const discovery: AssetDiscoveryReport = {
  targetMinecraftVersion: "1.21.4",
  targetDataVersion: 4189,
  targetResourcePackFormat: 46,
  defaultRootPresent: true,
  searchedRoots: ["C:\\mc"],
  installations: [
    {
      root: "C:\\mc",
      source: "default_root",
      versionsDirPresent: true,
      assetsDirPresent: true,
      assetIndexes: 5,
      versions: [targetVersion],
      compatibleVersions: ["1.21.4"],
      resourcePacks: [],
      warnings: [],
    },
  ],
  problems: [],
  manualHint: "Choose the folder that contains versions/.",
};

const noSource: SavedAssetSource = {
  source: null,
  report: null,
  loadWarning: null,
  packs: [],
};

describe("MinecraftAssetsSection", () => {
  beforeEach(() => {
    api.discover.mockReset();
    api.inspect.mockReset();
    api.saved.mockReset();
    api.save.mockReset();
    api.clear.mockReset();
    api.savePacks.mockReset();
    api.clearPacks.mockReset();
    dialog.open.mockReset();
    api.saved.mockResolvedValue(noSource);
    api.discover.mockResolvedValue(discovery);
  });

  it("shows a detected target installation with its verified client", async () => {
    render(<MinecraftAssetsSection />);

    expect(await screen.findByText("target found")).toBeInTheDocument();
    expect(screen.getAllByText("1.21.4").length).toBeGreaterThan(0);
    expect(screen.getByText("SHA-1 verified")).toBeInTheDocument();
    expect(screen.getByText(/1 version\(s\)/)).toBeInTheDocument();
    expect(screen.getByText(/No Minecraft asset source is selected/)).toBeInTheDocument();
  });

  it("re-runs discovery from the Detect again button", async () => {
    const user = userEvent.setup();
    render(<MinecraftAssetsSection />);
    await screen.findByText("target found");
    expect(api.discover).toHaveBeenCalledTimes(1);

    await user.click(screen.getByRole("button", { name: /Detect again/ }));
    await waitFor(() => expect(api.discover).toHaveBeenCalledTimes(2));
    expect(await screen.findByText(/Detected 1 launcher root/)).toBeInTheDocument();
  });

  it("saves a manually selected root after validating it", async () => {
    const user = userEvent.setup();
    const validReport: AssetPathReport = {
      path: "C:\\mc",
      kind: "minecraft_root",
      valid: true,
      targetMatch: true,
      installation: discovery.installations[0],
      version: targetVersion,
      resourcePack: null,
      errors: [],
      warnings: [],
    };
    dialog.open.mockResolvedValue("C:\\mc");
    api.inspect.mockResolvedValue(validReport);
    api.save.mockResolvedValue({
      source: { kind: "minecraft_root", path: "C:\\mc" },
      report: validReport,
      loadWarning: null,
    });
    render(<MinecraftAssetsSection />);
    await screen.findByText("target found");

    await user.click(screen.getByRole("button", { name: /\.minecraft/ }));
    await waitFor(() => expect(api.save).toHaveBeenCalledWith("minecraft_root", "C:\\mc"));
    expect(api.inspect).toHaveBeenCalledWith("C:\\mc");
    expect(await screen.findByText("Minecraft asset source saved.")).toBeInTheDocument();
  });

  it("explains an empty automatic detection without hiding manual fallback", async () => {
    api.discover.mockResolvedValue({
      ...discovery,
      defaultRootPresent: false,
      searchedRoots: [],
      installations: [],
      problems: [
        "no Minecraft Java launcher folder at C:\\Users\\Mike\\AppData\\Roaming\\.minecraft",
      ],
    });
    render(<MinecraftAssetsSection />);

    expect(
      await screen.findByText("Automatic detection found no Java Edition."),
    ).toBeInTheDocument();
    expect(screen.getByText(/no Minecraft Java launcher folder/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /client\.jar/ })).toBeInTheDocument();
  });

  it("warns when the saved source no longer validates", async () => {
    const invalidReport: AssetPathReport = {
      path: "C:\\gone",
      kind: "not_found",
      valid: false,
      targetMatch: false,
      installation: null,
      version: null,
      resourcePack: null,
      errors: ["the selected path does not exist"],
      warnings: [],
    };
    api.saved.mockResolvedValue({
      source: { kind: "minecraft_root", path: "C:\\gone" },
      report: invalidReport,
      loadWarning: null,
      packs: [],
    });
    render(<MinecraftAssetsSection />);

    expect(await screen.findByText(/no longer valid/)).toBeInTheDocument();
    expect(screen.getAllByText(/path does not exist/).length).toBeGreaterThan(0);
  });

  it("adds a validated pack at the highest priority", async () => {
    const user = userEvent.setup();
    const pack: ResourcePackReport = {
      path: "C:\\packs\\A.zip",
      name: "Pack A",
      kind: "zip",
      bytes: 1024,
      packFormat: 46,
      supportedFormats: null,
      description: null,
      compatible: true,
      hasBlockstates: true,
      hasTextures: true,
      entryCount: 3,
      status: "compatible",
      errors: [],
      warnings: [],
    };
    const validReport: AssetPathReport = {
      path: "C:\\packs\\A.zip",
      kind: "resource_pack",
      valid: true,
      targetMatch: true,
      installation: null,
      version: null,
      resourcePack: pack,
      errors: [],
      warnings: [],
    };
    dialog.open.mockResolvedValue("C:\\packs\\A.zip");
    api.inspect.mockResolvedValue(validReport);
    api.savePacks.mockResolvedValue({
      source: null,
      report: null,
      loadWarning: null,
      packs: [pack],
    });
    render(<MinecraftAssetsSection />);
    await screen.findByText("target found");

    await user.click(screen.getByRole("button", { name: /Add pack \.zip/ }));
    await waitFor(() => expect(api.savePacks).toHaveBeenCalledWith(["C:\\packs\\A.zip"]));
    expect(
      await screen.findByText("Resource pack added at the highest priority."),
    ).toBeInTheDocument();
    expect(screen.getByText("Pack A")).toBeInTheDocument();
    expect(screen.getByText(/format 46 · compatible/)).toBeInTheDocument();
  });

  it("reorders and removes packs through the saved stack API", async () => {
    const user = userEvent.setup();
    const pack = (name: string, path: string): ResourcePackReport => ({
      path,
      name,
      kind: "zip",
      bytes: 1024,
      packFormat: 46,
      supportedFormats: null,
      description: null,
      compatible: true,
      hasBlockstates: true,
      hasTextures: true,
      entryCount: 3,
      status: "compatible",
      errors: [],
      warnings: [],
    });
    const first = pack("Pack A", "C:\\packs\\A.zip");
    const second = pack("Pack B", "C:\\packs\\B.zip");
    api.saved.mockResolvedValue({
      source: { kind: "minecraft_root", path: "C:\\mc" },
      report: null,
      loadWarning: null,
      packs: [first, second],
    });
    api.savePacks.mockResolvedValue({
      source: null,
      report: null,
      loadWarning: null,
      packs: [second, first],
    });
    render(<MinecraftAssetsSection />);
    await screen.findByText("Pack A");

    await user.click(screen.getByRole("button", { name: "Move Pack A down" }));
    await waitFor(() =>
      expect(api.savePacks).toHaveBeenCalledWith(["C:\\packs\\B.zip", "C:\\packs\\A.zip"]),
    );

    api.savePacks.mockResolvedValue({
      source: null,
      report: null,
      loadWarning: null,
      packs: [second],
    });
    await user.click(screen.getByRole("button", { name: "Remove Pack A" }));
    await waitFor(() => expect(api.savePacks).toHaveBeenLastCalledWith(["C:\\packs\\B.zip"]));
  });
});
