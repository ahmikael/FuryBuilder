import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { ProjectVersionStep } from "@/features/project/ProjectVersionStep";
import type { VersionCatalogReport, VersionProfile } from "@/project/version-types";

function profile(
  id: string,
  version: string,
  dataVersion: number,
  supportLevel: VersionProfile["capabilities"]["supportLevel"],
  capabilities: Partial<VersionProfile["capabilities"]> = {},
): VersionProfile {
  return {
    id,
    minecraftVersion: version,
    dataVersion,
    releaseType: "release",
    family: "modern",
    worldLayout: "classic-anvil",
    chunkFamily: "modern-paletted",
    biomeFormat: "palette-4x4x4",
    dimensionProfiles: [
      {
        id: "minecraft:overworld",
        kind: "overworld",
        minY: -64,
        height: 384,
        folder: "region",
      },
    ],
    java: { minimumMajor: 21, recommendedMajor: 21 },
    resourcePackFormat: 46,
    datapackFormat: 61,
    contentCatalog: {
      registryFormat: "named",
      blockstateFormat: "json",
      itemModelFormat: "item-definitions",
      creativeCategories: true,
      dataComponents: true,
    },
    capabilities: {
      supportLevel,
      read: true,
      edit: true,
      write: true,
      export: true,
      generate: true,
      assets: true,
      ...capabilities,
    },
    notes: [],
  };
}

function catalog(entry: VersionProfile | null): VersionCatalogReport {
  return {
    source: "cache",
    fetchedAtUnix: 1_700_000_000,
    latestRelease: "1.21.4",
    latestSnapshot: "26.1-snapshot-3",
    recommended: "1.21.4",
    versions: [
      {
        id: entry?.minecraftVersion ?? "26.1-snapshot-3",
        versionType: "release",
        releaseTime: "2026-01-01T00:00:00+00:00",
        latestRelease: true,
        latestSnapshot: false,
        recommended: true,
        profile: entry,
        unavailableReason: entry
          ? null
          : "This build has no verified profile for this release yet.",
      },
    ],
  };
}

function renderStep(report: VersionCatalogReport) {
  render(
    <ProjectVersionStep
      catalog={report}
      loading={false}
      error={null}
      desktop
      selected={report.versions[0]!.id}
      locked={false}
      onSelect={() => undefined}
      onRetry={() => undefined}
    />,
  );
}

describe("project version step support contract", () => {
  it("shows every verified capability for the selected version", () => {
    renderStep(catalog(profile("java-classic-1.21.4", "1.21.4", 4189, "full")));
    expect(screen.getAllByText("Full support").length).toBeGreaterThan(0);
    expect(screen.getByText("What this build can do")).toBeInTheDocument();
    expect(screen.getByLabelText("Generate: available")).toBeInTheDocument();
    expect(
      screen.getByText("All listed capabilities are verified for this release."),
    ).toBeVisible();
  });

  it("explains an unverified split version instead of presenting it as selectable", () => {
    renderStep(
      catalog(
        profile("java-split-26.1", "26.1", 4786, "unverified", {
          read: false,
          edit: false,
          write: false,
          export: false,
          generate: false,
          assets: false,
        }),
      ),
    );
    expect(screen.getAllByText("Unverified").length).toBeGreaterThan(0);
    expect(screen.getByText("No verified world adapter")).toBeInTheDocument();
    expect(screen.getByLabelText(/^Read: No verified world adapter/)).toBeInTheDocument();
    expect(screen.getAllByText(/No verified world adapter exists for this version/).length).toBe(6);
  });

  it("states why generation is unavailable for a read-write profile without it", () => {
    renderStep(
      catalog(
        profile("java-classic-1.18.2", "1.18.2", 2975, "read-write-subset", {
          generate: false,
        }),
      ),
    );
    expect(screen.getAllByText("Read + write subset").length).toBeGreaterThan(0);
    expect(screen.getByLabelText(/^Generate: Vanilla generation/)).toBeInTheDocument();
  });
});
