import { randomUUID } from "node:crypto";

/**
 * Identity of one stdio bridge process. It is published by `bridge_info` and attached to
 * every desktop request so the editor's action journal can attribute tool calls without
 * relying on session state.
 */
export const BRIDGE_INSTANCE_ID = randomUUID();
export const BRIDGE_STARTED_AT_MS = Date.now();
