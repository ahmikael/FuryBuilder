/**
 * Shared editor-shell layout metadata. It stays free of React and state imports so the
 * persisted UI store, the toolbar, the bottom panel and the command palette agree on the
 * same tab ids and shortcut labels.
 */
export const BOTTOM_PANEL_TABS = [
  { id: "blocks", label: "Blocks" },
  { id: "terrain", label: "Terrain" },
  { id: "heightmap", label: "Heightmap" },
  { id: "biomes", label: "Biomes" },
  { id: "generators", label: "Generators" },
  { id: "layers", label: "Layers" },
  { id: "templates", label: "Templates" },
  { id: "objects", label: "Objects" },
  { id: "parametric", label: "Parametric" },
  { id: "dsl", label: "Build DSL" },
  { id: "analysis", label: "Analysis" },
  { id: "activity", label: "Activity" },
] as const;

export type BottomPanelTabId = (typeof BOTTOM_PANEL_TABS)[number]["id"];

export const DEFAULT_BOTTOM_PANEL_TAB: BottomPanelTabId = "blocks";

const BOTTOM_PANEL_TAB_IDS = new Set<string>(BOTTOM_PANEL_TABS.map((tab) => tab.id));

export function isBottomPanelTabId(value: string): value is BottomPanelTabId {
  return BOTTOM_PANEL_TAB_IDS.has(value);
}

export function normalizeBottomPanelTab(value: string): BottomPanelTabId {
  return isBottomPanelTabId(value) ? value : DEFAULT_BOTTOM_PANEL_TAB;
}

export const PANEL_LABELS = {
  left: "World panel",
  right: "Inspector",
  bottom: "Bottom panel",
} as const;

export const PANEL_SHORTCUTS = {
  left: "Ctrl+B",
  right: "Ctrl+Alt+B",
  bottom: "Ctrl+J",
} as const;

export const COMMAND_PALETTE_SHORTCUT = "Ctrl+K";
