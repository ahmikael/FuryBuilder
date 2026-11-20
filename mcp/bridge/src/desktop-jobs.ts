/**
 * Job API backed by the running FuryBuilder desktop process.
 *
 * The bridge only transports requests. Queueing, execution, progress, cancellation and
 * idempotency live in the Rust Core; this adapter validates that the desktop answered with
 * a complete snapshot instead of inventing completion.
 */

import { callDesktop, DesktopUnavailableError } from "./desktop-client.js";
import {
  JobApiError,
  JobApiUnavailableError,
  type JobApi,
  type JobSnapshot,
  type JobState,
  type StartJobRequest,
} from "./jobs.js";

const STATES: ReadonlySet<string> = new Set<JobState>([
  "queued",
  "running",
  "succeeded",
  "failed",
  "cancelled",
]);

const UNAVAILABLE_REASONS: ReadonlySet<string> = new Set([
  "runtime_file_missing",
  "invalid_runtime_file",
  "app_not_running",
  "authentication_failed",
  "protocol_mismatch",
  "timeout",
  "invalid_response",
]);

const START_TIMEOUT_MS = 120_000;
const QUERY_TIMEOUT_MS = 10_000;

function mapDesktopError(error: unknown): never {
  if (error instanceof DesktopUnavailableError) {
    if (UNAVAILABLE_REASONS.has(error.reason)) {
      throw new JobApiUnavailableError(
        `The FuryBuilder desktop Job API is unavailable (${error.reason}): ${error.message}`,
      );
    }
    throw new JobApiError(error.code ?? "desktop_error", error.message);
  }
  throw error;
}

function validateSnapshot(value: unknown): JobSnapshot {
  if (typeof value !== "object" || value === null) {
    throw new JobApiError("invalid_response", "FuryBuilder returned no job snapshot");
  }
  const record = value as Record<string, unknown>;
  if (
    typeof record.jobId !== "string" ||
    typeof record.projectId !== "string" ||
    typeof record.state !== "string" ||
    !STATES.has(record.state) ||
    typeof record.progress !== "number" ||
    !Number.isFinite(record.progress)
  ) {
    throw new JobApiError("invalid_response", "FuryBuilder returned an incomplete job snapshot");
  }
  return value as unknown as JobSnapshot;
}

async function call(
  method: string,
  params: Record<string, unknown>,
  timeoutMs: number,
): Promise<JobSnapshot> {
  try {
    const result = await callDesktop(method, params, { timeoutMs });
    return validateSnapshot(result);
  } catch (error) {
    return mapDesktopError(error);
  }
}

export function createDesktopJobApi(): JobApi {
  return {
    connected: true,
    start(request: StartJobRequest) {
      return call(
        "start_job",
        {
          projectId: request.projectId,
          kind: request.kind,
          parameters: request.parameters,
          idempotencyKey: request.idempotencyKey,
          worldRevision: request.expectedRevision ?? undefined,
        },
        START_TIMEOUT_MS,
      );
    },
    status(projectId: string, jobId: string) {
      return call("job_status", { projectId, jobId }, QUERY_TIMEOUT_MS);
    },
    cancel(projectId: string, jobId: string) {
      return call("cancel_job", { projectId, jobId }, QUERY_TIMEOUT_MS);
    },
  };
}
