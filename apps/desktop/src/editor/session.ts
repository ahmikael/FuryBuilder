import type {
  BiomeOperation,
  BlockChange,
  BoxBounds,
  ClipboardPayload,
  EditorOperation,
  EditorTool,
  GenerateRiverOperation,
  GenerateRoadOperation,
  GenerateVegetationOperation,
  HeightmapOperation,
  StructureSiteQuery,
  TerrainBrushKind,
} from "@/editor/types";
import type { BlockPosition, BlockState } from "@/renderer/types";
import type { RayHit } from "@/renderer/raycast";

export const AIR_STATE: BlockState = { name: "minecraft:air", properties: {} };
export const MAX_EDIT_VOLUME = 1_000_000;
export const MIN_BRUSH_RADIUS = 1;
export const MAX_BRUSH_RADIUS = 4;

export const TOOL_SHORTCUTS: Record<string, EditorTool> = {
  Digit1: "select",
  Digit2: "place",
  Digit3: "remove",
  Digit4: "brush",
  Digit5: "box",
  Digit6: "fill",
  Digit7: "line",
  Digit8: "replace",
};

export const TOOL_LABELS: Record<EditorTool, string> = {
  select: "Select",
  place: "Place",
  remove: "Remove",
  brush: "Brush",
  box: "Box select",
  fill: "Fill",
  line: "Line",
  replace: "Replace",
};

export const TERRAIN_BRUSH_LABELS: Record<TerrainBrushKind, string> = {
  raise: "Raise",
  lower: "Lower",
  smooth: "Smooth",
  flatten: "Flatten",
  noise: "Noise",
  erode: "Erode",
  terrace: "Terrace",
};

export const TERRAIN_BRUSH_ORDER: TerrainBrushKind[] = [
  "raise",
  "lower",
  "smooth",
  "flatten",
  "noise",
  "erode",
  "terrace",
];

export function isEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  const tag = target.tagName;
  return (
    tag === "INPUT" ||
    tag === "TEXTAREA" ||
    tag === "SELECT" ||
    target.isContentEditable ||
    target.getAttribute("contenteditable") === "true" ||
    target.getAttribute("contenteditable") === ""
  );
}

export function normalizeBounds(a: BlockPosition, b: BlockPosition): BoxBounds {
  return {
    min: { x: Math.min(a.x, b.x), y: Math.min(a.y, b.y), z: Math.min(a.z, b.z) },
    max: { x: Math.max(a.x, b.x), y: Math.max(a.y, b.y), z: Math.max(a.z, b.z) },
  };
}

export function boundsVolume(bounds: BoxBounds): number {
  return (
    (bounds.max.x - bounds.min.x + 1) *
    (bounds.max.y - bounds.min.y + 1) *
    (bounds.max.z - bounds.min.z + 1)
  );
}

export function boundsCenter(bounds: BoxBounds): [number, number, number] {
  return [
    (bounds.min.x + bounds.max.x + 1) / 2,
    (bounds.min.y + bounds.max.y + 1) / 2,
    (bounds.min.z + bounds.max.z + 1) / 2,
  ];
}

export function boundsFromPositions(positions: BlockPosition[]): BoxBounds | null {
  if (positions.length === 0) return null;
  let min = { ...positions[0]! };
  let max = { ...positions[0]! };
  for (const pos of positions) {
    min = { x: Math.min(min.x, pos.x), y: Math.min(min.y, pos.y), z: Math.min(min.z, pos.z) };
    max = { x: Math.max(max.x, pos.x), y: Math.max(max.y, pos.y), z: Math.max(max.z, pos.z) };
  }
  return { min, max };
}

/** Position adjacent to a picked block along the entered face. */
export function adjacentPosition(pos: BlockPosition, face: readonly number[]): BlockPosition {
  return {
    x: pos.x + (face[0] ?? 0),
    y: pos.y + (face[1] ?? 0),
    z: pos.z + (face[2] ?? 0),
  };
}

