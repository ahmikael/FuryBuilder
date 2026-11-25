import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { WorldTree } from "@/features/workspace/WorldTree";
import { viewportBus } from "@/features/viewport/viewport-bus";
import type { DimensionInfo } from "@/lib/contracts";
import { useEditorStore } from "@/state/editor-store";
import { useProjectStore } from "@/state/project-store";
import { useToolStore } from "@/state/tool-store";
import { useWorldStore } from "@/state/world-store";

const dimensions: DimensionInfo[] = [
  {
    id: "minecraft:overworld",
    kind: "overworld",
    minY: 0,
    maxY: 255,
    height: 256,
    chunkCount: 4,
    sectionCount: 12,
    blockCount: 1_024,
  },
  {
    id: "minecraft:the_nether",
    kind: "nether",
    minY: 0,
    maxY: 255,
    height: 256,
    chunkCount: 2,
    sectionCount: 4,
    blockCount: 512,
  },
  {
    id: "minecraft:the_end",
    kind: "end",
    minY: 0,
    maxY: 255,
    height: 256,
    chunkCount: 0,
    sectionCount: 0,
    blockCount: 0,
  },
];

describe("WorldTree", () => {
  beforeEach(() => {
    useWorldStore.setState({
      dimensions,
      dimension: "minecraft:overworld",
      source: "core",
      chunkCount: 6,
      sectionCount: 16,
      blockCount: 1_536,
    });
    useEditorStore.setState({ selectedDimension: "minecraft:overworld" });
    useToolStore.setState({
      selection: {
        min: { x: 0, y: 0, z: 0 },
        max: { x: 1, y: 1, z: 1 },
      },
    });
    useProjectStore.setState({
      phase: "ready",
      current: {
        folder: "Test.furybuilder",
        name: "Test",
        savedRevision: 1,
        autosavedRevision: null,
        revision: 1,
        dirty: false,
        editorState: { selectedDimension: "minecraft:overworld", loadRadius: 1 },
        availableCheckpoints: 0,
      },
    });
  });

  it("lists the dimensions and bounds reported by the world", () => {
    render(<WorldTree />);

    expect(screen.getByRole("button", { name: /Overworld/ })).toHaveTextContent("4 chunks");
    expect(screen.getByRole("button", { name: /The Nether/ })).toHaveTextContent("2 chunks");
    expect(screen.getByRole("button", { name: /The End/ })).toHaveTextContent("Empty");
    expect(screen.getByText("Y 0 … 255")).toBeInTheDocument();
    expect(screen.queryByText("-64 … 319")).not.toBeInTheDocument();
  });

  it("switches the rendered dimension and clears a selection from another dimension", () => {
    const reload = vi.fn();
    viewportBus.addEventListener("reload-world", reload);
    render(<WorldTree />);

    fireEvent.click(screen.getByRole("button", { name: /The Nether/ }));

    expect(useEditorStore.getState().selectedDimension).toBe("minecraft:the_nether");
    expect(useWorldStore.getState().dimension).toBe("minecraft:the_nether");
    expect(useToolStore.getState().selection).toBeNull();
    expect(reload).toHaveBeenCalledTimes(1);
    expect(screen.getByText("Y 0 … 255")).toBeInTheDocument();

    viewportBus.removeEventListener("reload-world", reload);
  });

  it("filters by derived labels and ids", () => {
    render(<WorldTree />);
    fireEvent.change(screen.getByLabelText("Filter world tree"), { target: { value: "nether" } });

    expect(screen.getByRole("button", { name: /The Nether/ })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Overworld/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /The End/ })).not.toBeInTheDocument();
  });

  it("derives a readable label for a custom dimension", () => {
    useWorldStore.setState({
      dimensions: [
        {
          id: "custom:sky_islands",
          kind: "custom",
          minY: -64,
          maxY: 319,
          height: 384,
          chunkCount: 1,
          sectionCount: 2,
          blockCount: 64,
        },
      ],
      dimension: "custom:sky_islands",
    });
    useEditorStore.setState({ selectedDimension: "custom:sky_islands" });
    render(<WorldTree />);

    expect(screen.getByRole("button", { name: /Sky Islands/ })).toBeInTheDocument();
    expect(screen.getByText("Y -64 … 319")).toBeInTheDocument();
  });

  it("explains that no dimensions have been reported yet", () => {
    useWorldStore.setState({ dimensions: [] });
    render(<WorldTree />);

    expect(screen.getByText("No dimensions")).toBeInTheDocument();
  });
});
