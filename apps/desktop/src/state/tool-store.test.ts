import { beforeEach, describe, expect, it } from "vitest";
import { clampBrushRadius, useToolStore } from "@/state/tool-store";

beforeEach(() => {
  useToolStore.setState({
    tool: "place",
    brushRadius: 1,
    anchor: { x: 1, y: 65, z: 1 },
    hover: null,
    ghost: {
      bounds: { min: { x: 1, y: 65, z: 1 }, max: { x: 1, y: 65, z: 1 } },
      kind: "place",
      blocks: 1,
      label: "Place block",
    },
    selection: null,
    clipboard: null,
    job: null,
    lastOperation: null,
    syncState: "pristine",
  });
});

describe("tool store", () => {
  it("clears dangling anchors and ghost geometry when switching tools", () => {
    useToolStore.getState().setTool("box");
    const state = useToolStore.getState();
    expect(state.tool).toBe("box");
    expect(state.anchor).toBeNull();
    expect(state.ghost).toBeNull();
  });

  it("clamps brush radii to the supported range", () => {
    expect(clampBrushRadius(0)).toBe(1);
    expect(clampBrushRadius(2.6)).toBe(3);
    expect(clampBrushRadius(99)).toBe(4);
    expect(clampBrushRadius(Number.NaN)).toBe(1);
    useToolStore.getState().setBrushRadius(10);
    expect(useToolStore.getState().brushRadius).toBe(4);
  });

  it("tracks cancellable jobs and marks edits as modified", () => {
    const id = useToolStore.getState().beginJob("Reading region", true);
    expect(useToolStore.getState().job).toMatchObject({
      id,
      label: "Reading region",
      cancellable: true,
      cancelRequested: false,
    });
    useToolStore.getState().requestCancel();
    expect(useToolStore.getState().job?.cancelRequested).toBe(true);
    useToolStore.getState().finishJob(id);
    expect(useToolStore.getState().job).toBeNull();

    useToolStore.getState().setLastOperation({
      operationId: "op",
      changedBlocks: 3,
      revision: 2,
      warnings: [],
    });
    expect(useToolStore.getState().syncState).toBe("modified");
  });

  it("ignores a stale finishJob call for a newer job", () => {
    const first = useToolStore.getState().beginJob("First");
    const second = useToolStore.getState().beginJob("Second");
    useToolStore.getState().finishJob(first);
    expect(useToolStore.getState().job?.id).toBe(second);
    useToolStore.getState().finishJob(second);
    expect(useToolStore.getState().job).toBeNull();
  });
});
