import { describe, expect, it } from "vitest";
import {
  adjacentPosition,
  AIR_STATE,
  boundsFromPositions,
  boundsVolume,
  editorShortcut,
  generatorBounds,
  ghostForContext,
  heightmapBounds,
  isValidBiomeId,
  linePoints,
  normalizeBounds,
  operationLabel,
  pasteChanges,
  planBiome,
  planBlockTool,
  planHeightmap,
  planRiver,
  planRoad,
  planStructureSiteQuery,
  planTerrainBrush,
  planVegetation,
  routeEndpoints,
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

describe("heightmap planning", () => {
  const selection = { min: { x: -4, y: 64, z: 2 }, max: { x: 3, y: 70, z: 9 } };
  const dirt: BlockState = { name: "minecraft:dirt", properties: {} };

  it("expands the vertical extent to include the base and full amplitude", () => {
    expect(heightmapBounds(selection, 60, 8)).toEqual({
      min: { x: -4, y: 60, z: 2 },
      max: { x: 3, y: 70, z: 9 },
    });
    expect(heightmapBounds(selection, 68, 16)).toEqual({
      min: { x: -4, y: 64, z: 2 },
      max: { x: 3, y: 84, z: 9 },
    });
  });

  it("requires a box selection and defaults the base to the selection floor", () => {
    expect(
      planHeightmap({
        dimension: "minecraft:overworld",
        selection: null,
        seed: 1,
        scale: 8,
        octaves: 3,
        amplitude: 8,
        baseY: null,
        surfaceDepth: 1,
        surface: stone,
        filler: dirt,
      }),
    ).toBeNull();

    expect(
      planHeightmap({
        dimension: "minecraft:overworld",
        selection,
        seed: 1,
        scale: 8,
        octaves: 3,
        amplitude: 8,
        baseY: null,
        surfaceDepth: 1,
        surface: stone,
        filler: dirt,
      }),
    ).toMatchObject({
      type: "generate_heightmap",
      baseY: 64,
      bounds: heightmapBounds(selection, 64, 8),
    });
  });

  it("normalizes limits without changing the requested materials", () => {
    expect(
      planHeightmap({
        dimension: "minecraft:overworld",
        selection,
        seed: -5,
        scale: 999,
        octaves: 0,
        amplitude: 9999,
        baseY: 60,
        surfaceDepth: 99,
        surface: stone,
        filler: dirt,
      }),
    ).toEqual({
      type: "generate_heightmap",
      dimension: "minecraft:overworld",
      bounds: heightmapBounds(selection, 60, 256),
      seed: 0,
      scale: 64,
      octaves: 1,
      amplitude: 256,
      baseY: 60,
      surfaceDepth: 32,
      surface: stone,
      filler: dirt,
    });
  });

  it("labels the generated patch for jobs and history", () => {
    const operation = planHeightmap({
      dimension: "minecraft:overworld",
      selection,
      seed: 42,
      scale: 8,
      octaves: 3,
      amplitude: 8,
      baseY: 64,
      surfaceDepth: 1,
      surface: stone,
      filler: dirt,
    });
    expect(operation).not.toBeNull();
    expect(operationLabel(operation!)).toBe("Generate heightmap in 576 blocks");
  });
});

describe("biome planning", () => {
  const selection = { min: { x: -4, y: 64, z: 2 }, max: { x: 3, y: 70, z: 9 } };

  it("requires a selection and a namespaced lowercase biome id", () => {
    expect(
      planBiome({ dimension: "minecraft:overworld", selection: null, biome: "minecraft:forest" }),
    ).toBeNull();
    expect(planBiome({ dimension: "minecraft:overworld", selection, biome: "forest" })).toBeNull();
    expect(
      planBiome({ dimension: "minecraft:overworld", selection, biome: "minecraft:Forest" }),
    ).toBeNull();
    expect(
      planBiome({ dimension: "minecraft:overworld", selection, biome: "minecraft:" }),
    ).toBeNull();
    expect(isValidBiomeId(" minecraft:forest ")).toBe(true);
    expect(isValidBiomeId("minecraft:nether_wastes")).toBe(true);
  });

  it("trims the id, keeps the selection and labels the paint", () => {
    const operation = planBiome({
      dimension: "minecraft:overworld",
      selection,
      biome: "  minecraft:desert  ",
    });
    expect(operation).toEqual({
      type: "set_biome",
      dimension: "minecraft:overworld",
      bounds: selection,
      biome: "minecraft:desert",
    });
    expect(operationLabel(operation!)).toBe("Paint minecraft:desert over 448 blocks");
  });
});

describe("local generator planning", () => {
  const selection = { min: { x: 0, y: 64, z: 0 }, max: { x: 7, y: 70, z: 7 } };
  const water: BlockState = { name: "minecraft:water", properties: { level: "0" } };
  const sand: BlockState = { name: "minecraft:sand", properties: {} };
  const grass: BlockState = { name: "minecraft:short_grass", properties: {} };
  const log: BlockState = { name: "minecraft:oak_log", properties: { axis: "y" } };
  const leaves: BlockState = { name: "minecraft:oak_leaves", properties: {} };

  it("expands the selection vertically and derives route endpoints", () => {
    expect(generatorBounds(selection)).toEqual({
      min: { x: 0, y: 48, z: 0 },
      max: { x: 7, y: 86, z: 7 },
    });
    const bounds = generatorBounds(selection);
    expect(routeEndpoints(bounds, "x")).toEqual({
      from: { x: 0, y: 48, z: 3 },
      to: { x: 7, y: 48, z: 3 },
    });
    expect(routeEndpoints(bounds, "z")).toEqual({
      from: { x: 3, y: 48, z: 0 },
      to: { x: 3, y: 48, z: 7 },
    });
  });

  it("builds and labels road, river and vegetation operations", () => {
    const road = planRoad({
      dimension: "minecraft:overworld",
      selection,
      axis: "x",
      seed: 42,
      width: 3,
      meander: 4,
      clearance: 2,
      surface: bricks,
    });
    expect(road).toMatchObject({
      type: "generate_road",
      seed: 42,
      width: 3,
      meander: 4,
      clearance: 2,
      surface: bricks,
    });
    expect(operationLabel(road!)).toBe("Generate road in 2496 blocks");

    const river = planRiver({
      dimension: "minecraft:overworld",
      selection,
      axis: "z",
      seed: 0,
      width: 99,
      meander: 999,
      depth: 99,
      water,
      bed: sand,
    });
    expect(river).toMatchObject({
      type: "generate_river",
      width: 16,
      meander: 64,
      depth: 16,
      water,
      bed: sand,
    });
    expect(operationLabel(river!)).toBe("Carve river in 2496 blocks");

    const vegetation = planVegetation({
      dimension: "minecraft:overworld",
      selection,
      seed: 7,
      groundCover: 500,
      treeChance: -1,
      treeSpacing: 0,
      plant: grass,
      trunk: log,
      leaves,
    });
    expect(vegetation).toMatchObject({
      type: "generate_vegetation",
      groundCover: 100,
      treeChance: 0,
      treeSpacing: 1,
    });
    expect(operationLabel(vegetation!)).toBe("Scatter vegetation in 2496 blocks");
  });

  it("requires a selection for every generator and bounded site query", () => {
    expect(
      planRoad({
        dimension: "minecraft:overworld",
        selection: null,
        axis: "x",
        seed: 0,
        width: 3,
        meander: 4,
        clearance: 2,
        surface: bricks,
      }),
    ).toBeNull();
    expect(
      planStructureSiteQuery({
        dimension: "minecraft:overworld",
        selection: null,
        width: 5,
        depth: 5,
        step: 5,
        maxSlope: 2,
        clearance: 2,
        limit: 8,
      }),
    ).toBeNull();

    expect(
      planStructureSiteQuery({
        dimension: "minecraft:overworld",
        selection,
        width: 99,
        depth: 0,
        step: 999,
        maxSlope: 999,
        clearance: -5,
        limit: 0,
      }),
    ).toEqual({
      dimension: "minecraft:overworld",
      bounds: generatorBounds(selection),
      width: 32,
      depth: 1,
      step: 64,
      maxSlope: 32,
      clearance: 0,
      limit: 1,
    });
  });
});
