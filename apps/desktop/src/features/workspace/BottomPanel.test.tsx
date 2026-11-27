import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type * as IpcModule from "@/lib/ipc";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  CatalogEntry,
  CatalogFlags,
  CatalogIconAtlas,
  CatalogSnapshot,
  CatalogStates,
} from "@/assets/types";
import type { BlockDefinition } from "@/editor/types";
import { BOTTOM_PANEL_TABS } from "@/editor/layout";
import { BottomPanel } from "@/features/workspace/BottomPanel";
import { useEditorStore } from "@/state/editor-store";
import { usePaletteStore } from "@/state/palette-store";
import { useToolStore } from "@/state/tool-store";
import { chooseSelectOption } from "@/test-utils/select";

const runtime = vi.hoisted(() => ({ tauri: true }));
const catalogApi = vi.hoisted(() => ({
  fetch: vi.fn(),
  curated: vi.fn(),
  project: vi.fn(),
  atlas: vi.fn(),
}));

vi.mock("@/lib/ipc", async (importOriginal) => {
  const actual = await importOriginal<typeof IpcModule>();
  return { ...actual, isTauriRuntime: () => runtime.tauri };
});

vi.mock("@/lib/world-api", () => ({
  fetchBlockCatalog: catalogApi.fetch,
  fetchCuratedBlockCatalog: catalogApi.curated,
}));

