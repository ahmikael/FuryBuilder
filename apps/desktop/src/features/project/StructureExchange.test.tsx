import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type * as IpcModule from "@/lib/ipc";
import type { DimensionInfo } from "@/lib/contracts";
import { StructureExchange } from "@/features/project/StructureExchange";
import type {
  StructureExportPreview,
  StructureExportResult,
  StructureImportPreview,
} from "@/project/structure-types";
import type { ProjectSummary, SessionStatus } from "@/project/types";
import type { VersionDimension } from "@/project/version-types";
import { useProjectStore } from "@/state/project-store";
import { useToolStore } from "@/state/tool-store";
import { chooseSelectOption } from "@/test-utils/select";
import { clearToasts } from "@/test-utils/toast-log";
import { useWorldStore } from "@/state/world-store";

const api = vi.hoisted(() => ({
  readFile: vi.fn(),
  previewImport: vi.fn(),
  importProject: vi.fn(),
  insert: vi.fn(),
  previewExport: vi.fn(),
  export: vi.fn(),
}));

vi.mock("@/lib/structure-api", () => ({
  readStructureFile: api.readFile,
  previewStructureImport: api.previewImport,
  importStructureProject: api.importProject,
  insertStructure: api.insert,
  previewStructureExport: api.previewExport,
  exportStructure: api.export,
}));

vi.mock("@/lib/ipc", async (importOriginal) => {
  const actual = await importOriginal<typeof IpcModule>();
  return { ...actual, isTauriRuntime: () => true };
});

const summary: ProjectSummary = {
  folder: "Castle.furybuilder",
  projectId: "project-castle",
  name: "Castle",
  creationMode: "empty",
  targetMinecraftVersion: "1.21.4",
  targetProfileId: "java-classic-1.21.4",
  targetDataVersion: 4189,
  worldRevision: 4,
  updatedAtMs: 1_700_000_000_000,
  lastSavedAtMs: 1_700_000_000_000,
  lastSaveKind: "manual",
  bytes: 2048,
  schemaVersion: 1,
  recoverable: false,
  capabilities: {
    supportLevel: "full",
    read: true,
    edit: true,
    write: true,
    export: true,
    generate: true,
    assets: true,
  },
};

function session(): SessionStatus {
  return {
    folder: summary.folder,
    name: summary.name,
    savedRevision: 4,
    autosavedRevision: null,
    revision: 4,
    dirty: false,
    editorState: { selectedDimension: "minecraft:overworld", loadRadius: 1 },
    availableCheckpoints: 0,
  };
}

const modernDimensions: VersionDimension[] = [
  { id: "minecraft:overworld", kind: "overworld", minY: -64, height: 384, folder: "" },
  { id: "minecraft:the_nether", kind: "nether", minY: 0, height: 256, folder: "DIM-1" },
  { id: "minecraft:the_end", kind: "end", minY: 0, height: 256, folder: "DIM1" },
];

const worldDimensions: DimensionInfo[] = modernDimensions.map(({ id, kind, minY, height }) => ({
  id,
  kind,
  minY,
  height,
  maxY: minY + height - 1,
  blockCount: 0,
  sectionCount: 0,
  chunkCount: 0,
}));

const importPreview: StructureImportPreview = {
  format: "java_structure",
  dataVersion: 4189,
  targetDataVersion: 4189,
  targetMinecraftVersion: "1.21.4",
  dimensions: modernDimensions,
  size: [3, 2, 2],
  volume: 12,
  paletteSize: 2,
  palette: [
    { state: "minecraft:stone", count: 2 },
    { state: "minecraft:oak_stairs[facing=west,half=top]", count: 1 },
  ],
  storedBlocks: 3,
  nonAirBlocks: 3,
  entities: 0,
  blockEntities: 0,
  unknownTags: [],
  warnings: [],
  compatible: true,
};

