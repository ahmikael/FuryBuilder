import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  buildRendererResponse,
  respondToRendererRequest,
  SUBMIT_RENDERER_RESPONSE_COMMAND,
  type RendererRequestPayload,
  type RendererViewport,
} from "@/lib/renderer-bridge";
import { invokeCommand } from "@/lib/ipc";

vi.mock("@/lib/ipc", () => ({
  invokeCommand: vi.fn().mockResolvedValue(undefined),
}));

const invoke = vi.mocked(invokeCommand);

const camera = {
  position: [1, 2, 3] as [number, number, number],
  target: [0, 0, 0] as [number, number, number],
  yaw: 0.5,
  pitch: -0.2,
  fov: 70,
};

const metadata = {
  schemaVersion: 1,
  mode: "viewport" as const,
  dimension: "minecraft:overworld",
  worldRevision: 4,
  camera,
  resolution: [640, 480] as [number, number],
  renderer: "three-webgl2" as const,
  rendererSchema: 1,
  textureSource: "placeholder" as const,
  legend: [],
  legendTruncated: false,
  requestedChunks: [],
  loadedChunks: [],
  missingChunks: [],
  partial: false,
  capturedAt: "2026-11-18T12:00:00.000Z",
  notes: [],
};

function fakeViewport(): RendererViewport {
  return {
    world: { dimension: "minecraft:overworld", revision: 4 },
    cameraPose: () => camera,
    capture: () => ({
      dataUrl: "data:image/png;base64,iVBORw0KGgo=",
      metadata,
    }),
  };
}

function request(overrides: Partial<RendererRequestPayload> = {}): RendererRequestPayload {
  return {
    requestId: "request-1",
    kind: "camera",
    projectId: "project-1",
    dimension: null,
    ...overrides,
  };
}

beforeEach(() => {
  invoke.mockClear();
});

describe("viewport bridge", () => {
  it("returns the current camera pose with the rendered revision", () => {
    expect(buildRendererResponse(fakeViewport(), request())).toEqual({
      kind: "camera",
      dimension: "minecraft:overworld",
      worldRevision: 4,
      pose: camera,
      capturedAtMs: expect.any(Number),
    });
  });

  it("returns a flattened capture with its real metadata", () => {
    expect(buildRendererResponse(fakeViewport(), request({ kind: "capture" }))).toMatchObject({
      kind: "capture",
      dimension: "minecraft:overworld",
      worldRevision: 4,
      dataUrl: "data:image/png;base64,iVBORw0KGgo=",
      metadata: { partial: false, missingChunks: [] },
    });
  });

  it("refuses to render a dimension the viewport is not showing", () => {
    expect(
      buildRendererResponse(
        fakeViewport(),
        request({ kind: "capture", dimension: "minecraft:the_nether" }),
      ),
    ).toEqual({
      kind: "error",
      code: "dimension_mismatch",
      message: "The editor viewport is showing minecraft:overworld, not minecraft:the_nether.",
    });
  });

  it("reports a capture failure instead of returning a fake image", async () => {
    const viewport = {
      ...fakeViewport(),
      capture: () => {
        throw new Error("WebGL context lost");
      },
    };
    await respondToRendererRequest(viewport, request({ kind: "capture" }));

    expect(invoke).toHaveBeenCalledWith(SUBMIT_RENDERER_RESPONSE_COMMAND, {
      requestId: "request-1",
      response: {
        kind: "error",
        code: "capture_failed",
        message: "WebGL context lost",
      },
    });
  });

  it("delivers successful responses to the authoritative endpoint", async () => {
    await respondToRendererRequest(fakeViewport(), request());

    expect(invoke).toHaveBeenCalledWith(SUBMIT_RENDERER_RESPONSE_COMMAND, {
      requestId: "request-1",
      response: expect.objectContaining({
        kind: "camera",
        dimension: "minecraft:overworld",
        pose: camera,
      }),
    });
  });
});
