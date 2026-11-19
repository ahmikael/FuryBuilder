import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { DiagnosticsSection } from "@/features/settings/DiagnosticsSection";
import { BackendError } from "@/lib/ipc";
import type { DiagnosticsInfo } from "@/lib/diagnostics-api";

const api = vi.hoisted(() => ({
  fetch: vi.fn(),
  clear: vi.fn(),
  open: vi.fn(),
}));

vi.mock("@/lib/diagnostics-api", () => ({
  fetchDiagnostics: api.fetch,
  clearDiagnostics: api.clear,
  openDiagnosticsFolder: api.open,
  recordUiError: vi.fn(),
}));

const info: DiagnosticsInfo = {
  appVersion: "0.1.0",
  logDir: "C:\\AppData\\FuryBuilder\\logs",
  fileName: "furybuilder.log",
  files: [{ name: "furybuilder.log", bytes: 2_048, modifiedAtMs: 1_700_000_000_000 }],
  totalBytes: 2_048,
  maxBytesPerFile: 2_097_152,
  maxFiles: 4,
};

describe("DiagnosticsSection", () => {
  beforeEach(() => {
    api.fetch.mockReset();
    api.clear.mockReset();
    api.open.mockReset();
  });

  it("shows the bounded local log and opens its folder", async () => {
    const user = userEvent.setup();
    api.fetch.mockResolvedValue(info);
    render(<DiagnosticsSection />);

    expect(await screen.findByText("furybuilder.log")).toBeInTheDocument();
    expect(screen.getByText("0.1.0")).toBeInTheDocument();
    expect(screen.getAllByText(/2.0 KiB/)).toHaveLength(2);
    expect(screen.getByText(/1 \/ 4/)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Open diagnostics folder" }));
    expect(api.open).toHaveBeenCalledTimes(1);
  });

  it("clears only diagnostics and reports an empty state", async () => {
    const user = userEvent.setup();
    api.fetch.mockResolvedValue(info);
    api.clear.mockResolvedValue({ ...info, files: [], totalBytes: 0 });
    render(<DiagnosticsSection />);
    await screen.findByText("furybuilder.log");

    await user.click(screen.getByRole("button", { name: "Clear diagnostic logs" }));
    expect(await screen.findByText(/Diagnostic logs cleared/)).toBeInTheDocument();
    expect(screen.getByText("No log entries yet.")).toBeInTheDocument();
  });

  it("explains the desktop fallback instead of pretending logs exist", async () => {
    api.fetch.mockRejectedValue(
      new BackendError("backend_unavailable", "The Rust backend is not available in this window."),
    );
    render(<DiagnosticsSection />);
    expect(await screen.findByText(/Start the desktop build/)).toBeInTheDocument();
    expect(screen.queryByText("furybuilder.log")).not.toBeInTheDocument();
  });
});
