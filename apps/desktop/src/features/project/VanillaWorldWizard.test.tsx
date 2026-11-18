import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  GenerationReport,
  GenerationSnapshot,
  ServerDiscoveryReport,
  VanillaSetupReport,
} from "@/runtime/server-types";
import type { JavaRuntimeReport, RuntimeDiscoveryReport } from "@/runtime/types";
import type * as RuntimeApi from "@/lib/runtime-api";
import { VanillaWorldWizard } from "@/features/project/VanillaWorldWizard";

const api = vi.hoisted(() => ({
  setup: vi.fn(),
  status: vi.fn(),
  discoverServers: vi.fn(),
  discoverJava: vi.fn(),
  selectJava: vi.fn(),
  selectJar: vi.fn(),
  inspect: vi.fn(),
  start: vi.fn(),
  cancel: vi.fn(),
  acceptEula: vi.fn(),
  install: vi.fn(),
  latest: vi.fn(),
}));

const dialog = vi.hoisted(() => ({ open: vi.fn() }));

vi.mock("@/lib/server-api", () => ({
  vanillaSetupCheck: api.setup,
  serverGenerationStatus: api.status,
  discoverMinecraftServers: api.discoverServers,
  inspectServerDatapacks: api.inspect,
  startServerGeneration: api.start,
  cancelServerGeneration: api.cancel,
  acceptServerEula: api.acceptEula,
  installMinecraftServer: api.install,
  latestMinecraftServerRelease: api.latest,
  selectMinecraftServerJar: api.selectJar,
}));

vi.mock("@/lib/runtime-api", async (importOriginal) => {
  const actual = await importOriginal<typeof RuntimeApi>();
  return {
    ...actual,
    discoverJavaRuntimes: api.discoverJava,
    selectJavaRuntime: api.selectJava,
  };
});

vi.mock("@tauri-apps/plugin-dialog", () => ({ open: dialog.open }));

const sha1 = "4707d00eb834b446575d89a61a11b5d548d8c001";

const java: JavaRuntimeReport = {
  path: "D:\\java\\bin\\java.exe",
  source: "manual",
  status: "compatible",
  version: "21.0.12",
  major: 21,
  vendor: "Eclipse Adoptium",
  arch: "x86_64",
  vm: "OpenJDK 64-Bit Server VM",
  managed: false,
  warnings: [],
  error: null,
};

const server = {
  path: "C:\\servers\\minecraft-1.21.4\\server.jar",
  source: "managed" as const,
  managed: true,
  ready: true,
  version: "1.21.4",
  dataVersion: 4189,
  javaMajor: 21,
  protocolVersion: 769,
  resourcePackFormat: 46,
  sha1,
  size: 56_880_250,
  matchesOfficial: true,
  warnings: [],
  error: null,
};

const readySetup: VanillaSetupReport = {
  targetMinecraftVersion: "1.21.4",
  targetDataVersion: 4189,
  targetJavaMajor: 21,
  serverDatapackFormat: 61,
  java,
  javaReady: true,
  server,
  serverReady: true,
  eulaAcceptedAt: 1_763_500_000,
  eulaMatchesSelected: true,
  blockers: [],
  ready: true,
};

const runtimeDiscovery: RuntimeDiscoveryReport = {
  targetJavaMajor: 21,
  targetMinecraftVersion: "1.21.4",
  candidates: [java],
  saved: java,
  managedRoot: "C:\\app\\runtimes",
  managed: [],
  managedBytes: 0,
  problems: [],
  manualHint: "",
};

const serverDiscovery: ServerDiscoveryReport = {
  targetMinecraftVersion: "1.21.4",
  targetDataVersion: 4189,
  targetJavaMajor: 21,
  selected: server,
  eula: {
    acceptedAt: 1_763_500_000,
    minecraftVersion: "1.21.4",
    jarSha1: sha1,
    eulaUrl: "https://aka.ms/MinecraftEULA",
  },
  eulaMatchesSelected: true,
  managedRoot: "C:\\app\\servers",
  managed: [],
  managedBytes: 0,
  problems: [],
  manualHint: "",
};

const running: GenerationSnapshot = {
  runId: "run-1",
  running: true,
  phase: "generating",
  elapsedMs: 12_000,
  expectedChunks: 9,
  generatedChunks: 4,
  regionFiles: 1,
  worldBytes: 8192,
  message: "Generating chunks; periodic saves keep progress observable",
  error: null,
};

const last: GenerationReport = {
  snapshot: {
    runId: "run-0",
    running: false,
    phase: "completed",
    elapsedMs: 60_000,
    expectedChunks: 9,
    generatedChunks: 9,
    regionFiles: 1,
    worldBytes: 32_768,
    message: "Isolated generation completed",
    error: null,
  },
  stopReason: "completed",
  worldComplete: true,
  exitCode: 0,
  serverPid: 1234,
  seed: "FuryBuilder",
  preset: "normal",
  centerX: 0,
  centerZ: 0,
  radiusChunks: 1,
  timeLimitSeconds: 120,
  memoryMb: 2048,
  viewDistance: 3,
  cpuThreads: 2,
  generateStructures: true,
  javaPath: java.path,
  javaVersion: "21.0.12",
  serverJar: server.path,
  serverSha1: sha1,
  eulaAcceptedAt: 1_763_500_000,
  consentAt: 1_763_500_001,
  datapacks: [],
  bindAddress: "127.0.0.1:51234",
  processTreeGuarded: true,
  runDir: "C:\\app\\server-runs\\run-0",
  worldDir: "C:\\app\\server-runs\\run-0\\world",
  logPath: "C:\\app\\server-runs\\run-0\\logs\\furybuilder-console.log",
  startedAt: 1,
  finishedAt: 2,
  logTail: [],
};

