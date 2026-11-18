import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { VanillaParityReport } from "@/project/types";
import { VanillaParityPanel } from "@/features/project/VanillaParityPanel";

const api = vi.hoisted(() => ({ check: vi.fn() }));

vi.mock("@/lib/project-api", () => ({
  checkVanillaParity: api.check,
}));

const base = {
  projectFolder: "Vanilla Test.furybuilder",
  projectName: "Vanilla Test",
  snapshotRevision: 1,
  fromAutosave: false,
  referenceKind: "generatedRun" as const,
  referenceRoot: "C:\\runs\\run-1\\world",
  seed: "FuryBuilder",
  preset: "normal",
  centerX: 0,
  centerZ: 0,
  radiusChunks: 1,
};

const matched: VanillaParityReport = {
  ...base,
  status: "matched",
  refusals: [],
  limitations: ["block states only"],
  comparison: {
    status: "matched",
    referenceRoot: base.referenceRoot,
    dimension: "minecraft:overworld",
    requestedChunks: 9,
    sampledChunks: 9,
    checkedChunks: 9,
    checkedSections: 72,
    checkedBlocks: 294_912,
    mismatchCount: 0,
    mismatches: [],
    missingReferenceChunks: [],
    missingProjectChunks: [],
    refusals: [],
    limitations: ["block states only"],
  },
};

describe("VanillaParityPanel", () => {
  beforeEach(() => {
    api.check.mockReset();
  });

  it("reports a matching generated base", async () => {
    const user = userEvent.setup();
    api.check.mockResolvedValue(matched);
    render(<VanillaParityPanel folder={base.projectFolder} />);

    await user.click(screen.getByRole("button", { name: "Verify generated base" }));

    expect(await screen.findByText("Block parity matched")).toBeInTheDocument();
    expect(api.check).toHaveBeenCalledWith(base.projectFolder);
    expect(screen.getByTestId("vanilla-parity-counts")).toHaveTextContent(
      /checked 9 of 9 sampled reference chunk/,
    );
    expect(screen.getByText(/original run/)).toBeInTheDocument();
  });

  it("shows a refusal without pretending the check ran", async () => {
    const user = userEvent.setup();
    api.check.mockResolvedValue({
      ...base,
      status: "refused",
      comparison: null,
      limitations: [],
      refusals: ["seed: project `A` vs reference `B`"],
    } satisfies VanillaParityReport);
    render(<VanillaParityPanel folder={base.projectFolder} />);

    await user.click(screen.getByRole("button", { name: "Verify generated base" }));

    expect(await screen.findByText("Parity check refused")).toBeInTheDocument();
    expect(screen.getByText(/seed: project/)).toBeInTheDocument();
    expect(screen.queryByText(/sampled reference chunk/)).not.toBeInTheDocument();
  });

  it("shows bounded block differences with coordinates", async () => {
    const user = userEvent.setup();
    api.check.mockResolvedValue({
      ...matched,
      status: "mismatched",
      comparison: {
        ...matched.comparison!,
        status: "mismatched",
        mismatchCount: 2,
        mismatches: [
          {
            chunkX: 0,
            chunkZ: 0,
            sectionY: 4,
            x: 3,
            y: 67,
            z: 5,
            reference: "minecraft:stone",
            project: "minecraft:dirt",
          },
        ],
      },
    } satisfies VanillaParityReport);
    render(<VanillaParityPanel folder={base.projectFolder} />);

    await user.click(screen.getByRole("button", { name: "Verify generated base" }));

    expect(await screen.findByText("Block parity differs")).toBeInTheDocument();
    expect(screen.getByTestId("vanilla-parity-counts")).toHaveTextContent(/2 difference/);
    expect(screen.getByText(/\(3, 67, 5\) reference minecraft:stone/)).toBeInTheDocument();
  });
});
