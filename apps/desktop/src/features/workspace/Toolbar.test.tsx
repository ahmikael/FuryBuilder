import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { Toolbar } from "@/features/workspace/Toolbar";
import { useEditorStore } from "@/state/editor-store";
import { useHistoryStore } from "@/state/history-store";
import { useProjectStore } from "@/state/project-store";
import { useToolStore } from "@/state/tool-store";
import { useWorldStore } from "@/state/world-store";

function renderToolbar() {
  render(
    <Toolbar
      backendStatus="online"
      theme="dark"
      onToggleTheme={vi.fn()}
      onOpenSettings={vi.fn()}
      onOpenMcpAccess={vi.fn()}
    />,
  );
}

describe("Toolbar", () => {
  beforeEach(() => {
    useEditorStore.setState({ leftPanelOpen: true, rightPanelOpen: true, bottomPanelOpen: true });
    useHistoryStore.setState({ canUndo: false, canRedo: false });
    useProjectStore.setState({
      phase: "ready",
      current: {
        folder: "Castle.furybuilder",
        name: "Castle",
        savedRevision: 3,
        autosavedRevision: null,
        revision: 3,
        dirty: false,
        editorState: { selectedDimension: "minecraft:overworld", loadRadius: 1 },
        availableCheckpoints: 0,
      },
    });
    useToolStore.setState({
      selectedBlock: { name: "minecraft:diamond_block", properties: {} },
    });
    useWorldStore.setState({ revision: 3 });
  });

  it("shows the project and one save indicator without the active palette block", () => {
    renderToolbar();

    expect(screen.getByText("Castle")).toBeInTheDocument();
    expect(screen.getByText("Preview scene")).toBeInTheDocument();
    expect(screen.queryByTitle("Active palette block")).not.toBeInTheDocument();
    expect(screen.queryByText("diamond_block")).not.toBeInTheDocument();
  });
});
