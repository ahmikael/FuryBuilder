import { create } from "zustand";
import type { HistoryState, HistorySummary } from "@/editor/types";
import { fetchHistoryState } from "@/lib/world-api";
import { normalizeError } from "@/lib/ipc";

interface HistoryUiState {
  canUndo: boolean;
  canRedo: boolean;
  revision: number;
  entries: HistorySummary[];
  loading: boolean;
  error: string | null;
  apply: (state: HistoryState) => void;
  refresh: () => Promise<void>;
  reset: () => void;
}

const empty = {
  canUndo: false,
  canRedo: false,
  revision: 0,
  entries: [] as HistorySummary[],
  loading: false,
  error: null,
};

export const useHistoryStore = create<HistoryUiState>()((set) => ({
  ...empty,
  apply: (state) =>
    set({
      canUndo: state.canUndo,
      canRedo: state.canRedo,
      revision: state.revision,
      entries: state.entries,
      loading: false,
      error: null,
    }),
  refresh: async () => {
    set({ loading: true });
    try {
      const state = await fetchHistoryState();
      set({
        canUndo: state.canUndo,
        canRedo: state.canRedo,
        revision: state.revision,
        entries: state.entries,
        loading: false,
        error: null,
      });
    } catch (error) {
      set({ loading: false, error: normalizeError(error).message });
    }
  },
  reset: () => set(empty),
}));
