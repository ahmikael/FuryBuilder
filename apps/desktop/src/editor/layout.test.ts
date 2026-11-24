import { describe, expect, it } from "vitest";

import {
  BOTTOM_PANEL_TABS,
  DEFAULT_BOTTOM_PANEL_TAB,
  isBottomPanelTabId,
  normalizeBottomPanelTab,
} from "@/editor/layout";

const REMOVED_TAB_IDS = [
  "palette",
  "tools",
  "warnings",
  "observation",
  "validation",
  "console",
  "history",
] as const;

describe("bottom panel layout", () => {
  it("lists the twelve current tabs once, with Blocks as the default", () => {
    const ids = BOTTOM_PANEL_TABS.map((tab) => tab.id);

    expect(ids).toEqual([
      "blocks",
      "terrain",
      "heightmap",
      "biomes",
      "generators",
      "layers",
      "templates",
      "objects",
      "parametric",
      "dsl",
      "analysis",
      "activity",
    ]);
    expect(new Set(ids).size).toBe(ids.length);
    expect(BOTTOM_PANEL_TABS[0]).toEqual({ id: "blocks", label: "Blocks" });
    expect(DEFAULT_BOTTOM_PANEL_TAB).toBe("blocks");
    for (const id of ids) {
      expect(isBottomPanelTabId(id)).toBe(true);
    }
  });

  it("does not expose removed tabs and resets them to Blocks", () => {
    for (const id of REMOVED_TAB_IDS) {
      expect(isBottomPanelTabId(id)).toBe(false);
      expect(normalizeBottomPanelTab(id)).toBe("blocks");
    }
  });

  it("keeps every current tab selectable", () => {
    for (const tab of BOTTOM_PANEL_TABS) {
      expect(normalizeBottomPanelTab(tab.id)).toBe(tab.id);
    }
  });
});
