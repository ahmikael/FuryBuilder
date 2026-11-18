import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type * as IpcModule from "@/lib/ipc";
import { StructureExchange } from "@/features/project/StructureExchange";
import type {
  StructureExportPreview,
  StructureExportResult,
  StructureImportPreview,
} from "@/project/structure-types";
import type { ProjectSummary, SessionStatus } from "@/project/types";
import { useProjectStore } from "@/state/project-store";
import { useToastStore, useToolStore } from "@/state/tool-store";
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
  targetDataVersion: 4189,
  worldRevision: 4,
  updatedAtMs: 1_700_000_000_000,
  lastSavedAtMs: 1_700_000_000_000,
  lastSaveKind: "manual",
  bytes: 2048,
  schemaVersion: 1,
  recoverable: false,
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

const importPreview: StructureImportPreview = {
  format: "java_structure",
  dataVersion: 4189,
  targetDataVersion: 4189,
  targetMinecraftVersion: "1.21.4",
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
    blockCount: 2048,
    chunkCount: 4,
    sectionCount: 5,
  });
  useToolStore.setState({ selection: null });
  useToastStore.setState({ toasts: [] });
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

  it("never reuses a box selection from another dimension", async () => {
    useToolStore.setState({
      selection: { min: { x: -2, y: 60, z: -2 }, max: { x: 2, y: 66, z: 2 } },
    });
    const user = userEvent.setup();
    render(<StructureExchange />);
    await user.click(screen.getByRole("button", { name: /Export structure/ }));
    const checkbox = screen.getByRole("checkbox", { name: /Use box selection/ });
    expect(checkbox).toBeEnabled();
    await user.selectOptions(screen.getByLabelText("Export dimension"), "minecraft:the_nether");
    expect(checkbox).toBeDisabled();
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
