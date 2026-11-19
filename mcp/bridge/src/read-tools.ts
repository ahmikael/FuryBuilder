import type { McpServer } from "@modelcontextprotocol/server";
import { createHash } from "node:crypto";
import * as z from "zod/v4";

import { requireProjectId } from "./capabilities.js";
import { callDesktop } from "./desktop-client.js";
import { errorResult, textResult, withRevision } from "./tool-result.js";

/**
 * Read-only MCP tools over the authoritative Application API. None of them mutates the
 * world or the project; screenshots are real viewport frames, never generated text.
 */

const PNG_DATA_URL_PREFIX = "data:image/png;base64,";
// The Tauri endpoint waits up to 10 s for a mounted viewport; the socket client must
// outlive that window instead of reporting a false timeout.
const RENDER_TIMEOUT_MS = 15_000;

const blockPos = z.object({
  x: z.number().int(),
  y: z.number().int(),
  z: z.number().int(),
});

const bounds = z.object({
  min: blockPos,
  max: blockPos,
});

const project = {
  project_id: z
    .string()
    .min(1)
    .max(128)
    .describe("Project identifier; never inferred from the MCP connection."),
  world_revision: z
    .number()
    .int()
    .nonnegative()
    .optional()
    .describe(
      "Revision the caller observed. A mismatch is reported instead of silently reading newer data.",
    ),
};

const readOnly = {
  readOnlyHint: true,
  destructiveHint: false,
  idempotentHint: true,
  openWorldHint: false,
} as const;

async function queryDesktop(
  projectId: string,
  revision: number | undefined,
  query: Record<string, unknown>,
): Promise<unknown> {
  return callDesktop("query", withRevision({ projectId, query }, revision));
}

