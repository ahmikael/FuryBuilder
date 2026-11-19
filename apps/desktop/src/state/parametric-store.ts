import { create } from "zustand";

import { isTauriRuntime, normalizeError } from "@/lib/ipc";
import {
  bakeParametricObject,
  createParametricObject,
  fetchParametricObjects,
  inspectParametricObject,
  rollbackParametricObject,
  updateParametricObject,
} from "@/lib/parametric-api";
import type {
  ParametricApplyResult,
  ParametricConflictPolicy,
  ParametricInspection,
  ParametricObjectSummary,
  ParametricSpec,
} from "@/project/parametric-types";
import { pushToast } from "@/state/tool-store";

interface ParametricUiState {
  objects: ParametricObjectSummary[];
  loading: boolean;
  saving: boolean;
  error: string | null;
  reset: () => void;
  refresh: () => Promise<void>;
  create: (spec: ParametricSpec) => Promise<ParametricApplyResult | null>;
  update: (
    id: string,
    spec: ParametricSpec,
    policy: ParametricConflictPolicy,
  ) => Promise<ParametricApplyResult | null>;
  bake: (id: string) => Promise<boolean>;
  rollback: (id: string, force: boolean) => Promise<boolean>;
  inspect: (id: string) => Promise<ParametricInspection | null>;
}

function messageOf(error: unknown): string {
  return normalizeError(error).message;
}

/**
 * Parametric recipes are enforced by the Rust Core. The UI keeps only summaries; rollback
 * baselines stay in `project.json` and never travel through React state.
 */
export const useParametricStore = create<ParametricUiState>()((set) => ({
  objects: [],
  loading: false,
  saving: false,
  error: null,

  reset: () => set({ objects: [], loading: false, saving: false, error: null }),

  refresh: async () => {
    if (!isTauriRuntime()) return;
    set({ loading: true });
    try {
      const objects = await fetchParametricObjects();
      set({ objects, loading: false, error: null });
    } catch (error) {
      set({ loading: false, error: messageOf(error) });
    }
  },

  create: async (spec) => {
    if (!isTauriRuntime()) return null;
    set({ saving: true, error: null });
    try {
      const result = await createParametricObject(spec);
      const objects = await fetchParametricObjects();
      set({ objects, saving: false, error: null });
      pushToast({
        kind: "success",
        title: "Parametric object applied",
        description: `${result.object.name} · ${result.object.generatedBlocks} generated blocks`,
      });
      return result;
    } catch (error) {
      const message = messageOf(error);
      set({ saving: false, error: message });
      pushToast({ kind: "error", title: "Parametric apply failed", description: message });
      return null;
    }
  },

  update: async (id, spec, policy) => {
    if (!isTauriRuntime()) return null;
    set({ saving: true, error: null });
    try {
      const result = await updateParametricObject(id, spec, policy);
      const objects = await fetchParametricObjects();
      set({ objects, saving: false, error: null });
      pushToast({
        kind: policy === "override" ? "warning" : "success",
        title: policy === "override" ? "Manual edits overridden" : "Parametric object updated",
        description: `${result.object.name} · ${result.object.generatedBlocks} generated blocks`,
      });
      return result;
    } catch (error) {
      const message = messageOf(error);
      set({ saving: false, error: message });
      pushToast({
        kind: "error",
        title: "Parametric update blocked",
        description: message,
      });
      return null;
    }
  },

  bake: async (id) => {
    if (!isTauriRuntime()) return false;
    set({ saving: true, error: null });
    try {
      const result = await bakeParametricObject(id);
      const objects = await fetchParametricObjects();
      set({ objects, saving: false, error: null });
      pushToast({
        kind: "info",
        title: `Baked "${result.object.name}"`,
        description: "The blocks stay in the world; the parametric recipe was detached.",
      });
      return true;
    } catch (error) {
      const message = messageOf(error);
      set({ saving: false, error: message });
      pushToast({ kind: "error", title: "Bake failed", description: message });
      return false;
    }
  },

  rollback: async (id, force) => {
    if (!isTauriRuntime()) return false;
    set({ saving: true, error: null });
    try {
      const result = await rollbackParametricObject(id, force);
      const objects = await fetchParametricObjects();
      set({ objects, saving: false, error: null });
      pushToast({
        kind: "warning",
        title: `Rolled back "${result.object.name}"`,
        description: "The captured pre-object blocks were restored.",
      });
      return true;
    } catch (error) {
      const message = messageOf(error);
      set({ saving: false, error: message });
      pushToast({ kind: "error", title: "Rollback blocked", description: message });
      return false;
    }
  },

  inspect: async (id) => {
    if (!isTauriRuntime()) return null;
    try {
      const inspection = await inspectParametricObject(id);
      set({ error: null });
      return inspection;
    } catch (error) {
      set({ error: messageOf(error) });
      return null;
    }
  },
}));
