/** Protocol and request-context facts shared by the MCP bridge and its tests. */

export const BRIDGE_NAME = "furybuilder";
export const BRIDGE_VERSION = "0.1.0";
export const MODERN_PROTOCOL_VERSION = "2026-07-28";

export type ProtocolEra = "modern" | "legacy";

/**
 * Optional protocol extensions are not advertised until they are implemented and tested.
 * Clients must keep working through ordinary tools when they are absent.
 */
export const OPTIONAL_EXTENSIONS = {
  tasks: false,
  apps: false,
} as const;

/**
 * Tasks stays an explicit option: the wire vocabulary is only advertised after the bridge
 * implements it and the authoritative Job API is connected. Until then every call uses
 * the ordinary start/status/cancel tools, which never invent a job or a result.
 */
export const BRIDGE_IMPLEMENTS_TASKS = false;

export type TasksFallbackReason =
  | "client_did_not_declare_tasks"
  | "job_backend_unavailable"
  | "tasks_not_implemented";

export interface TasksSupportDecision {
  mode: "tasks" | "fallback";
  reason: "tasks_negotiated" | TasksFallbackReason;
  extensionAdvertised: boolean;
}

function clientDeclaresTasks(clientCapabilities: unknown): boolean {
  if (typeof clientCapabilities !== "object" || clientCapabilities === null) {
    return false;
  }
  return "tasks" in clientCapabilities;
}

/**
 * Decides whether a caller may use the optional Tasks extension. All three conditions are
 * required, and the fallback is a normal result rather than an error: absence of Tasks is
 * not a failed request.
 */
export function resolveTasksSupport(input: {
  clientCapabilities: unknown;
  jobBackendConnected: boolean;
  bridgeImplementsTasks?: boolean;
}): TasksSupportDecision {
  const implementsTasks =
    input.bridgeImplementsTasks ?? BRIDGE_IMPLEMENTS_TASKS;
  if (!clientDeclaresTasks(input.clientCapabilities)) {
    return {
      mode: "fallback",
      reason: "client_did_not_declare_tasks",
      extensionAdvertised: false,
    };
  }
  if (!implementsTasks) {
    return {
      mode: "fallback",
      reason: "tasks_not_implemented",
      extensionAdvertised: false,
    };
  }
  if (!input.jobBackendConnected) {
    return {
      mode: "fallback",
      reason: "job_backend_unavailable",
      extensionAdvertised: false,
    };
  }
  return {
    mode: "tasks",
    reason: "tasks_negotiated",
    extensionAdvertised: true,
  };
}

export const BRIDGE_INSTRUCTIONS = [
  "FuryBuilder is a local Minecraft Java world editor.",
  "Every call must identify its project explicitly with project_id.",
  "The bridge never keeps a project, selection or job in the transport session.",
  "Pass expected_revision for revision-sensitive reads and writes when the tool supports it.",
  "This bridge does not expose arbitrary filesystem or shell access.",
].join(" ");

/** Rejects empty or unbounded project identifiers before a tool reaches the Core. */
export function requireProjectId(value: string): string {
  const projectId = value.trim();
  if (projectId.length === 0 || projectId.length > 128) {
    throw new Error("project_id must be between 1 and 128 characters");
  }
  return projectId;
}
