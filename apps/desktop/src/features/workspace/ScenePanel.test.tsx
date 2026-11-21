import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type * as IpcModule from "@/lib/ipc";
import { ScenePanel } from "@/features/workspace/ScenePanel";
import { useSceneStore } from "@/state/scene-store";
import { useToolStore } from "@/state/tool-store";
import { useWorldStore } from "@/state/world-store";
import { chooseSelectOption } from "@/test-utils/select";

const runtime = vi.hoisted(() => ({ tauri: true }));
const sceneApi = vi.hoisted(() => ({
  fetch: vi.fn(),
  create: vi.fn(),
  remove: vi.fn(),
  inspect: vi.fn(),
}));

vi.mock("@/lib/ipc", async (importOriginal) => {
  const actual = await importOriginal<typeof IpcModule>();
  return { ...actual, isTauriRuntime: () => runtime.tauri };
});

vi.mock("@/lib/scene-api", () => ({
  fetchSceneObjects: sceneApi.fetch,
  createSceneObject: sceneApi.create,
  deleteSceneObject: sceneApi.remove,
  inspectSceneObject: sceneApi.inspect,
}));

const selection = {
  min: { x: 0, y: 60, z: 0 },
  max: { x: 15, y: 80, z: 15 },
};

const district = {
  id: "old-town",
  name: "Old Town",
  kind: "district" as const,
  dimension: "minecraft:overworld",
  bounds: selection,
  tags: { style: "medieval" },
};

beforeEach(() => {
  runtime.tauri = true;
  sceneApi.fetch.mockReset();
  sceneApi.create.mockReset();
  sceneApi.remove.mockReset();
  sceneApi.inspect.mockReset();
  sceneApi.fetch.mockResolvedValue([district]);
  useSceneStore.getState().reset({ schemaVersion: 1, objects: [district] });
  useToolStore.setState({ selection, selectedBlock: { name: "minecraft:stone", properties: {} } });
  useWorldStore.setState({ dimension: "minecraft:overworld" });
});

describe("scene panel", () => {
  it("lists semantic objects and shows the authoritative inspection", async () => {
    const user = userEvent.setup();
    sceneApi.inspect.mockResolvedValue({
      object: district,
      volume: 5376,
      nonAirBlocks: 12,
      palette: [{ state: "minecraft:stone", count: 12 }],
      children: [],
    });
    render(<ScenePanel />);

    await user.click(screen.getByRole("button", { name: /Old Town/ }));

    expect(sceneApi.inspect).toHaveBeenCalledWith("old-town");
    const volume = await screen.findByText((content) => content.startsWith("volume:"));
    expect(volume.textContent).toContain("5");
    expect(screen.getByText(/non-air: 12/)).toBeInTheDocument();
    expect(screen.getByText("minecraft:stone")).toBeInTheDocument();
    expect(screen.getByText(/style: medieval/)).toBeInTheDocument();
  });

  it("creates an object from the current selection and refreshes the list", async () => {
    const user = userEvent.setup();
    sceneApi.create.mockResolvedValue({
      ...district,
      id: "new-landmark",
      name: "New landmark",
      kind: "landmark",
    });
    sceneApi.inspect.mockResolvedValue({
      object: { ...district, id: "new-landmark", name: "New landmark", kind: "landmark" },
      volume: 5376,
      nonAirBlocks: 0,
      palette: [],
      children: [],
    });
    sceneApi.fetch.mockResolvedValue([
      district,
      { ...district, id: "new-landmark", name: "New landmark", kind: "landmark" },
    ]);
    render(<ScenePanel />);

    await user.clear(screen.getByLabelText("Object name"));
    await user.type(screen.getByLabelText("Object name"), "New landmark");
    await chooseSelectOption(user, "Object kind", "Landmark");
    await user.click(screen.getByRole("button", { name: "Create object" }));

    await waitFor(() =>
      expect(sceneApi.create).toHaveBeenCalledWith({
        name: "New landmark",
        kind: "landmark",
        dimension: "minecraft:overworld",
        bounds: selection,
        parentId: null,
        note: null,
      }),
    );
    expect(sceneApi.fetch).toHaveBeenCalledTimes(1);
    expect((await screen.findAllByText("New landmark")).length).toBeGreaterThan(0);
  });

  it("refuses to create without a selection", async () => {
    const user = userEvent.setup();
    useToolStore.setState({ selection: null });
    render(<ScenePanel />);

    expect(screen.getByRole("button", { name: "Create object" })).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "Create object" }));
    expect(sceneApi.create).not.toHaveBeenCalled();
  });
});
