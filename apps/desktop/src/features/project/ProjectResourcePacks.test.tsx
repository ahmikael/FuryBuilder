import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { ProjectPackStack } from "@/assets/types";
import { ProjectResourcePacks } from "@/features/project/ProjectResourcePacks";

const api = vi.hoisted(() => ({
  load: vi.fn(),
  update: vi.fn(),
  open: vi.fn(),
}));
vi.mock("@/lib/assets-api", () => ({
  loadProjectPackStack: api.load,
  updateProjectPackStack: api.update,
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: api.open }));

const folder = "Test.furybuilder";
const stack: ProjectPackStack = {
  minecraftVersion: "1.21.4",
  targetFormat: "46",
  enabled: true,
  maxPacks: 16,
  packs: [
    {
      path: "resource-packs/blue",
      sha1: "blue",
      name: "Blue",
      state: "ready",
      message: null,
      packFormat: 46,
      supportedFormats: null,
    },
    {
      path: "resource-packs/green",
      sha1: "green",
      name: "Green",
      state: "changed",
      message: "Cached pack checksum no longer matches",
      packFormat: null,
      supportedFormats: null,
    },
  ],
};

describe("Project resource packs", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    api.load.mockResolvedValue(stack);
  });

  it("shows each pin in priority order and edits only the named project", async () => {
    render(<ProjectResourcePacks folder={folder} />);
    const list = await screen.findByRole("list", { name: /priority order/i });
    expect(api.load).toHaveBeenCalledWith(folder);
    expect(within(list).getByText("1. Blue")).toBeVisible();
    expect(within(list).getByText("Changed")).toBeVisible();
    expect(within(list).getByText(/checksum.*skipped/i)).toBeVisible();
    expect(screen.getByRole("button", { name: "Move Blue up" })).toBeDisabled();

    api.update.mockResolvedValue({ ...stack, packs: [...stack.packs].reverse() });
    fireEvent.click(screen.getByRole("button", { name: "Move Green up" }));
    expect(api.update).toHaveBeenCalledWith(folder, { kind: "move", sha1: "green", earlier: true });
    expect(await within(list).findByText("1. Green")).toBeVisible();

    api.update.mockResolvedValue({ ...stack, packs: stack.packs.slice(0, 1) });
    fireEvent.click(screen.getByRole("button", { name: "Remove Green" }));
    expect(api.update).toHaveBeenLastCalledWith(folder, { kind: "remove", sha1: "green" });
    await waitFor(() => expect(within(list).queryByText("Changed")).not.toBeInTheDocument());
  });

  it("shows validation errors without replacing the stack and refuses adds for placeholders", async () => {
    render(<ProjectResourcePacks folder={folder} />);
    await screen.findByText("1. Blue");
    api.open.mockResolvedValue("C:\\packs\\unsupported.zip");
    api.update.mockRejectedValue({ code: "unsupported_format", message: "Requires format 46" });
    fireEvent.click(screen.getByRole("button", { name: "Add ZIP" }));
    expect(await screen.findByText("Requires format 46")).toBeVisible();
    expect(api.update).toHaveBeenCalledWith(folder, {
      kind: "add",
      path: "C:\\packs\\unsupported.zip",
    });
    expect(screen.getByText("1. Blue")).toBeVisible();
    api.load.mockResolvedValue({ ...stack, enabled: false, packs: [] });
    fireEvent.click(screen.getByRole("button", { name: "Check pack files" }));
    expect(api.load).toHaveBeenLastCalledWith(folder, true);
    expect(await screen.findByText(/require pinned Minecraft client assets/i)).toBeVisible();
    expect(screen.getByRole("button", { name: "Add folder" })).toBeDisabled();
  });
});
