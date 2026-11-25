import { render, screen, waitFor, act, fireEvent, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from "vitest";
import type * as IpcModule from "@/lib/ipc";
import type { RenderSnapshot } from "@/renderer/types";
import { snapshotFromBlocks } from "@/renderer/builders";
import { useToolStore } from "@/state/tool-store";
import { useProjectStore } from "@/state/project-store";
import { useWorldStore } from "@/state/world-store";
import { viewportBus } from "@/features/viewport/viewport-bus";

interface FakeRenderer {
  setSnapshot: Mock;
  setGridVisible: Mock;
  setChunkBordersVisible: Mock;
  setSelectionBox: Mock;
  setGhostBox: Mock;
  resize: Mock;
  dispose: Mock;
  setCameraMode: Mock;
  pickAt: Mock;
  hoverAt: Mock;
  focusOn: Mock;
  capture: Mock;
  updateChunks: Mock;
  setAssetBundle: Mock;
}

const mocks = vi.hoisted(() => {
  const instances: FakeRenderer[] = [];
  const uniqueBlockStates = vi.fn();
  class ViewportRenderer {
    setSnapshot = vi.fn();
    world = { uniqueBlockStates };
    setGridVisible = vi.fn();
    setChunkBordersVisible = vi.fn();
    setSelectionBox = vi.fn();
    setGhostBox = vi.fn();
    resize = vi.fn();
    dispose = vi.fn();
    setCameraMode = vi.fn();
    pickAt = vi.fn(() => null);
    hoverAt = vi.fn(() => null);
    focusOn = vi.fn();
    capture = vi.fn(() => ({
      dataUrl: "data:image/png;base64,stub",
      metadata: {},
    }));
    updateChunks = vi.fn();
    setAssetBundle = vi.fn().mockResolvedValue(undefined);
    clearAssetBundle = vi.fn();
    constructor() {
      instances.push(this as unknown as FakeRenderer);
    }
  }
  return { instances, ViewportRenderer, uniqueBlockStates };
});

const ipc = vi.hoisted(() => ({ tauri: false, invoke: vi.fn() }));
const events = vi.hoisted(() => ({ listen: vi.fn().mockResolvedValue(() => undefined) }));

vi.mock("@/renderer/viewport-renderer", () => ({
  ViewportRenderer: mocks.ViewportRenderer,
}));

vi.mock("@/lib/ipc", async (importOriginal) => {
  const actual = await importOriginal<typeof IpcModule>();
  return { ...actual, isTauriRuntime: () => ipc.tauri, invokeCommand: ipc.invoke };
});

vi.mock("@tauri-apps/api/event", () => ({
  listen: events.listen,
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
    events.listen.mockClear();
    mocks.uniqueBlockStates.mockReturnValue([]);
    mocks.instances.length = 0;
    useToolStore.setState({
      tool: "place",
      selection: null,
      anchor: null,
      hover: null,
      ghost: null,
      clipboard: null,
      job: null,
      syncState: "pristine",
      lastOperation: null,
    });
    useProjectStore.setState({
      projects: [],
      warnings: [],
      phase: "ready",
      current: {
        folder: "Test.furybuilder",
        name: "Test",
        savedRevision: 1,
        autosavedRevision: null,
        revision: 1,
        dirty: false,
        editorState: { selectedDimension: "minecraft:overworld", loadRadius: 1 },
        availableCheckpoints: 0,
      },
      recovery: null,
      checkpoints: [],
      lastSave: null,
      loading: false,
      error: null,
      managerOpen: false,
    });
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

  it("refreshes only the active project's packs and ignores an in-flight stale bundle", async () => {
    ipc.tauri = true;
    mocks.uniqueBlockStates.mockReturnValue([{ name: "minecraft:stone", properties: {} }]);
    let resolveInitial: (bundle: { packIssues: string[] }) => void = () => undefined;
    const initial = new Promise<{ packIssues: string[] }>((resolve) => {
      resolveInitial = resolve;
    });
    const bundle = { packIssues: ["Missing pack"] };
    const load = vi.fn().mockReturnValueOnce(initial).mockResolvedValue(bundle);
    ipc.invoke.mockImplementation(async (command: string, args: unknown) => {
      if (command === "world_info") return worldInfo;
      if (command === "render_snapshot") return snapshot;
      if (command === "project_render_bundle") return load(args);
      if (command === "history_state") return { canUndo: false, canRedo: false, entries: [] };
      if (command === "project_status") return useProjectStore.getState().current;
      throw new Error(`unexpected ${command}`);
    });
    render(<Viewport />);
    await waitFor(() =>
      expect(load).toHaveBeenCalledWith({
        folder: "Test.furybuilder",
        blockStates: [{ name: "minecraft:stone", properties: {} }],
      }),
    );
    const subscription = events.listen.mock.calls.find(
      ([event]) => event === "furybuilder-project-packs-changed",
    );
    if (!subscription) throw new Error("missing pack change subscription");
    const signal = (payload: string) =>
      subscription[1]({ event: "furybuilder-project-packs-changed", id: 1, payload });
    act(() => signal("Other.furybuilder"));
    expect(load).toHaveBeenCalledTimes(1);
    act(() => signal("Test.furybuilder"));
    await act(async () => resolveInitial({ packIssues: ["Stale pack"] }));
    expect(await screen.findByText("1 resource pack skipped")).toHaveAttribute(
      "title",
      "Missing pack",
    );
    expect(load).toHaveBeenCalledTimes(2);
    expect(mocks.instances[0]?.setAssetBundle).toHaveBeenCalledExactlyOnceWith(
      bundle,
      expect.any(Function),
    );
  });

  it("loads the Rust world snapshot without inventing a demo scene", async () => {
    ipc.tauri = true;
    ipc.invoke.mockImplementation(async (command: string) => {
      if (command === "world_info") {
        return worldInfo;
      }
      if (command === "render_snapshot") {
        return snapshot;
      }
      if (command === "history_state") {
        return { canUndo: false, canRedo: false, revision: 1, entries: [] };
      }
      if (command === "project_status") {
        return (
          useProjectStore.getState().current ?? {
            folder: null,
            name: null,
            savedRevision: 0,
            autosavedRevision: null,
            revision: 1,
            dirty: false,
            editorState: null,
            availableCheckpoints: 0,
          }
        );
      }
      if (command === "renderer_ready") return undefined;
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
    expect(ipc.invoke).not.toHaveBeenCalledWith("load_demo_world");
    const state = useWorldStore.getState();
    expect(state.source).toBe("core");
    expect(state.revision).toBe(1);
    expect(state.chunkCount).toBe(9);
    expect(state.blockCount).toBe(288_000);
  });

  it("renders nothing and touches no world API without a ready project", async () => {
    ipc.tauri = true;
    useProjectStore.setState({ phase: "preparing", prepareMessage: "Opening…" });

    const { container } = render(<Viewport />);

    expect(container).toBeEmptyDOMElement();
    expect(mocks.instances).toHaveLength(0);
    expect(ipc.invoke).not.toHaveBeenCalled();
    expect(useWorldStore.getState().status).toBe("idle");
  });

  it("disposes the renderer when the session leaves the editor", async () => {
    render(<Viewport />);
    await waitFor(() => expect(useWorldStore.getState().status).toBe("ready"));
    const renderer = mocks.instances[0]!;

    act(() => {
      useProjectStore.setState({ phase: "closed" });
    });

    await waitFor(() => expect(renderer.dispose).toHaveBeenCalledTimes(1));
    expect(useWorldStore.getState().status).toBe("idle");
    expect(useWorldStore.getState().cameraPose).toBeNull();

    const renders = renderer.setSnapshot.mock.calls.length;
    act(() => {
      viewportBus.dispatchEvent(new CustomEvent("reload-world"));
      viewportBus.dispatchEvent(
        new CustomEvent("focus-position", { detail: { x: 0, y: 64, z: 0 } }),
      );
    });
    expect(renderer.setSnapshot).toHaveBeenCalledTimes(renders);
    expect(renderer.focusOn).not.toHaveBeenCalled();
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
    expect(screen.getByRole("button", { name: "Place block" })).toBeDisabled();
  });

  it("maps a viewport click to the Rust operation API and refreshes dirty chunks", async () => {
    ipc.tauri = true;
    const hit = {
      pos: { x: 0, y: 64, z: 0 },
      state: { name: "minecraft:stone", properties: {} },
      face: [0, 1, 0] as [number, number, number],
    };
    const operations: Array<{ command: string; args?: Record<string, unknown> | undefined }> = [];
    let infoCalls = 0;
    ipc.invoke.mockImplementation(async (command: string, args?: Record<string, unknown>) => {
      operations.push({ command, args });
      if (command === "world_info") {
        infoCalls += 1;
        return { ...worldInfo, revision: infoCalls === 1 ? 1 : 2 };
      }
      if (command === "render_snapshot") return snapshot;
      if (command === "history_state") {
        return { canUndo: true, canRedo: false, revision: 1, entries: [] };
      }
      if (command === "project_status") {
        return {
          ...useProjectStore.getState().current,
          revision: 1,
          dirty: false,
        };
      }
      if (command === "preview_operation") {
        return { affectedBlocks: 1, changedBlocks: 1, dirtyChunks: [{ x: 0, z: 0 }], warnings: [] };
      }
      if (command === "apply_operation") {
        return {
          operationId: "op-ui",
          worldRevision: 2,
          changedBlocks: 1,
          dirtyChunks: [{ x: 0, z: 0 }],
          warnings: [],
          replayed: false,
        };
      }
      if (command === "render_chunks") {
        return { dimension: "minecraft:overworld", revision: 2, chunks: [snapshot.chunks[0]] };
      }
      if (command === "renderer_ready") return undefined;
      throw new Error(`unexpected command ${command}`);
    });

    render(<Viewport />);
    await waitFor(() => expect(useWorldStore.getState().status).toBe("ready"));
    const renderer = mocks.instances[0]!;
    renderer.pickAt.mockReturnValue(hit);

    const surface = screen.getByTestId("viewport-canvas");
    fireEvent.pointerDown(surface, { button: 0, clientX: 100, clientY: 100 });
    fireEvent.pointerUp(surface, { button: 0, clientX: 100, clientY: 100 });

    await waitFor(() =>
      expect(operations.some((entry) => entry.command === "apply_operation")).toBe(true),
    );
    const preview = operations.find((entry) => entry.command === "preview_operation");
    const applied = operations.find((entry) => entry.command === "apply_operation");
    expect(preview?.args?.operation).toEqual({
      type: "set_block",
      dimension: "minecraft:overworld",
      pos: { x: 0, y: 65, z: 0 },
      state: { name: "minecraft:stone_bricks", properties: {} },
    });
    expect(applied?.args).toMatchObject({ expectedRevision: 1 });
    await waitFor(() =>
      expect(operations.some((entry) => entry.command === "render_chunks")).toBe(true),
    );
    expect(renderer.updateChunks).toHaveBeenCalled();
    expect(useWorldStore.getState().revision).toBe(2);
  });

  it("opens a context menu and dispatches the chosen tool", async () => {
    ipc.tauri = true;
    const hit = {
      pos: { x: -4, y: 64, z: 3 },
      state: { name: "minecraft:oak_planks", properties: {} },
      face: [0, 1, 0] as [number, number, number],
    };
    const operations: string[] = [];
    ipc.invoke.mockImplementation(async (command: string) => {
      operations.push(command);
      if (command === "world_info") return worldInfo;
      if (command === "render_snapshot") return snapshot;
      if (command === "history_state") {
        return { canUndo: false, canRedo: false, revision: 1, entries: [] };
      }
      if (command === "project_status") {
        return {
          ...useProjectStore.getState().current,
          revision: 1,
          dirty: false,
        };
      }
      if (command === "preview_operation") {
        return { affectedBlocks: 1, changedBlocks: 1, dirtyChunks: [], warnings: [] };
      }
      if (command === "apply_operation") {
        return {
          operationId: "op-menu",
          worldRevision: 2,
          changedBlocks: 1,
          dirtyChunks: [],
          warnings: [],
          replayed: false,
        };
      }
      if (command === "renderer_ready") return undefined;
      throw new Error(`unexpected command ${command}`);
    });

    render(<Viewport />);
    await waitFor(() => expect(useWorldStore.getState().status).toBe("ready"));
    const renderer = mocks.instances[0]!;
    renderer.pickAt.mockReturnValue(hit);
    fireEvent.contextMenu(screen.getByTestId("viewport-canvas"), {
      clientX: 200,
      clientY: 160,
    });

    const menu = screen.getByRole("menu");
    fireEvent.click(within(menu).getByRole("menuitem", { name: /Remove block/ }));

    await waitFor(() =>
      expect(operations.filter((command) => command === "apply_operation")).toHaveLength(1),
    );
    expect(useToolStore.getState().tool).toBe("remove");
    expect(renderer.pickAt).toHaveBeenCalled();
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });

  it("mirrors the box selection and ghost preview into the renderer", async () => {
    ipc.tauri = false;
    render(<Viewport />);
    await waitFor(() => expect(useWorldStore.getState().status).toBe("ready"));
    const renderer = mocks.instances[0]!;
    const selection = { min: { x: 0, y: 64, z: 0 }, max: { x: 3, y: 66, z: 3 } };
    const ghost = {
      bounds: selection,
      kind: "fill" as const,
      blocks: 4 * 3 * 4,
      label: "Fill 48 blocks",
    };
    act(() => {
      useToolStore.setState({ tool: "fill", selection, ghost });
    });
    expect(renderer.setSelectionBox).toHaveBeenLastCalledWith(selection);
    expect(renderer.setGhostBox).toHaveBeenLastCalledWith(selection, "fill");
  });

  it("keeps save status out of the performance panel", async () => {
    ipc.tauri = false;
    render(<Viewport />);
    await waitFor(() => expect(useWorldStore.getState().status).toBe("ready"));

    fireEvent.click(screen.getByRole("button", { name: /Performance/ }));

    expect(screen.queryByText("Save")).not.toBeInTheDocument();
    expect(screen.queryByText(/saved r\d/)).not.toBeInTheDocument();
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
