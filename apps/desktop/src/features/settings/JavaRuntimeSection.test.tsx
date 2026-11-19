import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  JavaRuntimeReport,
  ManagedCacheReport,
  RuntimeDiscoveryReport,
  RuntimeReleaseInfo,
} from "@/runtime/types";
import type * as RuntimeApi from "@/lib/runtime-api";
import { JavaRuntimeSection } from "@/features/settings/JavaRuntimeSection";

const api = vi.hoisted(() => ({
  discover: vi.fn(),
  saved: vi.fn(),
  select: vi.fn(),
  clear: vi.fn(),
  latest: vi.fn(),
  install: vi.fn(),
  managed: vi.fn(),
  clearManaged: vi.fn(),
  listenDownload: vi.fn(),
  listenChanged: vi.fn(),
}));

vi.mock("@/lib/runtime-api", async () => {
  const actual = await vi.importActual<typeof RuntimeApi>("@/lib/runtime-api");
  return {
    ...actual,
    discoverJavaRuntimes: api.discover,
    savedJavaRuntime: api.saved,
    selectJavaRuntime: api.select,
    clearJavaRuntime: api.clear,
    latestTemurinRuntime: api.latest,
    installTemurinRuntime: api.install,
    managedJavaRuntimes: api.managed,
    clearManagedJavaRuntimes: api.clearManaged,
    listenRuntimeDownload: api.listenDownload,
    listenRuntimeChanged: api.listenChanged,
  };
});

const compatible: JavaRuntimeReport = {
  path: "\\\\?\\C:\\Java\\jdk-21\\bin\\java.exe",
  source: "common",
  status: "compatible",
  version: "21.0.13",
  major: 21,
  vendor: "Eclipse Adoptium",
  arch: "amd64",
  vm: "OpenJDK 64-Bit Server VM",
  managed: false,
  warnings: [],
  error: null,
};

const outdated: JavaRuntimeReport = {
  ...compatible,
  path: "\\\\?\\C:\\Java\\jdk-17\\bin\\java.exe",
  status: "outdated",
  version: "17.0.12",
  major: 17,
  warnings: ["Java 17 is older than the required Java 21"],
};

const release: RuntimeReleaseInfo = {
  version: "21.0.13+9.0.LTS",
  releaseName: "jdk-21.0.13+9",
  downloadUrl:
    "https://github.com/adoptium/temurin21-binaries/releases/download/x/OpenJDK21U-jre_x64_windows_hotspot_21.0.13_9.zip",
  sha256: "2332c0b4cff6bed722da3567c91d5e47568bcba863fa4945d0a8d6fc76158628",
  size: 49_106_397,
  vendor: "Eclipse Adoptium",
  os: "windows",
  arch: "x64",
  imageType: "jre",
  source: "Adoptium API v3",
};

const discovery: RuntimeDiscoveryReport = {
  targetJavaMajor: 21,
  targetMinecraftVersion: "1.21.4",
  candidates: [compatible, outdated],
  saved: compatible,
  managedRoot: "C:\\app\\runtimes",
  managed: [],
  managedBytes: 0,
  problems: [],
  manualHint: "Choose the java.exe inside a Java 21 installation.",
};

const emptyCache: ManagedCacheReport = {
  root: "C:\\app\\runtimes",
  runtimes: [],
  totalBytes: 0,
  problems: [],
};

