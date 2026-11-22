import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type * as IpcModule from "@/lib/ipc";
import { ValidationPanel } from "@/features/workspace/ValidationPanel";
import type { ValidationReport } from "@/project/validation-types";
import { useToolStore } from "@/state/tool-store";
import { useWorldStore } from "@/state/world-store";

const runtime = vi.hoisted(() => ({ tauri: true }));
const validationApi = vi.hoisted(() => ({ validate: vi.fn() }));
const worldApi = vi.hoisted(() => ({ preview: vi.fn() }));
const editorApi = vi.hoisted(() => ({ apply: vi.fn() }));

vi.mock("@/lib/ipc", async (importOriginal) => {
  const actual = await importOriginal<typeof IpcModule>();
  return { ...actual, isTauriRuntime: () => runtime.tauri };
});

vi.mock("@/lib/validation-api", () => ({ validateRegion: validationApi.validate }));
vi.mock("@/lib/world-api", () => ({ previewOperation: worldApi.preview }));
vi.mock("@/editor/commands", () => ({ applyEditorOperation: editorApi.apply }));

const selection = {
  min: { x: 0, y: 63, z: 0 },
  max: { x: 1, y: 64, z: 0 },
};

const report: ValidationReport = {
  schemaVersion: 1,
  dimension: "minecraft:overworld",
  worldRevision: 3,
  bounds: selection,
  rules: [
    "floating_blocks",
    "palette",
    "export_compatibility",
    "block_state_orientation",
    "protected_region",
  ],
  findings: [
    {
      ruleId: "floating_blocks",
      severity: "warning",
      confidence: "heuristic",
      message:
        "2 structural block(s) have air directly below. This is a support heuristic, not a Minecraft stability simulation.",
      bounds: {
        min: { x: 0, y: 64, z: 0 },
        max: { x: 1, y: 64, z: 0 },
      },
      positions: [
        { x: 0, y: 64, z: 0 },
        { x: 1, y: 64, z: 0 },
      ],
      positionCount: 2,
      suggestedFix: {
        description: "Place minecraft:stone below 2 unsupported block(s) in this cluster.",
        operation: {
          type: "batch",
          dimension: "minecraft:overworld",
          changes: [
            {
              pos: { x: 0, y: 63, z: 0 },
              state: { name: "minecraft:stone", properties: {} },
            },
          ],
        },
      },
    },
  ],
  counts: { error: 0, warning: 1, info: 0 },
  truncated: false,
  scannedBlocks: 4,
  warnings: ["1 block(s) at the minimum scan Y were treated as supported."],
  notes: ["Validation is read-only and reports findings at the current authoritative revision."],
};

beforeEach(() => {
  runtime.tauri = true;
  validationApi.validate.mockReset();
  validationApi.validate.mockResolvedValue(report);
  worldApi.preview.mockReset();
  worldApi.preview.mockResolvedValue({
    affectedBlocks: 1,
    changedBlocks: 1,
    dirtyChunks: [{ x: 0, z: 0 }],
    warnings: [],
  });
  editorApi.apply.mockReset();
  editorApi.apply.mockResolvedValue({ revision: 4, changedBlocks: 1 });
  useToolStore.setState({ selection });
  useWorldStore.setState({ dimension: "minecraft:overworld", revision: 3 });
});

describe("validation panel", () => {
  it("runs the default rules and previews a suggested fix", async () => {
    const user = userEvent.setup();
    render(<ValidationPanel />);

    expect(screen.getByText(/\(0,63,0\)\.\.\(1,64,0\)/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Run validation" }));

    await waitFor(() =>
      expect(validationApi.validate).toHaveBeenCalledWith({
        dimension: "minecraft:overworld",
        bounds: selection,
        rules: [
          "floating_blocks",
          "palette",
          "export_compatibility",
          "block_state_orientation",
          "protected_region",
        ],
        maxSurfaceMaterials: 32,
        floatingFix: "support",
      }),
    );
    expect(await screen.findByText("world revision 3")).toBeInTheDocument();
    expect(screen.getByText("1 warning(s)")).toBeInTheDocument();
    expect(screen.getByText(/2 structural block\(s\) have air directly below/)).toBeInTheDocument();
    expect(screen.getByText("2 position(s)")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Preview fix" }));
    await waitFor(() => expect(worldApi.preview).toHaveBeenCalledTimes(1));
    expect(
      await screen.findByText(/Preview: 1 changed block\(s\), 1 dirty chunk\(s\)/),
    ).toBeInTheDocument();
  });

  it("applies a fix through the shared transaction path and re-validates", async () => {
    const user = userEvent.setup();
    render(<ValidationPanel />);

    await user.click(screen.getByRole("button", { name: "Run validation" }));
    await screen.findByText("1 warning(s)");
    await user.click(screen.getByRole("button", { name: "Apply fix" }));

    await waitFor(() => expect(editorApi.apply).toHaveBeenCalledTimes(1));
    expect(editorApi.apply.mock.calls[0][0]).toEqual(report.findings[0].suggestedFix?.operation);
    await waitFor(() => expect(validationApi.validate).toHaveBeenCalledTimes(2));
  });

  it("requires a selection before calling the Core", async () => {
    const user = userEvent.setup();
    useToolStore.setState({ selection: null });
    render(<ValidationPanel />);

    await user.click(screen.getByRole("button", { name: "Run validation" }));
    expect(screen.getByText("Select a box in the viewport first.")).toBeInTheDocument();
    expect(validationApi.validate).not.toHaveBeenCalled();
  });

  it("does not fake findings in browser preview", () => {
    runtime.tauri = false;
    render(<ValidationPanel />);
    expect(
      screen.getByText("Run validation and preview suggested fixes in the desktop app."),
    ).toBeInTheDocument();
    expect(validationApi.validate).not.toHaveBeenCalled();
  });
});
