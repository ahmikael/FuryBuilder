import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { ComponentStepState } from "@/features/project/ProjectComponentsStep";
import { ProjectReviewStep } from "@/features/project/ProjectReviewStep";
import type { JavaWorldAnalysis } from "@/project/types";
import type { VersionProfile } from "@/project/version-types";

const profile: VersionProfile = {
  id: "java-classic-test",
  minecraftVersion: "1.21.4",
  dataVersion: 4189,
  releaseType: "release",
  family: "modern",
  worldLayout: "classic-anvil",
  chunkFamily: "modern-paletted",
  biomeFormat: "palette-4x4x4",
  dimensionProfiles: [
    { id: "minecraft:overworld", kind: "overworld", minY: -64, height: 384, folder: "region" },
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
    supportLevel: "full",
    read: true,
    edit: true,
    write: true,
    export: true,
    generate: true,
    assets: true,
  },
  notes: [],
};

const components: ComponentStepState = {
  targetKey: "java-classic-test|empty",
  checking: false,
  ready: true,
  errors: [],
  warnings: [],
  summary: {
    java: {
      kind: "java",
      mode: "managed",
      state: "ready",
      version: "21.0.5",
      bytes: 120_000_000,
      hash: "a".repeat(64),
      path: "C:\\cache\\java\\bin\\java.exe",
    },
    assets: {
      kind: "assets",
      mode: "managed",
      state: "ready",
      version: "1.21.4",
      bytes: 54_200_000,
      hash: "b".repeat(40),
      path: "C:\\cache\\assets\\1.21.4\\client.jar",
    },
    server: null,
    catalog: {
      id: "java-1.21.4-test",
      hash: "e".repeat(64),
      blocks: 1234,
      items: 567,
    },
    totalBytes: 174_200_000,
  },
};

const analysis: JavaWorldAnalysis = {
  root: "C:\\saves\\World",
  layout: "classic_anvil",
  selectionKind: "world_folder",
  completeSave: true,
  supported: true,
  dataVersion: 4189,
  versionId: 4189,
  versionName: "1.21.4",
  levelName: "Review test",
  declaredDimensions: ["minecraft:overworld"],
  dimensions: [
    {
      id: "minecraft:overworld",
      kind: "overworld",
      minY: -64,
      height: 384,
      regionFiles: 4,
      chunks: 841,
    },
  ],
  regions: 4,
  chunks: 841,
  blockEntities: 3,
  entities: 2,
  sourceBytes: 48_000_000,
  sourcePath: "C:\\saves\\World",
  sourceKind: "folder",
  archive: null,
  warnings: ["the selected world is opened read-only and is never modified"],
  blockers: [],
  supportMatrix: { schemaVersion: 1, versions: [], categories: [] },
};

function review(overrides: Partial<Parameters<typeof ProjectReviewStep>[0]> = {}) {
  return (
    <ProjectReviewStep
      name="Review World"
      mode="empty"
      version="1.21.4"
      destination="Review World.furybuilder"
      projectsRoot="C:\\Users\\Test\\Documents\\FuryBuilder"
      analysis={null}
      flat={null}
      flatFootprint={33}
      importStrategy="importAsIs"
      profile={profile}
      components={components}
      {...overrides}
    />
  );
}

describe("project review step", () => {
  it("summarizes an empty source, components, catalog and bounded estimate", () => {
    render(review());

    expect(screen.getByText("Review World")).toBeInTheDocument();
    expect(screen.getByText("Empty Project")).toBeInTheDocument();
    expect(screen.getAllByText("Managed").length).toBeGreaterThan(0);
    expect(screen.getByText("21.0.5 · 114.4 MiB · aaaaaaaaaaaa…")).toBeInTheDocument();
    expect(screen.getByText("1.21.4 · 51.7 MiB · bbbbbbbbbbbb…")).toBeInTheDocument();
    expect(screen.getByText("1234 blocks · 567 items")).toBeInTheDocument();
    expect(screen.getByText("No world data yet")).toBeInTheDocument();
    expect(screen.getByText("Ready immediately")).toBeInTheDocument();
    expect(screen.getByText("Ready to create")).toBeInTheDocument();
  });

  it("shows the flat block estimate and warns that it is not vanilla generation", () => {
    render(
      review({
        mode: "flat",
        flat: {
          baseY: 62,
          layers: [
            { block: "minecraft:stone", thickness: 2 },
            { block: "minecraft:dirt", thickness: 1 },
          ],
          biome: "minecraft:plains",
        },
      }),
    );

    expect(screen.getByText("3,267 blocks · about 216.0 KiB")).toBeInTheDocument();
    expect(screen.getByText("Under a second")).toBeInTheDocument();
    expect(screen.getByText(/33×33 platform across 9 chunk column/)).toBeInTheDocument();
    expect(screen.getByText(/not vanilla terrain generation/)).toBeInTheDocument();
  });

  it("repeats import source size, chunk count and every relevant warning", () => {
    render(review({ mode: "import", analysis }));

    expect(screen.getByText(/World folder · C:\\saves\\World/)).toBeInTheDocument();
    expect(screen.getByText("Import as-is")).toBeInTheDocument();
    expect(screen.getByText(/45\.8 MiB source \+ about 19\.7 MiB overlay/)).toBeInTheDocument();
    expect(screen.getByText(/841 chunk\(s\) from 4 region file\(s\)/)).toBeInTheDocument();
    expect(screen.getAllByText(/never downgraded/).length).toBeGreaterThan(0);
    expect(screen.getByText(/3 block entities and 2 entities stay/)).toBeInTheDocument();
    expect(screen.getAllByText(/read-only/).length).toBeGreaterThan(0);
  });

  it("shows placeholder warnings and no catalog claim", () => {
    render(
      review({
        components: {
          ...components,
          warnings: [
            "Placeholder textures are active; there is no verified block/item catalog for this project.",
          ],
          summary: {
            ...components.summary!,
            assets: {
              kind: "assets",
              mode: "placeholder",
              state: "unverified",
              version: null,
              bytes: null,
              hash: null,
              path: null,
            },
            catalog: null,
          },
        },
      }),
    );

    expect(screen.getByText("Placeholder")).toBeInTheDocument();
    expect(screen.getByText("No client JAR or catalog")).toBeInTheDocument();
    expect(screen.queryByText("1234 blocks · 567 items")).not.toBeInTheDocument();
    expect(screen.getByText(/Placeholder textures are active/)).toBeInTheDocument();
  });
});
