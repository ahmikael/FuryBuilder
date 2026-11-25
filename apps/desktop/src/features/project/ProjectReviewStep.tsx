import type { ReactNode } from "react";
import { BookOpen, CheckCircle2, Clock, Database, TriangleAlert } from "lucide-react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent } from "@/components/ui/card";
import type {
  ComponentStepItem,
  ComponentStepState,
} from "@/features/project/ProjectComponentsStep";
import { estimateProjectCreation } from "@/features/project/project-estimate";
import {
  creationModeLabel,
  formatBytes,
  formatResourceId,
  type CreationMode,
  type FlatPreset,
  type ImportStrategy,
  type JavaWorldAnalysis,
} from "@/project/types";
import { supportLevelLabel, type VersionProfile } from "@/project/version-types";

export interface ProjectReviewStepProps {
  name: string;
  mode: CreationMode;
  version: string;
  destination: string;
  projectsRoot: string | null;
  analysis: JavaWorldAnalysis | null;
  flat: FlatPreset | null;
  flatFootprint: number;
  importStrategy: ImportStrategy;
  profile: VersionProfile | null;
  components: ComponentStepState | null;
}

function componentModeLabel(item: ComponentStepItem): string {
  switch (item.mode) {
    case "managed":
      return "Managed";
    case "manual":
      return "Manual";
    case "placeholder":
      return "Placeholder";
    default:
      return "Not set up";
  }
}

function componentStateLabel(item: ComponentStepItem): string {
  switch (item.state) {
    case "ready":
      return "Ready";
    case "missing":
      return "Missing";
    case "unverified":
      return "Unverified";
    case "incompatible":
      return "Incompatible";
    case "error":
      return "Error";
    case "not_required":
      return "Not required";
  }
}

function stateVariant(state: ComponentStepItem["state"]): "secondary" | "destructive" | "outline" {
  switch (state) {
    case "ready":
      return "secondary";
    case "incompatible":
    case "error":
      return "destructive";
    default:
      return "outline";
  }
}

function ComponentRow({
  label,
  item,
  fallback,
}: {
  label: string;
  item: ComponentStepItem | null;
  fallback: string;
}) {
  if (!item) {
    return (
      <div className="flex flex-wrap items-center gap-3 px-4 py-3">
        <dt className="w-28 text-sm text-muted-foreground">{label}</dt>
        <dd className="text-sm text-muted-foreground">{fallback}</dd>
      </div>
    );
  }
  const details = [
    item.version,
    item.bytes !== null ? formatBytes(item.bytes) : null,
    item.hash ? `${item.hash.slice(0, 12)}…` : null,
  ]
    .filter((value): value is string => Boolean(value))
    .join(" · ");
  return (
    <div className="flex flex-wrap items-center gap-3 px-4 py-3">
      <dt className="w-28 text-sm text-muted-foreground">{label}</dt>
      <dd className="flex min-w-0 flex-1 flex-wrap items-center gap-2 text-sm">
        <span>{componentModeLabel(item)}</span>
        {item.mode === "placeholder" ? (
          <span className="text-muted-foreground">No client JAR or catalog</span>
        ) : details ? (
          <span className="font-mono text-xs text-muted-foreground">{details}</span>
        ) : null}
        <Badge variant={stateVariant(item.state)}>{componentStateLabel(item)}</Badge>
      </dd>
    </div>
  );
}

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex flex-wrap items-center gap-3 px-4 py-3">
      <dt className="w-28 text-sm text-muted-foreground">{label}</dt>
      <dd className="min-w-0 flex-1 text-sm">{children}</dd>
    </div>
  );
}

