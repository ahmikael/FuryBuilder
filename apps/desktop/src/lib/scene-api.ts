import { invokeCommand } from "@/lib/ipc";
import type { Bounds3 } from "@/renderer/types";
import type { SceneObjectInspection, SceneObjectKind, SemanticObject } from "@/project/scene-types";

export interface SceneObjectDraft {
  name: string;
  kind: SceneObjectKind;
  dimension: string;
  bounds: Bounds3;
  parentId?: string | null;
  tags?: Record<string, string>;
  note?: string | null;
}

export function fetchSceneObjects(): Promise<SemanticObject[]> {
  return invokeCommand<SemanticObject[]>("scene_objects");
}

export function inspectSceneObject(objectId: string): Promise<SceneObjectInspection> {
  return invokeCommand<SceneObjectInspection>("scene_inspect", { objectId });
}

export function createSceneObject(draft: SceneObjectDraft): Promise<SemanticObject> {
  return invokeCommand<SemanticObject>("scene_create", {
    name: draft.name,
    kind: draft.kind,
    dimension: draft.dimension,
    bounds: draft.bounds,
    parentId: draft.parentId ?? null,
    tags: draft.tags ?? {},
    note: draft.note ?? null,
  });
}

export function deleteSceneObject(objectId: string): Promise<SemanticObject> {
  return invokeCommand<SemanticObject>("scene_delete", { objectId });
}
