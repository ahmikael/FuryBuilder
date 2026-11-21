import type * as IpcModule from "@/lib/ipc";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { BackendError } from "@/lib/ipc";
import { LARGE_PASTE_THRESHOLD } from "@/editor/clipboard";
import type { BoxBounds, EditorOperation } from "@/editor/types";
import type { BlockState } from "@/renderer/types";
import { useHistoryStore } from "@/state/history-store";
import { useToolStore } from "@/state/tool-store";
import { clearToasts, getToasts } from "@/test-utils/toast-log";
import { useWorldStore } from "@/state/world-store";
import { viewportBus } from "@/features/viewport/viewport-bus";
import { boundsVolume } from "@/editor/session";

const api = vi.hoisted(() => ({
  preview: vi.fn(),
  apply: vi.fn(),
  info: vi.fn(),
  history: vi.fn(),
  readRegion: vi.fn(),
  undo: vi.fn(),
  redo: vi.fn(),
  startJob: vi.fn(),
  jobStatus: vi.fn(),
  cancelJob: vi.fn(),
}));

const runtime = vi.hoisted(() => ({ tauri: true }));

vi.mock("@/lib/world-api", () => ({
  previewOperation: api.preview,
  applyOperation: api.apply,
  fetchWorldInfo: api.info,
  fetchHistoryState: api.history,
  readRegion: api.readRegion,
  undoOperation: api.undo,
  redoOperation: api.redo,
  startWorldJob: api.startJob,
  worldJobStatus: api.jobStatus,
  cancelWorldJob: api.cancelJob,
  newIdempotencyKey: () => "test-key",
}));

vi.mock("@/lib/ipc", async (importOriginal) => {
  const actual = await importOriginal<typeof IpcModule>();
  return { ...actual, isTauriRuntime: () => runtime.tauri };
});

const {
  applyEditorOperation,
  applyActiveTool,
  cancelInteraction,
  copySelection,
  handleToolClick,
  pasteClipboard,
  redoWorld,
  undoWorld,
} = await import("@/editor/commands");

const stone: BlockState = { name: "minecraft:stone", properties: {} };
const bricks: BlockState = { name: "minecraft:stone_bricks", properties: {} };

const operation: EditorOperation = {
  type: "set_block",
  dimension: "minecraft:overworld",
  pos: { x: 0, y: 64, z: 0 },
  state: bricks,
};

const worldInfo = {
  targetMinecraftVersion: "1.21.4",
  targetDataVersion: 4189,
  revision: 2,
  dimensions: [],
  chunkCount: 9,
  sectionCount: 27,
  blockCount: 1000,
};

function result(overrides: Record<string, unknown> = {}) {
  return {
    operationId: "op-1",
    worldRevision: 2,
    changedBlocks: 1,
    dirtyChunks: [{ x: 0, z: 0 }],
    warnings: [] as string[],
    replayed: false,
    ...overrides,
  };
}

function jobSnapshot(state: string, overrides: Record<string, unknown> = {}) {
  return {
    jobId: "job-1",
    projectId: "local-project",
    kind: "apply_operation",
    state,
    progress: state === "succeeded" ? 100 : 42,
    message: state,
    actorSource: "ui",
    worldRevision: null,
    result: null,
    error: null,
    cancelRequested: false,
    replayed: false,
    createdAtMs: 1,
    startedAtMs: 2,
    finishedAtMs: state === "running" ? null : 3,
    ...overrides,
  };
}

async function flush(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
}

