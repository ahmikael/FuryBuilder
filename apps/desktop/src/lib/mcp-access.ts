import { listen } from "@tauri-apps/api/event";

import { invokeCommand } from "@/lib/ipc";

/**
 * MCP access policy and write approvals. The browser can request a policy change, but
 * only the desktop process enforces it; the MCP bridge itself has no way to widen access.
 */

export type McpAccessMode = "read_only" | "edit_in_selection" | "full_project";

export interface BlockPosition {
  x: number;
  y: number;
  z: number;
}

export interface McpSelection {
  dimension: string;
  bounds: { min: BlockPosition; max: BlockPosition };
}

export interface McpAccessPolicy {
  mode: McpAccessMode;
  selection: McpSelection | null;
  maxBlocks: number;
  maxBatchChanges: number;
  requireConfirmation: boolean;
  writesEnabled: boolean;
}

export interface McpAccessStatus {
  policy: McpAccessPolicy;
  pendingConfirmations: McpConfirmationRequest[];
}

export type McpActivityStatus = "ok" | "denied" | "error";

export interface McpActivityEntry {
  sequence: number;
  requestId: string;
  clientId: string | null;
  method: string;
  status: McpActivityStatus;
  errorCode: string | null;
  projectId: string | null;
  requestedRevision: number | null;
  operationId: string | null;
  changedBlocks: number | null;
  confirmation: string | null;
  timestampMs: number;
  durationMs: number;
}

export interface McpActivityResponse {
  projectId: string;
  entries: McpActivityEntry[];
  limit: number;
  stored: number;
  bounded: boolean;
}

export interface McpAccessInput {
  mode: McpAccessMode;
  selection?: McpSelection | null;
  maxBlocks?: number;
  maxBatchChanges?: number;
  requireConfirmation?: boolean;
}

export interface McpConfirmationRequest {
  requestId: string;
  expiresAtMs: number;
  summary: {
    kind: "apply" | "undo";
    projectId: string;
    worldRevision: number;
    accessMode: McpAccessMode;
    operation?: {
      type: string;
      dimension: string;
      bounds: { min: BlockPosition; max: BlockPosition } | null;
      pos?: BlockPosition;
      state?: string;
      changes?: number;
      sample?: Array<{ pos: BlockPosition; state: string }>;
      sampleTruncated?: boolean;
      from?: string;
      to?: string;
    };
    operationId?: string;
    dimension?: string;
    bounds?: { min: BlockPosition; max: BlockPosition };
    blocks?: number | null;
    affectedBlocks?: number;
    changedBlocks?: number;
    dirtyChunks?: number;
    warnings?: string[];
    idempotencyKey?: string;
  };
}

export const MCP_CONFIRMATION_EVENT = "furybuilder-mcp-confirmation-request";
export const SET_MCP_ACCESS_COMMAND = "set_mcp_access";
export const MCP_ACCESS_STATUS_COMMAND = "mcp_access_status";
export const MCP_ACTIVITY_COMMAND = "mcp_activity";
export const SUBMIT_MCP_CONFIRMATION_COMMAND = "submit_mcp_confirmation";

export const ACCESS_MODE_LABELS: Record<McpAccessMode, string> = {
  read_only: "Read-only",
  edit_in_selection: "Edit in selection",
  full_project: "Full project",
};

export async function getMcpAccessStatus(): Promise<McpAccessStatus> {
  return invokeCommand<McpAccessStatus>(MCP_ACCESS_STATUS_COMMAND);
}

export async function fetchMcpActivity(
  limit = 20,
  operationId?: string,
): Promise<McpActivityResponse> {
  return invokeCommand<McpActivityResponse>(MCP_ACTIVITY_COMMAND, {
    limit,
    ...(operationId ? { operationId } : {}),
  });
}

export async function setMcpAccess(input: McpAccessInput): Promise<McpAccessPolicy> {
  return invokeCommand<McpAccessPolicy>(SET_MCP_ACCESS_COMMAND, { input });
}

export async function submitMcpConfirmation(requestId: string, approved: boolean): Promise<void> {
  await invokeCommand(SUBMIT_MCP_CONFIRMATION_COMMAND, { requestId, approved });
}

/**
 * Subscribes the editor UI to write approvals. Outside the Tauri runtime this returns a
 * no-op; an unmounted listener means writes that need approval fail instead of silently
 * proceeding.
 */
export async function listenForMcpConfirmations(
  handler: (request: McpConfirmationRequest) => void,
): Promise<() => void> {
  try {
    return await listen<McpConfirmationRequest>(MCP_CONFIRMATION_EVENT, (event) => {
      handler(event.payload);
    });
  } catch (error) {
    console.error("FuryBuilder could not subscribe to MCP confirmations", error);
    return () => {};
  }
}

/** Small, readable description of the current policy for status surfaces and tests. */
export function describeAccessPolicy(policy: McpAccessPolicy): string {
  if (policy.mode === "read_only") return "MCP read-only";
  if (policy.mode === "edit_in_selection") {
    const selection = policy.selection;
    if (!selection) return "MCP edit (selection missing)";
    return `MCP edit · ${selection.dimension} · ${selection.bounds.min.x},${selection.bounds.min.y},${selection.bounds.min.z} → ${selection.bounds.max.x},${selection.bounds.max.y},${selection.bounds.max.z}`;
  }
  return "MCP edit · full project";
}