export function translateBounds(bounds: BoxBounds, delta: BlockPosition): BoxBounds {
  return {
    min: {
      x: bounds.min.x + delta.x,
      y: bounds.min.y + delta.y,
      z: bounds.min.z + delta.z,
    },
    max: {
      x: bounds.max.x + delta.x,
      y: bounds.max.y + delta.y,
      z: bounds.max.z + delta.z,
    },
  };
}

/**
 * Integer sphere used by the brush. `radius + 0.5` keeps a radius-1 brush at the
 * familiar 19-block ball instead of a 7-block plus sign.
 */
export function spherePositions(center: BlockPosition, radius: number): BlockPosition[] {
  const r = Math.max(MIN_BRUSH_RADIUS, Math.min(MAX_BRUSH_RADIUS, Math.round(radius)));
  const limit = (r + 0.5) * (r + 0.5);
  const positions: BlockPosition[] = [];
  for (let dx = -r; dx <= r; dx += 1) {
    for (let dy = -r; dy <= r; dy += 1) {
      for (let dz = -r; dz <= r; dz += 1) {
        if (dx * dx + dy * dy + dz * dz <= limit) {
          positions.push({ x: center.x + dx, y: center.y + dy, z: center.z + dz });
        }
      }
    }
  }
  return positions;
}

/**
 * Rounded 3D line. Sampling on the dominant axis keeps the line exactly connected for
 * horizontal, vertical and diagonal directions, including negative deltas.
 */
export function linePoints(from: BlockPosition, to: BlockPosition): BlockPosition[] {
  const dx = to.x - from.x;
  const dy = to.y - from.y;
  const dz = to.z - from.z;
  const steps = Math.max(Math.abs(dx), Math.abs(dy), Math.abs(dz));
  if (steps === 0) return [{ ...from }];
  const points: BlockPosition[] = [];
  for (let step = 0; step <= steps; step += 1) {
    const point = {
      x: Math.round(from.x + (dx * step) / steps),
      y: Math.round(from.y + (dy * step) / steps),
      z: Math.round(from.z + (dz * step) / steps),
    };
    const previous = points[points.length - 1];
    if (!previous || previous.x !== point.x || previous.y !== point.y || previous.z !== point.z) {
      points.push(point);
    }
  }
  return points;
}

export function changesFromPositions(positions: BlockPosition[], state: BlockState): BlockChange[] {
  return positions.map((pos) => ({ pos, state }));
}

export function pasteChanges(clipboard: ClipboardPayload, min: BlockPosition): BlockChange[] {
  return clipboard.blocks.map((change) => ({
    pos: {
      x: min.x + change.pos.x,
      y: min.y + change.pos.y,
      z: min.z + change.pos.z,
    },
    state: change.state,
  }));
}

export function operationLabel(operation: EditorOperation): string {
  switch (operation.type) {
    case "set_block":
      return operation.state.name === "minecraft:air" ? "Remove block" : "Place block";
    case "batch":
      return `Edit ${operation.changes.length} block${operation.changes.length === 1 ? "" : "s"}`;
    case "fill_cuboid":
      return `Fill ${boundsVolume(operation.bounds).toLocaleString()} blocks`;
    case "replace":
      return `Replace in ${boundsVolume(operation.bounds).toLocaleString()} blocks`;
    case "terrain_brush":
      return `${TERRAIN_BRUSH_LABELS[operation.kind]} terrain in ${boundsVolume(
        operation.bounds,
      ).toLocaleString()} blocks`;
    case "generate_heightmap":
      return `Generate heightmap in ${boundsVolume(operation.bounds).toLocaleString()} blocks`;
    case "set_biome":
      return `Paint ${operation.biome} over ${boundsVolume(operation.bounds).toLocaleString()} blocks`;
    case "generate_road":
      return `Generate road in ${boundsVolume(operation.bounds).toLocaleString()} blocks`;
    case "generate_river":
      return `Carve river in ${boundsVolume(operation.bounds).toLocaleString()} blocks`;
    case "generate_vegetation":
      return `Scatter vegetation in ${boundsVolume(operation.bounds).toLocaleString()} blocks`;
  }
}

