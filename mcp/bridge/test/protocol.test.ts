import { Client } from "@modelcontextprotocol/client";
import { StdioClientTransport } from "@modelcontextprotocol/client/stdio";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, describe, expect, it } from "vitest";

import { MODERN_PROTOCOL_VERSION } from "../src/capabilities.js";

const bridgeRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const clients: Client[] = [];

function stdioTransport(): StdioClientTransport {
  return new StdioClientTransport({
    command: process.execPath,
    args: ["--import", "tsx", "src/main.ts"],
    cwd: bridgeRoot,
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
});
