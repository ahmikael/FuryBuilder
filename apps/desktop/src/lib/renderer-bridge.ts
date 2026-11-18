import { listen } from "@tauri-apps/api/event";

import { invokeCommand } from "@/lib/ipc";
import type { CaptureResult } from "@/renderer/observation";
import type { CameraPose } from "@/renderer/types";

/**
 * Read-only viewport bridge. The MCP local endpoint asks the running editor for the
 * current camera or a real rendered frame; the UI answers through a Tauri command.
 * It never edits the world and never fabricates an image when the viewport is absent.
 */

export const RENDERER_REQUEST_EVENT = "furybuilder-renderer-request";
export const SUBMIT_RENDERER_RESPONSE_COMMAND = "submit_renderer_response";
export const MARK_RENDERER_READY_COMMAND = "renderer_ready";

export interface RendererRequestPayload {
  requestId: string;
  kind: "camera" | "capture";
  projectId: string;
  dimension: string | null;
}

export interface RendererViewport {
  readonly world: {
    readonly dimension: string;
    readonly revision: number;
  };
  cameraPose(): CameraPose;
  capture(): CaptureResult;
}

export type RendererResponse =
  | {
      kind: "camera";
      dimension: string;
      worldRevision: number;
      pose: CameraPose;
      capturedAtMs: number;
    }
  | {
      kind: "capture";
      dimension: string;
      worldRevision: number;
      dataUrl: string;
      metadata: CaptureResult["metadata"];
      capturedAtMs: number;
    }
  | {
      kind: "error";
      code: string;
      message: string;
    };

/**
 * Builds the answer for one request. A requested dimension that the viewport is not
 * showing is an explicit error: silently rendering another dimension would mislead the
 * caller about what the screenshot contains.
 */
export function buildRendererResponse(
  viewport: RendererViewport,
  request: RendererRequestPayload,
): RendererResponse {
  const dimension = viewport.world.dimension;
  if (request.dimension !== null && request.dimension !== dimension) {
    return {
      kind: "error",
      code: "dimension_mismatch",
      message: `The editor viewport is showing ${dimension}, not ${request.dimension}.`,
    };
  }
  const capturedAtMs = Date.now();
  if (request.kind === "camera") {
    return {
      kind: "camera",
      dimension,
      worldRevision: viewport.world.revision,
      pose: viewport.cameraPose(),
      capturedAtMs,
    };
  }
  const result = viewport.capture();
  return {
    kind: "capture",
    dimension: result.metadata.dimension,
    worldRevision: result.metadata.worldRevision,
    dataUrl: result.dataUrl,
    metadata: result.metadata,
    capturedAtMs,
  };
}

/** Answers a request. Delivery failures are logged but never reported as a fake success. */
export async function respondToRendererRequest(
  viewport: RendererViewport,
  request: RendererRequestPayload,
): Promise<void> {
  let response: RendererResponse;
  try {
    response = buildRendererResponse(viewport, request);
  } catch (error) {
    response = {
      kind: "error",
      code: "capture_failed",
      message:
        error instanceof Error
          ? error.message
          : "The editor viewport could not build the response.",
    };
  }
  try {
    await invokeCommand(SUBMIT_RENDERER_RESPONSE_COMMAND, {
      requestId: request.requestId,
      response,
    });
  } catch (error) {
    console.error("FuryBuilder could not deliver the viewport response", error);
  }
}

/**
 * Subscribes the mounted viewport to local endpoint requests. Outside the Tauri runtime
 * (browser preview or tests) this returns a no-op instead of pretending to be connected.
 */
export async function listenForRendererRequests(viewport: RendererViewport): Promise<() => void> {
  try {
    const stop = await listen<RendererRequestPayload>(RENDERER_REQUEST_EVENT, (event) => {
      void respondToRendererRequest(viewport, event.payload);
    });
    try {
      await invokeCommand(MARK_RENDERER_READY_COMMAND);
    } catch (error) {
      // The listener still works, but `ping` will keep reporting listening=false.
      console.error("FuryBuilder could not report viewport readiness", error);
    }
    return stop;
  } catch (error) {
    console.error("FuryBuilder could not subscribe to viewport requests", error);
    return () => {};
  }
}
