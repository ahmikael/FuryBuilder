import { describe, expect, it, vi } from "vitest";

import type { BottomPanelTabId } from "@/editor/layout";
import { buildPaletteCommands, type PaletteCommand } from "@/features/workspace/CommandPalette";

function palette(bottomPanelTab: BottomPanelTabId = "blocks") {
  const setBottomPanelTab = vi.fn();
  const commands = buildPaletteCommands({
    theme: "dark",
    activeTool: "place",
    bottomPanelTab,
    panels: { left: true, right: true, bottom: true },
    togglePanel: vi.fn(),
    setTheme: vi.fn(),
    setBottomPanelTab,
    resetLayout: vi.fn(),
    setManagerOpen: vi.fn(),
    projectOpen: true,
    saveProject: vi.fn(),
  });
  return { commands, setBottomPanelTab };
}

function command(commands: PaletteCommand[], id: string): PaletteCommand {
  const match = commands.find((entry) => entry.id === id);
  if (!match) throw new Error(`missing command ${id}`);
  return match;
}

describe("command palette bottom tabs", () => {
  it("offers the grouped tabs and no removed tab commands", () => {
    const { commands } = palette();
    const tabCommands = commands.filter((entry) => entry.id.startsWith("open-tab-"));

    expect(tabCommands.map((entry) => entry.label)).toEqual([
      "Open Blocks",
      "Open Terrain",
      "Open Heightmap",
      "Open Biomes",
      "Open Generators",
      "Open Layers",
      "Open Templates",
      "Open Objects",
      "Open Parametric",
      "Open Build DSL",
      "Open Analysis",
      "Open Activity",
    ]);
    for (const removed of [
      "palette",
      "tools",
      "warnings",
      "observation",
      "validation",
      "console",
      "history",
    ]) {
      expect(commands.some((entry) => entry.id === `open-tab-${removed}`)).toBe(false);
    }
  });

  it("marks the active tab and switches to Analysis", () => {
    const { commands, setBottomPanelTab } = palette("analysis");

    expect(command(commands, "open-tab-analysis").detail).toBe("Active");
    expect(command(commands, "open-tab-blocks").detail).toBeUndefined();

    command(commands, "open-tab-activity").run();
    expect(setBottomPanelTab).toHaveBeenCalledWith("activity");
  });
});