const exportPreview: StructureExportPreview = {
  format: "java_structure",
  dataVersion: 4189,
  targetDataVersion: 4189,
  targetMinecraftVersion: "1.21.4",
  dimension: "minecraft:overworld",
  bounds: { min: { x: -2, y: 60, z: -2 }, max: { x: 2, y: 66, z: 2 } },
  size: [5, 7, 5],
  volume: 175,
  paletteSize: 3,
  palette: [{ state: "minecraft:stone", count: 140 }],
  nonAirBlocks: 140,
  storedBlocks: 140,
  bytes: 620,
  checksum: "a".repeat(64),
  fileName: "castle-overworld-r4-1234.nbt",
  relativePath: "exports/castle-overworld-r4-1234.nbt",
  warnings: ["Entities and block entities are not modeled by FuryBuilder and are not exported."],
  compatible: true,
};

const exportResult: StructureExportResult = {
  outcome: {
    relativePath: "exports/castle-overworld-r4-1234.nbt",
    fileName: "castle-overworld-r4-1234.nbt",
    bytes: 620,
    checksum: "a".repeat(64),
    createdAtMs: 1_700_000_100_000,
  },
  verified: true,
  size: [5, 7, 5],
  nonAirBlocks: 140,
  dataVersion: 4189,
  targetDataVersion: 4189,
  targetMinecraftVersion: "1.21.4",
};

beforeEach(() => {
  api.readFile.mockReset().mockResolvedValue("payload");
  api.previewImport.mockReset().mockResolvedValue(importPreview);
  api.importProject.mockReset();
  api.insert.mockReset();
  api.previewExport.mockReset().mockResolvedValue(exportPreview);
  api.export.mockReset().mockResolvedValue(exportResult);
  useProjectStore.setState({
    projects: [summary],
    warnings: [],
    current: session(),
    recovery: null,
    checkpoints: [],
    lastSavedAtMs: summary.lastSavedAtMs,
    lastSave: null,
    loading: false,
    error: null,
    managerOpen: true,
  });
  useWorldStore.setState({
    revision: 4,
    dimension: "minecraft:overworld",
    dimensions: worldDimensions,
    blockCount: 2048,
    chunkCount: 4,
    sectionCount: 5,
  });
  useToolStore.setState({ selection: null });
  clearToasts();
});

async function uploadStructure(): Promise<void> {
  const user = userEvent.setup();
  await user.upload(
    screen.getByLabelText("Structure file"),
    new File([new Uint8Array([1, 2, 3])], "castle.nbt"),
  );
}