export type GhostKind = "place" | "remove" | "brush" | "box" | "line" | "fill" | "replace";

export interface GhostPreview {
  bounds: BoxBounds;
  kind: GhostKind;
  blocks: number;
  label: string;
}

export interface GhostContext {
  tool: EditorTool;
  hit: RayHit | null;
  anchor: BlockPosition | null;
  selection: BoxBounds | null;
  brushRadius: number;
}

/** Local, synchronous preview geometry for the renderer. Never mutates world data. */
export function ghostForContext(context: GhostContext): GhostPreview | null {
  const { tool, hit, anchor, selection, brushRadius } = context;
  if (tool === "select") return null;

  if (tool === "fill" || tool === "replace") {
    if (!selection) return null;
    const blocks = boundsVolume(selection);
    return {
      bounds: selection,
      kind: tool,
      blocks,
      label: `${tool === "fill" ? "Fill" : "Replace"} ${blocks.toLocaleString()} blocks`,
    };
  }

  if (!hit) return null;
  const target =
    tool === "remove" ? hit.pos : tool === "brush" ? hit.pos : adjacentPosition(hit.pos, hit.face);

  if (tool === "brush") {
    const center = adjacentPosition(hit.pos, hit.face);
    const blocks = spherePositions(center, brushRadius).length;
    return {
      bounds: {
        min: { x: center.x - brushRadius, y: center.y - brushRadius, z: center.z - brushRadius },
        max: { x: center.x + brushRadius, y: center.y + brushRadius, z: center.z + brushRadius },
      },
      kind: "brush",
      blocks,
      label: `Brush ${blocks} blocks`,
    };
  }

  if (tool === "box") {
    const bounds = anchor ? normalizeBounds(anchor, hit.pos) : { min: target, max: target };
    const blocks = boundsVolume(bounds);
    return {
      bounds,
      kind: "box",
      blocks,
      label: anchor ? `Select ${blocks.toLocaleString()} blocks` : "Select first corner",
    };
  }

  if (tool === "line") {
    const end = target;
    const points = anchor ? linePoints(anchor, end) : [end];
    const bounds = boundsFromPositions(points) ?? { min: end, max: end };
    return {
      bounds,
      kind: "line",
      blocks: points.length,
      label: anchor ? `Line ${points.length} blocks` : "Set line start",
    };
  }

  return {
    bounds: { min: target, max: target },
    kind: tool,
    blocks: 1,
    label: tool === "remove" ? "Remove block" : "Place block",
  };
}

export interface BlockToolContext {
  tool: EditorTool;
  dimension: string;
  hit: RayHit | null;
  anchor: BlockPosition | null;
  selection: BoxBounds | null;
  brushRadius: number;
  selected: BlockState;
  replaceFrom: BlockState | null;
}

/**
 * Builds the immutable operation for tools that can be resolved from one click (plus an
 * optional anchor for lines). Box selection is a UI state machine and returns null.
 */