describe("JavaRuntimeSection", () => {
  beforeEach(() => {
    api.discover.mockReset();
    api.saved.mockReset();
    api.select.mockReset();
    api.clear.mockReset();
    api.latest.mockReset();
    api.install.mockReset();
    api.managed.mockReset();
    api.clearManaged.mockReset();
    api.listenDownload.mockReset();
    api.listenChanged.mockReset();
    api.discover.mockResolvedValue(discovery);
    api.select.mockResolvedValue(compatible);
    api.clear.mockResolvedValue(undefined);
    api.latest.mockResolvedValue(release);
    api.clearManaged.mockResolvedValue(emptyCache);
    api.listenDownload.mockResolvedValue(() => {});
    api.listenChanged.mockResolvedValue(() => {});
  });

  it("shows the verified selection, candidates and target version", async () => {
    render(<JavaRuntimeSection />);

    expect(await screen.findByText("Selected runtime")).toBeInTheDocument();
    expect(screen.getByText(/Java 21 · MC 1.21.4/)).toBeInTheDocument();
    expect(screen.getAllByText("C:\\Java\\jdk-21\\bin\\java.exe").length).toBeGreaterThan(0);
    expect(screen.getByText("17.0.12 · Eclipse Adoptium · common · outdated")).toBeInTheDocument();
    const outdatedButton = screen.getByRole("button", {
      name: "Use C:\\Java\\jdk-17\\bin\\java.exe",
    });
    expect(outdatedButton).toBeDisabled();
    const compatibleButton = screen.getByRole("button", {
      name: "Use C:\\Java\\jdk-21\\bin\\java.exe",
    });
    expect(compatibleButton).toBeInTheDocument();
  });

  it("selects a compatible candidate through the real API contract", async () => {
    const user = userEvent.setup();
    render(<JavaRuntimeSection />);
    await screen.findByText("Selected runtime");

    // The saved candidate is shown as in use; simulate a new compatible candidate.
    api.discover.mockResolvedValue({
      ...discovery,
      saved: null,
      candidates: [compatible],
    });
    await user.click(screen.getByRole("button", { name: "Refresh detected Java runtimes" }));
    const button = await screen.findByRole("button", {
      name: "Use C:\\Java\\jdk-21\\bin\\java.exe",
    });
    await user.click(button);
    await waitFor(() => expect(api.select).toHaveBeenCalledWith(compatible.path));
    expect(await screen.findByText("Java selected and verified.")).toBeInTheDocument();
  });

  it("requires an explicit click to fetch and install verified Temurin", async () => {
    const user = userEvent.setup();
    api.install.mockResolvedValue({
      runtime: {
        id: "temurin-21.0.13+9-windows-x64",
        path: "C:\\app\\runtimes\\temurin-21.0.13+9-windows-x64",
        javaPath: "C:\\app\\runtimes\\temurin-21.0.13+9-windows-x64\\bin\\java.exe",
        version: "21.0.13+9.0.LTS",
        releaseName: "jdk-21.0.13+9",
        sha256: release.sha256,
        sourceUrl: release.downloadUrl,
        vendor: "Eclipse Adoptium",
        os: "windows",
        arch: "x64",
        installedAt: 1,
        bytes: 200_000_000,
      },
      report: compatible,
      selected: true,
    });
    render(<JavaRuntimeSection />);
    await screen.findByText("Selected runtime");

    expect(api.latest).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Check official Temurin download" }));
    await waitFor(() => expect(api.latest).toHaveBeenCalledTimes(1));
    expect(await screen.findByText("21.0.13+9.0.LTS")).toBeInTheDocument();
    expect(screen.getByText(/2332c0b4cff6/)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Download and install Temurin" }));
    await waitFor(() => expect(api.install).toHaveBeenCalledWith("21.0.13+9.0.LTS"));
    expect(await screen.findByText(/installed, SHA-256 verified and selected/)).toBeInTheDocument();
  });

  it("reports checksum failures without selecting anything", async () => {
    const user = userEvent.setup();
    api.install.mockRejectedValue(
      new Error("runtime download failed an integrity check: SHA-256 mismatch"),
    );
    render(<JavaRuntimeSection />);
    await screen.findByText("Selected runtime");

    await user.click(screen.getByRole("button", { name: "Check official Temurin download" }));
    await user.click(await screen.findByRole("button", { name: "Download and install Temurin" }));
    expect(await screen.findByText(/integrity check: SHA-256 mismatch/)).toBeInTheDocument();
    expect(
      screen.getByText(/Empty Project and manual editing keep working without Java/),
    ).toBeInTheDocument();
    expect(api.select).not.toHaveBeenCalled();
  });

  it("clears only the managed cache after an explicit action", async () => {
    const user = userEvent.setup();
    api.discover.mockResolvedValue({
      ...discovery,
      managed: [
        {
          id: "temurin-21.0.13+9-windows-x64",
          path: "C:\\app\\runtimes\\temurin-21.0.13+9-windows-x64",
          javaPath: "C:\\app\\runtimes\\temurin-21.0.13+9-windows-x64\\bin\\java.exe",
          version: "21.0.13+9.0.LTS",
          releaseName: "jdk-21.0.13+9",
          sha256: release.sha256,
          sourceUrl: release.downloadUrl,
          vendor: "Eclipse Adoptium",
          os: "windows",
          arch: "x64",
          installedAt: 1,
          bytes: 200_000_000,
        },
      ],
      managedBytes: 200_000_000,
    });
    render(<JavaRuntimeSection />);
    await screen.findByText(/1 managed/);

    await user.click(screen.getByRole("button", { name: /Clear managed Java/ }));
    await waitFor(() => expect(api.clearManaged).toHaveBeenCalledTimes(1));
    expect(await screen.findByText(/External Java was not touched/)).toBeInTheDocument();
  });
});
