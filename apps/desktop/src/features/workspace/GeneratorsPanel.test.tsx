import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type * as IpcModule from "@/lib/ipc";
import { GeneratorsPanel } from "@/features/workspace/GeneratorsPanel";
import { useToolStore } from "@/state/tool-store";

const runtime = vi.hoisted(() => ({ tauri: true }));
const worldApi = vi.hoisted(() => ({
  preview: vi.fn(),
  catalog: vi.fn(),
  sites: vi.fn(),
}));
const editor = vi.hoisted(() => ({ apply: vi.fn() }));

vi.mock("@/lib/ipc", async (importOriginal) => {
  const actual = await importOriginal<typeof IpcModule>();
  return { ...actual, isTauriRuntime: () => runtime.tauri };
});

vi.mock("@/lib/world-api", () => ({
  previewOperation: worldApi.preview,
  fetchBlockCatalog: worldApi.catalog,
  findStructureSites: worldApi.sites,
}));

vi.mock("@/editor/commands", () => ({
  applyEditorOperation: editor.apply,
}));

const selection = {
  min: { x: 0, y: 64, z: 0 },
  max: { x: 7, y: 70, z: 7 },
};

beforeEach(() => {
  runtime.tauri = true;
  worldApi.preview.mockReset();
  worldApi.catalog.mockReset();
  worldApi.sites.mockReset();
  editor.apply.mockReset();
  worldApi.catalog.mockResolvedValue([
    { name: "minecraft:stone_bricks", properties: {} },
    { name: "minecraft:water", properties: { level: "0" } },
    { name: "minecraft:sand", properties: {} },
    { name: "minecraft:short_grass", properties: {} },
    { name: "minecraft:oak_log", properties: { axis: "y" } },
    {
      name: "minecraft:oak_leaves",
      properties: { distance: "7", persistent: "false", waterlogged: "false" },
    },
  ]);
  useToolStore.setState({
    selection,
    selectedBlock: { name: "minecraft:stone", properties: {} },
    job: null,
  });
});

describe("generators panel", () => {
  it("previews a deterministic road without writing", async () => {
    const user = userEvent.setup();
    worldApi.preview.mockResolvedValue({
      affectedBlocks: 448,
      changedBlocks: 12,
      dirtyChunks: [],
      warnings: ["1 route column rested on a fluid"],
    });
    render(<GeneratorsPanel />);

    await user.click(screen.getByRole("button", { name: "Preview generator" }));

    expect(worldApi.preview).toHaveBeenCalledWith({
      type: "generate_road",
      dimension: "minecraft:overworld",
      bounds: {
        min: { x: 0, y: 48, z: 0 },
        max: { x: 7, y: 86, z: 7 },
      },
      from: { x: 0, y: 48, z: 3 },
      to: { x: 7, y: 48, z: 3 },
      width: 3,
      meander: 4,
      seed: 0,
      clearance: 2,
      surface: { name: "minecraft:stone_bricks", properties: {} },
    });
    expect(await screen.findByTestId("generator-preview")).toHaveTextContent(
      "12 block(s) would change out of 448 selected.",
    );
    expect(screen.getByTestId("generator-preview")).toHaveTextContent(
      "1 route column rested on a fluid",
    );
    expect(editor.apply).not.toHaveBeenCalled();
  });

  it("applies a river with the selected materials and depth", async () => {
    const user = userEvent.setup();
    editor.apply.mockResolvedValue({ revision: 2, changedBlocks: 20 });
    render(<GeneratorsPanel />);

    await user.click(screen.getByRole("button", { name: "River" }));
    await user.clear(screen.getByLabelText("Depth"));
    await user.type(screen.getByLabelText("Depth"), "5");
    await user.click(screen.getByRole("button", { name: "Apply generator" }));

    expect(editor.apply).toHaveBeenCalledWith(
      expect.objectContaining({
        type: "generate_river",
        width: 3,
        meander: 4,
        depth: 5,
        from: { x: 0, y: 48, z: 3 },
        to: { x: 7, y: 48, z: 3 },
        water: { name: "minecraft:water", properties: { level: "0" } },
        bed: { name: "minecraft:sand", properties: {} },
      }),
    );
  });

  it("scatters vegetation with bounded percentages", async () => {
    const user = userEvent.setup();
    editor.apply.mockResolvedValue({ revision: 3, changedBlocks: 64 });
    render(<GeneratorsPanel />);

    await user.click(screen.getByRole("button", { name: "Vegetation" }));
    await user.clear(screen.getByLabelText("Ground cover"));
    await user.type(screen.getByLabelText("Ground cover"), "100");
    await user.clear(screen.getByLabelText("Tree chance"));
    await user.type(screen.getByLabelText("Tree chance"), "0");
    await user.click(screen.getByRole("button", { name: "Apply generator" }));

    expect(editor.apply).toHaveBeenCalledWith(
      expect.objectContaining({
        type: "generate_vegetation",
        seed: 0,
        groundCover: 100,
        treeChance: 0,
        treeSpacing: 5,
        plant: { name: "minecraft:short_grass", properties: {} },
        trunk: { name: "minecraft:oak_log", properties: { axis: "y" } },
        leaves: {
          name: "minecraft:oak_leaves",
          properties: { distance: "7", persistent: "false", waterlogged: "false" },
        },
      }),
    );
  });

  it("queries bounded structure sites and selects the first footprint", async () => {
    const user = userEvent.setup();
    worldApi.sites.mockResolvedValue({
      dimension: "minecraft:overworld",
      bounds: {
        min: { x: 0, y: 48, z: 0 },
        max: { x: 7, y: 86, z: 7 },
      },
      sites: [
        {
          origin: { x: 1, y: 65, z: 1 },
          minSurfaceY: 64,
          maxSurfaceY: 64,
        },
      ],
      scannedCandidates: 1,
      rejectedCandidates: 0,
      limit: 8,
    });
    render(<GeneratorsPanel />);

    await user.click(screen.getByRole("button", { name: "Sites" }));
    await user.click(screen.getByRole("button", { name: "Find structure sites" }));

    expect(worldApi.sites).toHaveBeenCalledWith({
      dimension: "minecraft:overworld",
      bounds: {
        min: { x: 0, y: 48, z: 0 },
        max: { x: 7, y: 86, z: 7 },
      },
      width: 5,
      depth: 5,
      step: 5,
      maxSlope: 2,
      clearance: 2,
      limit: 8,
    });
    expect(await screen.findByTestId("structure-sites-result")).toHaveTextContent("1 site(s)");

    await user.click(screen.getByRole("button", { name: "Use first structure site" }));
    expect(useToolStore.getState().selection).toEqual({
      min: { x: 1, y: 65, z: 1 },
      max: { x: 5, y: 67, z: 5 },
    });
  });

  it("requires a selection and reports query errors", async () => {
    const user = userEvent.setup();
    useToolStore.setState({ selection: null });
    const { unmount } = render(<GeneratorsPanel />);
    expect(screen.getByRole("button", { name: "Apply generator" })).toBeDisabled();
    unmount();

    useToolStore.setState({ selection });
    worldApi.preview.mockRejectedValue(new Error("route start is outside the selection"));
    render(<GeneratorsPanel />);
    await user.click(screen.getByRole("button", { name: "Preview generator" }));
    expect(await screen.findByText("route start is outside the selection")).toBeInTheDocument();
  });
});
