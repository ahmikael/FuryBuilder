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
          await callDesktop("world_info", withRevision({ projectId }, world_revision)),
        );
      } catch (error) {
        return errorResult(error);
      }
    },
  );

  server.registerTool(
    "get_workspace_policy",
    {
      title: "Get workspace policy",
      description:
        "Returns edit layers, the active layer, the optional active mask and protected regions. Enforcement stays in the Core; locked layers, mask violations and protected regions are returned as permission_denied.",
      inputSchema: z.object({ ...project }),
      annotations: readOnly,
    },
    async ({ project_id, world_revision }) => {
      try {
        const projectId = requireProjectId(project_id);
        return textResult(
          await queryDesktop(projectId, world_revision, { type: "workspace_policy" }),
        );
      } catch (error) {
        return errorResult(error);
      }
    },
  );

  server.registerTool(
    "list_templates",
    {
      title: "List structure templates",
      description:
        "Lists verified reusable Java .nbt templates in the open project's library with size, checksum, non-air block count and creation time. Read-only; insertion uses insert_template with explicit access and confirmation.",
      inputSchema: z.object({ ...project }),
      annotations: readOnly,
    },
    async ({ project_id, world_revision }) => {
      try {
        const projectId = requireProjectId(project_id);
        return textResult(
          await callDesktop("list_templates", withRevision({ projectId }, world_revision)),
        );
      } catch (error) {
        return errorResult(error);
      }
    },
  );

  server.registerTool(
    "list_scene_objects",
    {
      title: "List semantic objects",
      description:
        "Lists the project's editor-only semantic objects (districts, buildings, roads) with their kind, dimension, bounds, parent and tags. These are metadata; reading them never changes blocks.",
      inputSchema: z.object({ ...project }),
      annotations: readOnly,
    },
    async ({ project_id, world_revision }) => {
      try {
        const projectId = requireProjectId(project_id);
        return textResult(await queryDesktop(projectId, world_revision, { type: "scene_objects" }));
      } catch (error) {
        return errorResult(error);
      }
    },
  );

  server.registerTool(
    "inspect_scene_object",
    {
      title: "Inspect semantic object",
      description:
        "Returns one semantic object, its current block palette, volume, non-air count and direct children at the authoritative revision. Read-only; the bounds are project metadata, not a generated structure.",
      inputSchema: z.object({
        ...project,
        object_id: z
          .string()
          .min(1)
          .max(64)
          .describe("Semantic object id from list_scene_objects."),
      }),
      annotations: readOnly,
    },
    async ({ project_id, world_revision, object_id }) => {
      try {
        const projectId = requireProjectId(project_id);
        return textResult(
          await queryDesktop(projectId, world_revision, {
            type: "inspect_scene_object",
            objectId: object_id,
          }),
        );
      } catch (error) {
        return errorResult(error);
      }
    },
  );

  server.registerTool(
    "list_parametric_objects",
    {
      title: "List parametric objects",
      description:
        "Lists editor-only parametric recipes (generator, version, bounds, rotation, parameters, layer and applied revision). These are metadata; the generated voxels are ordinary reversible world content.",
      inputSchema: z.object({ ...project }),
      annotations: readOnly,
    },
    async ({ project_id, world_revision }) => {
      try {
        const projectId = requireProjectId(project_id);
        return textResult(
          await queryDesktop(projectId, world_revision, { type: "parametric_objects" }),
        );
      } catch (error) {
        return errorResult(error);
      }
    },
  );

  server.registerTool(
    "inspect_parametric_object",
    {
      title: "Inspect parametric object",
      description:
        "Returns one parametric recipe plus bounded conflict details: positions where manual edits no longer match the generated overlay. Read-only; transform, bake and rollback are confirmed desktop actions.",
      inputSchema: z.object({
        ...project,
        object_id: z
          .string()
          .min(1)
          .max(64)
          .describe("Parametric object id from list_parametric_objects."),
      }),
      annotations: readOnly,
    },
    async ({ project_id, world_revision, object_id }) => {
      try {
        const projectId = requireProjectId(project_id);
        return textResult(
          await queryDesktop(projectId, world_revision, {
            type: "inspect_parametric_object",
            objectId: object_id,
          }),
        );
      } catch (error) {
        return errorResult(error);
      }
    },
  );

  const houseParams = z.object({
    width: z.number().int().min(3).max(32),
    depth: z.number().int().min(3).max(32),
    height: z.number().int().min(2).max(32),
    wall: z.string().min(1).max(128),
    floor: z.string().min(1).max(128),
    roof: z.string().min(1).max(128),
    window: z.string().min(1).max(128).nullable().optional(),
  });

  const buildNode: z.ZodTypeAny = z.lazy(() =>
    z.discriminatedUnion("type", [
      z.object({
        type: z.literal("house"),
        at: z.tuple([z.number().int(), z.number().int(), z.number().int()]),
        rotation: z.enum(["none", "cw90", "cw180", "cw270"]).default("none"),
        params: houseParams,
      }),
      z.object({
        type: z.literal("repeat"),
        axis: z.enum(["x", "y", "z"]),
        count: z.number().int().min(1).max(16),
        spacing: z.number().int().min(1).max(64),
        child: buildNode,
      }),
    ]),
  );

  const buildRecipe = z.object({
    schemaVersion: z.literal(1).default(1),
    dimension: z.string().min(1).max(128),
    nodes: z.array(buildNode).min(1).max(64),
  });

  server.registerTool(
    "preview_build_dsl",
    {
      title: "Preview Build DSL",
      description:
        "Validates a versioned declarative build recipe, compiles it deterministically into one typed Batch operation and returns the ordinary preview, budget metadata and the exact operation. Read-only: nothing is written. Pass the returned operation to apply_operation (which requires explicit write access and confirmation) to commit it.",
      inputSchema: z.object({
        ...project,
        recipe: buildRecipe,
      }),
      annotations: readOnly,
    },
    async ({ project_id, world_revision, recipe }) => {
      try {
        const projectId = requireProjectId(project_id);
        return textResult(
          await callDesktop(
            "build_dsl_preview",
            withRevision({ projectId, recipe }, world_revision),
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
    "find_structure_sites",
    {
      title: "Find structure sites",
      description:
        "Runs a bounded, read-only placement-rule check over a region and returns non-overlapping flat, dry, clear footprints. This is a deterministic heuristic, not a vanilla structure-generation claim.",
      inputSchema: z.object({
        ...project,
        dimension: z.string().min(1).max(128),
        bounds,
        width: z.number().int().min(1).max(32),
        depth: z.number().int().min(1).max(32),
        step: z.number().int().min(1).max(64).default(5),
        maxSlope: z.number().int().min(0).max(32).default(2),
        clearance: z.number().int().min(0).max(32).default(2),
        limit: z.number().int().min(1).max(64).default(8),
      }),
      annotations: readOnly,
    },
    async ({
      project_id,
      dimension,
      bounds: region,
      width,
      depth,
      step,
      maxSlope,
      clearance,
      limit,
      world_revision,
    }) => {
      try {
        const projectId = requireProjectId(project_id);
        return textResult(
          await queryDesktop(projectId, world_revision, {
            type: "find_structure_sites",
            dimension,
            bounds: region,
            width,
            depth,
            step,
            maxSlope,
            clearance,
            limit,
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
          await callDesktop("get_camera", withRevision({ projectId }, world_revision), {
            timeoutMs: RENDER_TIMEOUT_MS,
          }),
        );
      } catch (error) {
        return errorResult(error);
      }
    },
  );

  server.registerTool(
    "observe",
    {
      title: "Observe region",
      description:
        "Renders a deterministic PNG observation from authoritative voxel data at the given world revision. Modes: top_down, heightmap, material_map, biome_map, cross_section (requires slice) and revision_diff (requires from_revision). Returns the image plus legend, bounds, scale, completeness and warnings. Read-only; exact block queries remain authoritative.",
      inputSchema: z.object({
        ...project,
        dimension: z.string().min(1).max(128),
        bounds,
        mode: z.enum([
          "top_down",
          "heightmap",
          "material_map",
          "biome_map",
          "cross_section",
          "revision_diff",
        ]),
        slice: z
          .object({
            axis: z.enum(["x", "y", "z"]),
            coordinate: z.number().int(),
          })
          .optional()
          .describe("Required for cross_section; ignored by other modes."),
        from_revision: z
          .number()
          .int()
          .nonnegative()
          .optional()
          .describe("Required for revision_diff; the current world revision is the upper bound."),
      }),
      annotations: readOnly,
    },
    async ({ project_id, world_revision, dimension, bounds, mode, slice, from_revision }) => {
      try {
        const projectId = requireProjectId(project_id);
        const observation: Record<string, unknown> = { dimension, bounds, mode };
        if (slice !== undefined) observation.slice = slice;
        if (from_revision !== undefined) observation.fromRevision = from_revision;
        const result = (await callDesktop(
          "observe",
          withRevision({ projectId, observation }, world_revision),
          { timeoutMs: RENDER_TIMEOUT_MS },
        )) as Record<string, unknown>;
        const dataUrl = result.image;
        if (typeof dataUrl !== "string" || !dataUrl.startsWith(PNG_DATA_URL_PREFIX)) {
          return {
            isError: true,
            content: [
              {
                type: "text",
                text: "invalid_response: the desktop endpoint did not return a PNG observation",
              },
            ],
          };
        }
        const base64 = dataUrl.slice(PNG_DATA_URL_PREFIX.length);
        const imageBytes = Buffer.from(base64, "base64");
        const metadata: Record<string, unknown> = { ...result };
        delete metadata.image;
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
                  ...metadata,
                  image: {
                    mimeType: "image/png",
                    bytes: imageBytes.byteLength,
                    sha256: createHash("sha256").update(imageBytes).digest("hex"),
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
        const payload: Record<string, unknown> = withRevision({ projectId }, world_revision);
        if (dimension !== undefined) payload.dimension = dimension;
        const result = (await callDesktop("capture_viewport", payload, {
          timeoutMs: RENDER_TIMEOUT_MS,
        })) as Record<string, unknown>;
        const dataUrl = result.dataUrl;
        if (typeof dataUrl !== "string" || !dataUrl.startsWith(PNG_DATA_URL_PREFIX)) {
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
                    sha256: createHash("sha256").update(imageBytes).digest("hex"),
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
