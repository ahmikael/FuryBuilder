import type { McpServer } from "@modelcontextprotocol/server";
import * as z from "zod/v4";

import { requireProjectId } from "./capabilities.js";
import { callDesktop } from "./desktop-client.js";
import { errorResult, textResult, withRevision } from "./tool-result.js";

/**
 * Safe write tools. Preview never mutates the world. Apply and undo require an explicit
 * user grant in the running editor; without it the authoritative endpoint refuses the
 * request. The bridge cannot grant itself write access.
 */

const PREVIEW_TIMEOUT_MS = 15_000;
const WRITE_TIMEOUT_MS = 120_000;
const MAX_BATCH_CHANGES = 8192;

const dimension = z
  .string()
  .min(1)
  .max(128)
  .describe("Dimension id, for example minecraft:overworld.");

const blockPos = z.object({
  x: z.number().int(),
  y: z.number().int(),
  z: z.number().int(),
});

const bounds = z.object({
  min: blockPos,
  max: blockPos,
});

const blockState = z.object({
  name: z.string().min(1).max(128),
  properties: z.record(z.string(), z.string()).default({}),
});

const change = z.object({ pos: blockPos, state: blockState });

const operation = z.discriminatedUnion("type", [
  z.object({
    type: z.literal("set_block"),
    dimension,
    pos: blockPos,
    state: blockState,
  }),
  z.object({
    type: z.literal("batch"),
    dimension,
    changes: z.array(change).min(1).max(MAX_BATCH_CHANGES),
  }),
  z.object({
    type: z.literal("fill_cuboid"),
    dimension,
    bounds,
    state: blockState,
  }),
  z.object({
    type: z.literal("replace"),
    dimension,
    bounds,
    from: blockState,
    to: blockState,
  }),
  z.object({
    type: z.literal("terrain_brush"),
    dimension,
    bounds,
    kind: z.enum([
      "raise",
      "lower",
      "smooth",
      "flatten",
      "noise",
      "erode",
      "terrace",
    ]),
    material: blockState,
    amount: z.number().int().min(1).max(16).default(1),
    seed: z
      .number()
      .int()
      .nonnegative()
      .max(Number.MAX_SAFE_INTEGER)
      .default(0),
    step: z.number().int().min(2).max(16).default(4),
    targetY: z.number().int().optional(),
  }),
  z.object({
    type: z.literal("generate_heightmap"),
    dimension,
    bounds,
    seed: z
      .number()
      .int()
      .nonnegative()
      .max(Number.MAX_SAFE_INTEGER)
      .default(0),
    scale: z.number().int().min(1).max(64).default(8),
    octaves: z.number().int().min(1).max(6).default(3),
    amplitude: z.number().int().min(1).max(256).default(8),
    baseY: z.number().int(),
    surfaceDepth: z.number().int().min(1).max(32).default(1),
    surface: blockState,
    filler: blockState,
  }),
  z.object({
    type: z.literal("set_biome"),
    dimension,
    bounds,
    biome: z
      .string()
      .min(1)
      .max(128)
      .describe(
        "Namespaced lowercase biome id. The core refuses worlds without source biome data and never invents a default.",
      ),
  }),
]);

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
      "Revision the caller observed. Apply/undo fail with revision_mismatch if the world moved on.",
    ),
};

/** Writes require the exact revision the caller previewed, so stale edits are refused. */
const writeProject = {
  project_id: project.project_id,
  world_revision: z
    .number()
    .int()
    .nonnegative()
    .describe(
      "Revision the caller previewed; the write fails if the world moved on.",
    ),
};

const idempotencyKey = z
  .string()
  .min(1)
  .max(128)
  .describe(
    "Unique key for this intended write. Repeating it with the same payload replays the result instead of applying twice.",
  );

const readOnly = {
  readOnlyHint: true,
  destructiveHint: false,
  idempotentHint: true,
  openWorldHint: false,
} as const;

const modifiesWorld = {
  readOnlyHint: false,
  destructiveHint: true,
  idempotentHint: false,
  openWorldHint: false,
} as const;

