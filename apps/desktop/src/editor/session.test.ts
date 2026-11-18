import { describe, expect, it } from "vitest";
import {
  adjacentPosition,
  AIR_STATE,
  boundsFromPositions,
  boundsVolume,
  editorShortcut,
  ghostForContext,
  linePoints,
  normalizeBounds,
  operationLabel,
  pasteChanges,
  planBlockTool,
  planTerrainBrush,
  spherePositions,
  terrainBrushBounds,
  translateBounds,
} from "@/editor/session";
import type { BlockChange, ClipboardPayload } from "@/editor/types";
import type { BlockState } from "@/renderer/types";
import type { RayHit } from "@/renderer/raycast";

const stone: BlockState = { name: "minecraft:stone", properties: {} };
const bricks: BlockState = { name: "minecraft:stone_bricks", properties: {} };

function hit(x: number, y: number, z: number, face: [number, number, number] = [0, 1, 0]): RayHit {
  return { pos: { x, y, z }, state: stone, face };
}

describe("editor geometry", () => {
  it("normalizes boxes across negative coordinates", () => {
    const bounds = normalizeBounds({ x: 4, y: 70, z: -3 }, { x: -2, y: 64, z: 8 });
    expect(bounds).toEqual({
      min: { x: -2, y: 64, z: -3 },
      max: { x: 4, y: 70, z: 8 },
    });
    expect(boundsVolume(bounds)).toBe(7 * 7 * 12);
  });

  it("translates inclusive bounds without dropping the maximum edge", () => {
    const shifted = translateBounds(
      { min: { x: -1, y: 0, z: -1 }, max: { x: 1, y: 0, z: 1 } },
      { x: 16, y: 0, z: -16 },
    );
    expect(shifted.min).toEqual({ x: 15, y: 0, z: -17 });
    expect(shifted.max).toEqual({ x: 17, y: 0, z: -15 });
    expect(boundsVolume(shifted)).toBe(9);
  });

  it("returns the position on each face of a picked block", () => {
    const pos = { x: 5, y: 64, z: -5 };
    expect(adjacentPosition(pos, [0, 1, 0])).toEqual({ x: 5, y: 65, z: -5 });
    expect(adjacentPosition(pos, [0, -1, 0])).toEqual({ x: 5, y: 63, z: -5 });
    expect(adjacentPosition(pos, [1, 0, 0])).toEqual({ x: 6, y: 64, z: -5 });
    expect(adjacentPosition(pos, [-1, 0, 0])).toEqual({ x: 4, y: 64, z: -5 });
    expect(adjacentPosition(pos, [0, 0, 1])).toEqual({ x: 5, y: 64, z: -4 });
    expect(adjacentPosition(pos, [0, 0, -1])).toEqual({ x: 5, y: 64, z: -6 });
  });

  it("builds a 19-block radius-1 brush and clamps oversized radii", () => {
    expect(spherePositions({ x: 0, y: 0, z: 0 }, 1)).toHaveLength(19);
    expect(spherePositions({ x: 0, y: 0, z: 0 }, 99)).toHaveLength(
      spherePositions({ x: 0, y: 0, z: 0 }, 4).length,
    );
    const positions = spherePositions({ x: -10, y: 70, z: 3 }, 2);
    expect(positions.every((pos) => Math.abs(pos.z - 3) <= 2)).toBe(true);
  });

  it("walks lines in every direction, including negative diagonals", () => {
    expect(linePoints({ x: 0, y: 64, z: 0 }, { x: 4, y: 64, z: 0 })).toHaveLength(5);
    expect(linePoints({ x: 0, y: 64, z: 0 }, { x: 0, y: 68, z: 0 })).toHaveLength(5);
    const diagonal = linePoints({ x: 2, y: 64, z: 2 }, { x: -2, y: 64, z: -2 });
    expect(diagonal).toEqual([
      { x: 2, y: 64, z: 2 },
      { x: 1, y: 64, z: 1 },
      { x: 0, y: 64, z: 0 },
      { x: -1, y: 64, z: -1 },
      { x: -2, y: 64, z: -2 },
    ]);
    const step = linePoints({ x: 0, y: 64, z: 0 }, { x: 2, y: 66, z: 1 });
    expect(step[0]).toEqual({ x: 0, y: 64, z: 0 });
    expect(step[step.length - 1]).toEqual({ x: 2, y: 66, z: 1 });
    expect(linePoints({ x: 9, y: 9, z: 9 }, { x: 9, y: 9, z: 9 })).toEqual([{ x: 9, y: 9, z: 9 }]);
  });

  it("keeps paste offsets relative and calculates their bounds", () => {
    const clipboard: ClipboardPayload = {
      dimension: "minecraft:overworld",
      size: [2, 1, 2],
      blocks: [
        { pos: { x: 0, y: 0, z: 0 }, state: bricks },
        { pos: { x: 1, y: 0, z: 1 }, state: stone },
      ],
    };
    const changes = pasteChanges(clipboard, { x: -5, y: 70, z: 3 });
    expect(changes[0]!.pos).toEqual({ x: -5, y: 70, z: 3 });
    expect(changes[1]!.pos).toEqual({ x: -4, y: 70, z: 4 });
    expect(boundsFromPositions(changes.map((change) => change.pos))).toEqual({
      min: { x: -5, y: 70, z: 3 },
      max: { x: -4, y: 70, z: 4 },
    });
    expect(boundsFromPositions([])).toBeNull();
  });
});

