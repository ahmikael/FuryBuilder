import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { ProjectVersionSupport } from "@/features/project/ProjectVersionSupport";
import type { ProjectSummary } from "@/project/types";
import type { VersionCapabilities } from "@/project/version-types";

const base: ProjectSummary = {
  folder: "Castle.furybuilder",
  projectId: "project-castle",
  name: "Castle",
  creationMode: "empty",
  targetMinecraftVersion: "1.21.4",
  targetProfileId: "java-classic-1.21.4",
  targetDataVersion: 4189,
  worldRevision: 4,
  updatedAtMs: 1_700_000_000_000,
  lastSavedAtMs: 1_700_000_000_000,
  lastSaveKind: "manual",
  bytes: 2048,
  schemaVersion: 2,
  recoverable: false,
  capabilities: null,
};

const full: VersionCapabilities = {
  supportLevel: "full",
  read: true,
  edit: true,
  write: true,
  export: true,
  generate: true,
  assets: true,
};

describe("project version support", () => {
  it("shows the exact target and its verified capabilities", () => {
    render(<ProjectVersionSupport project={{ ...base, capabilities: full }} />);

    expect(screen.getByText("Version and support")).toBeInTheDocument();
    expect(screen.getByText("Full support")).toBeInTheDocument();
    expect(screen.getByText(/1\.21\.4 · DataVersion 4189/)).toBeInTheDocument();
    expect(screen.getByText("java-classic-1.21.4")).toBeInTheDocument();
    expect(screen.getByText("rev 4")).toBeInTheDocument();
    expect(screen.getByLabelText("Generate: available")).toBeInTheDocument();
  });

  it("explains disabled actions instead of hiding them", () => {
    const readOnly: VersionCapabilities = {
      ...full,
      supportLevel: "read-only",
      edit: false,
      write: false,
      export: false,
      generate: false,
      assets: false,
    };
    render(<ProjectVersionSupport project={{ ...base, capabilities: readOnly }} />);

    expect(screen.getByText("Read-only")).toBeInTheDocument();
    expect(screen.getByLabelText(/^Edit: Read-only/)).toBeInTheDocument();
    expect(screen.getByText(/Read-only: the world can be viewed and copied/)).toBeVisible();
  });

  it("does not claim capabilities when the profile is unknown", () => {
    render(<ProjectVersionSupport project={base} />);
    expect(screen.getByText("No verified profile")).toBeInTheDocument();
    expect(screen.getByText(/no verified profile for the project version/)).toBeVisible();
  });
});
