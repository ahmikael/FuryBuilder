import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SettingsDialog } from "@/features/settings/SettingsDialog";
import type { BackendState } from "@/hooks/use-backend";
import { useEditorStore } from "@/state/editor-store";

vi.mock("@/features/settings/MinecraftAssetsSection", () => ({
  MinecraftAssetsSection: () => <h3>Minecraft assets</h3>,
}));

vi.mock("@/features/settings/JavaRuntimeSection", () => ({
  JavaRuntimeSection: () => <h3>Java runtime</h3>,
}));

vi.mock("@/features/settings/MinecraftServerSection", () => ({
  MinecraftServerSection: () => <h3>Minecraft server JAR</h3>,
}));

vi.mock("@/features/settings/DependencyReadiness", () => ({
  DependencyReadiness: () => <h3>Vanilla readiness</h3>,
}));

vi.mock("@/features/settings/UpdatesSection", () => ({
  UpdatesSection: () => <h3>Updates</h3>,
}));

vi.mock("@/features/settings/DiagnosticsSection", () => ({
  DiagnosticsSection: () => <h3>Diagnostics</h3>,
}));

const online: BackendState = {
  status: "online",
  info: { name: "FuryBuilder", version: "1.2.3" },
  error: null,
};

beforeEach(() => {
  useEditorStore.setState({ theme: "dark", uiScale: 1 });
});

function renderDialog(onOpenChange = vi.fn()) {
  render(<SettingsDialog open onOpenChange={onOpenChange} backend={online} />);
  return onOpenChange;
}

describe("settings dialog", () => {
  it("splits settings into five sections and opens on appearance", async () => {
    const user = userEvent.setup();
    renderDialog();

    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(screen.getAllByRole("tab").map((tab) => tab.textContent)).toEqual([
      "Appearance",
      "Components",
      "Updates",
      "Diagnostics",
      "Advanced",
    ]);
    expect(screen.getByRole("tab", { name: "Appearance" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(screen.getByRole("heading", { name: "Appearance" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Dark" })).toBeInTheDocument();

    await user.click(screen.getByRole("tab", { name: "Components" }));
    expect(screen.getByRole("heading", { name: "Components" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Minecraft assets" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Java runtime" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Minecraft server JAR" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Vanilla readiness" })).toBeInTheDocument();

    await user.click(screen.getByRole("tab", { name: "Updates" }));
    expect(screen.getByRole("heading", { name: "Updates" })).toBeInTheDocument();

    await user.click(screen.getByRole("tab", { name: "Diagnostics" }));
    expect(screen.getByRole("heading", { name: "Diagnostics" })).toBeInTheDocument();

    await user.click(screen.getByRole("tab", { name: "Advanced" }));
    expect(screen.getByRole("heading", { name: "Backend" })).toBeInTheDocument();
    expect(screen.getByText("online")).toBeInTheDocument();
    expect(screen.getByText("1.2.3")).toBeInTheDocument();
  });

  it("keeps theme controls in appearance and closes without saving dialogs", async () => {
    const user = userEvent.setup();
    const onOpenChange = renderDialog();

    await user.click(screen.getByRole("button", { name: "Light" }));
    expect(useEditorStore.getState().theme).toBe("light");

    const close = screen.getAllByRole("button", { name: "Close" }).at(-1);
    expect(close).toBeDefined();
    await user.click(close!);
    expect(onOpenChange.mock.calls[0]?.[0]).toBe(false);
  });
});
