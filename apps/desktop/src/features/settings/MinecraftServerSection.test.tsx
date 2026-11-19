import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  ManagedServerCacheReport,
  ManagedServerJar,
  ServerDiscoveryReport,
  ServerJarReport,
  ServerReleaseInfo,
} from "@/runtime/server-types";
import { MinecraftServerSection } from "@/features/settings/MinecraftServerSection";

const api = vi.hoisted(() => ({
  discover: vi.fn(),
  saved: vi.fn(),
  select: vi.fn(),
  clear: vi.fn(),
  release: vi.fn(),
  install: vi.fn(),
  managed: vi.fn(),
  clearManaged: vi.fn(),
  listenDownload: vi.fn(),
  listenChanged: vi.fn(),
}));
const dialog = vi.hoisted(() => ({ open: vi.fn() }));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: dialog.open,
}));

vi.mock("@/lib/server-api", () => ({
  discoverMinecraftServers: api.discover,
  savedMinecraftServer: api.saved,
  selectMinecraftServerJar: api.select,
  clearMinecraftServerSelection: api.clear,
  latestMinecraftServerRelease: api.release,
  installMinecraftServer: api.install,
  managedMinecraftServers: api.managed,
  clearManagedMinecraftServers: api.clearManaged,
  listenServerDownload: api.listenDownload,
  listenServerChanged: api.listenChanged,
}));

const managed: ManagedServerJar = {
  version: "1.21.4",
  path: "C:\\app\\servers\\minecraft-1.21.4",
  jarPath: "C:\\app\\servers\\minecraft-1.21.4\\server.jar",
  sha1: "4707d00eb834b446575d89a61a11b5d548d8c001",
  size: 56_880_250,
  dataVersion: 4189,
  javaMajor: 21,
  releaseTime: "2024-12-03T10:12:57+00:00",
  sourceUrl:
    "https://piston-data.mojang.com/v1/objects/4707d00eb834b446575d89a61a11b5d548d8c001/server.jar",
  installedAt: 1,
};

const report: ServerJarReport = {
  path: managed.jarPath,
  source: "managed",
  managed: true,
  ready: true,
  version: "1.21.4",
  dataVersion: 4189,
  javaMajor: 21,
  protocolVersion: 769,
  resourcePackFormat: 46,
  sha1: managed.sha1,
  size: managed.size,
  matchesOfficial: true,
  warnings: [],
  error: null,
};

const discovery: ServerDiscoveryReport = {
  targetMinecraftVersion: "1.21.4",
  targetDataVersion: 4189,
  targetJavaMajor: 21,
  selected: report,
  eula: {
    acceptedAt: 1_763_500_000,
    minecraftVersion: "1.21.4",
    jarSha1: managed.sha1,
    eulaUrl: "https://aka.ms/MinecraftEULA",
  },
  eulaMatchesSelected: true,
  managedRoot: "C:\\app\\servers",
  managed: [managed],
  managedBytes: managed.size,
  problems: [],
  manualHint:
    "Choose the official Minecraft 1.21.4 server.jar, or download it explicitly from Mojang's version manifest.",
};

const release: ServerReleaseInfo = {
  minecraftVersion: "1.21.4",
  versionType: "release",
  releaseTime: "2024-12-03T10:12:57+00:00",
  manifestUrl: "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json",
  versionJsonSha1: "c88775961e316f2be09859c54e13017660ee7770",
  downloadUrl:
    "https://piston-data.mojang.com/v1/objects/4707d00eb834b446575d89a61a11b5d548d8c001/server.jar",
  sha1: managed.sha1,
  size: managed.size,
  javaMajor: 21,
  expectedDataVersion: 4189,
  source: "Mojang version_manifest_v2",
};

const emptyCache: ManagedServerCacheReport = {
  root: "C:\\app\\servers",
  servers: [],
  totalBytes: 0,
  problems: [],
};

