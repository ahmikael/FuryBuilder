import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { TooltipProvider } from "@/components/ui/tooltip";
import { Toolbar } from "@/features/workspace/Toolbar";
import { WindowControls } from "@/features/workspace/WindowControls";

const windowApi = vi.hoisted(() => ({
  close: vi.fn(),
  isMaximized: vi.fn(),
  minimize: vi.fn(),
  onResized: vi.fn(),
  startDragging: vi.fn(),
  toggleMaximize: vi.fn(),
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => windowApi,
}));

function setDesktopRuntime(enabled: boolean): void {
  const runtime = window as unknown as Record<string, unknown>;
  if (enabled) {
    Object.defineProperty(runtime, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {},
    });
  } else {
    delete runtime["__TAURI_INTERNALS__"];
  }
}

describe("WindowControls", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    windowApi.close.mockResolvedValue(undefined);
    windowApi.isMaximized.mockResolvedValue(false);
    windowApi.minimize.mockResolvedValue(undefined);
    windowApi.onResized.mockResolvedValue(vi.fn());
    windowApi.startDragging.mockResolvedValue(undefined);
    windowApi.toggleMaximize.mockResolvedValue(undefined);
    setDesktopRuntime(false);
  });

  afterEach(() => {
    setDesktopRuntime(false);
  });

  it("does not show native window controls in a browser preview", () => {
    render(<WindowControls />);

    expect(screen.queryByRole("button", { name: "Minimize window" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Close window" })).not.toBeInTheDocument();
  });

  it("wires desktop window controls", async () => {
    const user = userEvent.setup();
    setDesktopRuntime(true);
    windowApi.isMaximized
      .mockResolvedValueOnce(false)
      .mockResolvedValueOnce(true)
      .mockResolvedValueOnce(false);
    render(<WindowControls />);

    await waitFor(() => expect(windowApi.onResized).toHaveBeenCalledOnce());
    await user.click(screen.getByRole("button", { name: "Minimize window" }));
    expect(windowApi.minimize).toHaveBeenCalledOnce();
    expect(windowApi.startDragging).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "Maximize window" }));
    expect(await screen.findByRole("button", { name: "Restore window" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Restore window" }));
    expect(windowApi.toggleMaximize).toHaveBeenCalledTimes(2);
    expect(await screen.findByRole("button", { name: "Maximize window" })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Close window" }));
    expect(windowApi.close).toHaveBeenCalledOnce();
  });

  it("starts dragging from the toolbar surface but not from its buttons", async () => {
    const user = userEvent.setup();
    setDesktopRuntime(true);
    render(
      <TooltipProvider>
        <Toolbar
          backendStatus="online"
          theme="dark"
          onToggleTheme={vi.fn()}
          onOpenSettings={vi.fn()}
          onOpenMcpAccess={vi.fn()}
        />
      </TooltipProvider>,
    );

    const toolbar = screen.getByRole("banner");
    fireEvent.mouseDown(toolbar, { button: 0 });
    expect(windowApi.startDragging).toHaveBeenCalledOnce();

    fireEvent.doubleClick(toolbar);
    expect(windowApi.toggleMaximize).toHaveBeenCalledOnce();

    await user.click(screen.getByRole("button", { name: "Settings" }));
    expect(windowApi.startDragging).toHaveBeenCalledOnce();
  });
});