export function planBlockTool(context: BlockToolContext): EditorOperation | null {
  const { tool, dimension, hit, anchor, selection, brushRadius, selected, replaceFrom } = context;
  if (tool === "select" || tool === "box") return null;

  if (tool === "fill") {
    if (!selection) return null;
    return { type: "fill_cuboid", dimension, bounds: selection, state: selected };
  }

  if (tool === "replace") {
    if (!selection || !replaceFrom) return null;
    return {
      type: "replace",
      dimension,
      bounds: selection,
      from: replaceFrom,
      to: selected,
    };
  }

  if (!hit) return null;

  if (tool === "remove") {
    return { type: "set_block", dimension, pos: hit.pos, state: AIR_STATE };
  }

  if (tool === "place") {
    return {
      type: "set_block",
      dimension,
      pos: adjacentPosition(hit.pos, hit.face),
      state: selected,
    };
  }

  if (tool === "brush") {
    const center = adjacentPosition(hit.pos, hit.face);
    return {
      type: "batch",
      dimension,
      changes: changesFromPositions(spherePositions(center, brushRadius), selected),
    };
  }

  if (tool === "line") {
    if (!anchor) return null;
    const end = adjacentPosition(hit.pos, hit.face);
    const changes = changesFromPositions(linePoints(anchor, end), selected);
    if (changes.length === 0) return null;
    return { type: "batch", dimension, changes };
  }

  return null;
}

export const MAX_TERRAIN_BRUSH_AMOUNT = 16;
export const MIN_TERRACE_STEP = 2;
export const MAX_TERRACE_STEP = 16;
/** Vertical air margin so a flat surface selection can both raise and lower terrain. */
export const TERRAIN_BRUSH_VERTICAL_MARGIN = 16;

export interface TerrainBrushContext {
  dimension: string;
  selection: BoxBounds | null;
  kind: TerrainBrushKind;
  material: BlockState;
  amount: number;
  seed: number;
  step: number;
  targetY: number | null;
}

export function clampTerrainAmount(amount: number): number {
  if (!Number.isFinite(amount)) return 1;
  return Math.min(MAX_TERRAIN_BRUSH_AMOUNT, Math.max(1, Math.round(amount)));
}

export function clampTerraceStep(step: number): number {
  if (!Number.isFinite(step)) return 4;
  return Math.min(MAX_TERRACE_STEP, Math.max(MIN_TERRACE_STEP, Math.round(step)));
}

export function clampTerrainSeed(seed: number): number {
  if (!Number.isFinite(seed)) return 0;
  return Math.min(Number.MAX_SAFE_INTEGER, Math.max(0, Math.trunc(seed)));
}

/**
 * Terrain brushes need air above (and below) the selected surface. The UI selection is a
 * horizontal footprint; the operation box adds a bounded vertical margin so raise, lower,
 * smooth and noise can write without escaping the recorded extent.
 */
export function terrainBrushBounds(selection: BoxBounds): BoxBounds {
  return {
    min: { ...selection.min, y: selection.min.y - TERRAIN_BRUSH_VERTICAL_MARGIN },
    max: { ...selection.max, y: selection.max.y + TERRAIN_BRUSH_VERTICAL_MARGIN },
  };
}

/**
 * Builds the typed terrain operation. The Rust core re-validates every parameter and the
 * bounds; this helper only normalizes UI numbers so the request is well-formed.
 */
export function planTerrainBrush(context: TerrainBrushContext): EditorOperation | null {
  if (!context.selection) return null;
  return {
    type: "terrain_brush",
    dimension: context.dimension,
    bounds: terrainBrushBounds(context.selection),
    kind: context.kind,
    material: context.material,
    amount: clampTerrainAmount(context.amount),
    seed: clampTerrainSeed(context.seed),
    step: clampTerraceStep(context.step),
    targetY: context.kind === "flatten" ? context.targetY : null,
  };
}

export const MAX_HEIGHTMAP_SCALE = 64;
export const MAX_HEIGHTMAP_OCTAVES = 6;
export const MAX_HEIGHTMAP_AMPLITUDE = 256;
export const MAX_SURFACE_DEPTH = 32;

export interface HeightmapContext {
  dimension: string;
  selection: BoxBounds | null;
  seed: number;
  scale: number;
  octaves: number;
  amplitude: number;
  /** `null` uses the selection's minimum Y as the base surface. */
  baseY: number | null;
  surfaceDepth: number;
  surface: BlockState;
  filler: BlockState;
}

export function clampHeightmapScale(scale: number): number {
  if (!Number.isFinite(scale)) return 8;
  return Math.min(MAX_HEIGHTMAP_SCALE, Math.max(1, Math.round(scale)));
}

