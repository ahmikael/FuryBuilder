import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type * as IpcModule from "@/lib/ipc";
import { BiomePanel } from "@/features/workspace/BiomePanel";
import { useToolStore } from "@/state/tool-store";
import { useWorldStore } from "@/state/world-store";

const runtime = vi.hoisted(() => ({ tauri: true }));
const worldApi = vi.hoisted(() => ({ preview: vi.fn(), readBiome: vi.fn() }));
const editor = vi.hoisted(() => ({ apply: vi.fn() }));

vi.mock("@/lib/ipc", async (importOriginal) => {
  const actual = await importOriginal<typeof IpcModule>();
  return { ...actual, isTauriRuntime: () => runtime.tauri };
});

vi.mock("@/lib/world-api", () => ({
  previewOperation: worldApi.preview,
  readBiome: worldApi.readBiome,
}));

vi.mock("@/editor/commands", () => ({
  applyEditorOperation: editor.apply,
}));

const selection = {
  min: { x: 0, y: 64, z: 0 },
  max: { x: 7, y: 71, z: 7 },
};

beforeEach(() => {
  runtime.tauri = true;
  worldApi.preview.mockReset();
  worldApi.readBiome.mockReset();
  worldApi.readBiome.mockResolvedValue("minecraft:plains");
  editor.apply.mockReset();
  useToolStore.setState({
    selection,
    selectedBlock: { name: "minecraft:stone", properties: {} },
    job: null,
  });
  useWorldStore.setState({ dimension: "minecraft:overworld", revision: 1 });
});

describe("biome panel", () => {
  it("reads the source biome and previews only the 4x4x4 cells", async () => {
    const user = userEvent.setup();
    worldApi.preview.mockResolvedValue({
      affectedBlocks: 512,
      changedBlocks: 512,
      dirtyChunks: [],
      warnings: [],
    });
    render(<BiomePanel />);

    expect(await screen.findByTestId("current-biome")).toHaveTextContent("minecraft:plains");
    expect(worldApi.readBiome).toHaveBeenCalledWith("minecraft:overworld", selection.min);

    await user.click(screen.getByRole("button", { name: "Preview biome" }));
    expect(worldApi.preview).toHaveBeenCalledWith({
      type: "set_biome",
      dimension: "minecraft:overworld",
      bounds: selection,
      biome: "minecraft:forest",
    });
    expect(await screen.findByTestId("biome-preview")).toHaveTextContent(
      "512 biome block(s) would change out of 512 selected.",
    );
    expect(editor.apply).not.toHaveBeenCalled();
  });

  it("applies the edited biome id", async () => {
    const user = userEvent.setup();
    editor.apply.mockResolvedValue({ revision: 2, changedBlocks: 512 });
    render(<BiomePanel />);

    const input = screen.getByLabelText("Biome id");
    await user.clear(input);
    await user.type(input, "minecraft:desert");
    await user.click(screen.getByRole("button", { name: "Apply biome" }));

    expect(editor.apply).toHaveBeenCalledWith({
      type: "set_biome",
      dimension: "minecraft:overworld",
      bounds: selection,
      biome: "minecraft:desert",
    });
  });

  it("requires a selection and a valid namespaced biome id", async () => {
    const user = userEvent.setup();
    useToolStore.setState({ selection: null });
    const { unmount } = render(<BiomePanel />);
    expect(screen.getByRole("button", { name: "Apply biome" })).toBeDisabled();
    expect(screen.getByText(/Use the Box select tool/)).toBeInTheDocument();
    unmount();

    useToolStore.setState({ selection });
    render(<BiomePanel />);
    const input = screen.getByLabelText("Biome id");
    await user.clear(input);
    await user.type(input, "forest");
    expect(screen.getByRole("button", { name: "Apply biome" })).toBeDisabled();
    expect(screen.getByText(/Enter a namespaced id/)).toBeInTheDocument();
  });

  it("shows preview errors from the Rust core", async () => {
    const user = userEvent.setup();
    worldApi.preview.mockRejectedValue(new Error("biome data is missing at the selection"));
    render(<BiomePanel />);

    await user.click(screen.getByRole("button", { name: "Preview biome" }));
    expect(await screen.findByText("biome data is missing at the selection")).toBeInTheDocument();
  });
});
