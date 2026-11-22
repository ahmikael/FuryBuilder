import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";
import { App } from "@/app/App";
import { useEditorStore } from "@/state/editor-store";

describe("App", () => {
  beforeEach(() => {
    useEditorStore.setState({ theme: "dark", uiScale: 1 });
  });

  it("renders the editor shell with one core status outside Tauri", async () => {
    render(<App />);

    expect(screen.getByRole("heading", { name: "3D viewport" })).toBeInTheDocument();
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
});
