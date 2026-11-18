import { Client } from "@modelcontextprotocol/client";
import { StdioClientTransport } from "@modelcontextprotocol/client/stdio";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, describe, expect, it } from "vitest";

import { MODERN_PROTOCOL_VERSION } from "../src/capabilities.js";

const bridgeRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const clients: Client[] = [];
// Keep the diagnostic deterministic even when the developer has the desktop app running.
const missingRuntime = join(tmpdir(), "furybuilder-bridge-no-runtime.json");

function bridgeEnv(
  overrides: Record<string, string> = {},
): Record<string, string> {
  const env: Record<string, string> = {};
  for (const [key, value] of Object.entries(process.env)) {
    if (value !== undefined) env[key] = value;
  }
  env.FURYBUILDER_MCP_RUNTIME = missingRuntime;
  return { ...env, ...overrides };
}

export function stdioTransport(
  runtimePath = missingRuntime,
): StdioClientTransport {
  return new StdioClientTransport({
    command: process.execPath,
    args: ["--import", "tsx", "src/main.ts"],
    cwd: bridgeRoot,
    env: bridgeEnv({ FURYBUILDER_MCP_RUNTIME: runtimePath }),
    stderr: "pipe",
  });
}

async function connect(modern: boolean): Promise<Client> {
  const client = new Client(
    {
      name: modern ? "modern-test-client" : "legacy-test-client",
      version: "1.0.0",
    },
    modern
      ? { versionNegotiation: { mode: { pin: MODERN_PROTOCOL_VERSION } } }
      : undefined,
  );
  clients.push(client);
  await client.connect(stdioTransport());
  return client;
}

function textResult(result: Awaited<ReturnType<Client["callTool"]>>): unknown {
  const text = result.content.find((block) => block.type === "text");
  if (!text || text.type !== "text")
    throw new Error("bridge_info returned no text");
  return JSON.parse(text.text);
}

afterEach(async () => {
  await Promise.all(clients.splice(0).map((client) => client.close()));
});

