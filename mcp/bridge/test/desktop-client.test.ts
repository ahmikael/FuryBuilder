import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { randomUUID } from "node:crypto";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";

import {
  callDesktop,
  DesktopUnavailableError,
  probeDesktop,
  readDesktopRuntime,
  runtimeFilePath,
} from "../src/desktop-client.js";
import {
  okResult,
  startMockDesktop,
  type MockDesktop,
} from "./mock-desktop.js";

const harnesses: MockDesktop[] = [];

afterEach(async () => {
  await Promise.all(harnesses.splice(0).map((harness) => harness.close()));
});

describe("desktop local IPC client", () => {
  it("reaches a local endpoint with the discovery token and method", async () => {
    let observed: Record<string, unknown> | null = null;
    const harness = await startMockDesktop((request) => {
      observed = request;
      return okResult(request, {
        app: { name: "FuryBuilder", version: "0.1.0" },
        project: { projectId: "project-1", name: "Castle", worldRevision: 9 },
        bridge: { protocolVersion: 1, transport: "local_socket", pid: 4242 },
      });
    });
    harnesses.push(harness);

    const result = (await callDesktop(
      "ping",
      { probe: true },
      {
        runtimePath: harness.runtimePath,
      },
    )) as { project: { projectId: string; worldRevision: number } };

    expect(result.project).toMatchObject({
      projectId: "project-1",
      worldRevision: 9,
    });
    expect(observed).toMatchObject({
      protocol: 1,
      token: harness.token,
      method: "ping",
      params: { probe: true },
    });
  });

  it("reports a rejected token as authentication failure", async () => {
    const harness = await startMockDesktop((request) => ({
      protocol: 1,
      id: request.id,
      ok: false,
      error: { code: "unauthorized", message: "authentication failed" },
    }));
    harnesses.push(harness);

    await expect(
      callDesktop("ping", {}, { runtimePath: harness.runtimePath }),
    ).rejects.toMatchObject({
      name: "DesktopUnavailableError",
      reason: "authentication_failed",
      code: "unauthorized",
    });
  });

  it("distinguishes a missing runtime file from a stale endpoint", async () => {
    const directory = await mkdtemp(join(tmpdir(), "furybuilder-desktop-"));
    try {
      const missing = join(directory, "missing.json");
      await expect(readDesktopRuntime(missing)).rejects.toBeInstanceOf(
        DesktopUnavailableError,
      );
      await expect(readDesktopRuntime(missing)).rejects.toMatchObject({
        reason: "runtime_file_missing",
      });

      const runtimePath = join(directory, "runtime.json");
      await writeFile(
        runtimePath,
        JSON.stringify({
          schemaVersion: 1,
          transport: "local_socket",
          endpoint:
            process.platform === "win32"
              ? `\\\\.\\pipe\\furybuilder-missing-${randomUUID()}`
              : join(directory, "missing.sock"),
          token: "b".repeat(64),
          pid: 999_999,
          protocolVersion: 1,
          appVersion: "0.1.0",
          startedAtMs: 1,
        }),
      );
      const probe = await probeDesktop({ runtimePath, timeoutMs: 500 });
      expect(probe).toMatchObject({
        connected: false,
        reason: "app_not_running",
      });
    } finally {
      await rm(directory, { recursive: true, force: true });
    }
  });

  it("rejects malformed discovery files and never echoes the token", async () => {
    const directory = await mkdtemp(join(tmpdir(), "furybuilder-desktop-"));
    try {
      const path = join(directory, "runtime.json");
      await writeFile(path, "{not json");
      await expect(readDesktopRuntime(path)).rejects.toMatchObject({
        reason: "invalid_runtime_file",
      });

      await writeFile(
        path,
        JSON.stringify({
          schemaVersion: 1,
          transport: "local_socket",
          endpoint: "endpoint",
          token: "short",
          pid: 1,
          protocolVersion: 1,
          appVersion: "0.1.0",
          startedAtMs: 1,
        }),
      );
      const error = await readDesktopRuntime(path).catch(
        (failure: unknown) => failure,
      );
      expect(error).toBeInstanceOf(DesktopUnavailableError);
      expect(String((error as Error).message)).not.toContain("short");
    } finally {
      await rm(directory, { recursive: true, force: true });
    }
  });

  it("times out instead of hanging when the desktop endpoint does not answer", async () => {
    const harness = await startMockDesktop(() => undefined, { respond: false });
    harnesses.push(harness);

    await expect(
      callDesktop(
        "ping",
        {},
        { runtimePath: harness.runtimePath, timeoutMs: 100 },
      ),
    ).rejects.toMatchObject({ reason: "timeout" });
  });

  it("honours the runtime path override without guessing a private location", () => {
    expect(
      runtimeFilePath({ FURYBUILDER_MCP_RUNTIME: "C:\\custom\\runtime.json" }),
    ).toBe("C:\\custom\\runtime.json");
  });
});
