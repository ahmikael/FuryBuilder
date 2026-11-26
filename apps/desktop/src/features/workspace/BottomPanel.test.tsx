import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type * as IpcModule from "@/lib/ipc";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CatalogSnapshot } from "@/assets/types";
import type { BlockDefinition } from "@/editor/types";
import { BOTTOM_PANEL_TABS } from "@/editor/layout";
import { BottomPanel } from "@/features/workspace/BottomPanel";
import { useEditorStore } from "@/state/editor-store";
import { usePaletteStore } from "@/state/palette-store";
import { useToolStore } from "@/state/tool-store";
import { chooseSelectOption } from "@/test-utils/select";

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
  schemaVersion: 2,
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
      propertyValuesComplete: false,
      defaultProperties: {},
      variants: 1,
      multipart: false,
      model: "minecraft:block/stone",
      iconTexture: "minecraft:block/stone",
      tags: ["minecraft:base_stone_overworld"],
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
      propertyValuesComplete: true,
      defaultProperties: null,
      variants: 2,
      multipart: false,
      model: "minecraft:block/oak_stairs",
      iconTexture: "minecraft:block/oak_planks",
      tags: ["minecraft:wooden_stairs"],
      hasItem: true,
    },
    {
      id: "minecraft:oak_slab",
      displayName: "Oak Slab",
      properties: { type: ["bottom", "top"], waterlogged: ["false", "true"] },
      propertyValuesComplete: true,
      defaultProperties: null,
      variants: 2,
      multipart: false,
      model: "minecraft:block/oak_slab",
      iconTexture: "minecraft:block/oak_planks",
      tags: ["minecraft:wooden_slabs"],
      hasItem: true,
    },
    {
      id: "minecraft:chiseled_deepslate",
      displayName: "Chiseled Deepslate",
      properties: {},
      propertyValuesComplete: false,
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
      propertyValuesComplete: false,
      defaultProperties: null,
      variants: 0,
      multipart: true,
      model: "minecraft:block/oak_fence_post",
      iconTexture: "minecraft:block/oak_planks",
      tags: ["minecraft:wooden_fences"],
      hasItem: true,
    },
    {
      id: "minecraft:oak_log",
      displayName: "Oak Log",
      properties: {},
      propertyValuesComplete: false,
      defaultProperties: {},
      variants: 1,
      multipart: false,
      model: "minecraft:block/oak_log",
      iconTexture: "minecraft:block/oak_log",
      tags: ["minecraft:oak_logs"],
      hasItem: true,
    },
    {
      id: "minecraft:redstone_lamp",
      displayName: "Redstone Lamp",
      properties: {},
      propertyValuesComplete: false,
      defaultProperties: {},
      variants: 1,
      multipart: false,
      model: "minecraft:block/redstone_lamp",
      iconTexture: "minecraft:block/redstone_lamp",
      tags: [],
      hasItem: true,
    },
    {
      id: "minecraft:crafting_table",
      displayName: "Crafting Table",
      properties: {},
      propertyValuesComplete: false,
      defaultProperties: {},
      variants: 1,
      multipart: false,
      model: "minecraft:block/crafting_table",
      iconTexture: "minecraft:block/crafting_table",
      tags: [],
      hasItem: true,
    },
    {
      id: "minecraft:command_block",
      displayName: "Command Block",
      properties: {},
      propertyValuesComplete: false,
      defaultProperties: {},
      variants: 1,
      multipart: false,
      model: "minecraft:block/command_block",
      iconTexture: "minecraft:block/command_block",
      tags: [],
      hasItem: true,
    },
  ],
  items: [],
  atlases: [],
  counts: {
    blocks: 9,
    items: 0,
    blocksWithItems: 9,
    itemOnly: 0,
    blockstateFiles: 9,
    blockModels: 9,
    itemModels: 0,
    itemDefinitions: 0,
    textures: 0,
    languages: 1,
    langEntries: 9,
    blockTags: 4,
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
    targetProfileId: "java-classic-1.21.4",
    targetDataVersion: 4189,
  },
  error: null,
};