describe("MCP protocol negotiation", () => {
  it("speaks 2026-07-28 when the client pins the modern era", async () => {
    const client = await connect(true);
    expect(client.getProtocolEra()).toBe("modern");

    const { tools } = await client.listTools();
    expect(tools.map((tool) => tool.name)).toContain("bridge_info");

    const info = textResult(
      await client.callTool({
        name: "bridge_info",
        arguments: { project_id: "project-modern", world_revision: 12 },
      }),
    ) as {
      bridge: {
        contextPolicy: string;
        protocolTarget: string;
        optionalExtensions: unknown;
        jobs: {
          backendConnected: boolean;
          fallbackTools: string[];
          tasks: {
            mode: string;
            reason: string;
            extensionAdvertised: boolean;
            bridgeImplementsTasks: boolean;
          };
        };
        desktop: {
          connected: boolean;
          reason?: string;
        };
        connection: {
          instanceId: string;
          startedAtMs: number;
          transport: string;
          lifecycle: string;
          expiresWithProcess: boolean;
          contextPolicy: string;
          state: string;
        };
        context: {
          state: string;
          valid: boolean;
          requested: { projectId: string; worldRevision: number | null };
          active: { projectId: string; worldRevision: number } | null;
        };
      };
      request: {
        projectId: string;
        worldRevision: number;
        protocolEra: string;
      };
    };
    expect(info.bridge.protocolTarget).toBe(MODERN_PROTOCOL_VERSION);
    expect(info.bridge.contextPolicy).toBe("explicit-per-request");
    expect(info.bridge.optionalExtensions).toEqual({
      tasks: false,
      apps: false,
    });
    expect(info.bridge.jobs).toMatchObject({
      backendConnected: false,
      fallbackTools: ["start_job", "get_job_status", "cancel_job"],
      tasks: {
        mode: "fallback",
        reason: "client_did_not_declare_tasks",
        extensionAdvertised: false,
        bridgeImplementsTasks: false,
      },
    });
    expect(info.bridge.desktop).toMatchObject({
      connected: false,
      reason: "runtime_file_missing",
    });
    expect(info.bridge.connection).toMatchObject({
      transport: "stdio",
      lifecycle: "ephemeral-stdio",
      expiresWithProcess: true,
      contextPolicy: "explicit-per-request",
      state: "desktop_unavailable",
    });
    expect(info.bridge.connection.instanceId.length).toBeGreaterThan(0);
    expect(info.bridge.context).toMatchObject({
      state: "desktop_unavailable",
      valid: false,
      requested: { projectId: "project-modern", worldRevision: 12 },
      active: null,
    });
    expect(info.request).toMatchObject({
      projectId: "project-modern",
      worldRevision: 12,
      protocolEra: "modern",
    });
  });

  it("serves the same tools to a legacy client without an initialize-era session assumption", async () => {
    const client = await connect(false);
    expect(client.getProtocolEra()).toBe("legacy");

    const info = textResult(
      await client.callTool({
        name: "bridge_info",
        arguments: { project_id: "project-legacy" },
      }),
    ) as {
      request: {
        projectId: string;
        protocolEra: string;
        protocolVersion: string | null;
      };
    };
    expect(info.request).toMatchObject({
      projectId: "project-legacy",
      protocolEra: "legacy",
      protocolVersion: null,
    });
  });

  it("requires explicit project context and never reuses a previous project", async () => {
    const client = await connect(true);

    const missing = await client.callTool({
      name: "bridge_info",
      arguments: {},
    });
    expect(missing.isError).toBe(true);

    const first = textResult(
      await client.callTool({
        name: "bridge_info",
        arguments: { project_id: "project-one" },
      }),
    ) as { request: { projectId: string } };
    const second = textResult(
      await client.callTool({
        name: "bridge_info",
        arguments: { project_id: "project-two" },
      }),
    ) as { request: { projectId: string } };

    expect(first.request.projectId).toBe("project-one");
    expect(second.request.projectId).toBe("project-two");
  });

  it("keeps compatible job tools without inventing a job when the Job API is absent", async () => {
    const client = await connect(false);
    const { tools } = await client.listTools();
    const names = tools.map((tool) => tool.name);
    expect(names).toEqual(
      expect.arrayContaining(["start_job", "get_job_status", "cancel_job"]),
    );
    const startJob = tools.find((tool) => tool.name === "start_job");
    expect(startJob).toBeDefined();
    expect(JSON.stringify(startJob?.inputSchema)).toContain("world_revision");
    expect(JSON.stringify(startJob?.inputSchema)).toContain("idempotency_key");

    const started = await client.callTool({
      name: "start_job",
      arguments: {
        project_id: "project-jobs",
        kind: "structure_export",
        parameters: {},
        idempotency_key: "job-1",
      },
    });
    expect(started.isError).toBe(true);
    const startText = started.content.find((block) => block.type === "text");
    expect(startText?.type === "text" ? startText.text : "").toMatch(
      /^job_api_unavailable:/,
    );

    const status = await client.callTool({
      name: "get_job_status",
      arguments: { project_id: "project-jobs", job_id: "job-1" },
    });
    expect(status.isError).toBe(true);
    const statusText = status.content.find((block) => block.type === "text");
    expect(statusText?.type === "text" ? statusText.text : "").toMatch(
      /^job_api_unavailable:/,
    );

    const cancelled = await client.callTool({
      name: "cancel_job",
      arguments: { project_id: "project-jobs", job_id: "job-1" },
    });
    expect(cancelled.isError).toBe(true);
  });

  it("registers the read-only tools and fails honestly without the desktop app", async () => {
    const client = await connect(true);
    const { tools } = await client.listTools();
    const readTools = tools.filter((tool) =>
      [
        "world_info",
        "get_block",
        "inspect_region",
        "get_region",
        "get_camera",
        "capture_viewport",
      ].includes(tool.name),
    );
    expect(readTools.map((tool) => tool.name).sort()).toEqual([
      "capture_viewport",
      "get_block",
      "get_camera",
      "get_region",
      "inspect_region",
      "world_info",
    ]);
    for (const tool of readTools) {
      expect(tool.annotations?.readOnlyHint).toBe(true);
      expect(tool.annotations?.destructiveHint).toBe(false);
    }

    const missing = await client.callTool({
      name: "world_info",
      arguments: { project_id: "project-read" },
    });
    expect(missing.isError).toBe(true);
    const text = missing.content.find((block) => block.type === "text");
    expect(text?.type === "text" ? text.text : "").toMatch(
      /^runtime_file_missing: FuryBuilder is not running/,
    );

    const block = await client.callTool({
      name: "get_block",
      arguments: {
        project_id: "project-read",
        dimension: "minecraft:overworld",
        pos: { x: 0, y: 64, z: 0 },
      },
    });
    expect(block.isError).toBe(true);
  });
});
