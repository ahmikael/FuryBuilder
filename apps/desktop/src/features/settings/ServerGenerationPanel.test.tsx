import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  GenerationReport,
  GenerationRunsReport,
  GenerationSnapshot,
  GenerationStatusReport,
  ServerDiscoveryReport,
} from "@/runtime/server-types";
import { ServerGenerationPanel } from "@/features/settings/ServerGenerationPanel";

const api = vi.hoisted(() => ({
  accept: vi.fn(),
  clearEula: vi.fn(),
  start: vi.fn(),
  status: vi.fn(),
  cancel: vi.fn(),
  runs: vi.fn(),
  clearRuns: vi.fn(),
}));

vi.mock("@/lib/server-api", () => ({
  acceptServerEula: api.accept,
  clearServerEula: api.clearEula,
  startServerGeneration: api.start,
  serverGenerationStatus: api.status,
  cancelServerGeneration: api.cancel,
  serverGenerationRuns: api.runs,
  clearServerGenerationRuns: api.clearRuns,
}));

const sha1 = "4707d00eb834b446575d89a61a11b5d548d8c001";

const discovery: ServerDiscoveryReport = {
  targetMinecraftVersion: "1.21.4",
  targetDataVersion: 4189,
  targetJavaMajor: 21,
  selected: {
    path: "C:\\app\\servers\\minecraft-1.21.4\\server.jar",
    source: "managed",
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
  },
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
  manualHint: "Choose the official server JAR.",
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
  javaPath: "C:\\java\\bin\\java.exe",
  javaVersion: "21.0.12",
  serverJar: discovery.selected?.path ?? "",
  serverSha1: sha1,
  eulaAcceptedAt: 1_763_500_000,
  bindAddress: "127.0.0.1:51234",
  processTreeGuarded: true,
  runDir: "C:\\app\\server-runs\\run-0",
  worldDir: "C:\\app\\server-runs\\run-0\\world",
  logPath: "C:\\app\\server-runs\\run-0\\logs\\furybuilder-console.log",
  startedAt: 1,
  finishedAt: 2,
  logTail: ['[12:00:00] [Server thread/INFO]: Done (1.0s)! For help, type "help"'],
};

const emptyStatus: GenerationStatusReport = { active: null, last: null };
const emptyRuns: GenerationRunsReport = {
  root: "C:\\app\\server-runs",
  runs: [],
  totalBytes: 0,
  problems: [],
};

describe("ServerGenerationPanel", () => {
  beforeEach(() => {
    for (const mock of Object.values(api)) mock.mockReset();
    api.status.mockResolvedValue(emptyStatus);
    api.runs.mockResolvedValue(emptyRuns);
    api.clearRuns.mockResolvedValue(emptyRuns);
    api.accept.mockResolvedValue(discovery.eula);
    api.clearEula.mockResolvedValue(undefined);
    api.cancel.mockResolvedValue(true);
    api.start.mockResolvedValue(running);
  });

  it("requires the EULA to be accepted explicitly for the selected JAR", async () => {
    const user = userEvent.setup();
    const onDiscoveryChanged = vi.fn();
    render(
      <ServerGenerationPanel
        discovery={{ ...discovery, eula: null, eulaMatchesSelected: false }}
        onDiscoveryChanged={onDiscoveryChanged}
      />,
    );

    const accept = await screen.findByRole("button", { name: "Accept EULA" });
    expect(accept).toBeDisabled();
    expect(api.accept).not.toHaveBeenCalled();

    await user.click(
      screen.getByRole("checkbox", {
        name: "I have read and accept the Minecraft EULA",
      }),
    );
    expect(accept).toBeEnabled();
    await user.click(accept);
    await waitFor(() => expect(api.accept).toHaveBeenCalledWith(true));
    expect(onDiscoveryChanged).toHaveBeenCalledTimes(1);
    expect(await screen.findByText(/EULA accepted for Minecraft 1\.21\.4/)).toBeInTheDocument();
  });

  it("starts an isolated generation with the validated form values", async () => {
    const user = userEvent.setup();
    api.status.mockResolvedValueOnce(emptyStatus).mockResolvedValue({
      active: running,
      last: null,
    });
    render(<ServerGenerationPanel discovery={discovery} onDiscoveryChanged={vi.fn()} />);

    const start = await screen.findByRole("button", {
      name: "Start isolated generation",
    });
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
    });
    expect(await screen.findByText(/chunks 4\/9/)).toBeInTheDocument();
    expect(screen.getByText("generating")).toBeInTheDocument();
  });

  it("requests cancellation of the running process tree", async () => {
    const user = userEvent.setup();
    api.status.mockResolvedValue({ active: running, last: null });
    render(<ServerGenerationPanel discovery={discovery} onDiscoveryChanged={vi.fn()} />);

    const cancel = await screen.findByRole("button", { name: "Cancel generation" });
    await waitFor(() => expect(cancel).toBeEnabled());
    await user.click(cancel);
    await waitFor(() => expect(api.cancel).toHaveBeenCalledTimes(1));
    expect(await screen.findByText(/Cancellation requested/)).toBeInTheDocument();
  });

  it("shows the last verified run and clears only its run directories", async () => {
    const user = userEvent.setup();
    api.status.mockResolvedValue({ active: null, last });
    api.runs.mockResolvedValue({
      ...emptyRuns,
      runs: [
        {
          runId: "run-0",
          path: "C:\\app\\server-runs\\run-0",
          status: "completed",
          createdAt: 1,
          finishedAt: 2,
          chunks: 9,
          expectedChunks: 9,
          regionFiles: 1,
          worldBytes: 32_768,
          worldComplete: true,
          stopReason: "completed",
          error: null,
        },
      ],
      totalBytes: 100_000,
    });
    render(<ServerGenerationPanel discovery={discovery} onDiscoveryChanged={vi.fn()} />);

    expect(await screen.findByText("Last isolated run")).toBeInTheDocument();
    expect(screen.getByText("job-object")).toBeInTheDocument();
    expect(screen.getByText(/127\.0\.0\.1:51234/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Clear generation runs" }));
    await waitFor(() => expect(api.clearRuns).toHaveBeenCalledTimes(1));
    expect(await screen.findByText(/generation run directories were removed/)).toBeInTheDocument();
  });
});
