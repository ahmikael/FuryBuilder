import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Toaster } from "@/components/ui/toast";
import { pushToast } from "@/state/toast-store";

describe("shadcn toaster", () => {
  it("renders a toast pushed through the shared manager", async () => {
    render(
      <Toaster limit={4}>
        <div>workspace</div>
      </Toaster>,
    );

    pushToast({
      kind: "success",
      title: "Project saved",
      description: "The world is safe on disk.",
    });

    expect(await screen.findByText("Project saved")).toBeInTheDocument();
    expect(screen.getByText("The world is safe on disk.")).toBeInTheDocument();
  });
});