describe("tool operation planning", () => {
  const base = {
    dimension: "minecraft:overworld",
    brushRadius: 1,
    selected: bricks,
    replaceFrom: null,
    anchor: null,
    selection: null,
  };

  it("places on the picked face and removes the picked block", () => {
    expect(planBlockTool({ ...base, tool: "place", hit: hit(1, 64, -2, [0, 0, -1]) })).toEqual({
      type: "set_block",
      dimension: "minecraft:overworld",
      pos: { x: 1, y: 64, z: -3 },
      state: bricks,
    });
    expect(planBlockTool({ ...base, tool: "remove", hit: hit(1, 64, -2) })).toEqual({
      type: "set_block",
      dimension: "minecraft:overworld",
      pos: { x: 1, y: 64, z: -2 },
      state: AIR_STATE,
    });
  });

  it("creates a batch for the brush and requires no empty anchors", () => {
    const plan = planBlockTool({ ...base, tool: "brush", hit: hit(0, 64, 0) });
    expect(plan?.type).toBe("batch");
    if (plan?.type === "batch") {
      expect(plan.changes).toHaveLength(19);
      expect(plan.changes.every((change) => change.state.name === "minecraft:stone_bricks")).toBe(
        true,
      );
    }
  });

  it("builds a line only after the second click", () => {
    expect(planBlockTool({ ...base, tool: "line", hit: hit(5, 64, 5) })).toBeNull();
    const plan = planBlockTool({
      ...base,
      tool: "line",
      anchor: { x: 0, y: 65, z: 0 },
      hit: hit(3, 64, 0),
    });
    expect(plan?.type).toBe("batch");
    if (plan?.type === "batch") {
      expect(plan.changes).toHaveLength(4);
      expect(plan.changes[0]!.pos).toEqual({ x: 0, y: 65, z: 0 });
      expect(plan.changes[3]!.pos).toEqual({ x: 3, y: 65, z: 0 });
    }
  });

  it("fills and replaces only with a selection and a source", () => {
    const bounds = { min: { x: 0, y: 64, z: 0 }, max: { x: 1, y: 64, z: 1 } };
    expect(planBlockTool({ ...base, tool: "fill", hit: null })).toBeNull();
    expect(planBlockTool({ ...base, tool: "fill", hit: null, selection: bounds })).toEqual({
      type: "fill_cuboid",
      dimension: "minecraft:overworld",
      bounds,
      state: bricks,
    });
    expect(planBlockTool({ ...base, tool: "replace", hit: null, selection: bounds })).toBeNull();
    expect(
      planBlockTool({
        ...base,
        tool: "replace",
        hit: null,
        selection: bounds,
        replaceFrom: stone,
      }),
    ).toEqual({
      type: "replace",
      dimension: "minecraft:overworld",
      bounds,
      from: stone,
      to: bricks,
    });
  });
});

