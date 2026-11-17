import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useEditorHotkeys } from "@/editor/use-editor-hotkeys";
import { useToolStore } from "@/state/tool-store";

const commands = vi.hoisted(() => ({
  undo: vi.fn(),
  redo: vi.fn(),
  copy: vi.fn(),
  paste: vi.fn(),
  del: vi.fn(),
  apply: vi.fn(),
  cancel: vi.fn(),
}));

vi.mock("@/editor/commands", () => ({
  undoWorld: commands.undo,
  redoWorld: commands.redo,
  copySelection: commands.copy,
  pasteClipboard: commands.paste,
  deleteSelection: commands.del,
  applyActiveTool: commands.apply,
  cancelInteraction: commands.cancel,
}));

function Harness() {
  useEditorHotkeys();
  return (
    <div>
      <input aria-label="Coordinate field" />
      <button type="button">Elsewhere</button>
    </div>
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  useToolStore.setState({ tool: "place" });
});

describe("editor hotkeys", () => {
  it("does not fire commands while typing in a field", async () => {
    const user = userEvent.setup();
    render(<Harness />);
    const input = screen.getByLabelText("Coordinate field");
    await user.click(input);
    await user.keyboard("{Control>}z{/Control}");
    await user.keyboard("5");
    await user.keyboard("{Escape}");
    expect(commands.undo).not.toHaveBeenCalled();
    expect(commands.cancel).not.toHaveBeenCalled();
    expect(useToolStore.getState().tool).toBe("place");
    expect(input).toHaveValue("5");
  });

  it("runs undo and redo with modifiers outside text fields", async () => {
    const user = userEvent.setup();
    render(<Harness />);
    await user.click(screen.getByRole("button", { name: "Elsewhere" }));
    await user.keyboard("{Control>}z{/Control}");
    expect(commands.undo).toHaveBeenCalledTimes(1);
    await user.keyboard("{Control>}{Shift>}z{/Shift}{/Control}");
    expect(commands.redo).toHaveBeenCalledTimes(1);
    await user.keyboard("{Control>}y{/Control}");
    expect(commands.redo).toHaveBeenCalledTimes(2);
  });

  it("switches tools with digits and cancels with Escape", async () => {
    const user = userEvent.setup();
    render(<Harness />);
    await user.click(screen.getByRole("button", { name: "Elsewhere" }));
    await user.keyboard("5");
    expect(useToolStore.getState().tool).toBe("box");
    await user.keyboard("8");
    expect(useToolStore.getState().tool).toBe("replace");
    await user.keyboard("{Escape}");
    expect(commands.cancel).toHaveBeenCalledTimes(1);
  });

  it("does not swallow Escape or Enter when a dialog is open", async () => {
    const user = userEvent.setup();
    render(
      <>
        <Harness />
        <div role="dialog" aria-label="Settings" />
      </>,
    );
    await user.click(screen.getByRole("button", { name: "Elsewhere" }));
    await user.keyboard("{Escape}");
    await user.keyboard("{Enter}");
    expect(commands.cancel).not.toHaveBeenCalled();
    expect(commands.apply).not.toHaveBeenCalled();
  });
});
