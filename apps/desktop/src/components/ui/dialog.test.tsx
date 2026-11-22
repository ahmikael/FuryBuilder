import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Dialog, DialogContent, DialogFooter, DialogTitle } from "@/components/ui/dialog";

describe("Dialog", () => {
  it("uses a comfortable default width and lets a caller select a wider modal", () => {
    const { rerender } = render(
      <Dialog open>
        <DialogContent>
          <DialogTitle>Default dialog</DialogTitle>
        </DialogContent>
      </Dialog>,
    );

    expect(screen.getByRole("dialog")).toHaveClass("max-w-lg");

    rerender(
      <Dialog open>
        <DialogContent className="max-w-5xl">
          <DialogTitle>Wide dialog</DialogTitle>
        </DialogContent>
      </Dialog>,
    );

    expect(screen.getByRole("dialog")).toHaveClass("max-w-5xl");
    expect(screen.getByRole("dialog")).not.toHaveClass("max-w-lg");
  });

  it("keeps footer actions in the dialog surface instead of a tinted panel", () => {
    render(
      <Dialog open>
        <DialogContent>
          <DialogTitle>Dialog with actions</DialogTitle>
          <DialogFooter data-testid="dialog-footer">Actions</DialogFooter>
        </DialogContent>
      </Dialog>,
    );

    const footer = screen.getByTestId("dialog-footer");
    expect(footer).toHaveClass("sm:flex-row", "sm:justify-end");
    expect(footer).not.toHaveClass("bg-muted/50", "border-t", "p-4");
  });
});