beforeEach(async () => {
  // Let fire-and-forget summary/history refreshes from the previous test settle.
  await new Promise((resolve) => setTimeout(resolve, 0));
  runtime.tauri = true;
  api.preview.mockReset();
  api.apply.mockReset();
  api.info.mockReset();
  api.history.mockReset();
  api.readRegion.mockReset();
  api.undo.mockReset();
  api.redo.mockReset();
  api.startJob.mockReset();
  api.jobStatus.mockReset();
  api.cancelJob.mockReset();
  api.preview.mockResolvedValue({
    affectedBlocks: 1,
    changedBlocks: 1,
    dirtyChunks: [{ x: 0, z: 0 }],
    warnings: [],
  });
  api.apply.mockResolvedValue(result());
  api.info.mockResolvedValue(worldInfo);
  api.history.mockResolvedValue({ canUndo: true, canRedo: false, revision: 2, entries: [] });
  api.startJob.mockResolvedValue(
    jobSnapshot("succeeded", { result: result({ changedBlocks: 4, worldRevision: 3 }) }),
  );
  api.jobStatus.mockResolvedValue(jobSnapshot("succeeded", { result: result() }));
  api.cancelJob.mockResolvedValue(jobSnapshot("cancelled", { progress: 42 }));
  useWorldStore.setState({
    revision: 0,
    blockCount: 0,
    chunkCount: 0,
    sectionCount: 0,
    dimension: "minecraft:overworld",
    status: "ready",
    source: "core",
  });
  useToolStore.setState({
    tool: "place",
    selection: null,
    anchor: null,
    hover: null,
    ghost: null,
    clipboard: null,
    pendingPaste: null,
    job: null,
    syncState: "pristine",
    lastOperation: null,
    replaceFrom: null,
    selectedBlock: bricks,
  });
  useHistoryStore.setState({
    canUndo: false,
    canRedo: false,
    revision: 0,
    entries: [],
    error: null,
  });
  clearToasts();
});

describe("edit commands", () => {
  it("validates, applies, refreshes dirty chunks and records revision", async () => {
    const refresh = vi.fn();
    viewportBus.addEventListener("refresh-chunks", refresh);
    try {
      const applied = await applyEditorOperation(operation);
      expect(applied).toEqual({ revision: 2, changedBlocks: 1 });
      expect(api.preview).toHaveBeenCalledWith(operation);
      expect(api.apply).toHaveBeenCalledWith(operation, 0, undefined);
      expect(useWorldStore.getState().revision).toBe(2);
      expect(useToolStore.getState().syncState).toBe("modified");
      expect(useToolStore.getState().lastOperation).toMatchObject({
        operationId: "op-1",
        changedBlocks: 1,
        revision: 2,
      });
      expect(refresh).toHaveBeenCalledTimes(1);
      expect((refresh.mock.calls[0]![0] as CustomEvent).detail).toEqual([{ x: 0, z: 0 }]);
      await flush();
      expect(api.info).toHaveBeenCalled();
      expect(api.history).toHaveBeenCalled();
      expect(getToasts().some((toast) => toast.kind === "success")).toBe(true);
    } finally {
      viewportBus.removeEventListener("refresh-chunks", refresh);
    }
  });

  it("never dispatches a write when the cancellable preview is cancelled", async () => {
    let resolvePreview: (value: unknown) => void = () => undefined;
    api.preview.mockReturnValue(
      new Promise((resolve) => {
        resolvePreview = resolve;
      }),
    );
    const pending = applyEditorOperation(operation);
    await flush();
    expect(useToolStore.getState().job).toMatchObject({
      cancellable: true,
      cancelRequested: false,
    });
    useToolStore.getState().requestCancel();
    resolvePreview({
      affectedBlocks: 1,
      changedBlocks: 1,
      dirtyChunks: [],
      warnings: [],
    });
    const applied = await pending;
    expect(applied).toBeNull();
    expect(api.apply).not.toHaveBeenCalled();
    expect(useToolStore.getState().job).toBeNull();
    expect(getToasts().map((toast) => toast.title)).toContain("Preview cancelled");
  });

  it("reloads the viewport and warns on a revision conflict", async () => {
    api.apply.mockRejectedValue(
      new BackendError("revision_mismatch", "world revision moved from 0 to 3"),
    );
    const reload = vi.fn();
    viewportBus.addEventListener("reload-world", reload);
    try {
      const applied = await applyEditorOperation(operation);
      expect(applied).toBeNull();
      expect(reload).toHaveBeenCalledTimes(1);
      expect(getToasts().map((toast) => toast.title)).toContain("World changed underneath");
    } finally {
      viewportBus.removeEventListener("reload-world", reload);
    }
  });

  it("requires the desktop core for writes", async () => {
    runtime.tauri = false;
    const applied = await applyEditorOperation(operation);
    expect(applied).toBeNull();
    expect(api.preview).not.toHaveBeenCalled();
    expect(getToasts()[0]).toMatchObject({
      kind: "warning",
      title: "Desktop core required",
    });
  });

  it("refuses fill without a selection and replace without a source", async () => {
    await applyActiveTool("fill");
    expect(api.apply).not.toHaveBeenCalled();
    expect(getToasts()[0]?.title).toBe("Select a box first");

    clearToasts();
    useToolStore.setState({
      selection: { min: { x: 0, y: 64, z: 0 }, max: { x: 1, y: 64, z: 1 } },
    });
    await applyActiveTool("replace");
    expect(api.apply).not.toHaveBeenCalled();
    expect(getToasts()[0]?.title).toBe("No replace source");
  });

  it("applies replace through the operation API with the picked source", async () => {
    useToolStore.setState({
      selection: { min: { x: 0, y: 64, z: 0 }, max: { x: 1, y: 64, z: 1 } },
      replaceFrom: stone,
    });
    await applyActiveTool("replace");
    expect(api.apply).toHaveBeenCalledWith(
      {
        type: "replace",
        dimension: "minecraft:overworld",
        bounds: { min: { x: 0, y: 64, z: 0 }, max: { x: 1, y: 64, z: 1 } },
        from: stone,
        to: bricks,
      },
      0,
      undefined,
    );
  });

  it("rejects replace when source and target are the same block", async () => {
    useToolStore.setState({
      selection: { min: { x: 0, y: 64, z: 0 }, max: { x: 1, y: 64, z: 1 } },
      replaceFrom: bricks,
      selectedBlock: bricks,
    });
    await applyActiveTool("replace");
    expect(api.preview).not.toHaveBeenCalled();
    expect(getToasts()[0]?.title).toBe("Source equals target");
  });
});