describe("ghost previews", () => {
  it("previews place, remove, brush and line bounds", () => {
    expect(
      ghostForContext({
        tool: "place",
        hit: hit(0, 64, 0),
        anchor: null,
        selection: null,
        brushRadius: 1,
      }),
    ).toMatchObject({
      kind: "place",
      blocks: 1,
      bounds: { min: { x: 0, y: 65, z: 0 }, max: { x: 0, y: 65, z: 0 } },
    });
    expect(
      ghostForContext({
        tool: "remove",
        hit: hit(0, 64, 0),
        anchor: null,
        selection: null,
        brushRadius: 1,
      }),
    ).toMatchObject({ kind: "remove", blocks: 1 });
    expect(
      ghostForContext({
        tool: "brush",
        hit: hit(0, 64, 0),
        anchor: null,
        selection: null,
        brushRadius: 2,
      }),
    ).toMatchObject({ kind: "brush", blocks: 81 });
    expect(
      ghostForContext({
        tool: "line",
        hit: hit(2, 64, 0),
        anchor: { x: -2, y: 65, z: 0 },
        selection: null,
        brushRadius: 1,
      }),
    ).toMatchObject({ kind: "line", blocks: 5 });
    expect(
      ghostForContext({
        tool: "box",
        hit: hit(2, 70, 2),
        anchor: { x: 0, y: 64, z: 0 },
        selection: null,
        brushRadius: 1,
      }),
    ).toMatchObject({
      kind: "box",
      blocks: 3 * 7 * 3,
      bounds: { min: { x: 0, y: 64, z: 0 }, max: { x: 2, y: 70, z: 2 } },
    });
  });

  it("uses the box selection for fill and replace ghosts", () => {
    const selection = { min: { x: 0, y: 64, z: 0 }, max: { x: 3, y: 64, z: 3 } };
    expect(
      ghostForContext({ tool: "fill", hit: null, anchor: null, selection, brushRadius: 1 }),
    ).toMatchObject({ kind: "fill", blocks: 16 });
    expect(
      ghostForContext({ tool: "replace", hit: null, anchor: null, selection, brushRadius: 1 }),
    ).toMatchObject({ kind: "replace", blocks: 16 });
    expect(
      ghostForContext({ tool: "fill", hit: null, anchor: null, selection: null, brushRadius: 1 }),
    ).toBeNull();
  });
});

describe("editor keyboard map", () => {
  const event = (
    code: string,
    init: Partial<Record<string, boolean>> = {},
    target: EventTarget | null = null,
  ) => ({
    key: code,
    code,
    ctrlKey: init.ctrlKey ?? false,
    metaKey: init.metaKey ?? false,
    shiftKey: init.shiftKey ?? false,
    altKey: init.altKey ?? false,
    target,
  });

  it("maps undo and redo for Ctrl and Cmd", () => {
    expect(editorShortcut(event("KeyZ", { ctrlKey: true }))).toEqual({ kind: "undo" });
    expect(editorShortcut(event("KeyZ", { metaKey: true }))).toEqual({ kind: "undo" });
    expect(editorShortcut(event("KeyZ", { ctrlKey: true, shiftKey: true }))).toEqual({
      kind: "redo",
    });
    expect(editorShortcut(event("KeyY", { ctrlKey: true }))).toEqual({ kind: "redo" });
  });

  it("maps tools, clipboard, apply and cancel", () => {
    expect(editorShortcut(event("Digit5"))).toEqual({ kind: "tool", tool: "box" });
    expect(editorShortcut(event("Digit8"))).toEqual({ kind: "tool", tool: "replace" });
    expect(editorShortcut(event("KeyC", { ctrlKey: true }))).toEqual({ kind: "copy" });
    expect(editorShortcut(event("KeyV", { ctrlKey: true }))).toEqual({ kind: "paste" });
    expect(editorShortcut(event("KeyS", { ctrlKey: true }))).toEqual({ kind: "save" });
    expect(editorShortcut(event("KeyS", { metaKey: true }))).toEqual({ kind: "save" });
    expect(editorShortcut(event("Enter"))).toEqual({ kind: "apply-selection" });
    expect(editorShortcut(event("Delete"))).toEqual({ kind: "delete-selection" });
    expect(editorShortcut(event("Escape"))).toEqual({ kind: "cancel" });
  });

  it("never hijacks shortcuts while typing in inputs or contenteditable", () => {
    const input = document.createElement("input");
    const textarea = document.createElement("textarea");
    const editable = document.createElement("div");
    editable.setAttribute("contenteditable", "true");
    for (const target of [input, textarea, editable]) {
      expect(editorShortcut(event("KeyZ", { ctrlKey: true }, target))).toBeNull();
      expect(editorShortcut(event("Digit3", {}, target))).toBeNull();
      expect(editorShortcut(event("Escape", {}, target))).toBeNull();
    }
  });

  it("ignores unrelated combinations", () => {
    expect(editorShortcut(event("KeyA", { ctrlKey: true }))).toBeNull();
    expect(editorShortcut(event("Digit5", { altKey: true }))).toBeNull();
  });
});