export function clampHeightmapOctaves(octaves: number): number {
  if (!Number.isFinite(octaves)) return 3;
  return Math.min(MAX_HEIGHTMAP_OCTAVES, Math.max(1, Math.round(octaves)));
}

export function clampHeightmapAmplitude(amplitude: number): number {
  if (!Number.isFinite(amplitude)) return 8;
  return Math.min(MAX_HEIGHTMAP_AMPLITUDE, Math.max(1, Math.round(amplitude)));
}

export function clampSurfaceDepth(depth: number): number {
  if (!Number.isFinite(depth)) return 1;
  return Math.min(MAX_SURFACE_DEPTH, Math.max(1, Math.round(depth)));
}

/**
 * The heightmap footprint is the selected X/Z extent; the vertical extent grows to include
 * the requested base surface and full amplitude, so every generated column stays inside the
 * recorded operation bounds.
 */
export function heightmapBounds(selection: BoxBounds, baseY: number, amplitude: number): BoxBounds {
  return {
    min: {
      ...selection.min,
      y: Math.min(selection.min.y, baseY),
    },
    max: {
      ...selection.max,
      y: Math.max(selection.max.y, baseY + amplitude),
    },
  };
}

/**
 * Builds the typed deterministic heightmap operation. Like the brush helper, this only
 * normalizes UI numbers; the Rust core validates ranges, materials and final bounds.
 */
export function planHeightmap(context: HeightmapContext): HeightmapOperation | null {
  if (!context.selection) return null;
  const amplitude = clampHeightmapAmplitude(context.amplitude);
  const baseY = context.baseY ?? context.selection.min.y;
  return {
    type: "generate_heightmap",
    dimension: context.dimension,
    bounds: heightmapBounds(context.selection, baseY, amplitude),
    seed: clampTerrainSeed(context.seed),
    scale: clampHeightmapScale(context.scale),
    octaves: clampHeightmapOctaves(context.octaves),
    amplitude,
    baseY,
    surfaceDepth: clampSurfaceDepth(context.surfaceDepth),
    surface: context.surface,
    filler: context.filler,
  };
}

const BIOME_ID_PATTERN = /^[a-z0-9_.-]+:[a-z0-9_.-]+$/;

/** The Rust reader accepts the same namespaced lowercase grammar. */
export function isValidBiomeId(value: string): boolean {
  return BIOME_ID_PATTERN.test(value.trim());
}

export interface BiomeContext {
  dimension: string;
  selection: BoxBounds | null;
  biome: string;
}

/**
 * Builds the typed biome paint operation. The Rust core verifies that the selection has
 * source biome data and that no cell palette overflows; the UI only normalizes the id.
 */
export function planBiome(context: BiomeContext): BiomeOperation | null {
  if (!context.selection) return null;
  const biome = context.biome.trim();
  if (!isValidBiomeId(biome)) return null;
  return {
    type: "set_biome",
    dimension: context.dimension,
    bounds: context.selection,
    biome,
  };
}

export const MAX_ROUTE_WIDTH = 16;
export const MAX_ROUTE_MEANDER = 64;
export const MAX_ROAD_CLEARANCE = 8;
export const MAX_RIVER_DEPTH = 16;
export const MAX_TREE_SPACING = 32;
export const MAX_STRUCTURE_FOOTPRINT = 32;
export const MAX_STRUCTURE_STEP = 64;
export const MAX_STRUCTURE_CLEARANCE = 32;
export const MAX_STRUCTURE_SLOPE = 32;
export const MAX_STRUCTURE_SITES = 64;
/** Vertical headroom so a flat footprint can hold roads, channels and tree canopies. */
export const GENERATOR_VERTICAL_MARGIN = 16;

export type RouteAxis = "x" | "z";

