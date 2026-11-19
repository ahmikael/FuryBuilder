import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type * as IpcModule from "@/lib/ipc";
import { BuildDslPanel } from "@/features/workspace/BuildDslPanel";

const runtime = vi.hoisted(() => ({ tauri: true }));
const buildApi = vi.hoisted(() => ({ preview: vi.fn() }));
const editor = vi.hoisted(() => ({ apply: vi.fn() }));

vi.mock("@/lib/ipc", async (importOriginal) => {
  const actual = await importOriginal<typeof IpcModule>();
  return { ...actual, isTauriRuntime: () => runtime.tauri };
});

vi.mock("@/lib/build-dsl-api", () => ({ previewBuildDsl: buildApi.preview }));
vi.mock("@/editor/commands", () => ({ applyEditorOperation: editor.apply }));

const operation = {
  type: "batch" as const,
  dimension: "minecraft:overworld",
  changes: [],
};

const preview = {
  summary: {
    dimension: "minecraft:overworld",
    bounds: {
      min: { x: 0, y: 64, z: 0 },
      max: { x: 18, y: 68, z: 4 },
    },
    houses: 3,
    blocks: 240,
    warnings: ["Composition expands to 3 houses."],
  },
  operation,
  preview: {
    affectedBlocks: 240,
    changedBlocks: 120,
    dirtyChunks: [],
    warnings: [],
  },
};

beforeEach(() => {
  runtime.tauri = true;
  buildApi.preview.mockReset();
  editor.apply.mockReset();
  buildApi.preview.mockResolvedValue(preview);
  editor.apply.mockResolvedValue({ revision: 1, changedBlocks: 120 });
});

describe("build DSL panel", () => {
  it("rejects invalid JSON before calling the Core", async () => {
    const user = userEvent.setup();
    render(<BuildDslPanel />);

    fireEvent.change(screen.getByLabelText("Build DSL recipe"), {
      target: { value: "{ not json" },
    });
    await user.click(screen.getByRole("button", { name: "Compile & preview" }));

    expect(screen.getByText(/Invalid JSON/)).toBeInTheDocument();
    expect(buildApi.preview).not.toHaveBeenCalled();
  });

  it("compiles through the Core and shows budgets and bounds", async () => {
    const user = userEvent.setup();
    render(<BuildDslPanel />);

    await user.click(screen.getByRole("button", { name: "Compile & preview" }));

    expect(buildApi.preview).toHaveBeenCalledWith(
      expect.objectContaining({
        schemaVersion: 1,
        dimension: "minecraft:overworld",
        nodes: expect.arrayContaining([expect.objectContaining({ type: "repeat" })]),
      }),
    );
    expect(await screen.findByText(/houses: 3/)).toBeInTheDocument();
    expect(screen.getByText(/blocks: 240/)).toBeInTheDocument();
    expect(screen.getByText(/changed: 120/)).toBeInTheDocument();
    expect(screen.getByText(/Composition expands to 3 houses/)).toBeInTheDocument();
  });

  it("applies only the explicitly compiled operation", async () => {
    const user = userEvent.setup();
    render(<BuildDslPanel />);

    await user.click(screen.getByRole("button", { name: "Compile & preview" }));
    await user.click(await screen.findByRole("button", { name: "Apply compiled" }));

    expect(editor.apply).toHaveBeenCalledWith(operation, { label: "Build DSL" });
  });
});
