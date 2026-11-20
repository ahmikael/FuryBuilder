import type { CallToolResult } from "@modelcontextprotocol/server";

import { DesktopUnavailableError } from "./desktop-client.js";

/** Shared MCP result helpers. Errors never include tokens or local paths. */

export function textResult(value: unknown): CallToolResult {
  return {
    content: [{ type: "text", text: JSON.stringify(value, null, 2) }],
  };
}

export function errorResult(error: unknown): CallToolResult {
  if (error instanceof DesktopUnavailableError) {
    const code = error.code ?? error.reason;
    return {
      isError: true,
      content: [{ type: "text", text: `${code}: ${error.message}` }],
    };
  }
  if (error instanceof Error) {
    return {
      isError: true,
      content: [{ type: "text", text: `bridge_error: ${error.message}` }],
    };
  }
  return {
    isError: true,
    content: [{ type: "text", text: "bridge_error: unknown failure" }],
  };
}

/** Omits the revision instead of sending null, which the local endpoint rejects. */
export function withRevision<T extends Record<string, unknown>>(
  payload: T,
  revision: number | undefined,
): T {
  return revision === undefined ? payload : { ...payload, worldRevision: revision };
}
