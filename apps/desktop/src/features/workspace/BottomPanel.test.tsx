import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type * as IpcModule from "@/lib/ipc";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { BlockDefinition } from "@/editor/types";
import { BottomPanel } from "@/features/workspace/BottomPanel";
import { useToolStore } from "@/state/tool-store";

const runtime = vi.hoisted(() => ({ tauri: true }));
const catalogApi = vi.hoisted(() => ({ fetch: vi.fn() }));

vi.mock("@/lib/ipc", async (importOriginal) => {
  const actual = await importOriginal<typeof IpcModule>();
  return { ...actual, isTauriRuntime: () => runtime.tauri };
});

vi.mock("@/lib/world-api", () => ({
  fetchBlockCatalog: catalogApi.fetch,
}));

const catalog: BlockDefinition[] = [
  { name: "minecraft:stone", properties: {} },
  {
    name: "minecraft:oak_stairs",
    properties: { facing: "north", half: "bottom", shape: "straight", waterlogged: "false" },
  },
  {
    name: "minecraft:oak_slab",
    properties: { type: "bottom", waterlogged: "false" },
  },
];

const backend = {
  status: "online" as const,
  info: {
    name: "FuryBuilder",
    version: "0.1.0",
    targetMinecraftVersion: "1.21.4",
    targetDataVersion: 4189,
  },
  error: null,
};

beforeEach(() => {
  runtime.tauri = true;
  catalogApi.fetch.mockReset();
  catalogApi.fetch.mockResolvedValue(catalog);
  useToolStore.setState({
    selectedBlock: { name: "minecraft:stone", properties: {} },
    replaceFrom: null,
    selection: null,
    tool: "place",
  });
});

describe("block palette", () => {
  it("loads the Rust catalog and filters by search", async () => {
    const user = userEvent.setup();
    render(<BottomPanel backend={backend} />);
    const search = await screen.findByLabelText("Search blocks");
    await waitFor(() => expect(catalogApi.fetch).toHaveBeenCalledTimes(1));
    expect(screen.getByText("stone")).toBeInTheDocument();
    expect(screen.getByText("oak_stairs")).toBeInTheDocument();

    await user.type(search, "stairs");
    expect(screen.getByText("oak_stairs")).toBeInTheDocument();
    expect(screen.queryByText("oak_slab")).not.toBeInTheDocument();
    expect(screen.queryByText("stone")).not.toBeInTheDocument();
  });

  it("selects canonical defaults and edits individual properties", async () => {
    const user = userEvent.setup();
    render(<BottomPanel backend={backend} />);
    await screen.findByText("oak_stairs");
    await user.click(screen.getByText("oak_stairs"));
    expect(useToolStore.getState().selectedBlock).toEqual({
      name: "minecraft:oak_stairs",
      properties: { facing: "north", half: "bottom", shape: "straight", waterlogged: "false" },
    });

    const facing = screen.getByLabelText("Property facing");
    await user.clear(facing);
    await user.type(facing, "south");
    expect(useToolStore.getState().selectedBlock.properties?.facing).toBe("south");

    await user.click(screen.getByRole("button", { name: /Defaults/ }));
    expect(useToolStore.getState().selectedBlock.properties?.facing).toBe("north");
  });

  it("can promote the palette state to the replace source", async () => {
    const user = userEvent.setup();
    render(<BottomPanel backend={backend} />);
    await screen.findByText("oak_slab");
    await user.click(screen.getByText("oak_slab"));
    await user.click(screen.getByRole("button", { name: /Use as replace source/ }));
    expect(useToolStore.getState().replaceFrom).toEqual({
      name: "minecraft:oak_slab",
      properties: { type: "bottom", waterlogged: "false" },
    });
  });

  it("does not fake a catalog in browser preview", () => {
    runtime.tauri = false;
    render(<BottomPanel backend={{ ...backend, status: "offline", info: null }} />);
    expect(screen.queryByLabelText("Search blocks")).not.toBeInTheDocument();
    expect(screen.getByText("The block palette requires the desktop app.")).toBeInTheDocument();
    expect(catalogApi.fetch).not.toHaveBeenCalled();
  });
});