describe("clipboard shape", () => {
  it("keeps the block list relative so pasting is dimension-agnostic", () => {
    const changes: BlockChange[] = [
      { pos: { x: 0, y: 0, z: 0 }, state: stone },
      { pos: { x: 1, y: 2, z: 3 }, state: bricks },
    ];
    const clipboard: ClipboardPayload = {
      dimension: "minecraft:overworld",
      size: [2, 3, 4],
      blocks: changes,
    };
    expect(pasteChanges(clipboard, { x: 10, y: 20, z: 30 })[1]!.pos).toEqual({
      x: 11,
      y: 22,
      z: 33,
    });
  });
});

describe("terrain brush planning", () => {
  const bounds = { min: { x: 0, y: 60, z: 0 }, max: { x: 3, y: 70, z: 3 } };
  const brushBounds = { min: { x: 0, y: 44, z: 0 }, max: { x: 3, y: 86, z: 3 } };

  it("expands the selected footprint by a bounded vertical air margin", () => {
    expect(terrainBrushBounds(bounds)).toEqual(brushBounds);
  });

  it("requires a box selection", () => {
    expect(
      planTerrainBrush({
        dimension: "minecraft:overworld",
        selection: null,
        kind: "raise",
        material: stone,
        amount: 1,
        seed: 0,
        step: 4,
        targetY: null,
      }),
    ).toBeNull();
  });

  it("normalizes numeric limits and only sends targetY for flatten", () => {
    expect(
      planTerrainBrush({
        dimension: "minecraft:overworld",
        selection: bounds,
        kind: "noise",
        material: stone,
        amount: 99,
        seed: -5,
        step: 1,
        targetY: 64,
      }),
    ).toEqual({
      type: "terrain_brush",
      dimension: "minecraft:overworld",
      bounds: brushBounds,
      kind: "noise",
      material: stone,
      amount: 16,
      seed: 0,
      step: 2,
      targetY: null,
    });

    expect(
      planTerrainBrush({
        dimension: "minecraft:overworld",
        selection: bounds,
        kind: "flatten",
        material: stone,
        amount: 0,
        seed: 0,
        step: 99,
        targetY: 66,
      }),
    ).toMatchObject({ kind: "flatten", amount: 1, step: 16, targetY: 66 });
  });

  it("labels the reversible operation for jobs and history", () => {
    const operation = planTerrainBrush({
      dimension: "minecraft:overworld",
      selection: { min: { x: 0, y: 60, z: 0 }, max: { x: 2, y: 70, z: 2 } },
      kind: "terrace",
      material: bricks,
      amount: 1,
      seed: 0,
      step: 4,
      targetY: null,
    });
    expect(operation).not.toBeNull();
    expect(operationLabel(operation!)).toBe("Terrace terrain in 387 blocks");
  });
});
