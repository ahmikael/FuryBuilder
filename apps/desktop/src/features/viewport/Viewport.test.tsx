import { render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from "vitest";
import type { RenderSnapshot } from "@/renderer/types";
import { snapshotFromBlocks } from "@/renderer/builders";
import { useWorldStore } from "@/state/world-store";

interface FakeRenderer {
  setSnapshot: Mock;
  setGridVisible: Mock;
  setChunkBordersVisible: Mock;
  resize: Mock;
  dispose: Mock;
  setCameraMode: Mock;
  pickAt: Mock;
  focusOn: Mock;
  capture: Mock;
  updateChunks: Mock;
}

const mocks = vi.hoisted(() => {
  const instances: FakeRenderer[] = [];
  class ViewportRenderer {
    setSnapshot = vi.fn();
    setGridVisible = vi.fn();
    setChunkBordersVisible = vi.fn();
    resize = vi.fn();
    dispose = vi.fn();
    setCameraMode = vi.fn();
    pickAt = vi.fn(() => null);
    focusOn = vi.fn();
    capture = vi.fn(() => ({
      dataUrl: "data:image/png;base64,stub",
      metadata: {},
    }));
    updateChunks = vi.fn();
    constructor() {
      instances.push(this as unknown as FakeRenderer);
    }
  }
  return { instances, ViewportRenderer };
});

const ipc = vi.hoisted(() => ({ tauri: false, invoke: vi.fn() }));

vi.mock("@/renderer/viewport-renderer", () => ({
  ViewportRenderer: mocks.ViewportRenderer,
}));

vi.mock("@/lib/ipc", () => ({
  isTauriRuntime: () => ipc.tauri,
  invokeCommand: ipc.invoke,
}));

import { Viewport } from "@/features/viewport/Viewport";

const snapshot: RenderSnapshot = snapshotFromBlocks(
  [{ x: 0, y: 64, z: 0, name: "minecraft:stone" }],
  1,
);

const worldInfo = {
  targetMinecraftVersion: "1.21.4",
  targetDataVersion: 4189,
  revision: 1,
  dimensions: [],
  chunkCount: 9,
  sectionCount: 27,
  blockCount: 288_000,
};

describe("Viewport", () => {
  beforeEach(() => {
    ipc.tauri = false;
    ipc.invoke.mockReset();
    mocks.instances.length = 0;
    useWorldStore.setState({
      status: "idle",
      source: null,
      error: null,
      revision: 0,
      blockCount: 0,
      chunkCount: 0,
      sectionCount: 0,
      dimension: "minecraft:overworld",
      loadRadius: 1,
      selection: null,
      metrics: null,
      lastCapture: null,
    });
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(
      {} as unknown as WebGL2RenderingContext,
    );
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("loads the Rust demo world and applies the render snapshot", async () => {
    ipc.tauri = true;
    ipc.invoke.mockImplementation(async (command: string) => {
      if (command === "world_info") {
        return { ...worldInfo, blockCount: 0, revision: 0 };
      }
      if (command === "load_demo_world") {
        return worldInfo;
      }
      if (command === "render_snapshot") {
        return snapshot;
      }
      throw new Error(`unexpected command ${command}`);
    });

    render(<Viewport />);

    await waitFor(() => expect(useWorldStore.getState().status).toBe("ready"));
    const renderer = mocks.instances[0]!;
    expect(renderer.setSnapshot).toHaveBeenCalledWith(snapshot);
    expect(ipc.invoke).toHaveBeenCalledWith("render_snapshot", {
      dimension: "minecraft:overworld",
      center: { x: 0, z: 0 },
      radius: 1,
    });
    const state = useWorldStore.getState();
    expect(state.source).toBe("core");
    expect(state.revision).toBe(1);
    expect(state.chunkCount).toBe(9);
    expect(state.blockCount).toBe(288_000);
  });

  it("uses an explicitly labelled preview scene in a plain browser", async () => {
    ipc.tauri = false;
    render(<Viewport />);

    await waitFor(() => expect(useWorldStore.getState().status).toBe("ready"));
    const renderer = mocks.instances[0]!;
    expect(renderer.setSnapshot).toHaveBeenCalledTimes(1);
    const applied = renderer.setSnapshot.mock.calls[0]![0] as RenderSnapshot;
    expect(applied.chunks.length).toBeGreaterThan(0);
    const state = useWorldStore.getState();
    expect(state.source).toBe("preview");
    expect(state.blockCount).toBeGreaterThan(0);
    expect(ipc.invoke).not.toHaveBeenCalled();
  });

  it("disposes the renderer on unmount", async () => {
    ipc.tauri = false;
    const view = render(<Viewport />);
    await waitFor(() => expect(useWorldStore.getState().status).toBe("ready"));
    const renderer = mocks.instances[0]!;
    view.unmount();
    expect(renderer.dispose).toHaveBeenCalledTimes(1);
  });
});