vi.mock("@/lib/assets-api", () => ({
  loadProjectCatalog: catalogApi.project,
  loadProjectCatalogAtlas: catalogApi.atlas,
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

type FixtureStates = Omit<CatalogStates, "variantCount" | "variants"> & {
  variantCount?: number;
  variants?: CatalogStates["variants"];
};

function blockEntry(
  id: string,
  displayName: string,
  inputStates: FixtureStates,
  iconTexture: string,
  tags: string[],
  inputFlags: Partial<CatalogFlags> = {},
): CatalogEntry {
  const states: CatalogStates = {
    ...inputStates,
    variantCount: inputStates.variantCount ?? (inputStates.multipart ? 0 : 1),
    variants: inputStates.variants ?? [],
  };
  return {
    id,
    legacyIds: [],
    displayName,
    category: null,
    tags,
    iconTexture,
    aliases: [],
    states,
    itemForm: {
      itemDefinition: true,
      modelType: "minecraft:model",
      model: iconTexture,
      legacyIds: [],
    },
    flags: {
      placeable: true,
      hasItem: true,
      obtainable: null,
      creativeOnly: null,
      technical: null,
      fluid: null,
      airlike: null,
      blockEntity: null,
      transparent: null,
      fullCube: null,
      gravity: null,
      waterloggable: null,
      tinted: null,
      hasVariants: states.multipart || states.variantCount > 1,
      ...inputFlags,
    },
  };
}

const projectCatalog: CatalogSnapshot = {
  schemaVersion: 5,
  catalogId: "java-1.21.4-0123456789abcdef",
  catalogHash: "a".repeat(64),
  minecraftVersion: "1.21.4",
  dataVersion: 4189,
  clientJar: "client.jar",
  clientJarSha1: "b".repeat(40),
  resourcePackFormat: 46,
  datapackFormat: 61,
  entries: [
    blockEntry(
      "minecraft:stone",
      "Stone",
      {
        properties: {},
        propertyValuesComplete: false,
        defaultProperties: {},
        variantCount: 1,
        multipart: false,
        model: "minecraft:block/stone",
      },
      "minecraft:block/stone",
      ["minecraft:base_stone_overworld"],
    ),
    blockEntry(
      "minecraft:oak_stairs",
      "Oak Stairs",
      {
        properties: {
          facing: ["east", "north"],
          half: ["bottom", "top"],
          shape: ["straight"],
          waterlogged: ["false", "true"],
        },
        propertyValuesComplete: true,
        defaultProperties: null,
        variantCount: 2,
        multipart: false,
        model: "minecraft:block/oak_stairs",
      },
      "minecraft:block/oak_planks",
      ["minecraft:wooden_stairs"],
    ),
    blockEntry(
      "minecraft:oak_slab",
      "Oak Slab",
      {
        properties: { type: ["bottom", "top"], waterlogged: ["false", "true"] },
        propertyValuesComplete: true,
        defaultProperties: null,
        variantCount: 2,
        multipart: false,
        model: "minecraft:block/oak_slab",
      },
      "minecraft:block/oak_planks",
      ["minecraft:wooden_slabs"],
    ),
    blockEntry(
      "minecraft:chiseled_deepslate",
      "Chiseled Deepslate",
      {
        properties: {},
        propertyValuesComplete: false,
        defaultProperties: {},
        variantCount: 1,
        multipart: false,
        model: "minecraft:block/chiseled_deepslate",
      },
      "minecraft:block/chiseled_deepslate",
      [],
    ),
    blockEntry(
      "minecraft:oak_fence",
      "Oak Fence",
      {
        properties: { east: ["true"], north: ["true"], south: ["true"], west: ["true"] },
        propertyValuesComplete: false,
        defaultProperties: null,
        variantCount: 0,
        multipart: true,
        model: "minecraft:block/oak_fence_post",
      },
      "minecraft:block/oak_planks",
      ["minecraft:wooden_fences"],
    ),
    blockEntry(
      "minecraft:oak_log",
      "Oak Log",
      {
        properties: {},
        propertyValuesComplete: false,
        defaultProperties: {},
        variantCount: 1,
        multipart: false,
        model: "minecraft:block/oak_log",
      },
      "minecraft:block/oak_log",
      ["minecraft:oak_logs"],
    ),
    blockEntry(
      "minecraft:redstone_lamp",
      "Redstone Lamp",
      {
        properties: {},
        propertyValuesComplete: false,
        defaultProperties: {},
        variantCount: 1,
        multipart: false,
        model: "minecraft:block/redstone_lamp",
      },
      "minecraft:block/redstone_lamp",
      [],
    ),
    blockEntry(
      "minecraft:crafting_table",
      "Crafting Table",
      {
        properties: {},
        propertyValuesComplete: false,
        defaultProperties: {},
        variantCount: 1,
        multipart: false,
        model: "minecraft:block/crafting_table",
      },
      "minecraft:block/crafting_table",
      [],
    ),
    blockEntry(
      "minecraft:command_block",
      "Command Block",
      {
        properties: {},
        propertyValuesComplete: false,
        defaultProperties: {},
        variantCount: 1,
        multipart: false,
        model: "minecraft:block/command_block",
      },
      "minecraft:block/command_block",
      [],
      { technical: true, creativeOnly: true, obtainable: false },
    ),
  ],
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
  catalogApi.curated.mockReset();
  catalogApi.project.mockReset();
  catalogApi.atlas.mockReset();
  catalogApi.fetch.mockResolvedValue(curatedCatalog);
  catalogApi.curated.mockResolvedValue(curatedCatalog);
  catalogApi.project.mockResolvedValue(projectCatalog);
  catalogApi.atlas.mockResolvedValue(null);
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
    expect(catalogApi.curated).toHaveBeenCalledTimes(1);
    expect(screen.getByText("Client JAR · 1.21.4")).toBeInTheDocument();
    expect(screen.getByText("chiseled_deepslate")).toBeInTheDocument();

    await user.type(search, "deep");
    expect(screen.getByText("chiseled_deepslate")).toBeInTheDocument();
    expect(screen.queryByText("stone")).not.toBeInTheDocument();

    await user.clear(search);
    await user.type(search, "Oak Stairs");
    expect(screen.getByText("oak_stairs")).toBeInTheDocument();
    expect(screen.queryByText("stone")).not.toBeInTheDocument();

    await user.clear(search);
    await user.type(search, "base_stone_overworld");
    expect(screen.getByText("stone")).toBeInTheDocument();
    expect(screen.queryByText("oak_stairs")).not.toBeInTheDocument();
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

  it("filters by catalog flags and labels special entries", async () => {
    const user = userEvent.setup();
    render(<BottomPanel backend={backend} />);
    const list = await screen.findByRole("list", { name: "Block palette" });

    await chooseSelectOption(user, "Block filter", /^Technical/);
    expect(within(list).getByText("command_block")).toBeInTheDocument();
    expect(within(list).queryByText("stone")).not.toBeInTheDocument();

    await chooseSelectOption(user, "Block filter", /^Creative only/);
    expect(within(list).getByText("command_block")).toBeInTheDocument();
    expect(within(list).queryByText("oak_log")).not.toBeInTheDocument();

    await user.click(within(list).getByText("command_block"));
    const details = screen.getByText("minecraft:command_block").closest('[data-slot="card"]');
    expect(details).not.toBeNull();
    expect(within(details as HTMLElement).getByText("Creative only")).toBeVisible();
    expect(within(details as HTMLElement).getAllByText("Technical")).toHaveLength(1);

    await chooseSelectOption(user, "Block filter", /^Block entity/);
    expect(within(list).getByRole("status")).toHaveTextContent("No blocks match block entity");

    await chooseSelectOption(user, "Block filter", /^Any block/);
    expect(within(list).getByText("stone")).toBeInTheDocument();
  });

  it("isolates palette history between pinned catalogs and hides ids missing from the current one", async () => {
    const user = userEvent.setup();
    const olderCatalog = {
      ...projectCatalog,
      catalogId: "java-1.20.6-fedcba9876543210",
      minecraftVersion: "1.20.6",
      entries: projectCatalog.entries.filter((entry) => entry.id === "minecraft:stone"),
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

  it("selects the game default and constrains complete property values", async () => {
    const user = userEvent.setup();
    render(<BottomPanel backend={backend} />);
    await screen.findByText("oak_stairs");
    await user.click(screen.getByText("oak_stairs"));
    expect(useToolStore.getState().selectedBlock).toEqual({
      name: "minecraft:oak_stairs",
      properties: {},
    });
    expect(screen.getByRole("combobox", { name: "Property facing" })).toHaveTextContent(
      "Game default",
    );
    expect(screen.getByRole("combobox", { name: "Property waterlogged" })).toHaveTextContent(
      "Game default",
    );

    await chooseSelectOption(user, "Property facing", /^north$/);
    expect(useToolStore.getState().selectedBlock.properties?.facing).toBe("north");
    expect(screen.getByRole("combobox", { name: "Property facing" })).toHaveTextContent("north");

    await chooseSelectOption(user, "Property facing", /^Game default$/);
    expect(useToolStore.getState().selectedBlock.properties).toEqual({});

    await chooseSelectOption(user, "Property half", /^top$/);
    await user.click(screen.getByRole("button", { name: /Game default/ }));
    expect(useToolStore.getState().selectedBlock.properties).toEqual({});
  });

  it("keeps an imported value outside the declared set visible", async () => {
    useToolStore.setState({
      selectedBlock: { name: "minecraft:oak_stairs", properties: { facing: "south" } },
    });
    const user = userEvent.setup();
    render(<BottomPanel backend={backend} />);
    await screen.findByText("oak_stairs");

    expect(screen.getByText("facing=south", { selector: "span" })).toBeVisible();
    expect(screen.getByText(/Imported value facing=south is not declared/i)).toBeVisible();
    await chooseSelectOption(user, "Property facing", /^Game default$/);
    expect(useToolStore.getState().selectedBlock.properties).toEqual({});
  });

  it("warns about an unknown selected state and disables its default action", async () => {
    useToolStore.setState({
      selectedBlock: { name: "minecraft:future_block", properties: { axis: "y" } },
      replaceFrom: null,
    });
    const user = userEvent.setup();
    render(<BottomPanel backend={backend} />);
    expect(await screen.findByText("minecraft:future_block")).toBeVisible();
    expect(screen.getByText(/absent from the exact 1\.21\.4 catalog/i)).toBeVisible();
    expect(screen.getByRole("button", { name: /Game default/ })).toBeDisabled();
    expect(screen.getByRole("button", { name: /Game default/ })).toHaveAttribute(
      "title",
      "Unavailable: this state is not declared by the exact catalog.",
    );

    await user.click(screen.getByRole("button", { name: /Use as replace source/ }));
    expect(useToolStore.getState().replaceFrom).toEqual({
      name: "minecraft:future_block",
      properties: { axis: "y" },
    });
  });

  it("does not claim a version for an unknown state without an exact catalog", async () => {
    catalogApi.project.mockResolvedValue(null);
    useToolStore.setState({
      selectedBlock: { name: "minecraft:future_block", properties: { axis: "y" } },
    });
    render(<BottomPanel backend={backend} />);
    expect(await screen.findByText("Unknown state")).toBeVisible();
    expect(
      screen.getByText(/No exact client JAR catalog is loaded, so this state cannot be matched/i),
    ).toBeVisible();
    expect(screen.queryByText(/\bundefined\b/)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Game default/ })).toHaveAttribute(
      "title",
      "Unavailable: no exact catalog is loaded.",
    );
  });

  it("renders only the virtualized window and moves it on scroll", async () => {
    const entries = Array.from({ length: 200 }, (_, index) =>
      blockEntry(
        `minecraft:test_block_${index}`,
        `Test Block ${index}`,
        {
          properties: {},
          propertyValuesComplete: false,
          defaultProperties: {},
          variantCount: 1,
          multipart: false,
          model: "minecraft:block/stone",
        },
        index < 100 ? "minecraft:block/stone" : "minecraft:block/dirt",
        [],
      ),
    );
    catalogApi.project.mockResolvedValue({
      ...projectCatalog,
      entries,
      counts: { ...projectCatalog.counts, blocks: entries.length },
    });
    render(<BottomPanel backend={backend} />);
    const list = await screen.findByRole("list", { name: "Block palette" });
    expect(within(list).getByText("test_block_0")).toBeVisible();
    expect(within(list).queryByText("test_block_150")).not.toBeInTheDocument();
    expect(within(list).getAllByRole("listitem").length).toBeLessThan(entries.length);
    await waitFor(() =>
      expect(
        catalogApi.atlas.mock.calls.some(([keys]) =>
          (keys as string[]).includes("minecraft:block/stone"),
        ),
      ).toBe(true),
    );
    expect(
      catalogApi.atlas.mock.calls.some(([keys]) =>
        (keys as string[]).includes("minecraft:block/dirt"),
      ),
    ).toBe(false);

    fireEvent.scroll(list, { target: { scrollTop: 140 * 32 } });
    await waitFor(() => expect(within(list).getByText("test_block_140")).toBeInTheDocument());
    expect(within(list).queryByText("test_block_0")).not.toBeInTheDocument();
    await waitFor(() =>
      expect(
        catalogApi.atlas.mock.calls.some(([keys]) =>
          (keys as string[]).includes("minecraft:block/dirt"),
        ),
      ).toBe(true),
    );
  });

  it("draws atlas icons and an isometric preview", async () => {
    const atlas: CatalogIconAtlas = {
      schemaVersion: 2,
      cacheKey: "java-1.21.4-atlas",
      cacheHit: false,
      catalogId: projectCatalog.catalogId,
      catalogHash: projectCatalog.catalogHash,
      catalogSchemaVersion: projectCatalog.schemaVersion,
      minecraftVersion: projectCatalog.minecraftVersion,
      clientJarSha1: projectCatalog.clientJarSha1 ?? "",
      packStackHash: "pack-stack",
      width: 64,
      height: 64,
      pngBase64: "iVBORw0KGgo=",
      textures: [
        {
          key: "minecraft:block/stone",
          path: "assets/minecraft/textures/block/stone.png",
          x: 0,
          y: 0,
          width: 16,
          height: 16,
          sourceWidth: 16,
          sourceHeight: 16,
          missing: false,
          hasAlpha: false,
        },
      ],
      requestedTextureCount: 1,
      totalIconTextureCount: 9,
      warnings: [],
      packIssues: [],
    };
    catalogApi.atlas.mockResolvedValue(atlas);
    const user = userEvent.setup();
    render(<BottomPanel backend={backend} />);
    await screen.findByText("stone");
    await waitFor(() =>
      expect(document.querySelector('[data-catalog-icon="sprite"]')).not.toBeNull(),
    );
    expect(catalogApi.atlas).toHaveBeenCalled();
    expect(catalogApi.atlas.mock.calls[0][0]).toContain("minecraft:block/stone");

    await user.click(screen.getByText("stone"));
    expect(document.querySelector('[data-catalog-icon="cube"]')).not.toBeNull();
  });

  it("lists quick picks that are proven absent from the exact version", async () => {
    catalogApi.curated.mockResolvedValue([
      ...curatedCatalog,
      { name: "minecraft:copper_bulb", properties: { lit: "false" } },
    ]);
    const user = userEvent.setup();
    render(<BottomPanel backend={backend} />);
    await screen.findByText("Client JAR · 1.21.4");
    await user.click(screen.getByRole("button", { name: /Quick picks not in this version/ }));
    const unavailable = screen.getByRole("list", {
      name: "Quick picks not in this version",
    });
    const row = within(unavailable).getByText("copper_bulb").closest('[role="listitem"]');
    expect(row).toHaveAttribute("aria-disabled", "true");
    expect(within(row as HTMLElement).getByText("Not in this version")).toBeVisible();
    expect(within(row as HTMLElement).getByText(/Absent from 1\.21\.4/)).toBeVisible();
    expect(screen.getByText(/not a complete cross-version catalog diff/i)).toBeVisible();
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
    expect(catalogApi.curated).toHaveBeenCalledTimes(1);
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
    expect(catalogApi.curated).not.toHaveBeenCalled();
  });
});
