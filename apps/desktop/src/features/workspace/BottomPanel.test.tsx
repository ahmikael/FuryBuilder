import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type * as IpcModule from "@/lib/ipc";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CatalogSnapshot } from "@/assets/types";
import type { BlockDefinition } from "@/editor/types";
import { BOTTOM_PANEL_TABS } from "@/editor/layout";
import { BottomPanel } from "@/features/workspace/BottomPanel";
import { useEditorStore } from "@/state/editor-store";
import { useToolStore } from "@/state/tool-store";

const runtime = vi.hoisted(() => ({ tauri: true }));
const catalogApi = vi.hoisted(() => ({ fetch: vi.fn(), project: vi.fn() }));

vi.mock("@/lib/ipc", async (importOriginal) => {
  const actual = await importOriginal<typeof IpcModule>();
  return { ...actual, isTauriRuntime: () => runtime.tauri };
});

vi.mock("@/lib/world-api", () => ({
  fetchBlockCatalog: catalogApi.fetch,
}));

vi.mock("@/lib/assets-api", () => ({
  loadProjectCatalog: catalogApi.project,
}));

const curatedCatalog: BlockDefinition[] = [
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

const projectCatalog: CatalogSnapshot = {
  schemaVersion: 1,
  catalogId: "java-1.21.4-0123456789abcdef",
  catalogHash: "a".repeat(64),
  minecraftVersion: "1.21.4",
  dataVersion: 4189,
  clientJar: "client.jar",
  clientJarSha1: "b".repeat(40),
  resourcePackFormat: 46,
  datapackFormat: 61,
  blocks: [
    {
      id: "minecraft:stone",
      displayName: "Stone",
      properties: {},
      defaultProperties: {},
      variants: 1,
      multipart: false,
      model: "minecraft:block/stone",
      iconTexture: "minecraft:block/stone",
      tags: [],
      hasItem: true,
    },
    {
      id: "minecraft:oak_stairs",
      displayName: "Oak Stairs",
      properties: {
        facing: ["east", "north"],
        half: ["bottom", "top"],
        shape: ["straight"],
        waterlogged: ["false", "true"],
      },
      defaultProperties: {
        facing: "north",
        half: "bottom",
        shape: "straight",
        waterlogged: "false",
      },
      variants: 2,
      multipart: false,
      model: "minecraft:block/oak_stairs",
      iconTexture: "minecraft:block/oak_planks",
      tags: [],
      hasItem: true,
    },
    {
      id: "minecraft:oak_slab",
      displayName: "Oak Slab",
      properties: { type: ["bottom", "top"], waterlogged: ["false", "true"] },
      defaultProperties: { type: "bottom", waterlogged: "false" },
      variants: 2,
      multipart: false,
      model: "minecraft:block/oak_slab",
      iconTexture: "minecraft:block/oak_planks",
      tags: [],
      hasItem: true,
    },
    {
      id: "minecraft:chiseled_deepslate",
      displayName: "Chiseled Deepslate",
      properties: {},
      defaultProperties: {},
      variants: 1,
      multipart: false,
      model: "minecraft:block/chiseled_deepslate",
      iconTexture: "minecraft:block/chiseled_deepslate",
      tags: [],
      hasItem: true,
    },
    {
      id: "minecraft:oak_fence",
      displayName: "Oak Fence",
      properties: { east: ["true"], north: ["true"], south: ["true"], west: ["true"] },
      defaultProperties: null,
      variants: 0,
      multipart: true,
      model: "minecraft:block/oak_fence_post",
      iconTexture: "minecraft:block/oak_planks",
      tags: [],
      hasItem: true,
    },
  ],
  items: [],
  atlases: [],
  counts: {
    blocks: 5,
    items: 0,
    blocksWithItems: 5,
    itemOnly: 0,
    blockstateFiles: 5,
    blockModels: 5,
    itemModels: 0,
    itemDefinitions: 0,
    textures: 0,
    languages: 1,
    langEntries: 5,
    blockTags: 0,
    itemTags: 0,
    atlases: 0,
    atlasSources: 0,
  },
  warnings: [],
  cacheHit: false,
};

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
  catalogApi.project.mockReset();
  catalogApi.fetch.mockResolvedValue(curatedCatalog);
  catalogApi.project.mockResolvedValue(projectCatalog);
  useEditorStore.setState({ bottomPanelTab: "blocks", bottomPanelOpen: true });
  useToolStore.setState({
    selectedBlock: { name: "minecraft:stone", properties: {} },
    replaceFrom: null,
    selection: null,
    tool: "place",
  });
});