describe("structure exchange", () => {
  it("previews size, palette and target version before importing", async () => {
    render(<StructureExchange />);
    await uploadStructure();
    expect(await screen.findByText(/Compatible structure/)).toBeInTheDocument();
    expect(api.readFile).toHaveBeenCalled();
    expect(api.previewImport).toHaveBeenCalledWith("payload");
    expect(screen.getByText("3 × 2 × 2")).toBeInTheDocument();
    expect(screen.getByText(/Volume/)).toBeInTheDocument();
    expect(screen.getByText(/stone/)).toBeInTheDocument();
    expect(screen.getByText("1.21.4")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Insert structure/ })).toBeEnabled();
  });

  it("blocks import when content would be dropped", async () => {
    api.previewImport.mockResolvedValue({
      ...importPreview,
      compatible: false,
      entities: 2,
      warnings: ["2 entities are present but FuryBuilder does not model entities."],
    });
    const user = userEvent.setup();
    render(<StructureExchange />);
    await uploadStructure();
    expect(await screen.findByText(/Import blocked/)).toBeInTheDocument();
    expect(screen.getByText(/2 entities are present/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "New project" }));
    expect(screen.getByRole("button", { name: /Create project from structure/ })).toBeDisabled();
  });

  it("defaults a flattened file to a new matching project and uses its height for placement", async () => {
    api.previewImport.mockResolvedValue({
      ...importPreview,
      dataVersion: 2730,
      targetDataVersion: 2730,
      targetMinecraftVersion: "1.17.1",
      dimensions: modernDimensions.map((dimension) => ({ ...dimension, minY: 0, height: 256 })),
      size: [1, 250, 1],
      volume: 250,
    });
    const user = userEvent.setup();
    render(<StructureExchange />);
    await uploadStructure();
    const y = await screen.findByRole("spinbutton", { name: "Origin y" });
    expect(y).toHaveValue(6);
    expect(screen.getByRole("button", { name: "Insert into open project" })).toBeDisabled();
    expect(
      screen.getByText(/Insertion requires a project with DataVersion 2730/),
    ).toBeInTheDocument();
    await chooseSelectOption(user, "Import dimension", "Nether");
    expect(y).toHaveValue(6);
    const create = screen.getByRole("button", { name: "Create project from structure" });
    await user.clear(y);
    await user.type(y, "7");
    expect(create).toBeDisabled();
    expect(screen.getByText(/must fit between Y 0 and 255/)).toBeInTheDocument();
    await user.clear(y);
    await user.type(y, "6");
    api.importProject.mockRejectedValue(new Error("stop after observing the request"));
    await user.click(create);
    expect(api.importProject).toHaveBeenCalledWith(
      expect.objectContaining({ dimension: "minecraft:the_nether", origin: [0, 6, 0] }),
    );
  });

  it("recomputes the default when placing a tall structure in a different dimension", async () => {
    api.previewImport.mockResolvedValue({ ...importPreview, size: [1, 300, 1], volume: 300 });
    const user = userEvent.setup();
    render(<StructureExchange />);
    await uploadStructure();
    const y = await screen.findByRole("spinbutton", { name: "Origin y" });
    expect(y).toHaveValue(20);
    await chooseSelectOption(user, "Import dimension", "End");
    expect(y).toHaveValue(0);
    expect(screen.getByRole("button", { name: "Insert structure" })).toBeDisabled();
    await chooseSelectOption(user, "Import dimension", "Overworld");
    expect(y).toHaveValue(20);
    expect(screen.getByRole("button", { name: "Insert structure" })).toBeEnabled();
  });

  it("never reuses a box selection from another dimension", async () => {
    useToolStore.setState({
      selection: { min: { x: -2, y: 60, z: -2 }, max: { x: 2, y: 66, z: 2 } },
    });
    const user = userEvent.setup();
    render(<StructureExchange />);
    await user.click(screen.getByRole("button", { name: /Export structure/ }));
    const checkbox = screen.getByRole("checkbox", { name: /Use box selection/ });
    expect(checkbox).toBeEnabled();
    await chooseSelectOption(user, "Export dimension", "Nether");
    expect(checkbox).toHaveAttribute("aria-disabled", "true");
    expect(screen.getByText(/made in overworld/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /Preview export/ }));
    await waitFor(() =>
      expect(api.previewExport).toHaveBeenCalledWith("minecraft:the_nether", null),
    );
  });

  it("previews the destination path and reports a verified export", async () => {
    const user = userEvent.setup();
    render(<StructureExchange />);
    await user.click(screen.getByRole("button", { name: /Export structure/ }));
    await user.click(screen.getByRole("button", { name: /Preview export/ }));
    await waitFor(() =>
      expect(api.previewExport).toHaveBeenCalledWith("minecraft:overworld", null),
    );
    expect(await screen.findByText(/Export ready/)).toBeInTheDocument();
    expect(screen.getByText(/exports\/castle-overworld-r4-1234\.nbt/)).toBeInTheDocument();
    expect(screen.getByText(/checksum: a{64}/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /Export \.nbt/ }));
    await waitFor(() =>
      expect(api.export).toHaveBeenCalledWith({
        dimension: "minecraft:overworld",
        bounds: null,
        fileName: "castle-overworld-r4-1234.nbt",
        expectedChecksum: "a".repeat(64),
      }),
    );
    expect(await screen.findByText(/Verified export/)).toBeInTheDocument();
  });
});