describe("tool click state machine", () => {
  const hit = (x: number, y: number, z: number): Parameters<typeof handleToolClick>[0] => ({
    pos: { x, y, z },
    state: stone,
    face: [0, 1, 0],
  });

  it("sets a box selection from two clicks, including negative coordinates", async () => {
    useToolStore.setState({ tool: "box" });
    await handleToolClick(hit(-3, 70, -5));
    expect(useToolStore.getState().anchor).toEqual({ x: -3, y: 70, z: -5 });
    expect(useToolStore.getState().selection).toBeNull();

    await handleToolClick(hit(-1, 64, -2));
    expect(useToolStore.getState().anchor).toBeNull();
    expect(useToolStore.getState().selection).toEqual({
      min: { x: -3, y: 64, z: -5 },
      max: { x: -1, y: 70, z: -2 },
    });
    expect(api.apply).not.toHaveBeenCalled();
    expect(getToasts().at(-1)?.title).toBe("Selected 84 blocks");

    await handleToolClick(hit(0, 65, 0));
    expect(useToolStore.getState().anchor).toEqual({ x: 0, y: 65, z: 0 });
  });

  it("places on the adjacent face and dispatches one atomic operation", async () => {
    useToolStore.setState({ tool: "place", selectedBlock: bricks });
    await handleToolClick(hit(2, 64, -2));
    expect(api.preview).toHaveBeenCalledWith({
      type: "set_block",
      dimension: "minecraft:overworld",
      pos: { x: 2, y: 65, z: -2 },
      state: bricks,
    });
    expect(api.apply).toHaveBeenCalledTimes(1);
  });

  it("builds a line only after two clicks and clears the anchor", async () => {
    useToolStore.setState({ tool: "line", selectedBlock: bricks });
    await handleToolClick(hit(0, 64, 0));
    expect(useToolStore.getState().anchor).toEqual({ x: 0, y: 65, z: 0 });
    expect(api.preview).not.toHaveBeenCalled();

    await handleToolClick(hit(2, 64, 0));
    const plan = api.preview.mock.calls[0]![0];
    expect(plan.type).toBe("batch");
    expect(plan.changes).toHaveLength(3);
    expect(plan.changes[2].pos).toEqual({ x: 2, y: 65, z: 0 });
    expect(useToolStore.getState().anchor).toBeNull();
  });

  it("does not write when a box selection exceeds the operation limit", async () => {
    useToolStore.setState({ tool: "box" });
    await handleToolClick(hit(0, 0, 0));
    await handleToolClick(hit(120, 120, 120));
    expect(useToolStore.getState().selection).toBeNull();
    expect(getToasts().at(-1)?.title).toBe("Selection too large");
  });
});