describe("block palette", () => {
  it("loads the project-pinned client JAR catalog and filters by id or display name", async () => {
    const user = userEvent.setup();
    render(<BottomPanel backend={backend} />);
    const search = await screen.findByLabelText("Search blocks");
    await waitFor(() => expect(catalogApi.project).toHaveBeenCalledTimes(1));
    expect(catalogApi.fetch).not.toHaveBeenCalled();
    expect(screen.getByText("Client JAR · 1.21.4")).toBeInTheDocument();
    expect(screen.getByText("chiseled_deepslate")).toBeInTheDocument();

    await user.type(search, "deep");
    expect(screen.getByText("chiseled_deepslate")).toBeInTheDocument();
    expect(screen.queryByText("stone")).not.toBeInTheDocument();

    await user.clear(search);
    await user.type(search, "Oak Stairs");
    expect(screen.getByText("oak_stairs")).toBeInTheDocument();
    expect(screen.queryByText("stone")).not.toBeInTheDocument();
  });

  it("selects catalog defaults and edits individual properties", async () => {
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

  it("shows multipart blocks without inventing a default state", async () => {
    const user = userEvent.setup();
    render(<BottomPanel backend={backend} />);
    await screen.findByText("oak_fence");
    await user.click(screen.getByText("oak_fence"));
    expect(useToolStore.getState().selectedBlock).toEqual({
      name: "minecraft:oak_fence",
      properties: {},
    });
    expect(screen.getByText(/multipart states are assembled by the game/i)).toBeInTheDocument();
  });

  it("falls back to curated quick picks with an explicit notice", async () => {
    catalogApi.project.mockRejectedValue(new Error("the client JAR moved"));
    render(<BottomPanel backend={backend} />);
    expect(await screen.findByText("Curated fallback")).toBeInTheDocument();
    expect(screen.getByText(/the client JAR moved/)).toBeInTheDocument();
    expect(screen.getByText("Curated quick picks")).toBeInTheDocument();
    expect(await screen.findByText("oak_stairs")).toBeInTheDocument();
    expect(catalogApi.fetch).toHaveBeenCalledTimes(1);
  });

  it("treats a placeholder project as an explicit curated fallback", async () => {
    catalogApi.project.mockResolvedValue(null);
    render(<BottomPanel backend={backend} />);
    expect(await screen.findByText("Curated fallback")).toBeInTheDocument();
    expect(screen.getByText(/placeholder textures/)).toBeInTheDocument();
    expect(await screen.findByText("stone")).toBeInTheDocument();
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

  it("groups observation and validation under Analysis", async () => {
    const user = userEvent.setup();
    render(<BottomPanel backend={backend} />);

    expect(screen.getByRole("tab", { name: /Blocks/ })).toHaveAttribute("aria-selected", "true");
    expect(screen.queryByRole("tab", { name: /Tools/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("tab", { name: /Warnings/ })).not.toBeInTheDocument();

    await user.click(screen.getByRole("tab", { name: /Analysis/ }));

    expect(screen.getByRole("tab", { name: /Observation/ })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(screen.getByRole("tab", { name: /Validation/ })).toBeInTheDocument();
    expect(screen.queryByRole("tab", { name: /Console/ })).not.toBeInTheDocument();
  });

  it("groups console and history under Activity", async () => {
    const user = userEvent.setup();
    render(<BottomPanel backend={backend} />);

    await user.click(screen.getByRole("tab", { name: /Activity/ }));

    expect(screen.getByRole("tab", { name: /Console/ })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tab", { name: /History/ })).toBeInTheDocument();
    expect(screen.queryByRole("tab", { name: /Observation/ })).not.toBeInTheDocument();
  });

  it("renders exactly the shared bottom-panel tabs", () => {
    render(<BottomPanel backend={backend} />);

    const tabs = screen.getAllByRole("tab").map((tab) => tab.textContent?.trim());
    expect(tabs).toEqual(BOTTOM_PANEL_TABS.map((tab) => tab.label));
  });

  it("does not fake a catalog in browser preview", () => {
    runtime.tauri = false;
    render(<BottomPanel backend={{ ...backend, status: "offline", info: null }} />);
    expect(screen.queryByLabelText("Search blocks")).not.toBeInTheDocument();
    expect(catalogApi.project).not.toHaveBeenCalled();
    expect(catalogApi.fetch).not.toHaveBeenCalled();
  });
});
