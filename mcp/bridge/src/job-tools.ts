import type { CallToolResult } from "@modelcontextprotocol/server";
import type { McpServer } from "@modelcontextprotocol/server";
import * as z from "zod/v4";

import { requireProjectId } from "./capabilities.js";
import {
  JOB_API_UNAVAILABLE,
  JobApiUnavailableError,
  type JobApi,
  type JobSnapshot,
} from "./jobs.js";

const jobId = z.string().min(1).max(128);

const startJobInput = z.object({
  project_id: z.string().min(1).max(128),
  kind: z
    .string()
    .min(1)
    .max(64)
    .describe("Job kind understood by the Application API."),
  parameters: z.record(z.string(), z.unknown()).default({}),
  idempotency_key: z.string().min(1).max(128),
  expected_revision: z.number().int().nonnegative().optional(),
});

const jobQueryInput = z.object({
  project_id: z.string().min(1).max(128),
  job_id: jobId,
});

function errorResult(error: unknown): CallToolResult {
  if (error instanceof JobApiUnavailableError) {
    return {
      isError: true,
      content: [{ type: "text", text: `${error.code}: ${error.message}` }],
    };
  }
  return {
    isError: true,
    content: [{ type: "text", text: `job_error: ${String(error)}` }],
  };
}

function snapshotResult(snapshot: JobSnapshot): CallToolResult {
  return {
    content: [{ type: "text", text: JSON.stringify(snapshot, null, 2) }],
  };
}

/**
 * Registers the capability-independent job tools. They are the required fallback when a
 * client does not negotiate the optional Tasks extension, and they remain authoritative
 * even when Tasks becomes available.
 */
export function registerJobTools(server: McpServer, jobs: JobApi): void {
  server.registerTool(
    "start_job",
    {
      title: "Start a long-running job",
      description:
        "Starts work through the authoritative Job API and returns a job_id. Requires idempotency_key.",
      inputSchema: startJobInput,
      annotations: {
        readOnlyHint: false,
        destructiveHint: false,
        openWorldHint: false,
      },
    },
    async ({
      project_id,
      kind,
      parameters,
      idempotency_key,
      expected_revision,
    }) => {
      try {
        const projectId = requireProjectId(project_id);
        return snapshotResult(
          await jobs.start({
            projectId,
            kind,
            parameters,
            idempotencyKey: idempotency_key,
            expectedRevision: expected_revision ?? null,
          }),
        );
      } catch (error) {
        return errorResult(error);
      }
    },
  );

  server.registerTool(
    "get_job_status",
    {
      title: "Get job status",
      description:
        "Reads progress and the final result of a job from the Job API.",
      inputSchema: jobQueryInput,
      annotations: {
        readOnlyHint: true,
        destructiveHint: false,
        idempotentHint: true,
        openWorldHint: false,
      },
    },
    async ({ project_id, job_id }) => {
      try {
        return snapshotResult(
          await jobs.status(requireProjectId(project_id), job_id),
        );
      } catch (error) {
        return errorResult(error);
      }
    },
  );

  server.registerTool(
    "cancel_job",
    {
      title: "Cancel job",
      description:
        "Requests cancellation through the Job API. Cancellation is not assumed to be instantaneous.",
      inputSchema: jobQueryInput,
      annotations: {
        readOnlyHint: false,
        destructiveHint: false,
        idempotentHint: true,
        openWorldHint: false,
      },
    },
    async ({ project_id, job_id }) => {
      try {
        return snapshotResult(
          await jobs.cancel(requireProjectId(project_id), job_id),
        );
      } catch (error) {
        return errorResult(error);
      }
    },
  );
}

export const JOB_TOOL_NAMES = [
  "start_job",
  "get_job_status",
  "cancel_job",
] as const;
export { JOB_API_UNAVAILABLE };
