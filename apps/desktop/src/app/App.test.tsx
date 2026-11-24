import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";
import { App } from "@/app/App";
import { useEditorStore } from "@/state/editor-store";
import { useProjectStore } from "@/state/project-store";
import { useUiStore } from "@/state/ui-store";

describe("App", () => {
  beforeEach(() => {
    useEditorStore.setState({ theme: "dark", uiScale: 1 });
    useUiStore.setState({
      settingsOpen: false,
      mcpAccessOpen: false,
      commandPaletteOpen: false,
    });
    useProjectStore.getState().reset();
  });

  it("shows the welcome screen instead of a viewport when no project is open", async () => {
    render(<App />);

    expect(screen.getByRole("heading", { name: "FuryBuilder" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "New project" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Open project" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Import world" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "3D viewport" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Project" })).toBeDisabled();
    expect(screen.getByText("Desktop core required")).toBeInTheDocument();

    const status = await screen.findByRole("status", { name: "Core status" });
    expect(status).toHaveTextContent("Core offline");
    expect(screen.getAllByRole("status", { name: "Core status" })).toHaveLength(1);
  });

  it("opens settings and persists a theme change", async () => {
    const user = userEvent.setup();
    render(<App />);

    await user.click(screen.getByRole("button", { name: "Settings" }));
    expect(await screen.findByRole("dialog")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Minecraft assets" })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Light" }));
    expect(useEditorStore.getState().theme).toBe("light");
  });

  it("mounts the editor only after a project is ready", () => {
    useProjectStore.setState({
      phase: "ready",
      current: {
        folder: "Castle.furybuilder",
        name: "Castle",
        savedRevision: 3,
        autosavedRevision: null,
        revision: 3,
        dirty: false,
        editorState: { selectedDimension: "minecraft:overworld", loadRadius: 1 },
        availableCheckpoints: 0,
      },
    });

    render(<App />);

    expect(screen.getByRole("heading", { name: "3D viewport" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Project" })).toBeEnabled();
    expect(screen.queryByRole("heading", { name: "FuryBuilder" })).not.toBeInTheDocument();
  });

  it("keeps the viewport closed while a project is preparing", () => {
    useProjectStore.setState({
      phase: "preparing",
      prepareMessage: "Loading the project…",
    });

    render(<App />);

    expect(screen.getByRole("status", { name: "Preparing project" })).toHaveTextContent(
      "Loading the project…",
    );
    expect(screen.queryByRole("heading", { name: "3D viewport" })).not.toBeInTheDocument();
  });
});
