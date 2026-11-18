/**
 * Explicit request context for every MCP tool call.
 *
 * FuryBuilder never stores a project, selection or world revision in the transport
 * connection. Callers name the project they intend to inspect, and revision-sensitive
 * calls may also pin the revision they observed. A stale revision is a conflict, never a
 * silent read of newer data.
 */

export interface RequestedContext {
  projectId: string;
  worldRevision: number | null;
}

export interface ActiveContext {
  projectId: string;
  worldRevision: number;
}

export type ContextState =
  "ready" | "desktop_unavailable" | "project_mismatch" | "revision_mismatch";

export interface ContextEvaluation {
  state: ContextState;
  valid: boolean;
  code: ContextState;
  message: string;
  requested: RequestedContext;
  active: ActiveContext | null;
}

export function evaluateContext(
  requested: RequestedContext,
  active: ActiveContext | null,
): ContextEvaluation {
  if (active === null) {
    return {
      state: "desktop_unavailable",
      valid: false,
      code: "desktop_unavailable",
      message:
        "The FuryBuilder desktop app is not connected; no world context is available.",
      requested,
      active: null,
    };
  }
  if (requested.projectId !== active.projectId) {
    return {
      state: "project_mismatch",
      valid: false,
      code: "project_mismatch",
      message:
        "project_id does not match the project currently open in FuryBuilder.",
      requested,
      active,
    };
  }
  if (
    requested.worldRevision !== null &&
    requested.worldRevision !== active.worldRevision
  ) {
    return {
      state: "revision_mismatch",
      valid: false,
      code: "revision_mismatch",
      message: `Requested world revision ${requested.worldRevision}, but the open world is at ${active.worldRevision}.`,
      requested,
      active,
    };
  }
  return {
    state: "ready",
    valid: true,
    code: "ready",
    message: "Project and world revision match the open editor.",
    requested,
    active,
  };
}
