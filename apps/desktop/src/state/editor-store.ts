import { create } from "zustand";
import { persist } from "zustand/middleware";

import {
  DEFAULT_BOTTOM_PANEL_TAB,
  normalizeBottomPanelTab,
  type BottomPanelTabId,
} from "@/editor/layout";

export type ThemeMode = "dark" | "light" | "system";
export type PanelId = "left" | "right" | "bottom";

export { DEFAULT_BOTTOM_PANEL_TAB };

/**
 * Bumped when the persisted layout shape changes. A stored layout from an older version is
 * discarded instead of being migrated; theme and UI scale are user preferences, not layout.
 */
export const LAYOUT_VERSION = 2;

interface EditorState {
  theme: ThemeMode;
  uiScale: number;
  layoutVersion: number;
  /** UI-only selection placeholder; world data will come from Rust snapshots. */
  selectedDimension: string;
  leftPanelOpen: boolean;
  rightPanelOpen: boolean;
  bottomPanelOpen: boolean;
  bottomPanelTab: BottomPanelTabId;
  setTheme: (theme: ThemeMode) => void;
  setUiScale: (scale: number) => void;
  setSelectedDimension: (dimension: string) => void;
  setPanelOpen: (panel: PanelId, open: boolean) => void;
  togglePanel: (panel: PanelId) => void;
  setBottomPanelTab: (tab: BottomPanelTabId) => void;
  resetLayout: () => void;
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
      layoutVersion: LAYOUT_VERSION,
      selectedDimension: "minecraft:overworld",
      leftPanelOpen: true,
      rightPanelOpen: true,
      bottomPanelOpen: true,
      bottomPanelTab: DEFAULT_BOTTOM_PANEL_TAB,
      setTheme: (theme) => set({ theme }),
      setUiScale: (uiScale) => set({ uiScale: clampScale(uiScale) }),
      setSelectedDimension: (selectedDimension) => set({ selectedDimension }),
      setPanelOpen: (panel, open) =>
        set(
          panel === "left"
            ? { leftPanelOpen: open }
            : panel === "right"
              ? { rightPanelOpen: open }
              : { bottomPanelOpen: open },
        ),
      togglePanel: (panel) =>
        set((state) =>
          panel === "left"
            ? { leftPanelOpen: !state.leftPanelOpen }
            : panel === "right"
              ? { rightPanelOpen: !state.rightPanelOpen }
              : { bottomPanelOpen: !state.bottomPanelOpen },
        ),
      setBottomPanelTab: (bottomPanelTab) => set({ bottomPanelOpen: true, bottomPanelTab }),
      resetLayout: () =>
        set({
          layoutVersion: LAYOUT_VERSION,
          leftPanelOpen: true,
          rightPanelOpen: true,
          bottomPanelOpen: true,
          bottomPanelTab: DEFAULT_BOTTOM_PANEL_TAB,
        }),
    }),
    {
      name: "furybuilder.editor-ui",
      version: 1,
      partialize: (state) => ({
        theme: state.theme,
        uiScale: state.uiScale,
        layoutVersion: state.layoutVersion,
        selectedDimension: state.selectedDimension,
        leftPanelOpen: state.leftPanelOpen,
        rightPanelOpen: state.rightPanelOpen,
        bottomPanelOpen: state.bottomPanelOpen,
        bottomPanelTab: state.bottomPanelTab,
      }),
      // The layout is a disposable cache of panel visibility and the active tab. A layout
      // from an older shape is not migrated: it is ignored and the current defaults are used.
      // Theme and UI scale are preferences and remain readable across layout versions.
      merge: (persisted, current) => {
        const stored = (persisted ?? {}) as Partial<EditorState>;
        const bool = (value: unknown, fallback: boolean): boolean =>
          typeof value === "boolean" ? value : fallback;
        const layoutCurrent = stored.layoutVersion === LAYOUT_VERSION;
        return {
          ...current,
          layoutVersion: LAYOUT_VERSION,
          theme:
            stored.theme === "light" || stored.theme === "system" || stored.theme === "dark"
              ? stored.theme
              : current.theme,
          uiScale:
            typeof stored.uiScale === "number" ? clampScale(stored.uiScale) : current.uiScale,
          selectedDimension:
            layoutCurrent && typeof stored.selectedDimension === "string"
              ? stored.selectedDimension
              : current.selectedDimension,
          leftPanelOpen: layoutCurrent
            ? bool(stored.leftPanelOpen, current.leftPanelOpen)
            : current.leftPanelOpen,
          rightPanelOpen: layoutCurrent
            ? bool(stored.rightPanelOpen, current.rightPanelOpen)
            : current.rightPanelOpen,
          bottomPanelOpen: layoutCurrent
            ? bool(stored.bottomPanelOpen, current.bottomPanelOpen)
            : current.bottomPanelOpen,
          bottomPanelTab: layoutCurrent
            ? normalizeBottomPanelTab(
                typeof stored.bottomPanelTab === "string"
                  ? stored.bottomPanelTab
                  : current.bottomPanelTab,
              )
            : current.bottomPanelTab,
        };
      },
    },
  ),
);
