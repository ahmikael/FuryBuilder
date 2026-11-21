import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type * as IpcModule from "@/lib/ipc";
import { ObservationPanel } from "@/features/workspace/ObservationPanel";
import type { Observation } from "@/project/observation-types";
import { useToolStore } from "@/state/tool-store";
import { useWorldStore } from "@/state/world-store";
import { chooseSelectOption } from "@/test-utils/select";

const runtime = vi.hoisted(() => ({ tauri: true }));
const observationApi = vi.hoisted(() => ({ observe: vi.fn() }));

vi.mock("@/lib/ipc", async (importOriginal) => {
  const actual = await importOriginal<typeof IpcModule>();
  return { ...actual, isTauriRuntime: () => runtime.tauri };
});

vi.mock("@/lib/observation-api", () => ({ observeRegion: observationApi.observe }));

const selection = {
  min: { x: 0, y: 60, z: 0 },
  max: { x: 15, y: 80, z: 15 },
};

const observation: Observation = {
  schemaVersion: 1,
  mode: "material_map",
  dimension: "minecraft:overworld",
  worldRevision: 3,
  bounds: selection,
  width: 2,
  height: 1,
  blocksPerPixel: 1,
  image: "data:image/png;base64,iVBORw0KGgo=",
  imageBytes: 8,
  legend: [
    { key: "minecraft:grass_block", label: "minecraft:grass_block", color: "#5f9e4a", count: 2 },
    { key: "minecraft:stone", label: "minecraft:stone", color: "#8a8f98", count: 1 },
  ],
  legendTruncated: false,
  partial: false,
  warnings: ["Legend shows the 16 most common entries; more distinct entries exist."],
  notes: ["Image is a deterministic derived map rendered from the authoritative Rust world."],
  scannedBlocks: 4,
  sampledUnits: 2,
  nonAirUnits: 2,
  counts: { added: 0, removed: 0, changed: 0 },
};

beforeEach(() => {
  runtime.tauri = true;
  observationApi.observe.mockReset();
  observationApi.observe.mockResolvedValue(observation);
  useToolStore.setState({ selection });
  useWorldStore.setState({ dimension: "minecraft:overworld", revision: 3 });
});

describe("observation panel", () => {
  it("renders the selected bounds through the Core and shows the legend", async () => {
    const user = userEvent.setup();
    render(<ObservationPanel />);

    expect(screen.getByText(/\(0,60,0\)\.\.\(15,80,15\)/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Render observation" }));

    await waitFor(() =>
      expect(observationApi.observe).toHaveBeenCalledWith({
        dimension: "minecraft:overworld",
        bounds: selection,
        mode: "material_map",
      }),
    );
    expect(await screen.findByText("world revision 3")).toBeInTheDocument();
    expect(screen.getByText("image 2×1")).toBeInTheDocument();
    expect(screen.getByText("minecraft:grass_block")).toBeInTheDocument();
    expect(screen.getByText("minecraft:stone")).toBeInTheDocument();
    expect(screen.getByText(/Legend shows the 16 most common entries/)).toBeInTheDocument();
    expect(screen.getByAltText("Material map observation")).toHaveAttribute(
      "src",
      observation.image,
    );
  });

  it("requires a selection before asking the Core for a map", async () => {
    const user = userEvent.setup();
    useToolStore.setState({ selection: null });
    render(<ObservationPanel />);

    await user.click(screen.getByRole("button", { name: "Render observation" }));

    expect(screen.getByText("Select a box in the viewport first.")).toBeInTheDocument();
    expect(observationApi.observe).not.toHaveBeenCalled();
  });

  it("passes cross-section and revision-diff parameters explicitly", async () => {
    const user = userEvent.setup();
    render(<ObservationPanel />);

    await chooseSelectOption(user, "Observation mode", "Cross-section");
    const coordinate = screen.getByLabelText("Slice coordinate");
    await user.clear(coordinate);
    await user.type(coordinate, "70");
    await user.click(screen.getByRole("button", { name: "Render observation" }));

    await waitFor(() =>
      expect(observationApi.observe).toHaveBeenCalledWith({
        dimension: "minecraft:overworld",
        bounds: selection,
        mode: "cross_section",
        slice: { axis: "y", coordinate: 70 },
      }),
    );

    observationApi.observe.mockClear();
    await chooseSelectOption(user, "Observation mode", "Revision diff");
    const from = screen.getByLabelText("From revision");
    await user.clear(from);
    await user.type(from, "2");
    await user.click(screen.getByRole("button", { name: "Render observation" }));

    await waitFor(() =>
      expect(observationApi.observe).toHaveBeenCalledWith({
        dimension: "minecraft:overworld",
        bounds: selection,
        mode: "revision_diff",
        fromRevision: 2,
      }),
    );
  });

  it("does not fake a map in browser preview", () => {
    runtime.tauri = false;
    render(<ObservationPanel />);
    expect(
      screen.getByText(/Observation maps are rendered and bounded by the Rust Core/),
    ).toBeInTheDocument();
    expect(observationApi.observe).not.toHaveBeenCalled();
  });
});