const manualReport: ServerJarReport = {
  ...report,
  source: "manual",
  managed: false,
  matchesOfficial: null,
  warnings: ["manual JAR: exact version and DataVersion are verified from its version.json"],
};

describe("MinecraftServerSection", () => {
  beforeEach(() => {
    for (const mock of Object.values(api)) mock.mockReset();
    dialog.open.mockReset();
    api.discover.mockResolvedValue(discovery);
    api.select.mockResolvedValue(manualReport);
    api.clear.mockResolvedValue(undefined);
    api.release.mockResolvedValue(release);
    api.clearManaged.mockResolvedValue(emptyCache);
    api.listenDownload.mockResolvedValue(() => {});
    api.listenChanged.mockResolvedValue(() => {});
  });

  it("shows the verified selected server and target version", async () => {
    render(<MinecraftServerSection />);

    expect(await screen.findByText("Selected server JAR")).toBeInTheDocument();
    expect(screen.getByText(/MC 1\.21\.4/)).toBeInTheDocument();
    expect(screen.getAllByText(/DV 4189/).length).toBeGreaterThan(0);
    expect(screen.getByText("ready")).toBeInTheDocument();
    expect(screen.getAllByText(new RegExp(managed.sha1)).length).toBeGreaterThan(0);
    expect(screen.getByText(/1 cached/)).toBeInTheDocument();
  });

  it("validates a manually chosen server.jar", async () => {
    const user = userEvent.setup();
    api.discover.mockResolvedValue({ ...discovery, selected: null, managed: [], managedBytes: 0 });
    dialog.open.mockResolvedValue("C:\\servers\\server.jar");
    render(<MinecraftServerSection />);

    await user.click(await screen.findByRole("button", { name: "Choose server.jar" }));
    await waitFor(() => expect(api.select).toHaveBeenCalledWith("C:\\servers\\server.jar"));
    expect(await screen.findByText("Server JAR selected and verified.")).toBeInTheDocument();
  });

  it("requires explicit clicks to fetch metadata and install the official JAR", async () => {
    const user = userEvent.setup();
    api.install.mockResolvedValue({
      server: managed,
      report,
      selected: true,
    });
    render(<MinecraftServerSection />);
    await screen.findByText("Selected server JAR");

    expect(api.release).not.toHaveBeenCalled();
    expect(api.install).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Check official server JAR" }));
    await waitFor(() => expect(api.release).toHaveBeenCalledTimes(1));
    expect(await screen.findByText(/piston-data\.mojang\.com/)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Download and install server JAR" }));
    await waitFor(() => expect(api.install).toHaveBeenCalledWith("1.21.4"));
    expect(await screen.findByText(/installed, SHA-1 verified and selected/)).toBeInTheDocument();
  });

  it("reports a checksum failure without clearing the event or pretending success", async () => {
    const user = userEvent.setup();
    api.install.mockRejectedValue(
      new Error("server JAR failed an integrity check: SHA-1 mismatch"),
    );
    render(<MinecraftServerSection />);
    await screen.findByText("Selected server JAR");

    await user.click(screen.getByRole("button", { name: "Check official server JAR" }));
    await user.click(
      await screen.findByRole("button", { name: "Download and install server JAR" }),
    );
    expect(await screen.findByText(/integrity check: SHA-1 mismatch/)).toBeInTheDocument();
    expect(
      screen.getByText(/Empty Project and Import World do not need this JAR/),
    ).toBeInTheDocument();
  });

  it("clears only the managed server cache after an explicit action", async () => {
    const user = userEvent.setup();
    render(<MinecraftServerSection />);
    await screen.findByText("Selected server JAR");

    await user.click(screen.getByRole("button", { name: /Clear managed server JARs/ }));
    await waitFor(() => expect(api.clearManaged).toHaveBeenCalledTimes(1));
    expect(await screen.findByText(/Manual files were not touched/)).toBeInTheDocument();
  });
});
