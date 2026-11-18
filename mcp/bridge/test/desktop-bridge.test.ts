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
    const harness = await startMockDesktop((request) => {
      observedToken = typeof request.token === "string" ? request.token : null;
      observedMethod =
        typeof request.method === "string" ? request.method : null;
      return okResult(request, {
        app: { name: "FuryBuilder", version: "0.1.0" },
        project: {
          projectId: "project-live",
          name: "Harbour",
          worldRevision: 9,
        },
        bridge: { protocolVersion: 1, transport: "local_socket", pid: 4242 },
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
      arguments: { project_id: "project-live" },
    });
    const text = result.content.find((block) => block.type === "text");
    expect(text?.type).toBe("text");
    const info = JSON.parse(text?.type === "text" ? text.text : "{}") as {
      bridge: {
        desktop: {
          connected: boolean;
          project: { projectId: string; worldRevision: number };
        };
      };
    };
    expect(info.bridge.desktop.connected).toBe(true);
    expect(info.bridge.desktop.project).toMatchObject({
      projectId: "project-live",
      worldRevision: 9,
    });
    expect(observedToken).toBe(harness.token);
    expect(observedMethod).toBe("ping");
  });
});
