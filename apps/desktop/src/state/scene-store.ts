import { create } from "zustand";

import { isTauriRuntime, normalizeError } from "@/lib/ipc";
import {
  createSceneObject,
  deleteSceneObject,
  fetchSceneObjects,
  inspectSceneObject,
  type SceneObjectDraft,
} from "@/lib/scene-api";
import {
  defaultScene,
  SCENE_SCHEMA_VERSION,
  type Scene,
  type SceneObjectInspection,
  type SemanticObject,
} from "@/project/scene-types";
import { pushToast } from "@/state/toast-store";

interface SceneUiState {
  scene: Scene;
  loading: boolean;
  saving: boolean;
  error: string | null;
  reset: (scene?: Scene | null) => void;
  refresh: () => Promise<void>;
  create: (draft: SceneObjectDraft) => Promise<SemanticObject | null>;
  remove: (id: string) => Promise<boolean>;
  inspect: (id: string) => Promise<SceneObjectInspection | null>;
}

function sceneFromObjects(objects: SemanticObject[]): Scene {
  return { schemaVersion: SCENE_SCHEMA_VERSION, objects };
}

/**
 * Semantic object catalog. The Rust Core remains authoritative: every change is sent to
 * the desktop, then the full list is re-read instead of trusting an optimistic copy.
 */
export const useSceneStore = create<SceneUiState>()((set) => ({
  scene: defaultScene(),
  loading: false,
  saving: false,
  error: null,

  reset: (scene) =>
    set({
      scene: scene ?? defaultScene(),
      loading: false,
      saving: false,
      error: null,
    }),

  refresh: async () => {
    if (!isTauriRuntime()) return;
    set({ loading: true });
    try {
      const objects = await fetchSceneObjects();
      set({ scene: sceneFromObjects(objects), loading: false, error: null });
    } catch (error) {
      set({ loading: false, error: normalizeError(error).message });
    }
  },

  create: async (draft) => {
    if (!isTauriRuntime()) return null;
    set({ saving: true, error: null });
    try {
      const object = await createSceneObject(draft);
      const objects = await fetchSceneObjects();
      set({ scene: sceneFromObjects(objects), saving: false, error: null });
      pushToast({
        kind: "success",
        title: "Object created",
        description: `${object.name} · ${object.id}`,
      });
      return object;
    } catch (error) {
      const message = normalizeError(error).message;
      set({ saving: false, error: message });
      pushToast({
        kind: "error",
        title: "Object creation failed",
        description: message,
      });
      return null;
    }
  },

  remove: async (id) => {
    if (!isTauriRuntime()) return false;
    set({ saving: true, error: null });
    try {
      const object = await deleteSceneObject(id);
      const objects = await fetchSceneObjects();
      set({ scene: sceneFromObjects(objects), saving: false, error: null });
      pushToast({ kind: "info", title: `Deleted object "${object.name}"` });
      return true;
    } catch (error) {
      const message = normalizeError(error).message;
      set({ saving: false, error: message });
      pushToast({
        kind: "error",
        title: "Object deletion failed",
        description: message,
      });
      return false;
    }
  },

  inspect: async (id) => {
    if (!isTauriRuntime()) return null;
    try {
      const inspection = await inspectSceneObject(id);
      set({ error: null });
      return inspection;
    } catch (error) {
      set({ error: normalizeError(error).message });
      return null;
    }
  },
}));
