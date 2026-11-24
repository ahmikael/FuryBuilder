import { beforeEach, describe, expect, it } from "vitest";

import { LAYOUT_VERSION, useEditorStore } from "@/state/editor-store";

const STORAGE_KEY = "furybuilder.editor-ui";

function storeLayout(state: Record<string, unknown>): void {
  localStorage.setItem(STORAGE_KEY, JSON.stringify({ state, version: 1 }));
}

describe("editor layout persistence", () => {
  beforeEach(() => {
    localStorage.clear();
    useEditorStore.setState({
      theme: "dark",
      uiScale: 1,
      layoutVersion: LAYOUT_VERSION,
      selectedDimension: "minecraft:overworld",
      leftPanelOpen: true,
      rightPanelOpen: true,
      bottomPanelOpen: true,
      bottomPanelTab: "blocks",
    });
  });

  it("discards a layout from an older version but keeps theme and scale", async () => {
    storeLayout({
      theme: "light",
      uiScale: 1.25,
      leftPanelOpen: false,
      rightPanelOpen: false,
      bottomPanelOpen: false,
      bottomPanelTab: "palette",
    });

    await useEditorStore.persist.rehydrate();

    const state = useEditorStore.getState();
    expect(state.theme).toBe("light");
    expect(state.uiScale).toBe(1.25);
    expect(state.layoutVersion).toBe(LAYOUT_VERSION);
    expect(state.leftPanelOpen).toBe(true);
    expect(state.rightPanelOpen).toBe(true);
    expect(state.bottomPanelOpen).toBe(true);
    expect(state.bottomPanelTab).toBe("blocks");
  });

  it("restores the current layout and resets a removed tab to the default", async () => {
    storeLayout({
      layoutVersion: LAYOUT_VERSION,
      theme: "dark",
      uiScale: 1,
      selectedDimension: "minecraft:the_nether",
      leftPanelOpen: false,
      rightPanelOpen: true,
      bottomPanelOpen: false,
      bottomPanelTab: "warnings",
    });

    await useEditorStore.persist.rehydrate();

    const state = useEditorStore.getState();
    expect(state.selectedDimension).toBe("minecraft:the_nether");
    expect(state.leftPanelOpen).toBe(false);
    expect(state.rightPanelOpen).toBe(true);
    expect(state.bottomPanelOpen).toBe(false);
    expect(state.bottomPanelTab).toBe("blocks");
  });
});