beforeEach(() => {
  runtime.tauri = true;
  localStorage.clear();
  usePaletteStore.setState({ catalogs: {} });
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

  it("filters by basic category and keeps recent and favorite blocks", async () => {
    const user = userEvent.setup();
    render(<BottomPanel backend={backend} />);
    const list = await screen.findByRole("list", { name: "Block palette" });

    expect(within(list).getByText("redstone_lamp")).toBeInTheDocument();
    expect(within(list).getByText("oak_log")).toBeInTheDocument();
    expect(within(list).getByText("command_block")).toBeInTheDocument();

    await chooseSelectOption(user, "Block category", /^Redstone/);
    expect(within(list).getByText("redstone_lamp")).toBeInTheDocument();
    expect(within(list).queryByText("oak_log")).not.toBeInTheDocument();
    expect(within(list).queryByText("stone")).not.toBeInTheDocument();

    await chooseSelectOption(user, "Block category", /^All/);
    await user.click(within(list).getByText("oak_log"));
    expect(usePaletteStore.getState().catalogs[projectCatalog.catalogId]?.recent).toEqual([
      "minecraft:oak_log",
    ]);

    await chooseSelectOption(user, "Block category", /^Recent/);
    expect(within(list).getByText("oak_log")).toBeInTheDocument();
    expect(within(list).queryByText("redstone_lamp")).not.toBeInTheDocument();

    await chooseSelectOption(user, "Block category", /^All/);
    await user.click(screen.getByRole("button", { name: "Favorite oak_log", pressed: false }));
    expect(usePaletteStore.getState().catalogs[projectCatalog.catalogId]?.favorites).toEqual([
      "minecraft:oak_log",
    ]);

    await chooseSelectOption(user, "Block category", /^Favorites/);
    expect(within(list).getByText("oak_log")).toBeInTheDocument();
    expect(within(list).queryByText("stone")).not.toBeInTheDocument();
    const search = screen.getByRole("textbox", { name: "Search blocks" });
    await user.type(search, "stone");
    expect(within(list).getByRole("status")).toHaveTextContent("No block matches");
    await user.clear(search);
    await user.click(screen.getByRole("button", { name: "Favorite oak_log", pressed: true }));
    expect(within(list).getByRole("status")).toHaveTextContent("No favorites yet");
  });

  it("isolates palette history between pinned catalogs and hides ids missing from the current one", async () => {
    const user = userEvent.setup();
    const olderCatalog = {
      ...projectCatalog,
      catalogId: "java-1.20.6-fedcba9876543210",
      minecraftVersion: "1.20.6",
      blocks: projectCatalog.blocks.filter((block) => block.id === "minecraft:stone"),
      counts: { ...projectCatalog.counts, blocks: 1 },
    };
    const { recordRecent, toggleFavorite } = usePaletteStore.getState();
    recordRecent(projectCatalog.catalogId, "minecraft:stone");
    toggleFavorite(projectCatalog.catalogId, "minecraft:stone");
    toggleFavorite(olderCatalog.catalogId, "minecraft:oak_log");

    const current = render(<BottomPanel backend={backend} />);
    await screen.findByText("Client JAR · 1.21.4");
    await chooseSelectOption(user, "Block category", /^Favorites/);
    expect(screen.getByRole("button", { name: "Favorite stone", pressed: true })).toBeVisible();
    current.unmount();

    catalogApi.project.mockResolvedValue(olderCatalog);
    render(<BottomPanel backend={backend} />);
    await screen.findByText("Client JAR · 1.20.6");
    expect(screen.getByRole("button", { name: "Favorite stone", pressed: false })).toBeVisible();
    await chooseSelectOption(user, "Block category", /^Favorites/);
    expect(screen.getByText(/No favorites yet/)).toBeVisible();
    await chooseSelectOption(user, "Block category", /^Recent/);
    expect(screen.getByText(/No recent blocks yet/)).toBeVisible();
  });

  it("selects the game default and edits individual properties", async () => {
    const user = userEvent.setup();
    render(<BottomPanel backend={backend} />);
    await screen.findByText("oak_stairs");
    await user.click(screen.getByText("oak_stairs"));
    expect(useToolStore.getState().selectedBlock).toEqual({
      name: "minecraft:oak_stairs",
      properties: {},
    });
    expect(screen.getByLabelText("Property facing")).toHaveValue("");
    expect(screen.getByLabelText("Property waterlogged")).toHaveValue("");

    const facing = screen.getByLabelText("Property facing");
    await user.type(facing, "south");
    expect(useToolStore.getState().selectedBlock.properties?.facing).toBe("south");

    await user.click(screen.getByRole("button", { name: /Defaults/ }));
    expect(useToolStore.getState().selectedBlock.properties).toEqual({});
  });

  it("uses the game default when the blockstate declares no properties", async () => {
    const user = userEvent.setup();
    render(<BottomPanel backend={backend} />);
    await screen.findByText("stone");
    await user.click(screen.getByText("stone"));
    expect(useToolStore.getState().selectedBlock).toEqual({
      name: "minecraft:stone",
      properties: {},
    });
    expect(
      screen.getByText(/declares no properties; the game default is used/i),
    ).toBeInTheDocument();
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
    expect(screen.getByText(/declares only part of the state space/i)).toBeInTheDocument();
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
      properties: {},
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
