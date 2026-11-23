import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { DependencyReadiness } from "@/features/settings/DependencyReadiness";
import type { JavaRuntimeReport } from "@/runtime/types";
import type { ServerJarReport, VanillaSetupReport } from "@/runtime/server-types";

const server = vi.hoisted(() => ({
  setup: vi.fn(),
  listenChanged: vi.fn(),
}));
const runtime = vi.hoisted(() => ({
  listenChanged: vi.fn(),
}));

vi.mock("@/lib/server-api", () => ({
  vanillaSetupCheck: server.setup,
  listenServerChanged: server.listenChanged,
}));

vi.mock("@/lib/runtime-api", () => ({
  listenRuntimeChanged: runtime.listenChanged,
}));

const java: JavaRuntimeReport = {
  path: "C:\\Java\\jdk-21\\bin\\java.exe",
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

const serverJar: ServerJarReport = {
  path: "C:\\app\\servers\\1.21.4\\server.jar",
  source: "managed",
  managed: true,
  ready: true,
  version: "1.21.4",
  dataVersion: 4189,
  javaMajor: 21,
  protocolVersion: 769,
  resourcePackFormat: 46,
  sha1: "4707d00eb834b446575d89a61a11b5d548d8c001",
  size: 56_880_250,
  matchesOfficial: true,
  warnings: [],
  error: null,
};

const ready: VanillaSetupReport = {
  targetMinecraftVersion: "1.21.4",
  targetDataVersion: 4189,
  targetJavaMajor: 21,
  serverDatapackFormat: 61,
  java,
  javaReady: true,
  server: serverJar,
  serverReady: true,
  eulaAcceptedAt: 1_763_500_000,
  eulaMatchesSelected: true,
  blockers: [],
  ready: true,
};

const blocked: VanillaSetupReport = {
  ...ready,
  java: null,
  javaReady: false,
  server: null,
  serverReady: false,
  eulaAcceptedAt: null,
  eulaMatchesSelected: false,
  blockers: [
    "Select a compatible Java 21 runtime.",
    "Select or download the official Minecraft 1.21.4 server.jar.",
    "Read and accept the Minecraft EULA for the selected server.jar.",
  ],
  ready: false,
};

describe("DependencyReadiness", () => {
  beforeEach(() => {
    server.setup.mockReset();
    server.listenChanged.mockReset();
    runtime.listenChanged.mockReset();
    server.listenChanged.mockResolvedValue(() => {});
    runtime.listenChanged.mockResolvedValue(() => {});
  });

  it("shows a ready roll-up with version, server and EULA state", async () => {
    server.setup.mockResolvedValue(ready);
    render(<DependencyReadiness />);

    expect(await screen.findByText("Vanilla World is ready")).toBeInTheDocument();
    expect(screen.getByText(/Minecraft 1\.21\.4 · DV 4189 · Java 21/)).toBeInTheDocument();
    expect(screen.getByText(/21\.0\.13 · Eclipse Adoptium · compatible/)).toBeInTheDocument();
    expect(screen.getByText(/Minecraft 1\.21\.4 · DV 4189 · ready/)).toBeInTheDocument();
    expect(screen.getByText(/Accepted/)).toBeInTheDocument();
    expect(screen.getByText(/No dependency is downloaded automatically/)).toBeInTheDocument();
  });

  it("lists every blocker instead of claiming readiness", async () => {
    server.setup.mockResolvedValue(blocked);
    render(<DependencyReadiness />);

    expect(await screen.findByText("Setup required")).toBeInTheDocument();
    expect(screen.getByText(/3 blocker\(s\)/)).toBeInTheDocument();
    for (const blocker of blocked.blockers) {
      expect(screen.getByText(blocker)).toBeInTheDocument();
    }
    expect(screen.getAllByText("Not selected")).toHaveLength(2);
    expect(screen.getByText("Not accepted")).toBeInTheDocument();
  });

  it("explains offline/backend failure and retries without hiding local-only safety", async () => {
    const user = userEvent.setup();
    server.setup.mockRejectedValueOnce(
      new Error("network unavailable: could not read version metadata"),
    );
    render(<DependencyReadiness />);

    expect(await screen.findByRole("alert")).toHaveTextContent("Readiness check unavailable");
    expect(screen.getByText(/network unavailable/)).toBeInTheDocument();
    expect(
      screen.getByText(/Empty Project and Import World still work offline/),
    ).toBeInTheDocument();

    server.setup.mockResolvedValue(ready);
    await user.click(screen.getByRole("button", { name: /Retry/ }));
    await waitFor(() => expect(server.setup).toHaveBeenCalledTimes(2));
    expect(await screen.findByText("Vanilla World is ready")).toBeInTheDocument();
  });
});