export function registerWriteTools(server: McpServer): void {
  server.registerTool(
    "preview_operation",
    {
      title: "Preview operation",
      description:
        "Validates a typed operation and reports affected/changed blocks, dirty chunks and warnings without modifying the world. Always preview before applying a large change.",
      inputSchema: z.object({
        ...project,
        operation,
      }),
      annotations: readOnly,
    },
    async ({ project_id, operation: requested, world_revision }) => {
      try {
        const projectId = requireProjectId(project_id);
        return textResult(
          await callDesktop(
            "preview_operation",
            withRevision({ projectId, operation: requested }, world_revision),
            { timeoutMs: PREVIEW_TIMEOUT_MS },
          ),
        );
      } catch (error) {
        return errorResult(error);
      }
    },
  );

  server.registerTool(
    "apply_operation",
    {
      title: "Apply operation",
      description:
        "Applies one typed, reversible operation. Requires explicit MCP write access in FuryBuilder; otherwise returns permission_denied. Use preview_operation first and send world_revision plus a unique idempotency_key.",
      inputSchema: z.object({
        ...writeProject,
        operation,
        idempotency_key: idempotencyKey,
      }),
      annotations: modifiesWorld,
    },
    async ({
      project_id,
      operation: requested,
      world_revision,
      idempotency_key,
    }) => {
      try {
        const projectId = requireProjectId(project_id);
        return textResult(
          await callDesktop(
            "apply_operation",
            withRevision(
              {
                projectId,
                operation: requested,
                idempotencyKey: idempotency_key,
              },
              world_revision,
            ),
            { timeoutMs: WRITE_TIMEOUT_MS },
          ),
        );
      } catch (error) {
        return errorResult(error);
      }
    },
  );

  server.registerTool(
    "apply_operations_batch",
    {
      title: "Apply block batch",
      description: `Atomically applies up to ${MAX_BATCH_CHANGES} block changes in one dimension. Requires explicit MCP write access; preview first and reuse the same idempotency_key when retrying.`,
      inputSchema: z.object({
        ...writeProject,
        dimension,
        changes: z.array(change).min(1).max(MAX_BATCH_CHANGES),
        idempotency_key: idempotencyKey,
      }),
      annotations: modifiesWorld,
    },
    async ({
      project_id,
      dimension: targetDimension,
      changes,
      world_revision,
      idempotency_key,
    }) => {
      try {
        const projectId = requireProjectId(project_id);
        return textResult(
          await callDesktop(
            "apply_operation",
            withRevision(
              {
                projectId,
                operation: {
                  type: "batch",
                  dimension: targetDimension,
                  changes,
                },
                idempotencyKey: idempotency_key,
              },
              world_revision,
            ),
            { timeoutMs: WRITE_TIMEOUT_MS },
          ),
        );
      } catch (error) {
        return errorResult(error);
      }
    },
  );

  server.registerTool(
    "undo_operation",
    {
      title: "Undo operation",
      description:
        "Undoes the latest operation, or the named operation_id when it is still the latest. Requires explicit MCP write access. The response reports the new revision and dirty chunks.",
      inputSchema: z.object({
        ...writeProject,
        operation_id: z.string().min(1).max(128).optional(),
      }),
      annotations: modifiesWorld,
    },
    async ({ project_id, world_revision, operation_id }) => {
      try {
        const projectId = requireProjectId(project_id);
        const payload: Record<string, unknown> = withRevision(
          { projectId },
          world_revision,
        );
        if (operation_id !== undefined) payload.operationId = operation_id;
        return textResult(
          await callDesktop("undo_operation", payload, {
            timeoutMs: WRITE_TIMEOUT_MS,
          }),
        );
      } catch (error) {
        return errorResult(error);
      }
    },
  );

  server.registerTool(
    "get_operation_status",
    {
      title: "Get operation status",
      description:
        "Reads the authoritative status of one operation_id, or a bounded recent-operation summary. It also reports the job queue and, for a named operation, the retained job that produced it. Long-running work itself goes through the start_job/get_job_status/cancel_job tools.",
      inputSchema: z.object({
        project_id: project.project_id,
        operation_id: z.string().min(1).max(128).optional(),
      }),
      annotations: readOnly,
    },
    async ({ project_id, operation_id }) => {
      try {
        const projectId = requireProjectId(project_id);
        const payload: Record<string, unknown> = { projectId };
        if (operation_id !== undefined) payload.operationId = operation_id;
        return textResult(await callDesktop("operation_status", payload));
      } catch (error) {
        return errorResult(error);
      }
    },
  );
}
