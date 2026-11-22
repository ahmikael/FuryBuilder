import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { App } from "@/app/App";
import { viewportBus } from "@/features/viewport/viewport-bus";
import { useEditorStore } from "@/state/editor-store";
import { useUiStore } from "@/state/ui-store";
import type { BlockPosition } from "@/renderer/types";

const defaultStore = {
  theme: "dark" as const,
  uiScale: 1,
  selectedDimension: "minecraft:overworld",
  leftPanelOpen: true,
  rightPanelOpen: true,
  bottomPanelOpen: true,
  bottomPanelTab: "palette" as const,
};

function resetUi() {
  useEditorStore.setState(defaultStore);
  useUiStore.setState({
    settingsOpen: false,
    mcpAccessOpen: false,
    commandPaletteOpen: false,
  });
}

describe("editor UX scenarios", () => {
  beforeEach(() => {
    localStorage.clear();
    resetUi();
  });

  it("finds a theme command by search and applies it", async () => {
    const user = userEvent.setup();
    render(<App />);

    await user.click(screen.getByRole("button", { name: "Open command palette" }));
    const dialog = await screen.findByRole("dialog", { name: "Command palette" });
    const input = within(dialog).getByRole("combobox", { name: "Search commands" });
    await user.type(input, "theme light");
    await user.click(within(dialog).getByRole("option", { name: /Use light theme/ }));

    expect(useEditorStore.getState().theme).toBe("light");
    expect(screen.queryByRole("dialog", { name: "Command palette" })).not.toBeInTheDocument();
  });

  it("supports a keyboard-only layout workflow and persists the result", async () => {
    const user = userEvent.setup();
    render(<App />);

    expect(screen.getByRole("button", { name: "Hide inspector" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Hide world panel" })).toBeInTheDocument();

    await user.keyboard("{Control>}k{/Control}");
    const dialog = await screen.findByRole("dialog", { name: "Command palette" });
    const input = within(dialog).getByRole("combobox", { name: "Search commands" });
    await user.type(input, "hide inspector");
    await user.keyboard("{Enter}");

    expect(useEditorStore.getState().rightPanelOpen).toBe(false);
    expect(screen.queryByRole("button", { name: "Hide inspector" })).not.toBeInTheDocument();

    await user.keyboard("{Control>}b{/Control}");
    expect(useEditorStore.getState().leftPanelOpen).toBe(false);
    expect(screen.queryByRole("button", { name: "Hide world panel" })).not.toBeInTheDocument();

    const persisted = JSON.parse(localStorage.getItem("furybuilder.editor-ui") ?? "{}") as {
      state?: { leftPanelOpen?: boolean; rightPanelOpen?: boolean };
    };
    expect(persisted.state?.leftPanelOpen).toBe(false);
    expect(persisted.state?.rightPanelOpen).toBe(false);
  });

  it("restores hidden panels from the toolbar and View menu", async () => {
    const user = userEvent.setup();
    render(<App />);

    await user.click(screen.getByRole("button", { name: "Toggle world panel" }));
    expect(useEditorStore.getState().leftPanelOpen).toBe(false);
    expect(screen.queryByRole("button", { name: "Hide world panel" })).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "View" }));
    const menu = await screen.findByRole("menu", { name: "View menu" });
    expect(within(menu).getByRole("menuitemcheckbox", { name: /World panel/ })).toHaveAttribute(
      "aria-checked",
      "false",
    );
    await user.click(within(menu).getByRole("menuitemcheckbox", { name: /World panel/ }));

    expect(useEditorStore.getState().leftPanelOpen).toBe(true);
    expect(screen.getByRole("button", { name: "Hide world panel" })).toBeInTheDocument();
  });

  it("opens a bottom panel tab by search and keeps the layout in one store", async () => {
    const user = userEvent.setup();
    useEditorStore.setState({ bottomPanelOpen: false });
    render(<App />);

    expect(screen.queryByRole("tab", { name: /History/ })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Open command palette" }));
    const dialog = await screen.findByRole("dialog", { name: "Command palette" });
    await user.type(
      within(dialog).getByRole("combobox", { name: "Search commands" }),
      "open history",
    );
    await user.click(within(dialog).getByRole("option", { name: /Open History/ }));

    expect(useEditorStore.getState().bottomPanelTab).toBe("history");
    expect(useEditorStore.getState().bottomPanelOpen).toBe(true);
    expect(screen.getByRole("tab", { name: /History/ })).toHaveAttribute("aria-selected", "true");
  });

  it("filters the world tree instead of showing a dead search field", async () => {
    const user = userEvent.setup();
    render(<App />);

    await user.type(screen.getByRole("textbox", { name: "Filter world tree" }), "nether");
    expect(screen.getByRole("button", { name: /The Nether/ })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Overworld/ })).not.toBeInTheDocument();

    await user.clear(screen.getByRole("textbox", { name: "Filter world tree" }));
    await user.type(screen.getByRole("textbox", { name: "Filter world tree" }), "no such entry");
    expect(screen.queryByRole("button", { name: /The Nether/ })).not.toBeInTheDocument();
    expect(screen.getByText(/Nothing in the world tree matches/)).toBeInTheDocument();
  });

  it("moves the camera through a coordinate search command", async () => {
    const user = userEvent.setup();
    const onFocus = vi.fn();
    viewportBus.addEventListener("focus-position", onFocus);
    render(<App />);

    await user.click(screen.getByRole("button", { name: "Open command palette" }));
    const dialog = await screen.findByRole("dialog", { name: "Command palette" });
    await user.type(within(dialog).getByRole("combobox", { name: "Search commands" }), "12 64 -8");
    await user.click(within(dialog).getByRole("option", { name: /Go to 12, 64, -8/ }));

    expect(onFocus).toHaveBeenCalledTimes(1);
    const event = onFocus.mock.calls[0]?.[0] as CustomEvent<BlockPosition>;
    expect(event.detail).toEqual({ x: 12, y: 64, z: -8 });
    viewportBus.removeEventListener("focus-position", onFocus);
  });
});
