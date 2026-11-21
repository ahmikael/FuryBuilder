import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type * as IpcModule from "@/lib/ipc";
import { TerrainPanel } from "@/features/workspace/TerrainPanel";
import { useToolStore } from "@/state/tool-store";
import { chooseSelectOption } from "@/test-utils/select";

const runtime = vi.hoisted(() => ({ tauri: true }));
const worldApi = vi.hoisted(() => ({ preview: vi.fn() }));
const editor = vi.hoisted(() => ({ apply: vi.fn() }));

vi.mock("@/lib/ipc", async (importOriginal) => {
  const actual = await importOriginal<typeof IpcModule>();
  return { ...actual, isTauriRuntime: () => runtime.tauri };
});

vi.mock("@/lib/world-api", () => ({
  previewOperation: worldApi.preview,
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
  editor.apply.mockReset();
  useToolStore.setState({
    selection,
    selectedBlock: { name: "minecraft:stone", properties: {} },
    job: null,
  });
});

describe("terrain brush panel", () => {
  it("previews the selected brush without writing", async () => {
    const user = userEvent.setup();
    worldApi.preview.mockResolvedValue({
      affectedBlocks: 14,
      changedBlocks: 4,
      dirtyChunks: [],
      warnings: ["1 empty column(s) had no terrain surface and were skipped"],
    });
    render(<TerrainPanel />);

    await user.click(screen.getByRole("button", { name: "Preview terrain brush" }));

    expect(worldApi.preview).toHaveBeenCalledWith({
      type: "terrain_brush",
      dimension: "minecraft:overworld",
      bounds: {
        min: { x: 0, y: 48, z: 0 },
        max: { x: 1, y: 86, z: 1 },
      },
      kind: "raise",
      material: { name: "minecraft:stone", properties: {} },
      amount: 1,
      seed: 0,
      step: 4,
      targetY: null,
    });
    expect(await screen.findByTestId("terrain-preview")).toHaveTextContent(
      "4 block(s) would change out of 14 selected.",
    );
    expect(screen.getByTestId("terrain-preview")).toHaveTextContent("1 empty column");
    expect(editor.apply).not.toHaveBeenCalled();
  });

  it("plans deterministic noise and terrace parameters from the UI", async () => {
    const user = userEvent.setup();
    editor.apply.mockResolvedValue({ revision: 3, changedBlocks: 2 });
    render(<TerrainPanel />);

    await chooseSelectOption(user, "Terrain brush kind", "Noise");
    expect(screen.getByLabelText("Terrain brush seed")).toBeInTheDocument();
    await user.clear(screen.getByLabelText("Terrain brush seed"));
    await user.type(screen.getByLabelText("Terrain brush seed"), "42");
    await user.clear(screen.getByLabelText("Terrain brush amount"));
    await user.type(screen.getByLabelText("Terrain brush amount"), "2");
    await user.click(screen.getByRole("button", { name: "Apply terrain brush" }));

    expect(editor.apply).toHaveBeenCalledWith(
      expect.objectContaining({
        type: "terrain_brush",
        kind: "noise",
        seed: 42,
        amount: 2,
      }),
    );

    await chooseSelectOption(user, "Terrain brush kind", "Terrace");
    expect(screen.queryByLabelText("Terrain brush amount")).not.toBeInTheDocument();
    await user.clear(screen.getByLabelText("Terrace step"));
    await user.type(screen.getByLabelText("Terrace step"), "8");
    await user.click(screen.getByRole("button", { name: "Apply terrain brush" }));
    expect(editor.apply).toHaveBeenLastCalledWith(
      expect.objectContaining({
        type: "terrain_brush",
        kind: "terrace",
        step: 8,
        targetY: null,
      }),
    );
  });

  it("requires a selection and shows preview errors", async () => {
    const user = userEvent.setup();
    useToolStore.setState({ selection: null });
    const { unmount } = render(<TerrainPanel />);
    expect(screen.getByRole("button", { name: "Apply terrain brush" })).toBeDisabled();
    expect(screen.getByText(/Use the Box select tool/)).toBeInTheDocument();
    unmount();

    useToolStore.setState({ selection });
    worldApi.preview.mockRejectedValue(new Error("targetY 90 is outside the selection"));
    render(<TerrainPanel />);
    await chooseSelectOption(user, "Terrain brush kind", "Flatten");
    await user.type(screen.getByLabelText("Flatten target Y"), "90");
    await user.click(screen.getByRole("button", { name: "Preview terrain brush" }));
    expect(await screen.findByText("targetY 90 is outside the selection")).toBeInTheDocument();
  });
});