/**
 * Route and scatter tools need air above the selected surface. The UI selection is a
 * footprint; the operation box adds a bounded vertical margin, and the Rust core clips
 * every write to its own bounds.
 */
export function generatorBounds(selection: BoxBounds): BoxBounds {
  return {
    min: { ...selection.min, y: selection.min.y - GENERATOR_VERTICAL_MARGIN },
    max: { ...selection.max, y: selection.max.y + GENERATOR_VERTICAL_MARGIN },
  };
}

/** Deterministic start/end on the selection's X or Z axis; Y is only a bounds anchor. */
export function routeEndpoints(
  bounds: BoxBounds,
  axis: RouteAxis,
): { from: BlockPosition; to: BlockPosition } {
  if (axis === "x") {
    const z = Math.floor((bounds.min.z + bounds.max.z) / 2);
    return {
      from: { x: bounds.min.x, y: bounds.min.y, z },
      to: { x: bounds.max.x, y: bounds.min.y, z },
    };
  }
  const x = Math.floor((bounds.min.x + bounds.max.x) / 2);
  return {
    from: { x, y: bounds.min.y, z: bounds.min.z },
    to: { x, y: bounds.min.y, z: bounds.max.z },
  };
}

function clampInteger(value: number, min: number, max: number, fallback: number): number {
  if (!Number.isFinite(value)) return fallback;
  return Math.min(max, Math.max(min, Math.round(value)));
}

export function clampRouteWidth(width: number): number {
  return clampInteger(width, 1, MAX_ROUTE_WIDTH, 3);
}

export function clampRouteMeander(meander: number): number {
  return clampInteger(meander, 0, MAX_ROUTE_MEANDER, 4);
}

export function clampRoadClearance(clearance: number): number {
  return clampInteger(clearance, 0, MAX_ROAD_CLEARANCE, 2);
}

export function clampRiverDepth(depth: number): number {
  return clampInteger(depth, 1, MAX_RIVER_DEPTH, 3);
}

export function clampGroundCover(cover: number): number {
  return clampInteger(cover, 0, 100, 35);
}

export function clampTreeChance(chance: number): number {
  return clampInteger(chance, 0, 100, 2);
}

export function clampTreeSpacing(spacing: number): number {
  return clampInteger(spacing, 1, MAX_TREE_SPACING, 5);
}

export function clampStructureFootprint(value: number): number {
  return clampInteger(value, 1, MAX_STRUCTURE_FOOTPRINT, 5);
}

export function clampStructureStep(step: number): number {
  return clampInteger(step, 1, MAX_STRUCTURE_STEP, 5);
}

export function clampStructureSlope(slope: number): number {
  return clampInteger(slope, 0, MAX_STRUCTURE_SLOPE, 2);
}

export function clampStructureClearance(clearance: number): number {
  return clampInteger(clearance, 0, MAX_STRUCTURE_CLEARANCE, 2);
}

export function clampStructureSites(limit: number): number {
  return clampInteger(limit, 1, MAX_STRUCTURE_SITES, 8);
}

export interface RoadContext {
  dimension: string;
  selection: BoxBounds | null;
  axis: RouteAxis;
  seed: number;
  width: number;
  meander: number;
  clearance: number;
  surface: BlockState;
}

export interface RiverContext extends Omit<RoadContext, "clearance" | "surface"> {
  depth: number;
  water: BlockState;
  bed: BlockState;
}

export interface VegetationContext {
  dimension: string;
  selection: BoxBounds | null;
  seed: number;
  groundCover: number;
  treeChance: number;
  treeSpacing: number;
  plant: BlockState;
  trunk: BlockState;
  leaves: BlockState;
}

