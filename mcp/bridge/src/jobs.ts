/**
 * Contract between the MCP bridge and the authoritative Job API.
 *
 * The bridge must never invent job completion. Until the desktop Application API is
 * connected, the default implementation reports an actionable unavailable error. Job
 * scheduling and execution belong to the Rust Core, not to this transport adapter.
 */

export type JobState =
  "queued" | "running" | "succeeded" | "failed" | "cancelled";

export interface JobSnapshot {
  jobId: string;
  projectId: string;
  state: JobState;
  progress: number;
  message: string;
  worldRevision: number | null;
  result: unknown;
}

export interface StartJobRequest {
  projectId: string;
  kind: string;
  parameters: Record<string, unknown>;
  idempotencyKey: string;
  expectedRevision: number | null;
}

export interface JobApi {
  readonly connected: boolean;
  start(request: StartJobRequest): Promise<JobSnapshot>;
  status(projectId: string, jobId: string): Promise<JobSnapshot>;
  cancel(projectId: string, jobId: string): Promise<JobSnapshot>;
}

export const JOB_API_UNAVAILABLE = "job_api_unavailable";

/** Stable refusal used until the bridge can reach the desktop Job API. */
export class JobApiUnavailableError extends Error {
  readonly code = JOB_API_UNAVAILABLE;

  constructor() {
    super(
      "The FuryBuilder desktop Job API is not connected yet; no job was started, polled or cancelled.",
    );
    this.name = "JobApiUnavailableError";
  }
}

export function createUnavailableJobApi(): JobApi {
  return {
    connected: false,
    async start() {
      throw new JobApiUnavailableError();
    },
    async status() {
      throw new JobApiUnavailableError();
    },
    async cancel() {
      throw new JobApiUnavailableError();
    },
  };
}
