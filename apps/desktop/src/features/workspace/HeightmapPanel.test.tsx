import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type * as IpcModule from "@/lib/ipc";
import { HeightmapPanel } from "@/features/workspace/HeightmapPanel";
import { useToolStore } from "@/state/tool-store";
import { chooseSelectOption } from "@/test-utils/select";

const runtime = vi.hoisted(() => ({ tauri: true }));
const worldApi = vi.hoisted(() => ({ preview: vi.fn(), catalog: vi.fn() }));
const editor = vi.hoisted(() => ({ apply: vi.fn() }));

vi.mock("@/lib/ipc", async (importOriginal) => {
  const actual = await importOriginal<typeof IpcModule>();
  return { ...actual, isTauriRuntime: () => runtime.tauri };
});

vi.mock("@/lib/world-api", () => ({
  previewOperation: worldApi.preview,
  fetchBlockCatalog: worldApi.catalog,
}));

vi.mock("@/editor/commands", () => ({
  applyEditorOperation: editor.apply,
}));

const selection = {
  min: { x: 0, y: 64, z: 0 },
  max: { x: 1, y: 70, z: 1 },
};

beforeEach(() => {
  runtime.tauri = true;
  worldApi.preview.mockReset();
  worldApi.catalog.mockReset();
  worldApi.catalog.mockResolvedValue([
    { name: "minecraft:dirt", properties: {} },
    { name: "minecraft:gravel", properties: {} },
  ]);
  editor.apply.mockReset();
  useToolStore.setState({
    selection,
    selectedBlock: { name: "minecraft:stone", properties: {} },
    job: null,
  });
});

describe("heightmap panel", () => {
  it("previews the deterministic patch without writing", async () => {
    const user = userEvent.setup();
    worldApi.preview.mockResolvedValue({
      affectedBlocks: 18,
      changedBlocks: 12,
      dirtyChunks: [],
      warnings: [],
    });
    render(<HeightmapPanel />);

    await user.click(screen.getByRole("button", { name: "Preview heightmap" }));

    expect(worldApi.preview).toHaveBeenCalledWith({
      type: "generate_heightmap",
      dimension: "minecraft:overworld",
      bounds: {
        min: { x: 0, y: 64, z: 0 },
        max: { x: 1, y: 72, z: 1 },
      },
      seed: 0,
      scale: 8,
      octaves: 3,
      amplitude: 8,
      baseY: 64,
      surfaceDepth: 1,
      surface: { name: "minecraft:stone", properties: {} },
      filler: { name: "minecraft:dirt", properties: {} },
    });
    expect(await screen.findByTestId("heightmap-preview")).toHaveTextContent(
      "12 block(s) would change out of 18 selected.",
    );
    expect(editor.apply).not.toHaveBeenCalled();
  });

  it("applies edited seed, amplitude, base and filler material", async () => {
    const user = userEvent.setup();
    editor.apply.mockResolvedValue({ revision: 3, changedBlocks: 2 });
    render(<HeightmapPanel />);

    await chooseSelectOption(user, "Heightmap filler", "minecraft:gravel");
    await user.clear(screen.getByLabelText("Heightmap seed"));
    await user.type(screen.getByLabelText("Heightmap seed"), "42");
    await user.clear(screen.getByLabelText("Heightmap amplitude"));
    await user.type(screen.getByLabelText("Heightmap amplitude"), "4");
    await user.clear(screen.getByLabelText("Heightmap base Y"));
    await user.type(screen.getByLabelText("Heightmap base Y"), "60");
    await user.clear(screen.getByLabelText("Heightmap surface depth"));
    await user.type(screen.getByLabelText("Heightmap surface depth"), "2");
    await user.click(screen.getByRole("button", { name: "Apply heightmap" }));

    expect(editor.apply).toHaveBeenCalledWith(
      expect.objectContaining({
        type: "generate_heightmap",
        seed: 42,
        amplitude: 4,
        baseY: 60,
        surfaceDepth: 2,
        filler: { name: "minecraft:gravel", properties: {} },
        bounds: {
          min: { x: 0, y: 60, z: 0 },
          max: { x: 1, y: 70, z: 1 },
        },
      }),
    );
  });

  it("requires a selection and shows preview errors", async () => {
    const user = userEvent.setup();
    useToolStore.setState({ selection: null });
    const { unmount } = render(<HeightmapPanel />);
    expect(screen.getByRole("button", { name: "Apply heightmap" })).toBeDisabled();
    expect(screen.getByText(/Use the Box select tool/)).toBeInTheDocument();
    unmount();

    useToolStore.setState({ selection });
    worldApi.preview.mockRejectedValue(new Error("baseY 90 is outside the selection"));
    render(<HeightmapPanel />);
    await user.click(screen.getByRole("button", { name: "Preview heightmap" }));
    expect(await screen.findByText("baseY 90 is outside the selection")).toBeInTheDocument();
  });
});
