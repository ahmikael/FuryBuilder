import type { Bounds3 } from "@/renderer/types";

export type SceneObjectKind = "district" | "building" | "road" | "entrance" | "landmark" | "other";

/** Editor-only semantic metadata; it never replaces authoritative voxel data. */
export interface SemanticObject {
  id: string;
  name: string;
  kind: SceneObjectKind;
  dimension: string;
  bounds: Bounds3;
  parentId?: string | null;
  tags?: Record<string, string>;
  note?: string | null;
}

export interface Scene {
  schemaVersion: number;
  objects: SemanticObject[];
}

export interface ScenePaletteEntry {
  state: string;
  count: number;
}

export interface SceneObjectInspection {
  object: SemanticObject;
  volume: number;
  nonAirBlocks: number;
  palette: ScenePaletteEntry[];
  children: SemanticObject[];
}

export const SCENE_SCHEMA_VERSION = 1;

export const SCENE_OBJECT_KINDS: { value: SceneObjectKind; label: string }[] = [
  { value: "district", label: "District" },
  { value: "building", label: "Building" },
  { value: "road", label: "Road" },
  { value: "entrance", label: "Entrance" },
  { value: "landmark", label: "Landmark" },
  { value: "other", label: "Other" },
];

export function defaultScene(): Scene {
  return { schemaVersion: SCENE_SCHEMA_VERSION, objects: [] };
}

export function sceneObjectKindLabel(kind: SceneObjectKind): string {
  return SCENE_OBJECT_KINDS.find((entry) => entry.value === kind)?.label ?? "Other";
}
