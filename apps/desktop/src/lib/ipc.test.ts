import { beforeEach, describe, expect, it, vi } from "vitest";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

import { BackendError, invokeCommand, isTauriRuntime, normalizeError } from "@/lib/ipc";

function enableTauriRuntime(): void {
  Object.defineProperty(window, "__TAURI_INTERNALS__", {
    value: {},
    configurable: true,
    writable: true,
  });
}

function disableTauriRuntime(): void {
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
}

describe("ipc", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    disableTauriRuntime();
  });

  it("detects the Tauri runtime", () => {
    expect(isTauriRuntime()).toBe(false);
    enableTauriRuntime();
    expect(isTauriRuntime()).toBe(true);
  });

  it("fails explicitly outside Tauri instead of faking world data", async () => {
    await expect(invokeCommand("app_info")).rejects.toMatchObject({
      code: "backend_unavailable",
    });
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("forwards typed commands and arguments", async () => {
    enableTauriRuntime();
    invokeMock.mockResolvedValue({ message: "pong, ui" });

    await expect(invokeCommand("ping", { name: "ui" })).resolves.toEqual({ message: "pong, ui" });
    expect(invokeMock).toHaveBeenCalledWith("ping", { name: "ui" });
  });

  it("normalizes structured backend errors", async () => {
    enableTauriRuntime();
    invokeMock.mockRejectedValue({ code: "out_of_bounds", message: "Y=400 is outside the world" });

    await expect(invokeCommand("set_block")).rejects.toMatchObject({
      name: "BackendError",
      code: "out_of_bounds",
      message: "Y=400 is outside the world",
    });
  });

  it("wraps unknown failures safely", () => {
    const wrapped = normalizeError(new Error("boom"));
    expect(wrapped).toBeInstanceOf(BackendError);
    expect(wrapped.code).toBe("unknown");
    expect(normalizeError("plain text").message).toBe("plain text");
  });
});
