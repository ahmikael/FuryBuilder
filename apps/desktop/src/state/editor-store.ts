import { create } from "zustand";
import { persist } from "zustand/middleware";

export type ThemeMode = "dark" | "light" | "system";

interface EditorState {
  theme: ThemeMode;
  uiScale: number;
  /** UI-only selection placeholder; world data will come from Rust snapshots. */
  selectedDimension: string;
  setTheme: (theme: ThemeMode) => void;
  setUiScale: (scale: number) => void;
  setSelectedDimension: (dimension: string) => void;
}

export const MIN_UI_SCALE = 0.8;
export const MAX_UI_SCALE = 1.4;

function clampScale(value: number): number {
  if (!Number.isFinite(value)) return 1;
  return Math.min(MAX_UI_SCALE, Math.max(MIN_UI_SCALE, Math.round(value * 100) / 100));
}

export const useEditorStore = create<EditorState>()(
  persist(
    (set) => ({
      theme: "dark",
      uiScale: 1,
      selectedDimension: "minecraft:overworld",
      setTheme: (theme) => set({ theme }),
      setUiScale: (uiScale) => set({ uiScale: clampScale(uiScale) }),
      setSelectedDimension: (selectedDimension) => set({ selectedDimension }),
    }),
    {
      name: "furybuilder.editor-ui",
      version: 1,
    },
  ),
);