/** Builds a deterministic road operation; the Rust core re-validates all values. */
export function planRoad(context: RoadContext): GenerateRoadOperation | null {
  if (!context.selection) return null;
  const bounds = generatorBounds(context.selection);
  const { from, to } = routeEndpoints(bounds, context.axis);
  return {
    type: "generate_road",
    dimension: context.dimension,
    bounds,
    from,
    to,
    width: clampRouteWidth(context.width),
    meander: clampRouteMeander(context.meander),
    seed: clampTerrainSeed(context.seed),
    clearance: clampRoadClearance(context.clearance),
    surface: context.surface,
  };
}

/** Builds a deterministic river operation; the Rust core re-validates all values. */
export function planRiver(context: RiverContext): GenerateRiverOperation | null {
  if (!context.selection) return null;
  const bounds = generatorBounds(context.selection);
  const { from, to } = routeEndpoints(bounds, context.axis);
  return {
    type: "generate_river",
    dimension: context.dimension,
    bounds,
    from,
    to,
    width: clampRouteWidth(context.width),
    meander: clampRouteMeander(context.meander),
    seed: clampTerrainSeed(context.seed),
    depth: clampRiverDepth(context.depth),
    water: context.water,
    bed: context.bed,
  };
}

/** Builds a deterministic vegetation scatter; the Rust core re-validates all values. */
export function planVegetation(context: VegetationContext): GenerateVegetationOperation | null {
  if (!context.selection) return null;
  return {
    type: "generate_vegetation",
    dimension: context.dimension,
    bounds: generatorBounds(context.selection),
    seed: clampTerrainSeed(context.seed),
    groundCover: clampGroundCover(context.groundCover),
    treeChance: clampTreeChance(context.treeChance),
    treeSpacing: clampTreeSpacing(context.treeSpacing),
    plant: context.plant,
    trunk: context.trunk,
    leaves: context.leaves,
  };
}

export interface StructureSiteContext {
  dimension: string;
  selection: BoxBounds | null;
  width: number;
  depth: number;
  step: number;
  maxSlope: number;
  clearance: number;
  limit: number;
}

/** A read-only query; it never creates an operation or mutates the world. */
export function planStructureSiteQuery(context: StructureSiteContext): StructureSiteQuery | null {
  if (!context.selection) return null;
  return {
    dimension: context.dimension,
    bounds: generatorBounds(context.selection),
    width: clampStructureFootprint(context.width),
    depth: clampStructureFootprint(context.depth),
    step: clampStructureStep(context.step),
    maxSlope: clampStructureSlope(context.maxSlope),
    clearance: clampStructureClearance(context.clearance),
    limit: clampStructureSites(context.limit),
  };
}

export type ShortcutAction =
  | { kind: "tool"; tool: EditorTool }
  | { kind: "undo" }
  | { kind: "redo" }
  | { kind: "copy" }
  | { kind: "paste" }
  | { kind: "delete-selection" }
  | { kind: "apply-selection" }
  | { kind: "save" }
  | { kind: "cancel" };

/**
 * Keyboard map shared by the hook and tests. Returns null while the user is typing so
 * Ctrl+Z in a coordinate field never edits the world.
 */
export function editorShortcut(event: {
  key: string;
  code: string;
  ctrlKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
  target: EventTarget | null;
}): ShortcutAction | null {
  if (isEditableTarget(event.target)) return null;
  const mod = event.ctrlKey || event.metaKey;
  if (mod && !event.altKey) {
    if (event.code === "KeyZ") return event.shiftKey ? { kind: "redo" } : { kind: "undo" };
    if (event.code === "KeyY") return { kind: "redo" };
    if (event.code === "KeyC") return { kind: "copy" };
    if (event.code === "KeyV") return { kind: "paste" };
    if (event.code === "KeyS") return { kind: "save" };
    return null;
  }
  if (event.altKey) return null;
  if (event.code === "Escape") return { kind: "cancel" };
  if (event.code === "Delete" || event.code === "Backspace") return { kind: "delete-selection" };
  if (event.code === "Enter") return { kind: "apply-selection" };
  const tool = TOOL_SHORTCUTS[event.code];
  if (tool) return { kind: "tool", tool };
  return null;
}