/** Final read-only summary of every choice, warning and bounded estimate before creation. */
export function ProjectReviewStep({
  name,
  mode,
  version,
  destination,
  projectsRoot,
  analysis,
  flat,
  flatFootprint,
  importStrategy,
  profile,
  components,
}: ProjectReviewStepProps) {
  const summary = components?.summary ?? null;
  const estimate = estimateProjectCreation({ mode, flat, flatFootprint, analysis });
  const flatSource =
    mode === "flat"
      ? flat
        ? `Flat platform · ${flat.layers.length} layer(s) · Y ${flat.baseY}..=${flat.baseY + flat.layers.reduce((total, layer) => total + layer.thickness, 0) - 1}`
        : "Flat starting platform"
      : null;
  const layerSummary = flat?.layers
    .map((layer) => `${formatResourceId(layer.block)} ×${layer.thickness}`)
    .join(" → ");

  const warnings: string[] = [];
  const addWarning = (warning: string): void => {
    if (warning.trim() && !warnings.includes(warning)) warnings.push(warning);
  };
  for (const note of profile?.notes ?? []) addWarning(note);
  for (const warning of components?.warnings ?? []) addWarning(warning);
  for (const warning of analysis?.warnings ?? []) addWarning(warning);
  if (mode === "flat") {
    addWarning("Flat base is editable FuryBuilder world data, not vanilla terrain generation.");
  }
  if (mode === "import") {
    addWarning("The detected version is never downgraded; conversion needs a verified converter.");
    if (
      analysis &&
      analysis.blockEntities !== null &&
      analysis.entities !== null &&
      (analysis.blockEntities > 0 || analysis.entities > 0)
    ) {
      addWarning(
        `${analysis.blockEntities} block entit${analysis.blockEntities === 1 ? "y" : "ies"} and ${analysis.entities} entit${analysis.entities === 1 ? "y" : "ies"} stay in the immutable source copy and are not editable here.`,
      );
    }
  }
  if (profile && profile.capabilities.supportLevel !== "full") {
    addWarning(
      `This version is ${supportLevelLabel(profile.capabilities.supportLevel)}; only the capabilities shown in the version card are promised.`,
    );
  }

  return (
    <div className="space-y-4">
      <Card className="gap-0 py-0">
        <CardContent className="p-0">
          <dl className="divide-y">
            <Row label="Name">{name.trim() || "Named by the generated world"}</Row>
            <Row label="Mode">{creationModeLabel(mode)}</Row>
            <Row label="Version">
              <span className="font-mono">{version || "Unknown"}</span>
            </Row>
            <Row label="Folder">
              <span className="block truncate font-mono text-xs">
                {destination || "Named after the project"}
              </span>
            </Row>
            <Row label="Location">
              <span className="break-all font-mono text-xs">
                {projectsRoot ?? "Default library folder"}
              </span>
            </Row>
            <Row label="Source">
              <span className="block truncate font-mono text-xs">
                {mode === "import"
                  ? `${analysis?.sourceKind === "archive" ? "World archive" : "World folder"} · ${analysis?.sourcePath ?? "not analyzed"}`
                  : mode === "flat"
                    ? flatSource
                    : "Empty world"}
              </span>
            </Row>
            {mode === "import" ? (
              <Row label="Strategy">
                {importStrategy === "importAsIs" ? "Import as-is" : "Convert"}
              </Row>
            ) : null}
            {mode === "flat" && layerSummary ? <Row label="Layers">{layerSummary}</Row> : null}
            {mode === "flat" && flat ? (
              <Row label="Biome">
                <span className="font-mono text-xs">{flat.biome}</span>
              </Row>
            ) : null}
            {analysis ? (
              <Row label="Detected">
                <span className="font-mono text-xs">
                  {analysis.versionName ?? "unknown version"} · DataVersion{" "}
                  {analysis.dataVersion ?? "unknown"} · {analysis.chunks} chunk(s)
                </span>
              </Row>
            ) : null}
            <ComponentRow
              label="Java"
              item={summary?.java ?? null}
              fallback={mode === "vanilla" ? "Required in Components" : "Not set up (optional)"}
            />
            <ComponentRow
              label="Client assets"
              item={summary?.assets ?? null}
              fallback="Not captured; reopen Components"
            />
            {mode === "vanilla" || (summary?.server && summary.server.state !== "not_required") ? (
              <ComponentRow
                label="Server"
                item={summary?.server ?? null}
                fallback="Required in Components"
              />
            ) : null}
            {summary?.catalog ? (
              <Row label="Catalog">
                <span className="flex flex-wrap items-center gap-2">
                  <BookOpen className="size-4" />
                  <span>
                    {summary.catalog.blocks} blocks · {summary.catalog.items} items
                  </span>
                  <span className="truncate font-mono text-xs text-muted-foreground">
                    {summary.catalog.id}
                  </span>
                </span>
              </Row>
            ) : null}
            <Row label="Expected size">
              <span className="flex flex-wrap items-center gap-2">
                <Database className="size-4 text-muted-foreground" />
                {estimate.sizeLabel}
              </span>
              <p className="mt-1 text-xs text-muted-foreground">{estimate.detail}</p>
            </Row>
            <Row label="Expected time">
              <span className="flex flex-wrap items-center gap-2">
                <Clock className="size-4 text-muted-foreground" />
                {estimate.timeLabel}
              </span>
            </Row>
          </dl>
        </CardContent>
      </Card>

      {warnings.length > 0 ? (
        <Alert>
          <TriangleAlert />
          <AlertTitle>Review these limitations</AlertTitle>
          <AlertDescription>
            <ul className="list-disc space-y-1 pl-4">
              {warnings.map((warning) => (
                <li key={warning}>{warning}</li>
              ))}
            </ul>
          </AlertDescription>
        </Alert>
      ) : (
        <Alert>
          <CheckCircle2 />
          <AlertTitle>Ready to create</AlertTitle>
          <AlertDescription>
            No warnings apply to this exact version and source. Creation still verifies the project
            after it is written.
          </AlertDescription>
        </Alert>
      )}

      <p className="text-sm leading-relaxed text-muted-foreground">
        {mode === "import"
          ? "The source is copied byte for byte and stays read-only. Edits live in the project overlay; the version is never downgraded."
          : "The project is saved with the current project schema and the supported target profile."}
      </p>
    </div>
  );
}
