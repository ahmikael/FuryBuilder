import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ComponentInventorySection } from "@/features/settings/ComponentInventorySection";
import { BackendError } from "@/lib/ipc";
import type { ComponentInventory, ManagedClientJar } from "@/lib/component-types";
import type { ManagedRuntime } from "@/runtime/types";
import type { ManagedServerJar } from "@/runtime/server-types";

const api = vi.hoisted(() => ({
  inventory: vi.fn(),
  listen: vi.fn(),
  verify: vi.fn(),
  remove: vi.fn(),
  openFolder: vi.fn(),
}));

vi.mock("@/lib/components-api", () => ({
  componentInventory: api.inventory,
  listenComponentChanged: api.listen,
  verifyComponentInventory: api.verify,
  removeComponent: api.remove,
  openComponentFolder: api.openFolder,
}));

const runtime: ManagedRuntime = {
  id: "temurin-21.0.5-11",
  major: 21,
  path: "C:\\FuryBuilder\\components\\java\\21\\temurin-21.0.5-11",
  javaPath: "C:\\FuryBuilder\\components\\java\\21\\temurin-21.0.5-11\\bin\\java.exe",
  version: "21.0.5+11",
  releaseName: "jdk-21.0.5+11",
  sha256: "a".repeat(64),
  sourceUrl:
    "https://api.adoptium.net/v3/binary/latest/21/ga/windows/x64/jre/hotspot/normal/eclipse",
  vendor: "Eclipse Adoptium",
  os: "windows",
  arch: "x64",
  installedAt: 1_700_000_000,
  bytes: 123_456_789,
};

const client: ManagedClientJar = {
  id: "assets-managed-1.21.4",
  version: "1.21.4",
  path: "C:\\FuryBuilder\\components\\assets\\1.21.4",
  jarPath: "C:\\FuryBuilder\\components\\assets\\1.21.4\\client.jar",
  sha1: "b".repeat(40),
  size: 55_000_000,
  dataVersion: 4189,
  resourcePackFormat: 46,
  datapackFormat: 61,
  sourceUrl: "https://piston-data.mojang.com/v1/objects/client.jar",
  installedAt: 1_700_000_000,
};

const server: ManagedServerJar = {
  version: "1.21.4",
  path: "C:\\FuryBuilder\\components\\servers\\1.21.4",
  jarPath: "C:\\FuryBuilder\\components\\servers\\1.21.4\\server.jar",
  sha1: "c".repeat(40),
  size: 52_000_000,
  dataVersion: 4189,
  javaMajor: 21,
  releaseTime: "2024-12-03T10:12:57+00:00",
  sourceUrl: "https://piston-data.mojang.com/v1/objects/server.jar",
  versionJsonSha1: "d".repeat(40),
  installedAt: 1_700_000_000,
};

const inventory: ComponentInventory = {
  java: {
    root: "C:\\FuryBuilder\\components\\java",
    runtimes: [runtime],
    totalBytes: runtime.bytes,
    problems: [],
  },
  assets: {
    root: "C:\\FuryBuilder\\components\\assets",
    clients: [client],
    totalBytes: client.size,
    problems: [],
  },
  servers: {
    root: "C:\\FuryBuilder\\components\\servers",
    servers: [server],
    totalBytes: server.size,
    problems: [],
  },
};

const empty: ComponentInventory = {
  java: { root: "C:\\FuryBuilder\\components\\java", runtimes: [], totalBytes: 0, problems: [] },
  assets: { root: "C:\\FuryBuilder\\components\\assets", clients: [], totalBytes: 0, problems: [] },
  servers: {
    root: "C:\\FuryBuilder\\components\\servers",
    servers: [],
    totalBytes: 0,
    problems: [],
  },
};

beforeEach(() => {
  api.inventory.mockReset();
  api.listen.mockReset();
  api.verify.mockReset();
  api.remove.mockReset();
  api.openFolder.mockReset();
  api.listen.mockResolvedValue(vi.fn());
  api.openFolder.mockResolvedValue(undefined);
});

