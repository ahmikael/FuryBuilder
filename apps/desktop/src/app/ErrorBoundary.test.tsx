import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { ErrorBoundary } from "@/app/ErrorBoundary";

const api = vi.hoisted(() => ({
  record: vi.fn(),
  open: vi.fn(),
}));

vi.mock("@/lib/diagnostics-api", () => ({
  recordUiError: api.record,
  openDiagnosticsFolder: api.open,
}));

function Boom(): never {
  throw new Error("boom render");
}

describe("ErrorBoundary", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    api.record.mockReset();
    api.open.mockReset();
  });

  it("keeps a safe fallback and records the failure locally", async () => {
    const user = userEvent.setup();
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
    api.open.mockResolvedValue(undefined);

    render(
      <ErrorBoundary>
        <Boom />
      </ErrorBoundary>,
    );

    expect(screen.getByText(/stopped unexpectedly/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Error details" }));
    expect(await screen.findByText("boom render")).toBeInTheDocument();
    expect(api.record).toHaveBeenCalledWith(
      expect.stringContaining("boom render"),
      "react.error_boundary",
    );

    await user.click(screen.getByRole("button", { name: "Open diagnostics folder" }));
    expect(api.open).toHaveBeenCalledTimes(1);
    consoleError.mockRestore();
  });
});
