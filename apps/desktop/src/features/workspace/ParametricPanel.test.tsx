import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type * as IpcModule from "@/lib/ipc";
import { ParametricPanel } from "@/features/workspace/ParametricPanel";
import type { ParametricObjectSummary } from "@/project/parametric-types";
import { useParametricStore } from "@/state/parametric-store";
import { useToolStore } from "@/state/tool-store";
import { useWorldStore } from "@/state/world-store";

const runtime = vi.hoisted(() => ({ tauri: true }));
const parametricApi = vi.hoisted(() => ({
  fetch: vi.fn(),
  inspect: vi.fn(),
  create: vi.fn(),
  update: vi.fn(),
  bake: vi.fn(),
  rollback: vi.fn(),
}));

vi.mock("@/lib/ipc", async (importOriginal) => {
  const actual = await importOriginal<typeof IpcModule>();
  return { ...actual, isTauriRuntime: () => runtime.tauri };
});

vi.mock("@/lib/parametric-api", () => ({
  fetchParametricObjects: parametricApi.fetch,
  inspectParametricObject: parametricApi.inspect,
  createParametricObject: parametricApi.create,
  updateParametricObject: parametricApi.update,
  bakeParametricObject: parametricApi.bake,
  rollbackParametricObject: parametricApi.rollback,
}));

const selection = {
  min: { x: 0, y: 60, z: 0 },
  max: { x: 15, y: 80, z: 15 },
};

const house: ParametricObjectSummary = {
  id: "old-house",
  name: "Old House",
  generator: "house",
  generatorVersion: 1,
  dimension: "minecraft:overworld",
  bounds: {
    min: { x: 0, y: 60, z: 0 },
    max: { x: 8, y: 65, z: 6 },
  },
  origin: { x: 0, y: 60, z: 0 },
  rotation: "none",
  params: {
    width: 9,
    depth: 7,
    height: 4,
    wall: "minecraft:stone_bricks",
    floor: "minecraft:oak_planks",
    roof: "minecraft:dark_oak_planks",
    window: "minecraft:glass",
  },
  layerId: null,
  appliedRevision: 2,
  lastOperationId: "op-1",
  generatedBlocks: 138,
};

beforeEach(() => {
  runtime.tauri = true;
  parametricApi.fetch.mockReset();
  parametricApi.inspect.mockReset();
  parametricApi.create.mockReset();
  parametricApi.update.mockReset();
  parametricApi.bake.mockReset();
  parametricApi.rollback.mockReset();
  parametricApi.fetch.mockResolvedValue([house]);
  useParametricStore.getState().reset();
  useParametricStore.setState({ objects: [house] });
  useToolStore.setState({
    selection,
    selectedBlock: { name: "minecraft:stone", properties: {} },
  });
  useWorldStore.setState({ dimension: "minecraft:overworld" });
});

describe("parametric panel", () => {
  it("lists recipes and shows conflicts from the authoritative inspection", async () => {
    const user = userEvent.setup();
    parametricApi.inspect.mockResolvedValue({
      object: house,
      generatedBlocks: 138,
      conflictCount: 1,
      conflicts: [
        {
          pos: { x: 1, y: 61, z: 0 },
          expected: { name: "minecraft:stone_bricks", properties: {} },
          found: { name: "minecraft:glass", properties: {} },
        },
      ],
    });
    render(<ParametricPanel />);

    await user.click(screen.getByRole("button", { name: /Old House/ }));

    expect(parametricApi.inspect).toHaveBeenCalledWith("old-house");
    expect(await screen.findByText(/conflicts: 1/)).toBeInTheDocument();
    expect(screen.getByText(/Manual edits no longer match the recipe/)).toBeInTheDocument();
    expect(
      screen.getByText(/expected minecraft:stone_bricks found minecraft:glass/),
    ).toBeInTheDocument();
    expect(screen.getByLabelText("Force rollback")).toBeInTheDocument();
  });

  it("creates a deterministic house from the current selection", async () => {
    const user = userEvent.setup();
    const created = { ...house, id: "new-house", name: "New House", generatedBlocks: 160 };
    parametricApi.create.mockResolvedValue({
      object: created,
      operation: null,
      conflicts: [],
    });
    parametricApi.inspect.mockResolvedValue({
      object: created,
      generatedBlocks: 160,
      conflictCount: 0,
      conflicts: [],
    });
    parametricApi.fetch.mockResolvedValue([house, created]);
    render(<ParametricPanel />);

    await user.clear(screen.getByLabelText("Parametric object name"));
    await user.type(screen.getByLabelText("Parametric object name"), "New House");
    await user.click(screen.getByRole("button", { name: "Use selection" }));
    await user.click(screen.getByRole("button", { name: "Create house" }));

    await waitFor(() =>
      expect(parametricApi.create).toHaveBeenCalledWith(
        expect.objectContaining({
          name: "New House",
          generator: "house",
          generatorVersion: 1,
          dimension: "minecraft:overworld",
          origin: { x: 0, y: 60, z: 0 },
          params: expect.objectContaining({ width: 16, depth: 16, height: 19 }),
        }),
      ),
    );
    expect(parametricApi.inspect).toHaveBeenCalledWith("new-house");
    expect(await screen.findByText(/generated: 160/)).toBeInTheDocument();
  });

  it("requires an explicit override before a forced rollback discards manual edits", async () => {
    const user = userEvent.setup();
    parametricApi.inspect.mockResolvedValue({
      object: house,
      generatedBlocks: 138,
      conflictCount: 1,
      conflicts: [
        {
          pos: { x: 1, y: 61, z: 0 },
          expected: { name: "minecraft:stone_bricks", properties: {} },
          found: { name: "minecraft:glass", properties: {} },
        },
      ],
    });
    parametricApi.rollback.mockResolvedValue({
      object: house,
      operation: null,
      conflicts: [],
    });
    render(<ParametricPanel />);

    await user.click(screen.getByRole("button", { name: /Old House/ }));
    await user.click(await screen.findByRole("button", { name: "Roll back" }));

    expect(await screen.findByText(/Enable forced rollback/)).toBeInTheDocument();
    expect(parametricApi.rollback).not.toHaveBeenCalled();

    await user.click(screen.getByLabelText("Force rollback"));
    await user.click(screen.getByRole("button", { name: "Roll back" }));

    await waitFor(() => expect(parametricApi.rollback).toHaveBeenCalledWith("old-house", true));
  });
});
