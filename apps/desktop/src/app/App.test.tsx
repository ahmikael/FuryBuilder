import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";
import { App } from "@/app/App";
import { useEditorStore } from "@/state/editor-store";

describe("App", () => {
  beforeEach(() => {
    useEditorStore.setState({ theme: "dark", uiScale: 1 });
  });

  it("renders the editor shell and reports the offline core without Tauri", async () => {
    render(<App />);

    expect(screen.getByText("FuryBuilder")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "3D viewport" })).toBeInTheDocument();
    expect(await screen.findByText("Core offline")).toBeInTheDocument();
    expect(screen.getAllByText("Core offline")).toHaveLength(1);
    expect(screen.queryByText("Core online")).not.toBeInTheDocument();
    expect(screen.queryByText("Rust core")).not.toBeInTheDocument();
    expect(screen.queryByText("M9")).not.toBeInTheDocument();
    expect(screen.queryByText("M8")).not.toBeInTheDocument();
    expect(screen.getByText("Overworld")).toBeInTheDocument();
    expect(screen.queryByText("Minecraft 1.21.4")).not.toBeInTheDocument();
    expect(screen.queryByText("DV 4189")).not.toBeInTheDocument();
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
});