export function registerReadTools(server: McpServer): void {
  server.registerTool(
    "world_info",
    {
      title: "World information",
      description:
        "Returns the target Minecraft version, dimensions, bounds, revision and load status of the open project.",
      inputSchema: z.object({ ...project }),
      annotations: readOnly,
    },
    async ({ project_id, world_revision }) => {
      try {
        const projectId = requireProjectId(project_id);
        return textResult(
          await callDesktop(
            "world_info",
            withRevision({ projectId }, world_revision),
          ),
        );
      } catch (error) {
        return errorResult(error);
      }
    },
  );

  server.registerTool(
    "get_block",
    {
      title: "Get block",
      description:
        "Reads the exact block state at one position, including a canonical state string and the authoritative world revision it was read at.",
      inputSchema: z.object({
        ...project,
        dimension: z.string().min(1).max(128),
        pos: blockPos,
      }),
      annotations: readOnly,
    },
    async ({ project_id, dimension, pos, world_revision }) => {
      try {
        const projectId = requireProjectId(project_id);
        return textResult(
          await queryDesktop(projectId, world_revision, {
            type: "get_block",
            dimension,
            pos,
          }),
        );
      } catch (error) {
        return errorResult(error);
      }
    },
  );

  server.registerTool(
    "get_biome",
    {
      title: "Get biome",
      description:
        "Reads the biome covering one block position from the editable 4x4x4 cell grid, with the authoritative world revision. null means the source world has no biome data.",
      inputSchema: z.object({
        ...project,
        dimension: z.string().min(1).max(128),
        pos: blockPos,
      }),
      annotations: readOnly,
    },
    async ({ project_id, dimension, pos, world_revision }) => {
      try {
        const projectId = requireProjectId(project_id);
        return textResult(
          await queryDesktop(projectId, world_revision, {
            type: "get_biome",
            dimension,
            pos,
          }),
        );
      } catch (error) {
        return errorResult(error);
      }
    },
  );

  server.registerTool(
    "inspect_region",
    {
      title: "Inspect region",
      description:
        "Summarizes an inclusive block region: volume, non-air count, palette and the authoritative world revision. Use it before editing an area.",
      inputSchema: z.object({
        ...project,
        dimension: z.string().min(1).max(128),
        bounds,
      }),
      annotations: readOnly,
    },
    async ({ project_id, dimension, bounds, world_revision }) => {
      try {
        const projectId = requireProjectId(project_id);
        return textResult(
          await queryDesktop(projectId, world_revision, {
            type: "region_summary",
            dimension,
            bounds,
          }),
        );
      } catch (error) {
        return errorResult(error);
      }
    },
  );

  server.registerTool(
    "get_region",
    {
      title: "Get region blocks",
      description:
        "Returns the non-air blocks of the requested inclusive region up to the Core read limit, with the authoritative world revision. The result reports the volume so callers can detect an empty box.",
      inputSchema: z.object({
        ...project,
        dimension: z.string().min(1).max(128),
        bounds,
      }),
      annotations: readOnly,
    },
    async ({ project_id, dimension, bounds, world_revision }) => {
      try {
        const projectId = requireProjectId(project_id);
        return textResult(
          await queryDesktop(projectId, world_revision, {
            type: "region_blocks",
            dimension,
            bounds,
          }),
        );
      } catch (error) {
        return errorResult(error);
      }
    },
  );

  server.registerTool(
    "get_camera",
    {
      title: "Get viewport camera",
      description:
        "Returns the current editor camera pose, the dimension it is showing and the render revision. The verification block marks the pose stale if the world advanced.",
      inputSchema: z.object({ ...project }),
      annotations: readOnly,
    },
    async ({ project_id, world_revision }) => {
      try {
        const projectId = requireProjectId(project_id);
        return textResult(
          await callDesktop(
            "get_camera",
            withRevision({ projectId }, world_revision),
            {
              timeoutMs: RENDER_TIMEOUT_MS,
            },
          ),
        );
      } catch (error) {
        return errorResult(error);
      }
    },
  );

  server.registerTool(
    "capture_viewport",
    {
      title: "Capture viewport",
      description:
        "Renders the current editor viewport and returns a real PNG image plus observation metadata. partial=true means requested chunks were not loaded; stale=true means the rendered revision is behind the authoritative world.",
      inputSchema: z.object({
        ...project,
        dimension: z
          .string()
          .min(1)
          .max(128)
          .optional()
          .describe(
            "Expected visible dimension. The capture is refused if the viewport shows another one.",
          ),
      }),
      annotations: readOnly,
    },
    async ({ project_id, dimension, world_revision }) => {
      try {
        const projectId = requireProjectId(project_id);
        const payload: Record<string, unknown> = withRevision(
          { projectId },
          world_revision,
        );
        if (dimension !== undefined) payload.dimension = dimension;
        const result = (await callDesktop("capture_viewport", payload, {
          timeoutMs: RENDER_TIMEOUT_MS,
        })) as Record<string, unknown>;
        const dataUrl = result.dataUrl;
        if (
          typeof dataUrl !== "string" ||
          !dataUrl.startsWith(PNG_DATA_URL_PREFIX)
        ) {
          return {
            isError: true,
            content: [
              {
                type: "text",
                text: "invalid_response: the desktop endpoint did not return a PNG image",
              },
            ],
          };
        }
        const base64 = dataUrl.slice(PNG_DATA_URL_PREFIX.length);
        const imageBytes = Buffer.from(base64, "base64");
        return {
          content: [
            {
              type: "image",
              data: base64,
              mimeType: "image/png",
            },
            {
              type: "text",
              text: JSON.stringify(
                {
                  metadata: result.metadata ?? null,
                  verification: result.verification ?? null,
                  capturedAtMs: result.capturedAtMs ?? null,
                  // The image itself travels in the preceding content block. Publishing
                  // its size and digest lets a text-only client verify what it received.
                  image: {
                    mimeType: "image/png",
                    bytes: imageBytes.byteLength,
                    sha256: createHash("sha256")
                      .update(imageBytes)
                      .digest("hex"),
                  },
                },
                null,
                2,
              ),
            },
          ],
        };
      } catch (error) {
        return errorResult(error);
      }
    },
  );
}
