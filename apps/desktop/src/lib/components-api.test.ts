import { beforeEach, describe, expect, it, vi } from "vitest";
import { listen } from "@tauri-apps/api/event";
import { invokeCommand } from "@/lib/ipc";
import {
  cancelComponentInstall,
  componentAssetsRelease,
  componentInventory,
  componentJavaRelease,
  componentServerRelease,
  componentStatus,
  installComponent,
  listenComponentChanged,
  listenComponentError,
  listenComponentProgress,
  relinkComponent,
  removeComponent,
  resolveComponent,
  verifyComponent,
} from "@/lib/components-api";
import type {
  ComponentInstallRequest,
  ComponentRequirements,
  ComponentTarget,
} from "@/lib/component-types";

vi.mock("@/lib/ipc", () => ({
  invokeCommand: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => undefined),
}));

const invoke = vi.mocked(invokeCommand);
const listenMock = vi.mocked(listen);

const target: ComponentTarget = {
  profileId: "java-classic-test",
  minecraftVersion: "test-version",
  dataVersion: 12345,
  javaMajor: 21,
};

const requirements: ComponentRequirements = {
  java: true,
  assets: true,
  server: false,
  assetsPlaceholder: false,
};

const request: ComponentInstallRequest = {
  kind: "assets",
  target,
  expectedVersion: "test-version",
  expectedHash: { algorithm: "sha1", value: "a".repeat(40) },
};

beforeEach(() => {
  invoke.mockClear();
  listenMock.mockClear();
});

describe("component IPC contract", () => {
  it("uses the unified component commands and camelCase payloads", async () => {
    await componentStatus(target, requirements);
    expect(invoke).toHaveBeenLastCalledWith("components_status", { target, requirements });

    await resolveComponent("java", target, requirements);
    expect(invoke).toHaveBeenLastCalledWith("components_resolve", {
      kind: "java",
      target,
      requirements,
    });

    await componentInventory();
    expect(invoke).toHaveBeenLastCalledWith("components_inventory");

    await componentJavaRelease(target);
    expect(invoke).toHaveBeenLastCalledWith("components_java_release", { target });

    await componentAssetsRelease(target);
    expect(invoke).toHaveBeenLastCalledWith("components_assets_release", { target });

    await componentServerRelease(target);
    expect(invoke).toHaveBeenLastCalledWith("components_server_release", { target });

    await installComponent(request);
    expect(invoke).toHaveBeenLastCalledWith("components_install", { request });

    await cancelComponentInstall("assets");
    expect(invoke).toHaveBeenLastCalledWith("components_cancel", { kind: "assets" });

    await relinkComponent("java", "C:\\Java\\bin\\java.exe", target);
    expect(invoke).toHaveBeenLastCalledWith("components_relink", {
      kind: "java",
      path: "C:\\Java\\bin\\java.exe",
      target,
    });

    await removeComponent("server", "server-managed-test-version");
    expect(invoke).toHaveBeenLastCalledWith("components_remove", {
      kind: "server",
      id: "server-managed-test-version",
    });

    await verifyComponent("assets", "assets-managed-test-version", target);
    expect(invoke).toHaveBeenLastCalledWith("components_verify", {
      kind: "assets",
      id: "assets-managed-test-version",
      target,
    });
  });

  it("subscribes to progress, change and error events with typed payloads", async () => {
    const progressHandler = vi.fn();
    const changedHandler = vi.fn();
    const errorHandler = vi.fn();

    await listenComponentProgress(progressHandler);
    expect(listenMock).toHaveBeenLastCalledWith(
      "furybuilder-component-progress",
      expect.any(Function),
    );

    await listenComponentChanged(changedHandler);
    expect(listenMock).toHaveBeenLastCalledWith(
      "furybuilder-component-changed",
      expect.any(Function),
    );

    await listenComponentError(errorHandler);
    expect(listenMock).toHaveBeenLastCalledWith(
      "furybuilder-component-error",
      expect.any(Function),
    );

    const progress = { kind: "assets", phase: "downloading", downloaded: 1, total: 2 };
    const changed = { kind: "assets" };
    const failure = { kind: "assets", code: "offline", message: "network access is disabled" };
    const callbacks = listenMock.mock.calls.map((call) => call[1] as (event: unknown) => void);
    callbacks[0]({ payload: progress });
    callbacks[1]({ payload: changed });
    callbacks[2]({ payload: failure });

    expect(progressHandler).toHaveBeenCalledWith(progress);
    expect(changedHandler).toHaveBeenCalledWith(changed);
    expect(errorHandler).toHaveBeenCalledWith(failure);
  });
});
