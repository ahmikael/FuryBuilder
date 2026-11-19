import type { OperationResult } from "@/editor/types";
import type { BlockState, Bounds3 } from "@/renderer/types";

export type ParametricGenerator = "house";
export type ParametricRotation = "none" | "cw90" | "cw180" | "cw270";
export type ParametricConflictPolicy = "fail" | "override";

export interface HouseParams {
  width: number;
  depth: number;
  height: number;
  wall: string;
  floor: string;
  roof: string;
  window?: string | null;
}

export interface ParametricSpec {
  name: string;
  generator: ParametricGenerator;
  generatorVersion: number;
  dimension: string;
  origin: { x: number; y: number; z: number };
  rotation: ParametricRotation;
  params: HouseParams;
  layerId?: string | null;
}

/** UI/MCP metadata; the block baseline stays in the Rust project store. */
export interface ParametricObjectSummary {
  id: string;
  name: string;
  generator: ParametricGenerator;
  generatorVersion: number;
  dimension: string;
  bounds: Bounds3;
  origin: { x: number; y: number; z: number };
  rotation: ParametricRotation;
  params: HouseParams;
  layerId?: string | null;
  appliedRevision: number;
  lastOperationId?: string | null;
  generatedBlocks: number;
}

export interface ParametricConflict {
  pos: { x: number; y: number; z: number };
  expected: BlockState;
  found: BlockState;
}

export interface ParametricInspection {
  object: ParametricObjectSummary;
  generatedBlocks: number;
  conflictCount: number;
  conflicts: ParametricConflict[];
}

export interface ParametricApplyResult {
  object: ParametricObjectSummary;
  operation: OperationResult | null;
  conflicts: ParametricConflict[];
}

export const HOUSE_GENERATOR_VERSION = 1;
export const PARAMETRIC_ROTATIONS: { value: ParametricRotation; label: string }[] = [
  { value: "none", label: "0°" },
  { value: "cw90", label: "90°" },
  { value: "cw180", label: "180°" },
  { value: "cw270", label: "270°" },
];

export function defaultHouseParams(): HouseParams {
  return {
    width: 9,
    depth: 7,
    height: 4,
    wall: "minecraft:stone_bricks",
    floor: "minecraft:oak_planks",
    roof: "minecraft:dark_oak_planks",
    window: "minecraft:glass",
  };
}

export function defaultParametricSpec(): ParametricSpec {
  return {
    name: "New House",
    generator: "house",
    generatorVersion: HOUSE_GENERATOR_VERSION,
    dimension: "minecraft:overworld",
    origin: { x: 0, y: 64, z: 0 },
    rotation: "none",
    params: defaultHouseParams(),
    layerId: null,
  };
}

export function parametricRotationLabel(rotation: ParametricRotation): string {
  return PARAMETRIC_ROTATIONS.find((entry) => entry.value === rotation)?.label ?? rotation;
}
