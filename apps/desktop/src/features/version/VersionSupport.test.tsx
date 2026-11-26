import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { VersionCapabilities, VersionSupportBadge } from "@/features/version/VersionSupport";
import type { VersionCapabilities as Capabilities } from "@/project/version-types";

const full: Capabilities = {
  supportLevel: "full",
  read: true,
  edit: true,
  write: true,
  export: true,
  generate: true,
  assets: true,
};

const readOnly: Capabilities = {
  supportLevel: "read-only",
  read: true,
  edit: false,
  write: false,
  export: false,
  generate: false,
  assets: false,
};

describe("version support UI", () => {
  it("renders the support badge from the verified level", () => {
    render(<VersionSupportBadge level="read-write-subset" />);
    const badge = screen.getByText("Read + write subset");
    expect(badge).toHaveAttribute("title");
    expect(badge.getAttribute("title")).toContain("verified subset");
  });

  it("shows every capability as an explicit available or disabled fact", () => {
    render(<VersionCapabilities capabilities={full} />);
    expect(screen.getByLabelText("Version capabilities")).toBeInTheDocument();
    for (const label of ["Read", "Edit", "Save", "Export", "Generate", "Assets"]) {
      expect(screen.getByLabelText(`${label}: available`)).toBeInTheDocument();
    }
    expect(
      screen.getByText("All listed capabilities are verified for this release."),
    ).toBeVisible();
  });

  it("keeps disabled capabilities visible with their reasons", () => {
    render(<VersionCapabilities capabilities={readOnly} />);
    expect(screen.getByLabelText("Read: available")).toBeInTheDocument();
    expect(screen.getByLabelText(/^Edit: Read-only/)).toBeInTheDocument();
    expect(screen.getByLabelText(/^Save: Read-only/)).toBeInTheDocument();
    expect(screen.getByText(/Read-only: the world can be viewed and copied/)).toBeVisible();
    expect(screen.getByText(/Vanilla generation is not available for this version/)).toBeVisible();
  });

  it("does not invent capabilities when the profile is missing", () => {
    render(<VersionCapabilities capabilities={null} />);
    expect(screen.queryByLabelText("Version capabilities")).not.toBeInTheDocument();
    expect(screen.getByText(/no verified profile for the project version/)).toBeVisible();
  });
});
