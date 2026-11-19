import type { EditorOperation, PreviewResult } from "@/editor/types";
import type { HouseParams, ParametricRotation } from "@/project/parametric-types";
import type { Bounds3 } from "@/renderer/types";

export type BuildRepeatAxis = "x" | "y" | "z";

export interface BuildHouseNode {
  type: "house";
  at: [number, number, number];
  rotation: ParametricRotation;
  params: HouseParams;
}

export interface BuildRepeatNode {
  type: "repeat";
  axis: BuildRepeatAxis;
  count: number;
  spacing: number;
  child: BuildNode;
}

export type BuildNode = BuildHouseNode | BuildRepeatNode;

/** Versioned data-only recipe. The Core validates it; this type is not an interpreter. */
export interface BuildRecipe {
  schemaVersion: 1;
  dimension: string;
  nodes: BuildNode[];
}

export interface BuildDslSummary {
  dimension: string;
  bounds: Bounds3;
  houses: number;
  blocks: number;
  warnings: string[];
}

export interface BuildDslPreview {
  summary: BuildDslSummary;
  operation: EditorOperation;
  preview: PreviewResult;
  worldRevision?: number;
}

export function defaultBuildRecipe(): BuildRecipe {
  return {
    schemaVersion: 1,
    dimension: "minecraft:overworld",
    nodes: [
      {
        type: "repeat",
        axis: "x",
        count: 3,
        spacing: 8,
        child: {
          type: "house",
          at: [0, 64, 0],
          rotation: "none",
          params: {
            width: 6,
            depth: 5,
            height: 3,
            wall: "minecraft:stone_bricks",
            floor: "minecraft:oak_planks",
            roof: "minecraft:dark_oak_planks",
            window: "minecraft:glass",
          },
        },
      },
    ],
  };
}

export function defaultBuildRecipeText(): string {
  return JSON.stringify(defaultBuildRecipe(), null, 2);
}
