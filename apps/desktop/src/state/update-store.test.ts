import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  canCheckForUpdates: vi.fn(() => true),
  checkForUpdate: vi.fn(),
  relaunchApp: vi.fn(async () => undefined),
}));

vi.mock("@/lib/update-api", () => mocks);

import { useUpdateStore } from "@/state/update-store";

function fakeUpdate(version = "0.2.0") {
  return {
    currentVersion: "0.1.0",
    version,
    date: "2026-11-19T00:00:00Z",
    notes: "Release notes",
    install: vi.fn(async (onProgress: (progress: unknown) => void) => {
      onProgress({ downloadedBytes: 0, totalBytes: 100, percent: 0 });
      onProgress({ downloadedBytes: 50, totalBytes: 100, percent: 50 });
      onProgress({ downloadedBytes: 100, totalBytes: 100, percent: 100 });
    }),
    close: vi.fn(async () => undefined),
  };
}

describe("update store", () => {
  beforeEach(() => {
    useUpdateStore.getState().dismiss();
    useUpdateStore.setState({
      status: "idle",
      available: null,
      progress: null,
      error: null,
      lastCheckedAt: null,
    });
    vi.clearAllMocks();
    mocks.canCheckForUpdates.mockReturnValue(true);
  });

  it("reports an available signed update", async () => {
    mocks.checkForUpdate.mockResolvedValue(fakeUpdate());

    await useUpdateStore.getState().check();

    const state = useUpdateStore.getState();
    expect(state.status).toBe("available");
    expect(state.available).toMatchObject({
      version: "0.2.0",
      currentVersion: "0.1.0",
      notes: "Release notes",
    });
    expect(state.lastCheckedAt).not.toBeNull();
  });

  it("reports up to date when the manifest has no newer version", async () => {
    mocks.checkForUpdate.mockResolvedValue(null);

    await useUpdateStore.getState().check();

    expect(useUpdateStore.getState().status).toBe("up-to-date");
    expect(useUpdateStore.getState().available).toBeNull();
  });

  it("downloads, installs and requests a relaunch", async () => {
    const update = fakeUpdate();
    mocks.checkForUpdate.mockResolvedValue(update);
    await useUpdateStore.getState().check();

    await useUpdateStore.getState().install();

    expect(update.install).toHaveBeenCalledOnce();
    expect(mocks.relaunchApp).toHaveBeenCalledOnce();
    expect(useUpdateStore.getState().status).toBe("installing");
    expect(useUpdateStore.getState().progress?.percent).toBe(100);
  });

  it("keeps a failed background check silent but reports a manual failure", async () => {
    mocks.checkForUpdate.mockRejectedValue(new Error("network unavailable"));

    await useUpdateStore.getState().check({ silent: true });
    expect(useUpdateStore.getState().status).toBe("idle");
    expect(useUpdateStore.getState().error).toBeNull();

    await useUpdateStore.getState().check();
    expect(useUpdateStore.getState().status).toBe("error");
    expect(useUpdateStore.getState().error).toBe("network unavailable");
  });

  it("does not check when the runtime cannot install updates", async () => {
    mocks.canCheckForUpdates.mockReturnValue(false);

    await useUpdateStore.getState().check();

    expect(mocks.checkForUpdate).not.toHaveBeenCalled();
    expect(useUpdateStore.getState().status).toBe("error");
  });
});
