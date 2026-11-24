import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Coffee } from "lucide-react";
import { describe, expect, it, vi } from "vitest";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import type { ComponentStatus as ComponentStatusData } from "@/lib/component-types";

import { ComponentCard, ComponentPicker, ComponentStatus } from "./index";

const ready: ComponentStatusData = {
  kind: "java",
  required: true,
  mode: "managed",
  state: "ready",
  version: "21.0.5",
  bytes: 120_000_000,
  hash: { algorithm: "sha256", value: "a".repeat(64) },
  path: "C:\\cache\\java\\bin\\java.exe",
  notes: ["probed locally"],
};

describe("shared component primitives", () => {
  it("renders one card layout for status, inventory metadata and actions", () => {
    render(
      <ComponentCard
        label="Java runtimes"
        icon={<Coffee className="size-4" />}
        title="Java 21"
        description="Runtime used by the official server."
        status={ready}
        meta={<Badge variant="secondary">1 · 114.4 MiB</Badge>}
        action={<Button size="sm">Open folder</Button>}
      >
        <p>Card body</p>
      </ComponentCard>,
    );

    const card = screen.getByRole("group", { name: "Java runtimes" });
    expect(card).toHaveTextContent("Java 21");
    expect(card).toHaveTextContent("Runtime used by the official server.");
    expect(card).toHaveTextContent("Ready");
    expect(card).toHaveTextContent("Managed");
    expect(card).toHaveTextContent("21.0.5");
    expect(card).toHaveTextContent("114.4 MiB");
    expect(card).toHaveTextContent("SHA256 aaaaaaaaaaaa…");
    expect(card).toHaveTextContent("probed locally");
    expect(card).toHaveTextContent("Card body");
    expect(screen.getByRole("button", { name: "Open folder" })).toBeInTheDocument();
    expect(screen.getByTitle(ready.path as string)).toBeInTheDocument();
  });

  it("shows verification facts with an optional surface-specific label", () => {
    render(<ComponentStatus status={ready} label="Verified" />);
    expect(screen.getByText("Verified")).toBeInTheDocument();
    expect(screen.queryByText("Ready")).not.toBeInTheDocument();
  });

  it("explains placeholder, missing and failed component states", () => {
    const { rerender } = render(
      <ComponentStatus
        status={{
          kind: "assets",
          required: true,
          mode: "placeholder",
          state: "unverified",
          notes: ["placeholder textures are active"],
        }}
      />,
    );
    expect(screen.getByText("Unverified")).toBeInTheDocument();
    expect(screen.getByText("Placeholder")).toBeInTheDocument();
    expect(
      screen.getByText(
        "No client JAR, catalog or checksum is associated with this explicit choice.",
      ),
    ).toBeInTheDocument();
    expect(screen.getByText("placeholder textures are active")).toBeInTheDocument();

    rerender(
      <ComponentStatus
        status={{
          kind: "server",
          required: true,
          state: "incompatible",
          error: "the cached server JAR does not match its SHA-1",
        }}
      />,
    );
    expect(screen.getByText("Incompatible")).toBeInTheDocument();
    expect(screen.getByText("the cached server JAR does not match its SHA-1")).toBeInTheDocument();

    rerender(<ComponentStatus status={{ kind: "java", required: false, state: "missing" }} />);
    expect(screen.getByText("Nothing is selected for this project yet.")).toBeInTheDocument();
  });

  it("selects only declared picker options and respects disabled", async () => {
    const onChange = vi.fn();
    const user = userEvent.setup();
    const { rerender } = render(
      <ComponentPicker
        label="Java source"
        value="auto"
        options={[
          { value: "auto", label: "Auto" },
          { value: "existing", label: "Use existing" },
          { value: "manual", label: "Manual path" },
        ]}
        onChange={onChange}
      />,
    );

    const group = screen.getByRole("group", { name: "Java source" });
    await user.click(screen.getByRole("button", { name: "Use existing" }));
    expect(onChange).toHaveBeenCalledWith("existing");
    expect(group).toBeInTheDocument();

    rerender(
      <ComponentPicker
        label="Java source"
        value="auto"
        disabled
        options={[{ value: "auto", label: "Auto" }]}
        onChange={onChange}
      />,
    );
    expect(screen.getByRole("button", { name: "Auto" })).toBeDisabled();
  });
});