describe("cancellation", () => {
  it("cancels a pending preview job before clearing an older selection", () => {
    useToolStore.setState({
      job: {
        id: 99,
        label: "Validating paste",
        cancellable: true,
        cancelRequested: false,
        startedAt: Date.now(),
        progress: null,
      },
      selection: { min: { x: 0, y: 64, z: 0 }, max: { x: 1, y: 64, z: 1 } },
    });
    cancelInteraction();
    expect(useToolStore.getState().job?.cancelRequested).toBe(true);
    expect(useToolStore.getState().selection).not.toBeNull();
  });

  it("clears a dangling anchor before a selection", () => {
    useToolStore.setState({
      anchor: { x: 1, y: 65, z: 1 },
      selection: { min: { x: 0, y: 64, z: 0 }, max: { x: 1, y: 64, z: 1 } },
    });
    cancelInteraction();
    expect(useToolStore.getState().anchor).toBeNull();
    expect(useToolStore.getState().selection).not.toBeNull();
  });
});

describe("clipboard commands", () => {
  it("stores a relative structure and cancels the read job", async () => {
    useToolStore.setState({
      selection: { min: { x: 5, y: 65, z: -3 }, max: { x: 6, y: 65, z: -2 } },
    });
    api.readRegion.mockResolvedValue({
      dimension: "minecraft:overworld",
      bounds: { min: { x: 5, y: 65, z: -3 }, max: { x: 6, y: 65, z: -2 } },
      volume: 4,
      blocks: [
        { pos: { x: 5, y: 65, z: -3 }, state: bricks },
        { pos: { x: 6, y: 65, z: -2 }, state: stone },
      ],
    });
    await copySelection();
    const clipboard = useToolStore.getState().clipboard;
    expect(clipboard?.size).toEqual([2, 1, 2]);
    expect(clipboard?.blocks).toEqual([
      { pos: { x: 0, y: 0, z: 0 }, state: bricks },
      { pos: { x: 1, y: 0, z: 1 }, state: stone },
    ]);
  });

  it("pastes at the selection minimum and selects the result", async () => {
    useToolStore.setState({
      selection: { min: { x: 10, y: 70, z: 10 }, max: { x: 11, y: 70, z: 11 } },
      clipboard: {
        dimension: "minecraft:overworld",
        size: [2, 1, 2],
        blocks: [
          { pos: { x: 0, y: 0, z: 0 }, state: bricks },
          { pos: { x: 1, y: 0, z: 1 }, state: stone },
        ],
      },
    });
    await pasteClipboard();
    expect(api.apply.mock.calls[0]![0]).toEqual({
      type: "batch",
      dimension: "minecraft:overworld",
      changes: [
        { pos: { x: 10, y: 70, z: 10 }, state: bricks },
        { pos: { x: 11, y: 70, z: 11 }, state: stone },
      ],
    });
    expect(useToolStore.getState().selection).toEqual({
      min: { x: 10, y: 70, z: 10 },
      max: { x: 11, y: 70, z: 11 },
    });
  });

  it("reads a large selection in bounded tiles and merges relative blocks", async () => {
    const selection: BoxBounds = {
      min: { x: 100, y: 40, z: -20 },
      max: { x: 180, y: 60, z: 28 },
    };
    useToolStore.setState({ selection });
    const tiles: BoxBounds[] = [];
    const progressSeen: Array<number | null> = [];
    api.readRegion.mockImplementation(async (dimension: string, tile: BoxBounds) => {
      progressSeen.push(useToolStore.getState().job?.progress ?? null);
      tiles.push(tile);
      return {
        dimension,
        bounds: tile,
        volume: boundsVolume(tile),
        blocks: [{ pos: { ...tile.min }, state: bricks }],
      };
    });

    await copySelection();

    expect(tiles.length).toBeGreaterThan(1);
    for (const tile of tiles) {
      expect(boundsVolume(tile)).toBeLessThanOrEqual(65_536);
    }
    expect(progressSeen[0]).toBe(0);
    expect(progressSeen.at(-1)!).toBeLessThan(100);
    expect(progressSeen).toEqual([...progressSeen].sort((a, b) => (a ?? 0) - (b ?? 0)));
    const clipboard = useToolStore.getState().clipboard;
    expect(clipboard?.size).toEqual([81, 21, 49]);
    expect(clipboard?.blocks).toHaveLength(tiles.length);
    expect(clipboard?.blocks[0]).toEqual({ pos: { x: 0, y: 0, z: 0 }, state: bricks });
    expect(clipboard?.blocks[1]?.pos).toEqual({
      x: tiles[1]!.min.x - selection.min.x,
      y: tiles[1]!.min.y - selection.min.y,
      z: tiles[1]!.min.z - selection.min.z,
    });
    expect(getToasts().at(-1)?.description).toContain("bounded tiles");
  });

  it("cancels a tiled copy without storing a partial clipboard", async () => {
    useToolStore.setState({
      selection: { min: { x: 0, y: 0, z: 0 }, max: { x: 80, y: 20, z: 48 } },
    });
    api.readRegion.mockImplementation(async (dimension: string, tile: BoxBounds) => {
      useToolStore.getState().requestCancel();
      return { dimension, bounds: tile, volume: boundsVolume(tile), blocks: [] };
    });

    await copySelection();

    expect(api.readRegion).toHaveBeenCalledTimes(1);
    expect(useToolStore.getState().clipboard).toBeNull();
    expect(useToolStore.getState().job).toBeNull();
    expect(getToasts().map((toast) => toast.title)).toContain("Copy cancelled");
  });

  it("previews a large paste before starting a cancellable job", async () => {
    const blocks = Array.from({ length: LARGE_PASTE_THRESHOLD }, (_, index) => ({
      pos: { x: index % 256, y: 0, z: Math.floor(index / 256) },
      state: bricks,
    }));
    useToolStore.setState({
      selection: { min: { x: 2, y: 64, z: 3 }, max: { x: 3, y: 65, z: 4 } },
      clipboard: {
        dimension: "minecraft:overworld",
        size: [256, 1, 256],
        blocks,
      },
    });

    await pasteClipboard();

    expect(useToolStore.getState().pendingPaste).toEqual({
      target: { x: 2, y: 64, z: 3 },
      bounds: { min: { x: 2, y: 64, z: 3 }, max: { x: 257, y: 64, z: 258 } },
      blocks: LARGE_PASTE_THRESHOLD,
    });
    expect(useToolStore.getState().ghost?.kind).toBe("paste");
    expect(api.preview).not.toHaveBeenCalled();
    expect(api.apply).not.toHaveBeenCalled();
    expect(api.startJob).not.toHaveBeenCalled();
    expect(getToasts().at(-1)?.title).toBe("Large paste preview");

    api.preview.mockResolvedValue({
      affectedBlocks: LARGE_PASTE_THRESHOLD,
      changedBlocks: LARGE_PASTE_THRESHOLD,
      dirtyChunks: [{ x: 0, z: 0 }],
      warnings: [],
    });
    api.info.mockResolvedValue({ ...worldInfo, revision: 3 });

    await pasteClipboard();

    expect(useToolStore.getState().pendingPaste).toBeNull();
    expect(useToolStore.getState().ghost).toBeNull();
    expect(api.preview).toHaveBeenCalledTimes(1);
    expect(api.startJob).toHaveBeenCalledTimes(1);
    const [kind, parameters, revision] = api.startJob.mock.calls[0]!;
    expect(kind).toBe("apply_operation");
    expect(revision).toBe(0);
    expect((parameters as { operation: EditorOperation }).operation).toMatchObject({
      type: "batch",
      dimension: "minecraft:overworld",
    });
    expect((parameters as { operation: { changes: unknown[] } }).operation.changes).toHaveLength(
      LARGE_PASTE_THRESHOLD,
    );
    expect(useToolStore.getState().selection).toEqual({
      min: { x: 2, y: 64, z: 3 },
      max: { x: 257, y: 64, z: 258 },
    });
    expect(useWorldStore.getState().revision).toBe(3);
    expect(getToasts().at(-1)?.title).toBe("Large paste applied");
  });

  it("cancels a staged large-paste preview without writing", async () => {
    useToolStore.setState({
      pendingPaste: {
        target: { x: 1, y: 2, z: 3 },
        bounds: { min: { x: 1, y: 2, z: 3 }, max: { x: 9, y: 9, z: 9 } },
        blocks: LARGE_PASTE_THRESHOLD,
      },
      ghost: {
        bounds: { min: { x: 1, y: 2, z: 3 }, max: { x: 9, y: 9, z: 9 } },
        kind: "paste",
        blocks: LARGE_PASTE_THRESHOLD,
        label: "Paste preview",
      },
    });

    cancelInteraction();

    expect(useToolStore.getState().pendingPaste).toBeNull();
    expect(useToolStore.getState().ghost).toBeNull();
    expect(api.startJob).not.toHaveBeenCalled();
    expect(getToasts().at(-1)?.title).toBe("Paste preview cancelled");
  });

  it("does not read when there is no selection", async () => {
    await copySelection();
    expect(api.readRegion).not.toHaveBeenCalled();
    expect(getToasts()[0]?.title).toBe("Select a box to copy.");
  });
});

describe("history commands", () => {
  it("undo refreshes dirty chunks, revision and history", async () => {
    api.undo.mockResolvedValue(
      result({ operationId: "undo-1", changedBlocks: 4, worldRevision: 5 }),
    );
    api.info.mockResolvedValue({ ...worldInfo, revision: 5 });
    const refresh = vi.fn();
    viewportBus.addEventListener("refresh-chunks", refresh);
    try {
      await undoWorld();
      expect(api.undo).toHaveBeenCalledWith();
      expect(useWorldStore.getState().revision).toBe(5);
      expect(refresh).toHaveBeenCalledTimes(1);
      expect(useToolStore.getState().lastOperation?.changedBlocks).toBe(4);
      expect(getToasts()[0]?.title).toBe("Undo applied");
    } finally {
      viewportBus.removeEventListener("refresh-chunks", refresh);
    }
  });

  it("redo reports errors without claiming success", async () => {
    api.redo.mockRejectedValue(new BackendError("no_history", "nothing to redo"));
    await redoWorld();
    expect(getToasts()[0]).toMatchObject({
      kind: "error",
      title: "Redo failed",
    });
    expect(useWorldStore.getState().revision).toBe(0);
  });
});
