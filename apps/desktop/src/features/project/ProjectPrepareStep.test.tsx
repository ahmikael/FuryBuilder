import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { ComponentStepState } from "@/features/project/ProjectComponentsStep";
import { ProjectPrepareStep } from "@/features/project/ProjectPrepareStep";
import type { PrepareProgress } from "@/lib/project-api";

const components: ComponentStepState = {
  targetKey: "java-classic-test|import",
  checking: false,
  ready: true,
  errors: [],
  warnings: [],
  summary: {
    java: {
      kind: "java",
      mode: "managed",
      state: "ready",
      version: "21.0.5",
      bytes: 120_000_000,
      hash: "a".repeat(64),
      path: "C:\\cache\\java\\bin\\java.exe",
    },
    assets: {
      kind: "assets",
      mode: "managed",
      state: "ready",
      version: "1.21.4",
      bytes: 54_200_000,
      hash: "b".repeat(40),
      path: "C:\\cache\\assets\\1.21.4\\client.jar",
    },
    server: null,
    catalog: {
      id: "java-1.21.4-test",
      hash: "e".repeat(64),
      blocks: 1234,
      items: 567,
    },
    totalBytes: 174_200_000,
  },
};

function progress(overrides: Partial<PrepareProgress> = {}): PrepareProgress {
  return {
    jobId: "job-1",
    stage: "metadata",
    state: "running",
    message: "Writing project metadata",
    completed: 0,
    total: 0,
    ...overrides,
  };
}

function prepare(
  events: PrepareProgress[],
  overrides: Partial<Parameters<typeof ProjectPrepareStep>[0]> = {},
) {
  return (
    <ProjectPrepareStep
      mode="import"
      components={components}
      progress={events}
      message="Importing the world…"
      cancelling={false}
      failure={null}
      onCancel={vi.fn()}
      onBack={vi.fn()}
      onRetry={vi.fn()}
      canCancel
      {...overrides}
    />
  );
}

describe("project prepare step", () => {
  it("lists prepared components and leaves project stages pending without events", () => {
    render(prepare([]));

    expect(screen.getByText("Prepared components")).toBeInTheDocument();
    expect(screen.getByText("Java runtime")).toBeInTheDocument();
    expect(screen.getByText("21.0.5 · ready")).toBeInTheDocument();
    expect(screen.getByText("1234 blocks · 567 items")).toBeInTheDocument();
    expect(screen.getByText("Source world")).toBeInTheDocument();
    expect(screen.getAllByText("Waiting.").length).toBeGreaterThan(0);
    expect(screen.getByRole("button", { name: "Cancel preparation" })).toBeEnabled();
  });

  it("renders real stage progress and byte progress for the source copy", () => {
    render(
      prepare([
        progress({ stage: "source", state: "done", message: "841 chunk(s) in 4 region file(s)" }),
        progress({ stage: "world", state: "done", message: "12,000 editable block(s) ready" }),
        progress({ stage: "metadata", state: "done", message: "Project manifest written" }),
        progress({
          stage: "copy",
          state: "running",
          message: "Copying the source byte for byte",
          completed: 24_000_000,
          total: 48_000_000,
        }),
      ]),
    );

    expect(screen.getByText("841 chunk(s) in 4 region file(s)")).toBeInTheDocument();
    expect(screen.getByText(/22\.9 MiB of 45\.8 MiB/)).toBeInTheDocument();
    expect(screen.getByLabelText("Source copy progress")).toBeInTheDocument();
    expect(screen.getByText(/Copying the source byte for byte/)).toBeInTheDocument();
  });

  it("marks the running stage as failed and offers a way back to review", async () => {
    const onBack = vi.fn();
    const user = userEvent.setup();
    render(
      prepare(
        [
          progress({ stage: "source", state: "done", message: "Analyzed" }),
          progress({ stage: "world", state: "running", message: "Reading editable blocks" }),
        ],
        {
          failure: {
            code: "disk_space",
            message: "Not enough free space",
            hint: "Free some space.",
          },
          onBack,
        },
      ),
    );

    expect(screen.getByText(/Preparation stopped/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Back to review" }));
    expect(onBack).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("button", { name: "Cancel preparation" })).not.toBeInTheDocument();
  });

  it("disables cancellation while a request is in flight", () => {
    render(prepare([progress()], { cancelling: true, message: "Cancelling preparation…" }));

    expect(screen.getByRole("button", { name: "Cancelling…" })).toBeDisabled();
    expect(screen.getByText("Cancelling preparation…")).toBeInTheDocument();
  });
});
