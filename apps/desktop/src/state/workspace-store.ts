import { create } from "zustand";

import { isTauriRuntime, normalizeError } from "@/lib/ipc";
import { fetchWorkspacePolicy, setWorkspacePolicy } from "@/lib/project-api";
import { defaultWorkspacePolicy, type WorkspacePolicy } from "@/project/types";
import { pushToast } from "@/state/tool-store";

interface WorkspaceUiState {
  policy: WorkspacePolicy;
  loading: boolean;
  saving: boolean;
  error: string | null;
  reset: (policy?: WorkspacePolicy | null) => void;
  refresh: () => Promise<void>;
  apply: (next: WorkspacePolicy, successMessage?: string) => Promise<boolean>;
}

/**
 * Editor-only layer/mask/protection policy. The Core remains authoritative: every
 * update is validated and persisted through `workspace_set_policy` before it is shown
 * as saved, and failed operations never mutate the local policy.
 */
export const useWorkspaceStore = create<WorkspaceUiState>()((set) => ({
  policy: defaultWorkspacePolicy(),
  loading: false,
  saving: false,
  error: null,

  reset: (policy) =>
    set({
      policy: policy ?? defaultWorkspacePolicy(),
      loading: false,
      saving: false,
      error: null,
    }),

  refresh: async () => {
    if (!isTauriRuntime()) return;
    set({ loading: true });
    try {
      const policy = await fetchWorkspacePolicy();
      set({ policy, loading: false, error: null });
    } catch (error) {
      set({ loading: false, error: normalizeError(error).message });
    }
  },

  apply: async (next, successMessage) => {
    set({ saving: true, error: null });
    try {
      const saved = await setWorkspacePolicy(next);
      set({ policy: saved, saving: false, error: null });
      if (successMessage) {
        pushToast({ kind: "success", title: successMessage });
      }
      return true;
    } catch (error) {
      const message = normalizeError(error).message;
      set({ saving: false, error: message });
      pushToast({
        kind: "error",
        title: "Workspace policy update failed",
        description: message,
      });
      return false;
    }
  },
}));
