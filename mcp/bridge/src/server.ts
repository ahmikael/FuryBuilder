import {
  CLIENT_CAPABILITIES_META_KEY,
  McpServer,
  PROTOCOL_VERSION_META_KEY,
} from "@modelcontextprotocol/server";
import * as z from "zod/v4";

import {
  BRIDGE_IMPLEMENTS_TASKS,
  BRIDGE_INSTRUCTIONS,
  BRIDGE_NAME,
  BRIDGE_VERSION,
  MODERN_PROTOCOL_VERSION,
  OPTIONAL_EXTENSIONS,
  requireProjectId,
  resolveTasksSupport,
  type ProtocolEra,
} from "./capabilities.js";
import { evaluateContext } from "./context.js";
import { BRIDGE_INSTANCE_ID, BRIDGE_STARTED_AT_MS } from "./instance.js";
import { registerJobTools, JOB_TOOL_NAMES } from "./job-tools.js";
import { createUnavailableJobApi, type JobApi } from "./jobs.js";
import { probeDesktop } from "./desktop-client.js";
import { registerReadTools } from "./read-tools.js";
import { registerWriteTools } from "./write-tools.js";

const bridgeInfoInput = z.object({
  project_id: z
    .string()
    .min(1)
    .max(128)
    .describe("Stable project identifier; never inferred from the connection."),
  world_revision: z
    .number()
    .int()
    .nonnegative()
    .optional()
    .describe("Revision the caller intends to inspect, when known."),
});

/**
 * Builds the MCP server for one connection. `serveStdio` decides the protocol era before
 * calling this factory, so the returned instance serves exactly one client connection.
 * Handlers read explicit arguments; none of them consult transport-global project state.
 */
export function createFuryBuilderServer(
  dependencies: { jobs?: JobApi } = {},
): McpServer {
  const jobs = dependencies.jobs ?? createUnavailableJobApi();
  const server = new McpServer(
    { name: BRIDGE_NAME, version: BRIDGE_VERSION },
    { instructions: BRIDGE_INSTRUCTIONS },
  );

  registerJobTools(server, jobs);
  registerReadTools(server);
  registerWriteTools(server);

  server.registerTool(
    "bridge_info",
    {
      title: "Bridge information",
      description:
        "Read-only handshake diagnostic. Returns the negotiated protocol era and confirms that project context is explicitly supplied.",
      inputSchema: bridgeInfoInput,
      annotations: {
        readOnlyHint: true,
        destructiveHint: false,
        idempotentHint: true,
        openWorldHint: false,
      },
    },
    async ({ project_id, world_revision }, ctx) => {
      const projectId = requireProjectId(project_id);
      // The 2026-07-28 era attaches this envelope to every request. Its absence is the
      // legacy marker; neither path stores project context on the connection.
      const envelope = ctx.mcpReq.envelope as
        Record<string, unknown> | undefined;
      const era: ProtocolEra = envelope ? "modern" : "legacy";
      const protocolVersion = envelope?.[PROTOCOL_VERSION_META_KEY] ?? null;
      const clientCapabilities =
        envelope?.[CLIENT_CAPABILITIES_META_KEY] ?? null;
      const desktop = await probeDesktop();
      const jobBackendConnected =
        jobs.connected && desktop.connected && desktop.jobs?.available === true;
      const tasks = resolveTasksSupport({
        clientCapabilities,
        jobBackendConnected,
      });
      const activeContext = desktop.connected
        ? {
            projectId: desktop.project.projectId,
            worldRevision: desktop.project.worldRevision,
          }
        : null;
      const context = evaluateContext(
        {
          projectId,
          worldRevision: world_revision ?? null,
        },
        activeContext,
      );

      return {
        content: [
          {
            type: "text",
            text: JSON.stringify(
              {
                bridge: {
                  name: BRIDGE_NAME,
                  version: BRIDGE_VERSION,
                  protocolTarget: MODERN_PROTOCOL_VERSION,
                  legacyFallback: true,
                  contextPolicy: "explicit-per-request",
                  optionalExtensions: OPTIONAL_EXTENSIONS,
                  jobs: {
                    backendConnected: jobBackendConnected,
                    fallbackTools: JOB_TOOL_NAMES,
                    tasks: {
                      ...tasks,
                      bridgeImplementsTasks: BRIDGE_IMPLEMENTS_TASKS,
                    },
                  },
                  desktop,
                  connection: {
                    instanceId: BRIDGE_INSTANCE_ID,
                    startedAtMs: BRIDGE_STARTED_AT_MS,
                    transport: "stdio",
                    lifecycle: "ephemeral-stdio",
                    expiresWithProcess: true,
                    contextPolicy: "explicit-per-request",
                    state: context.state,
                  },
                  context,
                },
                request: {
                  projectId,
                  worldRevision: world_revision ?? null,
                  protocolEra: era,
                  protocolVersion,
                  clientCapabilities,
                },
              },
              null,
              2,
            ),
          },
        ],
      };
    },
  );

  return server;
}