describe("ComponentInventorySection", () => {
  it("lists managed Java, client and server artifacts with sizes and paths", async () => {
    api.inventory.mockResolvedValue(inventory);
    render(<ComponentInventorySection />);

    expect(await screen.findByText("Java 21.0.5+11")).toBeInTheDocument();
    const java = screen.getByRole("group", { name: "Java runtimes" });
    expect(within(java).getByText("117.7 MiB")).toBeInTheDocument();
    expect(within(java).getByText("Eclipse Adoptium")).toBeInTheDocument();
    expect(within(java).getByTitle(runtime.javaPath)).toBeInTheDocument();

    const assets = screen.getByRole("group", { name: "Minecraft client JARs" });
    expect(within(assets).getByText("Minecraft 1.21.4 client JAR")).toBeInTheDocument();
    expect(within(assets).getByText("DataVersion 4189")).toBeInTheDocument();
    expect(within(assets).getByText("52.5 MiB")).toBeInTheDocument();
    expect(within(assets).getByTitle(client.jarPath)).toBeInTheDocument();

    const servers = screen.getByRole("group", { name: "Official server JARs" });
    expect(within(servers).getByText("Minecraft 1.21.4 server JAR")).toBeInTheDocument();
    expect(within(servers).getByText("DataVersion 4189")).toBeInTheDocument();
    expect(within(servers).getByText("49.6 MiB")).toBeInTheDocument();
    expect(within(servers).getByTitle(server.jarPath)).toBeInTheDocument();
    expect(api.inventory).toHaveBeenCalledTimes(1);
  });

  it("explains each empty cache without offering project setup", async () => {
    api.inventory.mockResolvedValue(empty);
    render(<ComponentInventorySection />);

    expect(await screen.findByText(/No managed Java runtimes/)).toBeInTheDocument();
    expect(screen.getByText(/No managed client JARs/)).toBeInTheDocument();
    expect(screen.getByText(/No managed server JARs/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /download/i })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /choose/i })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /placeholder/i })).not.toBeInTheDocument();
  });

  it("retries a failed inventory read with an actionable message", async () => {
    api.inventory
      .mockRejectedValueOnce(
        new BackendError(
          "backend_unavailable",
          "The Rust backend is not available in this window.",
        ),
      )
      .mockResolvedValueOnce(inventory);
    const user = userEvent.setup();
    render(<ComponentInventorySection />);

    expect(await screen.findByText("Component inventory unavailable")).toBeInTheDocument();
    expect(screen.getByText(/Start the desktop build/)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Retry" }));
    expect(await screen.findByText("Java 21.0.5+11")).toBeInTheDocument();
    expect(api.inventory).toHaveBeenCalledTimes(2);
  });

  it("reloads when a component changes elsewhere in the app", async () => {
    let changed: (() => void) | null = null;
    api.listen.mockImplementation((handler: () => void) => {
      changed = handler;
      return Promise.resolve(vi.fn());
    });
    api.inventory.mockResolvedValueOnce(empty).mockResolvedValueOnce(inventory);
    render(<ComponentInventorySection />);

    expect(await screen.findByText(/No managed Java runtimes/)).toBeInTheDocument();
    await act(async () => {
      changed?.();
    });
    expect(await screen.findByText("Java 21.0.5+11")).toBeInTheDocument();
    expect(api.inventory).toHaveBeenCalledTimes(2);
  });

  it("verifies a managed artifact against the recorded facts", async () => {
    api.inventory.mockResolvedValue(inventory);
    api.verify.mockResolvedValue({
      kind: "assets",
      id: "assets-managed-1.21.4",
      verified: true,
      state: "ready",
      notes: ["checked against the cached manifest"],
    });
    const user = userEvent.setup();
    render(<ComponentInventorySection />);

    const assets = await screen.findByRole("group", { name: "Minecraft client JARs" });
    await user.click(
      within(assets).getByRole("button", { name: "Verify Minecraft 1.21.4 client JAR" }),
    );

    expect(api.verify).toHaveBeenCalledWith("assets", "assets-managed-1.21.4");
    expect(await within(assets).findByText("Verified")).toBeInTheDocument();
    expect(
      await screen.findByText("Minecraft 1.21.4 client JAR passed local verification."),
    ).toBeInTheDocument();
    expect(within(assets).getByText("checked against the cached manifest")).toBeInTheDocument();
  });

  it("shows the reason when local verification finds a changed artifact", async () => {
    api.inventory.mockResolvedValue(inventory);
    api.verify.mockResolvedValue({
      kind: "server",
      id: "server-managed-1.21.4",
      verified: false,
      state: "incompatible",
      notes: [],
      error: "the cached server JAR for Minecraft 1.21.4 does not match its SHA-1",
    });
    const user = userEvent.setup();
    render(<ComponentInventorySection />);

    const servers = await screen.findByRole("group", { name: "Official server JARs" });
    await user.click(
      within(servers).getByRole("button", { name: "Verify Minecraft 1.21.4 server JAR" }),
    );

    expect(api.verify).toHaveBeenCalledWith("server", "server-managed-1.21.4");
    expect(await within(servers).findByText("Needs attention")).toBeInTheDocument();
    expect(
      within(servers).getByText(
        "the cached server JAR for Minecraft 1.21.4 does not match its SHA-1",
      ),
    ).toBeInTheDocument();
    expect(
      await screen.findByText("Minecraft 1.21.4 server JAR did not pass local verification."),
    ).toBeInTheDocument();
  });

  it("asks before removing an artifact and reloads the cache afterwards", async () => {
    api.inventory.mockResolvedValue(inventory);
    api.remove.mockImplementation((kind: string, id: string) =>
      Promise.resolve({ kind, id, removedManaged: true, clearedSelection: false }),
    );
    const user = userEvent.setup();
    render(<ComponentInventorySection />);

    expect(await screen.findByText("Java 21.0.5+11")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Remove Java 21.0.5+11" }));

    const dialog = await screen.findByRole("dialog", { name: "Remove cached component?" });
    expect(api.remove).not.toHaveBeenCalled();

    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(api.remove).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "Remove Java 21.0.5+11" }));
    const confirmation = await screen.findByRole("dialog", {
      name: "Remove cached component?",
    });
    await user.click(within(confirmation).getByRole("button", { name: "Remove" }));

    await waitFor(() => expect(api.remove).toHaveBeenCalledWith("java", "temurin-21.0.5-11"));
    expect(
      await screen.findByText("Removed Java 21.0.5+11 from the FuryBuilder cache."),
    ).toBeInTheDocument();
    await waitFor(() => expect(api.inventory).toHaveBeenCalledTimes(2));
  });

  it("closes the confirmation and explains a failed removal", async () => {
    api.inventory.mockResolvedValue(inventory);
    api.remove.mockRejectedValue(
      new BackendError("not_found", "component `server` was not found: server-managed-1.21.4"),
    );
    const user = userEvent.setup();
    render(<ComponentInventorySection />);

    expect(await screen.findByText("Java 21.0.5+11")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Remove Minecraft 1.21.4 server JAR" }));
    const dialog = await screen.findByRole("dialog", { name: "Remove cached component?" });
    await user.click(within(dialog).getByRole("button", { name: "Remove" }));

    expect(await screen.findByText("Component action failed")).toBeInTheDocument();
    expect(
      screen.getByText("component `server` was not found: server-managed-1.21.4"),
    ).toBeInTheDocument();
    await waitFor(() =>
      expect(screen.queryByRole("dialog", { name: "Remove cached component?" })).toBeNull(),
    );
  });

  it("opens a known cache folder and reports a platform failure", async () => {
    api.inventory.mockResolvedValue(inventory);
    api.openFolder
      .mockResolvedValueOnce(undefined)
      .mockRejectedValueOnce(new BackendError("internal", "opening the assets folder failed"));
    const user = userEvent.setup();
    render(<ComponentInventorySection />);

    expect(await screen.findByText("Java 21.0.5+11")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Open Java runtimes folder" }));
    expect(api.openFolder).toHaveBeenLastCalledWith("java");

    await user.click(screen.getByRole("button", { name: "Open Minecraft client JARs folder" }));
    expect(api.openFolder).toHaveBeenLastCalledWith("assets");
    expect(await screen.findByText("Component action failed")).toBeInTheDocument();
    expect(screen.getByText("opening the assets folder failed")).toBeInTheDocument();
  });
});
