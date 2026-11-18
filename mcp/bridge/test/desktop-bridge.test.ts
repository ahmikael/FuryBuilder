import { Client } from "@modelcontextprotocol/client";
import { StdioClientTransport } from "@modelcontextprotocol/client/stdio";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, describe, expect, it } from "vitest";

import { MODERN_PROTOCOL_VERSION } from "../src/capabilities.js";
import {
  okResult,
  startMockDesktop,
  type MockDesktop,
} from "./mock-desktop.js";

const bridgeRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const clients: Client[] = [];
const harnesses: MockDesktop[] = [];

function envWith(runtimePath: string): Record<string, string> {
  const env: Record<string, string> = {};
  for (const [key, value] of Object.entries(process.env)) {
    if (value !== undefined) env[key] = value;
  }
  env.FURYBUILDER_MCP_RUNTIME = runtimePath;
  return env;
}

afterEach(async () => {
  await Promise.all(clients.splice(0).map((client) => client.close()));
  await Promise.all(harnesses.splice(0).map((harness) => harness.close()));
});

describe("MCP bridge to the authoritative desktop API", () => {
  it("reports a live local endpoint instead of inventing a connection", async () => {
    let observedToken: string | null = null;
    let observedMethod: string | null = null;
    let observedClientId: string | null = null;
    const harness = await startMockDesktop((request) => {
      observedToken = typeof request.token === "string" ? request.token : null;
      observedMethod =
        typeof request.method === "string" ? request.method : null;
      observedClientId =
        typeof request.clientId === "string" ? request.clientId : null;
      return okResult(request, {
        app: { name: "FuryBuilder", version: "0.1.0" },
        project: {
          projectId: "project-live",
          name: "Harbour",
          worldRevision: 9,
        },
        bridge: { protocolVersion: 1, transport: "local_socket", pid: 4242 },
        mcp: {
          mode: "edit_in_selection",
          writesEnabled: true,
          maxBlocks: 4000,
          maxBatchChanges: 100,
          requireConfirmation: true,
        },
        renderer: { listening: true },
        pendingConfirmations: 1,
      });
    });
    harnesses.push(harness);

    const client = new Client(
      { name: "desktop-integration-client", version: "1.0.0" },
      { versionNegotiation: { mode: { pin: MODERN_PROTOCOL_VERSION } } },
    );
    clients.push(client);
    await client.connect(
      new StdioClientTransport({
        command: process.execPath,
        args: ["--import", "tsx", "src/main.ts"],
        cwd: bridgeRoot,
        env: envWith(harness.runtimePath),
        stderr: "pipe",
      }),
    );

    const result = await client.callTool({
      name: "bridge_info",
      arguments: { project_id: "project-live", world_revision: 9 },
    });
    const text = result.content.find((block) => block.type === "text");
    expect(text?.type).toBe("text");
    const info = JSON.parse(text?.type === "text" ? text.text : "{}") as {
      bridge: {
        desktop: {
          connected: boolean;
          project: { projectId: string; worldRevision: number };
          access: {
            mode: string;
            writesEnabled: boolean;
            maxBlocks: number;
            requireConfirmation: boolean;
          } | null;
          renderer: { listening: boolean } | null;
          pendingConfirmations: number;
        };
        connection: {
          state: string;
          lifecycle: string;
          instanceId: string;
          startedAtMs: number;
        };
        context: {
          state: string;
          valid: boolean;
          requested: { projectId: string; worldRevision: number | null };
          active: { projectId: string; worldRevision: number } | null;
        };
      };
    };
    expect(info.bridge.desktop.connected).toBe(true);
    expect(info.bridge.desktop.project).toMatchObject({
      projectId: "project-live",
      worldRevision: 9,
    });
    expect(info.bridge.desktop.access).toMatchObject({
      mode: "edit_in_selection",
      writesEnabled: true,
      maxBlocks: 4000,
      requireConfirmation: true,
    });
    expect(info.bridge.desktop.renderer).toEqual({ listening: true });
    expect(info.bridge.desktop.pendingConfirmations).toBe(1);
    expect(info.bridge.connection).toMatchObject({
      state: "ready",
      lifecycle: "ephemeral-stdio",
    });
    expect(observedClientId).toBe(info.bridge.connection.instanceId);
    expect(info.bridge.context).toMatchObject({
      state: "ready",
      valid: true,
      requested: { projectId: "project-live", worldRevision: 9 },
      active: { projectId: "project-live", worldRevision: 9 },
    });
    expect(observedToken).toBe(harness.token);
    expect(observedMethod).toBe("ping");

    const stale = await client.callTool({
      name: "bridge_info",
      arguments: { project_id: "project-live", world_revision: 8 },
    });
    const staleText = stale.content.find((block) => block.type === "text");
    const staleInfo = JSON.parse(
      staleText?.type === "text" ? staleText.text : "{}",
    ) as {
      bridge: {
        connection: { state: string };
        context: {
          state: string;
          valid: boolean;
          active: { projectId: string; worldRevision: number } | null;
        };
      };
    };
    expect(staleInfo.bridge.connection.state).toBe("revision_mismatch");
    expect(staleInfo.bridge.context).toMatchObject({
      state: "revision_mismatch",
      valid: false,
      active: { projectId: "project-live", worldRevision: 9 },
    });
  });

  it("serves world, block, camera and capture reads from the live endpoint", async () => {
    const harness = await startMockDesktop((request) => {
      const method = request.method;
      if (method === "world_info") {
        return okResult(request, {
          targetMinecraftVersion: "1.21.4",
          revision: 9,
          dimensions: ["minecraft:overworld"],
        });
      }
      if (method === "query") {
        return okResult(request, {
          type: "block",
          data: {
            dimension: "minecraft:overworld",
            pos: { x: 0, y: 64, z: 0 },
            state: { name: "minecraft:stone", properties: {} },
          },
        });
      }
      if (method === "get_camera") {
        return okResult(request, {
          kind: "camera",
          dimension: "minecraft:overworld",
          worldRevision: 9,
          pose: {
            position: [1, 2, 3],
            target: [0, 0, 0],
            yaw: 0.5,
            pitch: -0.2,
            fov: 70,
          },
          capturedAtMs: 1_700_000_000_000,
          verification: {
            authoritativeRevision: 9,
            renderRevision: 9,
            stale: false,
          },
        });
      }
      if (method === "capture_viewport") {
        return okResult(request, {
          kind: "capture",
          dataUrl: "data:image/png;base64,iVBORw0KGgo=",
          metadata: {
            schemaVersion: 1,
            mode: "viewport",
            dimension: "minecraft:overworld",
            worldRevision: 9,
            partial: true,
            missingChunks: [{ x: 0, z: 0 }],
          },
          capturedAtMs: 1_700_000_000_000,
          verification: {
            authoritativeRevision: 9,
            renderRevision: 9,
            stale: false,
            partial: true,
          },
        });
      }
      return okResult(request, {});
    });
    harnesses.push(harness);

    const client = new Client(
      { name: "desktop-read-client", version: "1.0.0" },
      { versionNegotiation: { mode: { pin: MODERN_PROTOCOL_VERSION } } },
    );
    clients.push(client);
    await client.connect(
      new StdioClientTransport({
        command: process.execPath,
        args: ["--import", "tsx", "src/main.ts"],
        cwd: bridgeRoot,
        env: envWith(harness.runtimePath),
        stderr: "pipe",
      }),
    );

    const world = await client.callTool({
      name: "world_info",
      arguments: { project_id: "project-live", world_revision: 9 },
    });
    const worldText = world.content.find((block) => block.type === "text");
    expect(worldText?.type === "text" ? worldText.text : "").toContain(
      '"targetMinecraftVersion": "1.21.4"',
    );

    const block = await client.callTool({
      name: "get_block",
      arguments: {
        project_id: "project-live",
        dimension: "minecraft:overworld",
        pos: { x: 0, y: 64, z: 0 },
        world_revision: 9,
      },
    });
    const blockText = block.content.find((entry) => entry.type === "text");
    expect(blockText?.type === "text" ? blockText.text : "").toContain(
      '"name": "minecraft:stone"',
    );

    const camera = await client.callTool({
      name: "get_camera",
      arguments: { project_id: "project-live" },
    });
    const cameraText = camera.content.find((entry) => entry.type === "text");
    const cameraResult = JSON.parse(
      cameraText?.type === "text" ? cameraText.text : "{}",
    ) as { kind: string; pose: { fov: number } };
    expect(cameraResult.kind).toBe("camera");
    expect(cameraResult.pose.fov).toBe(70);

    const capture = await client.callTool({
      name: "capture_viewport",
      arguments: {
        project_id: "project-live",
        dimension: "minecraft:overworld",
        world_revision: 9,
      },
    });
    const image = capture.content.find((entry) => entry.type === "image");
    expect(image).toMatchObject({
      type: "image",
      mimeType: "image/png",
      data: "iVBORw0KGgo=",
    });
    const captureText = capture.content.find((entry) => entry.type === "text");
    const captureMetadata = JSON.parse(
      captureText?.type === "text" ? captureText.text : "{}",
    ) as { metadata: { partial: boolean }; verification: { partial: boolean } };
    expect(captureMetadata.metadata.partial).toBe(true);
    expect(captureMetadata.verification.partial).toBe(true);
  });

  it("maps preview, batch apply, status and undo to the endpoint", async () => {
    const observed: Array<Record<string, unknown>> = [];
    const harness = await startMockDesktop((request) => {
      observed.push(request);
      if (request.method === "preview_operation") {
        return okResult(request, {
          affectedBlocks: 2,
          changedBlocks: 1,
          dirtyChunks: [{ x: 0, z: 0 }],
          warnings: [],
          worldRevision: 9,
        });
      }
      if (request.method === "apply_operation") {
        return okResult(request, {
          operationId: "op-1",
          worldRevision: 10,
          changedBlocks: 2,
          dirtyChunks: [{ x: 0, z: 0 }],
          warnings: [],
          replayed: false,
        });
      }
      if (request.method === "operation_status") {
        return okResult(request, {
          operationId: "op-1",
          status: "completed",
          changedBlocks: 2,
          currentRevision: 10,
          canUndo: true,
          job: { available: false, reason: "job_api_unavailable" },
        });
      }
      if (request.method === "undo_operation") {
        return okResult(request, {
          operationId: "op-2",
          worldRevision: 11,
          changedBlocks: 2,
          dirtyChunks: [],
          warnings: [],
          replayed: false,
        });
      }
      return okResult(request, {});
    });
    harnesses.push(harness);

    const client = new Client(
      { name: "desktop-write-client", version: "1.0.0" },
      { versionNegotiation: { mode: { pin: MODERN_PROTOCOL_VERSION } } },
    );
    clients.push(client);
    await client.connect(
      new StdioClientTransport({
        command: process.execPath,
        args: ["--import", "tsx", "src/main.ts"],
        cwd: bridgeRoot,
        env: envWith(harness.runtimePath),
        stderr: "pipe",
      }),
    );

    const preview = await client.callTool({
      name: "preview_operation",
      arguments: {
        project_id: "project-live",
        operation: {
          type: "set_block",
          dimension: "minecraft:overworld",
          pos: { x: 0, y: 64, z: 0 },
          state: { name: "minecraft:stone" },
        },
      },
    });
    expect(preview.isError).not.toBe(true);

    const applied = await client.callTool({
      name: "apply_operations_batch",
      arguments: {
        project_id: "project-live",
        world_revision: 9,
        dimension: "minecraft:overworld",
        changes: [
          {
            pos: { x: 0, y: 64, z: 0 },
            state: { name: "minecraft:stone" },
          },
        ],
        idempotency_key: "batch-1",
      },
    });
    const appliedText = applied.content.find((entry) => entry.type === "text");
    expect(appliedText?.type === "text" ? appliedText.text : "").toContain(
      '"operationId": "op-1"',
    );

    const status = await client.callTool({
      name: "get_operation_status",
      arguments: { project_id: "project-live", operation_id: "op-1" },
    });
    const statusText = status.content.find((entry) => entry.type === "text");
    expect(statusText?.type === "text" ? statusText.text : "").toContain(
      '"job": {',
    );

    const undone = await client.callTool({
      name: "undo_operation",
      arguments: {
        project_id: "project-live",
        world_revision: 10,
        operation_id: "op-1",
      },
    });
    const undoneText = undone.content.find((entry) => entry.type === "text");
    expect(undoneText?.type === "text" ? undoneText.text : "").toContain(
      '"worldRevision": 11',
    );

    const applyRequest = observed.find(
      (request) => request.method === "apply_operation",
    ) as { params?: Record<string, unknown> } | undefined;
    expect(applyRequest?.params).toMatchObject({
      projectId: "project-live",
      worldRevision: 9,
      idempotencyKey: "batch-1",
      operation: {
        type: "batch",
        dimension: "minecraft:overworld",
      },
    });
    const undoRequest = observed.find(
      (request) => request.method === "undo_operation",
    ) as { params?: Record<string, unknown> } | undefined;
    expect(undoRequest?.params).toMatchObject({
      projectId: "project-live",
      worldRevision: 10,
      operationId: "op-1",
    });
  });
});