describe("VanillaWorldWizard", () => {
  beforeEach(() => {
    for (const mock of Object.values(api)) mock.mockReset();
    dialog.open.mockReset();
    api.setup.mockResolvedValue(readySetup);
    api.status.mockResolvedValue({ active: null, last: null });
    api.discoverServers.mockResolvedValue(serverDiscovery);
    api.discoverJava.mockResolvedValue(runtimeDiscovery);
    api.selectJava.mockResolvedValue(java);
    api.start.mockResolvedValue(running);
    api.cancel.mockResolvedValue(true);
    api.inspect.mockResolvedValue([]);
  });

  it("shows setup blockers and keeps generation disabled", async () => {
    api.setup.mockResolvedValue({
      ...readySetup,
      java: null,
      javaReady: false,
      server: null,
      serverReady: false,
      eulaAcceptedAt: null,
      eulaMatchesSelected: false,
      ready: false,
      blockers: [
        "Select a compatible Java 21 runtime (Settings → Java runtime or this wizard).",
        "Select or download the official Minecraft 1.21.4 server.jar.",
        "Read and accept the Minecraft EULA for the selected server.jar in this wizard.",
      ],
    });
    render(<VanillaWorldWizard />);

    expect(await screen.findByText("Setup is incomplete")).toBeInTheDocument();
    expect(screen.getByText(/Select a compatible Java 21 runtime/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Start Vanilla World generation" })).toBeDisabled();
    expect(api.start).not.toHaveBeenCalled();
  });

  it("requires the explicit run consent and sends the supported world settings", async () => {
    const user = userEvent.setup();
    render(<VanillaWorldWizard />);

    const start = await screen.findByRole("button", {
      name: "Start Vanilla World generation",
    });
    expect(await screen.findByText("Ready to generate")).toBeInTheDocument();
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Refresh Vanilla World setup" })).toBeEnabled(),
    );
    expect(start).toBeDisabled();

    await user.click(screen.getByRole("checkbox", { name: "Confirm local official server run" }));
    expect(start).toBeEnabled();
    await user.click(start);

    await waitFor(() => expect(api.start).toHaveBeenCalledTimes(1));
    expect(api.start).toHaveBeenCalledWith({
      seed: "FuryBuilder",
      preset: "normal",
      centerX: 0,
      centerZ: 0,
      radiusChunks: 1,
      timeLimitSeconds: 120,
      memoryMb: 2048,
      viewDistance: 3,
      cpuThreads: 2,
      generateStructures: true,
      consent: true,
      datapacks: [],
    });
    expect(await screen.findByText("generating")).toBeInTheDocument();
  });

  it("validates selected data packs and includes only valid paths in the run", async () => {
    const user = userEvent.setup();
    dialog.open.mockResolvedValue(["C:\\packs\\valid.zip", "C:\\packs\\broken.zip"]);
    api.inspect.mockResolvedValue([
      {
        path: "C:\\packs\\valid.zip",
        name: "valid.zip",
        sha256: "a".repeat(64),
        size: 2048,
        packFormat: 61,
        valid: true,
        error: null,
        warnings: [],
      },
      {
        path: "C:\\packs\\broken.zip",
        name: "broken.zip",
        sha256: null,
        size: null,
        packFormat: null,
        valid: false,
        error: "broken.zip has no root pack.mcmeta",
        warnings: [],
      },
    ]);
    render(<VanillaWorldWizard />);

    const add = await screen.findByRole("button", { name: "Add data packs" });
    await user.click(add);
    expect(await screen.findByText("valid.zip")).toBeInTheDocument();
    expect(screen.getByText(/has no root pack.mcmeta/)).toBeInTheDocument();
    expect(screen.getByText(/Remove invalid data packs/)).toBeInTheDocument();

    const start = screen.getByRole("button", { name: "Start Vanilla World generation" });
    expect(start).toBeDisabled();

    await user.click(screen.getByRole("checkbox", { name: "Confirm local official server run" }));
    expect(start).toBeDisabled();

    await user.click(screen.getByRole("button", { name: "Remove data pack broken.zip" }));
    expect(start).toBeEnabled();
    await user.click(start);
    await waitFor(() => expect(api.start).toHaveBeenCalledTimes(1));
    expect(api.start.mock.calls[0][0].datapacks).toEqual([{ path: "C:\\packs\\valid.zip" }]);
  });

  it("requests cancellation of a running official server", async () => {
    const user = userEvent.setup();
    api.status.mockResolvedValue({ active: running, last: null });
    render(<VanillaWorldWizard />);

    const cancel = await screen.findByRole("button", {
      name: "Cancel Vanilla World generation",
    });
    await waitFor(() => expect(cancel).toBeEnabled());
    await user.click(cancel);
    await waitFor(() => expect(api.cancel).toHaveBeenCalledTimes(1));
    expect(await screen.findByText(/Cancellation requested/)).toBeInTheDocument();
  });

  it("shows the completed world and states that project import is the next task", async () => {
    api.status.mockResolvedValue({ active: null, last });
    render(<VanillaWorldWizard />);

    expect(await screen.findByText("Last isolated generation")).toBeInTheDocument();
    expect(screen.getAllByText("completed").length).toBeGreaterThan(0);
    expect(screen.getByText(/M9\.7/)).toBeInTheDocument();
    expect(screen.getByText(/127\.0\.0\.1:51234/)).toBeInTheDocument();
  });
});
