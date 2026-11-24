import { useEffect, useMemo, useState } from "react";
import {
  ArrowLeft,
  ArrowRight,
  Check,
  FileArchive,
  FolderOpen,
  Info,
  Loader2,
  RefreshCw,
  Sparkles,
  TriangleAlert,
  X,
} from "lucide-react";
import logoUrl from "@/assets/branding/furybuilder-logo.png";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Field, FieldDescription, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Progress } from "@/components/ui/progress";
import { Separator } from "@/components/ui/separator";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { ProjectVersionStep } from "@/features/project/ProjectVersionStep";
import {
  ProjectComponentsStep,
  type ComponentStepState,
} from "@/features/project/ProjectComponentsStep";
import { ProjectReviewStep } from "@/features/project/ProjectReviewStep";
import { EmptySourceStep, FlatSourceStep } from "@/features/project/ProjectSourceStep";
import { VanillaWorldWizard } from "@/features/project/VanillaWorldWizard";
import { presentError, type ErrorPresentation } from "@/lib/error-messages";
import { isTauriRuntime } from "@/lib/ipc";
import { fetchProjectDefaults, type CreateProjectOptions } from "@/lib/project-api";
import { fetchVersionCatalog } from "@/lib/version-api";
import {
  CREATION_MODES,
  formatBytes,
  type CreationMode,
  type FlatDefaults,
  type FlatPreset,
  type ImportSourceKind,
  type ImportStrategy,
  type JavaWorldAnalysis,
  type ProjectDefaults,
} from "@/project/types";
import type { VersionCatalogReport, VersionProfile } from "@/project/version-types";
import { useProjectStore } from "@/state/project-store";
import { pushToast } from "@/state/toast-store";

interface WizardStep {
  id: "basics" | "version" | "source" | "components" | "review";
  title: string;
  description: string;
}

const BASICS_STEP: WizardStep = {
  id: "basics",
  title: "Basics",
  description: "Name the project, choose its folder and pick a starting world.",
};

const VERSION_STEP: WizardStep = {
  id: "version",
  title: "Version",
  description: "Pick the exact Minecraft version and review what this build can do with it.",
};

const SOURCE_STEP: WizardStep = {
  id: "source",
  title: "Source",
  description: "Describe or inspect the world data that the project starts with.",
};

const COMPONENTS_STEP: WizardStep = {
  id: "components",
  title: "Components",
  description:
    "Review and prepare the Java runtime, Minecraft client assets and official server for this exact version.",
};

const REVIEW_STEP: WizardStep = {
  id: "review",
  title: "Review",
  description: "Check the summary before the project is created.",
};

function wizardSteps(mode: CreationMode): WizardStep[] {
  if (mode === "import") {
    return [BASICS_STEP, SOURCE_STEP, VERSION_STEP, COMPONENTS_STEP, REVIEW_STEP];
  }
  if (mode === "vanilla") {
    return [BASICS_STEP, VERSION_STEP, COMPONENTS_STEP, SOURCE_STEP];
  }
  return [BASICS_STEP, VERSION_STEP, SOURCE_STEP, COMPONENTS_STEP, REVIEW_STEP];
}

function projectFolderName(folder: string, suffix: string): string {
  const trimmed = folder.trim();
  if (trimmed.length === 0 || trimmed.endsWith(suffix)) return trimmed;
  return `${trimmed}${suffix}`;
}

function folderError(folder: string, suffix: string): string | null {
  const trimmed = folder.trim();
  if (trimmed.length === 0) return null;
  if (
    trimmed === "." ||
    trimmed === ".." ||
    trimmed.includes("/") ||
    trimmed.includes("\\") ||
    trimmed.includes(":") ||
    /[<>"|?*]/.test(trimmed) ||
    [...trimmed].some((character) => character.charCodeAt(0) < 32)
  ) {
    return "Use a folder name without path separators or reserved characters.";
  }
  const normalized = projectFolderName(trimmed, suffix);
  if (normalized.length <= suffix.length + 1) return "Enter a folder name.";
  if (normalized.length > 160) return "The project folder must be at most 160 characters.";
  return null;
}

function versionErrors({
  catalog,
  loading,
  catalogError,
  selected,
  mode,
  analysis,
  desktop,
}: {
  catalog: VersionCatalogReport | null;
  loading: boolean;
  catalogError: ErrorPresentation | null;
  selected: string;
  mode: CreationMode;
  analysis: JavaWorldAnalysis | null;
  desktop: boolean;
}): string[] {
  if (!desktop) return ["Version selection needs the desktop app."];
  if (catalogError) return ["Retry loading the version catalog."];
  if (loading) return ["Wait for the version catalog to finish loading."];
  if (!catalog) return ["Load the version catalog before continuing."];
  if (mode === "import") {
    if (!analysis) return ["Analyze the world to detect its version."];
    if (!analysis.versionName) return ["The world does not declare a Minecraft version."];
  }
  if (selected.length === 0) return ["Choose a Minecraft version."];
  const entry = catalog.versions.find((version) => version.id === selected);
  if (!entry) return [`Minecraft ${selected} is not in the official version catalog.`];
  if (!entry.profile) {
    return [
      `This build has no verified profile for Minecraft ${selected}. Choose a supported version.`,
    ];
  }
  return [];
}

function flatPresetErrors(
  preset: FlatPreset | null,
  defaults: FlatDefaults | null,
  profile: VersionProfile | null,
): string[] {
  if (!profile) return ["Choose a verified version before describing the flat source."];
  if (!preset || !defaults) {
    return ["Flat source defaults are not available. Retry loading the project settings."];
  }
  const overworld = profile.dimensionProfiles.find((dimension) => dimension.kind === "overworld");
  if (!overworld) return ["The selected version has no overworld dimension."];

  const errors: string[] = [];
  const minY = overworld.minY;
  const maxY = overworld.minY + overworld.height - 1;
  const limits = defaults.limits;

  if (!Number.isInteger(preset.baseY)) {
    errors.push("The bottom Y must be a whole number.");
  }
  if (preset.layers.length === 0) {
    errors.push("Add at least one flat layer.");
  }
  if (preset.layers.length > limits.maxLayers) {
    errors.push(`A flat source supports at most ${limits.maxLayers} layers.`);
  }
  preset.layers.forEach((layer, index) => {
    if (!layer.block) {
      errors.push(`Choose a block for layer ${index + 1}.`);
    } else if (layer.block === "minecraft:air") {
      errors.push(`Layer ${index + 1} cannot be air.`);
    }
    if (
      !Number.isInteger(layer.thickness) ||
      layer.thickness < 1 ||
      layer.thickness > limits.maxLayerThickness
    ) {
      errors.push(
        `Layer ${index + 1} must be between 1 and ${limits.maxLayerThickness} blocks thick.`,
      );
    }
  });
  const total = preset.layers.reduce((sum, layer) => sum + layer.thickness, 0);
  if (total > limits.maxTotalThickness) {
    errors.push(`The flat source must be at most ${limits.maxTotalThickness} blocks thick.`);
  }
  if (!preset.biome.trim()) {
    errors.push("Choose a flat biome.");
  }
  if (Number.isInteger(preset.baseY) && (preset.baseY < minY || preset.baseY + total - 1 > maxY)) {
    errors.push(`Keep the flat source inside overworld Y ${minY}..=${maxY}.`);
  }
  return errors;
}

function stepErrors(
  step: WizardStep,
  name: string,
  mode: CreationMode,
  analysis: JavaWorldAnalysis | null,
  folder: string,
  projectFolder: string,
  folderSuffix: string,
  version: {
    catalog: VersionCatalogReport | null;
    loading: boolean;
    error: ErrorPresentation | null;
    selected: string;
    desktop: boolean;
  },
  source: {
    flat: FlatPreset | null;
    flatDefaults: FlatDefaults | null;
    profile: VersionProfile | null;
    importStrategy: ImportStrategy;
  },
  components: ComponentStepState | null,
  componentTargetKey: string | null,
): string[] {
  if (step.id === "basics") {
    const errors: string[] = [];
    if (mode !== "vanilla" && name.trim().length === 0) errors.push("Enter a project name.");
    const invalidFolder = folderError(projectFolder, folderSuffix);
    if (invalidFolder) errors.push(invalidFolder);
    return errors;
  }
  if (step.id === "version") {
    return versionErrors({
      catalog: version.catalog,
      loading: version.loading,
      catalogError: version.error,
      selected: version.selected,
      mode,
      analysis,
      desktop: version.desktop,
    });
  }
  if (step.id === "source" && mode === "import") {
    const errors: string[] = [];
    if (folder.trim().length === 0) {
      errors.push("Choose a Java world folder or .zip archive.");
    }
    if (!analysis) errors.push("Analyze the world before continuing.");
    if (analysis && !analysis.supported) {
      errors.push("This world has blockers and cannot be imported.");
    }
    if (analysis && analysis.sourcePath !== folder.trim()) {
      errors.push("Analyze the selected source again.");
    }
    if (source.importStrategy !== "importAsIs") {
      errors.push("Choose Import as-is; conversion is not available in this build.");
    }
    return errors;
  }
  if (step.id === "source" && mode === "flat") {
    return flatPresetErrors(source.flat, source.flatDefaults, source.profile);
  }
  if (step.id === "components") {
    if (!componentTargetKey) {
      return ["Choose a verified version before setting up components."];
    }
    if (!components || components.targetKey !== componentTargetKey) {
      return ["Open the Components step and verify the project components."];
    }
    return components.errors;
  }
  return [];
}

function StepNav({
  steps,
  active,
  furthest,
  onSelect,
}: {
  steps: WizardStep[];
  active: number;
  furthest: number;
  onSelect: (index: number) => void;
}) {
  return (
    <nav aria-label="Project creation steps">
      <ol className="flex flex-wrap items-center gap-2">
        {steps.map((step, index) => {
          const current = index === active;
          const done = index < active;
          return (
            <li key={step.id}>
              <Button
                type="button"
                size="sm"
                variant={current ? "default" : "ghost"}
                disabled={index > furthest}
                aria-current={current ? "step" : undefined}
                onClick={() => onSelect(index)}
              >
                {done ? (
                  <Check data-icon="inline-start" />
                ) : (
                  <span className="font-mono text-xs tabular-nums">{index + 1}</span>
                )}
                {step.title}
              </Button>
            </li>
          );
        })}
      </ol>
    </nav>
  );
}

function BasicsStep({
  name,
  mode,
  folder,
  folderSuffix,
  projectsRoot,
  disabled,
  onName,
  onFolder,
  onMode,
}: {
  name: string;
  mode: CreationMode;
  folder: string;
  folderSuffix: string;
  projectsRoot: string | null;
  disabled: boolean;
  onName: (value: string) => void;
  onFolder: (value: string) => void;
  onMode: (mode: CreationMode) => void;
}) {
  const modeEntry = CREATION_MODES.find((entry) => entry.mode === mode);
  return (
    <div className="space-y-6">
      {mode === "vanilla" ? null : (
        <Field>
          <FieldLabel htmlFor="wizard-project-name">Project name</FieldLabel>
          <Input
            id="wizard-project-name"
            value={name}
            placeholder="My Castle"
            maxLength={64}
            autoFocus
            disabled={disabled}
            onChange={(event) => onName(event.target.value)}
          />
          <FieldDescription>Shown in the project list and project settings.</FieldDescription>
        </Field>
      )}

      <Field>
        <FieldLabel htmlFor="wizard-project-folder">Project folder</FieldLabel>
        <Input
          id="wizard-project-folder"
          value={folder}
          placeholder="Named after the project"
          maxLength={160}
          disabled={disabled}
          onChange={(event) => onFolder(event.target.value)}
        />
        <FieldDescription>
          Created inside{" "}
          <span className="break-all font-mono text-xs">
            {projectsRoot ?? "the default library folder"}
          </span>
          . Leave blank to name the folder after the project;{" "}
          <span className="font-mono text-xs">{folderSuffix}</span> is added automatically. An
          existing folder is never overwritten.
        </FieldDescription>
      </Field>

      <div className="space-y-2">
        <p className="text-sm font-medium">Creation mode</p>
        <ToggleGroup
          value={[mode]}
          onValueChange={(value) => {
            const next = value[0];
            if (next === "empty" || next === "flat" || next === "import" || next === "vanilla") {
              onMode(next);
            }
          }}
          spacing={2}
          className="grid w-full grid-cols-1 sm:grid-cols-2"
        >
          {CREATION_MODES.map((entry) => (
            <ToggleGroupItem
              key={entry.mode}
              value={entry.mode}
              disabled={!entry.available || disabled}
              className="h-auto min-w-0 flex-col items-start gap-1.5 whitespace-normal p-4 text-left"
            >
              <span className="flex items-center gap-2 text-sm font-medium">
                {entry.label}
                {entry.available ? null : <Badge variant="secondary">Future</Badge>}
              </span>
              <span className="text-sm leading-relaxed text-muted-foreground">
                {entry.description}
              </span>
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
      </div>

      {modeEntry ? (
        <Alert>
          <Info />
          <AlertTitle>{modeEntry.label} requirements</AlertTitle>
          <AlertDescription>
            <ul className="list-disc space-y-1 pl-4">
              {modeEntry.requirements.map((requirement) => (
                <li key={requirement}>{requirement}</li>
              ))}
            </ul>
          </AlertDescription>
        </Alert>
      ) : null}
    </div>
  );
}

function ImportSourceStep({
  sourceKind,
  source,
  analysis,
  analysisLoading,
  strategy,
  disabled,
  onSourceKind,
  onSource,
  onBrowse,
  onAnalyze,
  onStrategy,
}: {
  sourceKind: ImportSourceKind;
  source: string;
  analysis: JavaWorldAnalysis | null;
  analysisLoading: boolean;
  strategy: ImportStrategy;
  disabled: boolean;
  onSourceKind: (value: ImportSourceKind) => void;
  onSource: (value: string) => void;
  onBrowse: () => void;
  onAnalyze: () => void;
  onStrategy: (value: ImportStrategy) => void;
}) {
  const archive = sourceKind === "archive";
  return (
    <div className="space-y-4">
      <Field>
        <FieldLabel>Source type</FieldLabel>
        <ToggleGroup
          variant="outline"
          size="sm"
          value={[sourceKind]}
          disabled={disabled}
          onValueChange={(value) => {
            const next = value[0];
            if (next === "folder" || next === "archive") onSourceKind(next);
          }}
        >
          <ToggleGroupItem value="folder">
            <FolderOpen data-icon="inline-start" />
            World folder
          </ToggleGroupItem>
          <ToggleGroupItem value="archive">
            <FileArchive data-icon="inline-start" />
            .zip archive
          </ToggleGroupItem>
        </ToggleGroup>
        <FieldDescription>
          Import a complete Java world folder or a .zip archive that contains one. The original
          source is never written.
        </FieldDescription>
      </Field>

      <Field>
        <FieldLabel htmlFor="wizard-import-source">
          {archive ? "World archive (.zip)" : "Java world folder"}
        </FieldLabel>
        <div className="flex flex-wrap items-center gap-2">
          <Input
            id="wizard-import-source"
            className="min-w-56 flex-1 font-mono text-xs"
            value={source}
            placeholder={archive ? "…/downloads/My World.zip" : "…/.minecraft/saves/My World"}
            disabled={disabled}
            onChange={(event) => onSource(event.target.value)}
          />
          <Button type="button" size="sm" variant="outline" disabled={disabled} onClick={onBrowse}>
            {archive ? (
              <FileArchive data-icon="inline-start" />
            ) : (
              <FolderOpen data-icon="inline-start" />
            )}
            Browse
          </Button>
          <Button
            type="button"
            size="sm"
            variant="outline"
            disabled={disabled || analysisLoading || source.trim().length === 0}
            onClick={onAnalyze}
          >
            {analysisLoading ? (
              <Loader2 data-icon="inline-start" className="animate-spin" />
            ) : (
              <RefreshCw data-icon="inline-start" />
            )}
            Analyze
          </Button>
        </div>
        <FieldDescription>
          Read-only analysis validates level.dat, the layout and every chunk before import. Archive
          contents are extracted to a temporary folder and removed after analysis.
        </FieldDescription>
      </Field>

      {analysis ? (
        <div className="space-y-3 rounded-xl border bg-card p-4">
          <div className="flex flex-wrap items-center gap-2 text-sm">
            <Badge variant={analysis.supported ? "secondary" : "destructive"} className="uppercase">
              {analysis.supported ? "supported" : "blocked"}
            </Badge>
            <span className="font-mono text-xs">
              DataVersion {analysis.dataVersion ?? "unknown"}
              {analysis.versionName ? ` · ${analysis.versionName}` : ""}
            </span>
            <span className="text-muted-foreground">{analysis.layout}</span>
            <Badge variant="outline">{analysis.sourceKind === "archive" ? ".zip" : "folder"}</Badge>
            <span className="ml-auto font-mono text-xs text-muted-foreground">
              {analysis.regions} region(s) · {analysis.chunks} chunk(s)
            </span>
          </div>

          <div className="grid gap-2 text-sm sm:grid-cols-3">
            <div>
              <p className="text-xs text-muted-foreground">Level</p>
              <p className="font-medium">{analysis.levelName ?? "Not declared"}</p>
            </div>
            <div>
              <p className="text-xs text-muted-foreground">Block entities</p>
              <p className="font-medium tabular-nums">{analysis.blockEntities}</p>
            </div>
            <div>
              <p className="text-xs text-muted-foreground">Entities</p>
              <p className="font-medium tabular-nums">{analysis.entities}</p>
            </div>
          </div>

          {analysis.archive ? (
            <p className="text-xs text-muted-foreground">
              Archive: {analysis.archive.entries} file(s) · {formatBytes(analysis.archive.bytes)}{" "}
              extracted for analysis.
            </p>
          ) : null}

          {analysis.dimensions.length > 0 ? (
            <div className="space-y-2">
              <h4 className="text-sm font-medium">Dimensions</h4>
              {analysis.dimensions.map((dimension) => (
                <div
                  key={dimension.id}
                  className="flex flex-wrap items-center gap-2 rounded-lg border bg-background px-3 py-2 font-mono text-xs"
                >
                  <span className="truncate">{dimension.id}</span>
                  <span className="text-muted-foreground">
                    Y {dimension.minY}..{dimension.minY + dimension.height - 1}
                  </span>
                  <span className="ml-auto text-muted-foreground">
                    {dimension.regionFiles} region(s) · {dimension.chunks} chunk(s)
                  </span>
                </div>
              ))}
            </div>
          ) : null}

          {analysis.blockers.length > 0 ? (
            <Alert variant="destructive">
              <TriangleAlert />
              <AlertTitle>Import blocked</AlertTitle>
              <AlertDescription>
                {analysis.blockers.map((blocker) => (
                  <p key={blocker}>{blocker}</p>
                ))}
              </AlertDescription>
            </Alert>
          ) : null}
          {analysis.warnings.length > 0 ? (
            <Alert>
              <TriangleAlert />
              <AlertTitle>Warnings</AlertTitle>
              <AlertDescription>
                <div className="scrollbar-thin max-h-24 space-y-1 overflow-y-auto">
                  {analysis.warnings.map((warning) => (
                    <p key={warning}>{warning}</p>
                  ))}
                </div>
              </AlertDescription>
            </Alert>
          ) : null}

          <Field>
            <FieldLabel>Import strategy</FieldLabel>
            <ToggleGroup
              variant="outline"
              value={[strategy]}
              disabled={disabled}
              className="grid w-full gap-2 sm:grid-cols-2"
              onValueChange={(value) => {
                const next = value[0];
                if (next === "importAsIs" || next === "convert") {
                  onStrategy(next);
                }
              }}
            >
              <ToggleGroupItem
                value="importAsIs"
                className="h-auto flex-col items-start gap-1 whitespace-normal p-4 text-left"
              >
                <span className="text-sm font-medium">Import as-is</span>
                <span className="text-xs font-normal text-muted-foreground">
                  Keep the detected version and edit decoded blocks over an immutable source copy.
                </span>
              </ToggleGroupItem>
              <ToggleGroupItem
                value="convert"
                disabled
                className="h-auto flex-col items-start gap-1 whitespace-normal p-4 text-left"
              >
                <span className="flex items-center gap-2 text-sm font-medium">
                  Convert
                  <Badge variant="outline">Unavailable</Badge>
                </span>
                <span className="text-xs font-normal text-muted-foreground">
                  No verified converter ships in this build. Import a supported world as-is.
                </span>
              </ToggleGroupItem>
            </ToggleGroup>
            <FieldDescription>
              Import as-is keeps the source version and never downgrades it. Conversion stays
              disabled until a verified converter and target profile are available.
            </FieldDescription>
          </Field>
        </div>
      ) : (
        <Card className="gap-0 py-0">
          <CardContent className="flex items-start gap-3 p-4 text-sm text-muted-foreground">
            <Sparkles className="mt-0.5 size-4 shrink-0 text-primary" />
            <p>
              Pick a world folder or .zip archive and analyze it. Import stays disabled until the
              version, layout and chunks pass verification.
            </p>
          </CardContent>
        </Card>
      )}
    </div>
  );
}

function FailurePanel({
  message,
  hint,
  retryDisabled,
  onRetry,
}: {
  message: string;
  hint: string | null;
  retryDisabled: boolean;
  onRetry: () => void;
}) {
  return (
    <div className="space-y-4">
      <Alert variant="destructive" role="alert">
        <TriangleAlert />
        <AlertTitle>Project could not be created</AlertTitle>
        <AlertDescription>
          <p>{message}</p>
          {hint ? <p className="mt-1">{hint}</p> : null}
        </AlertDescription>
      </Alert>
      <Button type="button" disabled={retryDisabled} onClick={onRetry}>
        <RefreshCw data-icon="inline-start" />
        Retry
      </Button>
    </div>
  );
}

const BROWSER_PROJECT_DEFAULTS: ProjectDefaults = {
  projectsRoot: "Documents/FuryBuilder",
  folderSuffix: ".furybuilder",
  flat: {
    preset: {
      baseY: 62,
      layers: [
        { block: "minecraft:stone", thickness: 1 },
        { block: "minecraft:dirt", thickness: 1 },
        { block: "minecraft:grass_block", thickness: 1 },
      ],
      biome: "minecraft:plains",
    },
    limits: { maxLayers: 8, maxLayerThickness: 16, maxTotalThickness: 64, footprint: 33 },
    blockChoices: [
      "minecraft:stone",
      "minecraft:dirt",
      "minecraft:grass_block",
      "minecraft:sand",
      "minecraft:oak_planks",
    ],
    biomeChoices: [
      "minecraft:plains",
      "minecraft:forest",
      "minecraft:desert",
      "minecraft:snowy_plains",
    ],
  },
};

/**
 * Full-screen project creation wizard. The shell owns the step order, navigation,
 * validation and progress; each step only describes its own fields. Steps that need
 * backend data are added as they become available, so the wizard never shows a dead
 * control.
 */
export function ProjectWizard() {
  const mode = useProjectStore((state) => state.managerCreateMode);
  const setMode = useProjectStore((state) => state.setManagerCreateMode);
  const setOpen = useProjectStore((state) => state.setManagerOpen);
  const phase = useProjectStore((state) => state.phase);
  const prepareMessage = useProjectStore((state) => state.prepareMessage);
  const failure = useProjectStore((state) => state.failure);
  const analysis = useProjectStore((state) => state.worldAnalysis);
  const analysisLoading = useProjectStore((state) => state.analysisLoading);
  const importGeneratedWorld = useProjectStore((state) => state.importGeneratedWorld);
  const desktop = isTauriRuntime();

  const [name, setName] = useState("");
  const [projectFolder, setProjectFolder] = useState("");
  const [defaults, setDefaults] = useState<ProjectDefaults | null>(null);
  const [importSource, setImportSource] = useState("");
  const [importSourceKind, setImportSourceKind] = useState<ImportSourceKind>("folder");
  const [importStrategy, setImportStrategy] = useState<ImportStrategy>("importAsIs");
  const [catalog, setCatalog] = useState<VersionCatalogReport | null>(null);
  const [catalogLoading, setCatalogLoading] = useState(false);
  const [catalogError, setCatalogError] = useState<ErrorPresentation | null>(null);
  const [selectedVersion, setSelectedVersion] = useState("");
  const [flatOverride, setFlatOverride] = useState<FlatPreset | null>(null);
  const [componentStep, setComponentStep] = useState<ComponentStepState | null>(null);
  const [requestedStep, setRequestedStep] = useState(0);
  const [furthestStep, setFurthestStep] = useState(0);
  const [attempted, setAttempted] = useState(false);
  const [submitted, setSubmitted] = useState(false);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const value = desktop ? await fetchProjectDefaults() : BROWSER_PROJECT_DEFAULTS;
        if (!cancelled) setDefaults(value);
      } catch (error) {
        if (!cancelled) {
          pushToast({
            kind: "warning",
            title: "Project folder unavailable",
            description: String(error),
          });
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [desktop]);

  const steps = useMemo(() => wizardSteps(mode), [mode]);
  const activeStep = Math.min(requestedStep, steps.length - 1);
  const step = steps[activeStep]!;
  const furthest = Math.min(furthestStep, steps.length - 1);
  const busy = phase === "preparing" || phase === "saving";
  const folderSuffix = defaults?.folderSuffix ?? BROWSER_PROJECT_DEFAULTS.folderSuffix;
  const projectsRoot = defaults?.projectsRoot ?? null;
  const destination = projectFolderName(projectFolder, folderSuffix) || undefined;
  const effectiveVersion = mode === "import" ? (analysis?.versionName ?? "") : selectedVersion;
  const selectedProfile =
    catalog?.versions.find((version) => version.id === effectiveVersion)?.profile ?? null;
  const componentTargetKey = selectedProfile ? `${selectedProfile.id}|${mode}` : null;
  const flatDefaults = defaults?.flat ?? null;
  const flatPreset = flatOverride ?? flatDefaults?.preset ?? null;
  const errorsForStep = (candidate: WizardStep): string[] =>
    stepErrors(
      candidate,
      name,
      mode,
      analysis,
      importSource,
      projectFolder,
      folderSuffix,
      {
        catalog,
        loading: catalogLoading,
        error: catalogError,
        selected: effectiveVersion,
        desktop,
      },
      {
        flat: flatPreset,
        flatDefaults,
        profile: selectedProfile,
        importStrategy,
      },
      componentStep,
      componentTargetKey,
    );
  const errors = errorsForStep(step);
  const progress = ((activeStep + 1) / steps.length) * 100;
  const stepDescription =
    step.id === "basics" && mode === "vanilla"
      ? "Choose the project folder and review what world generation needs."
      : step.id === "version" && mode === "import"
        ? "The source world decides its version; review the verified profile before continuing."
        : step.id === "source" && mode === "import"
          ? "Choose a world folder or .zip archive, analyze it read-only, then choose how to import."
          : step.id === "components" && mode === "vanilla"
            ? "Prepare Java and the official server JAR before generation. Assets stay shared with the editor."
            : step.description;

  const loadVersions = async (): Promise<void> => {
    if (!desktop) return;
    setCatalogLoading(true);
    setCatalogError(null);
    try {
      const report = await fetchVersionCatalog();
      setCatalog(report);
      setSelectedVersion((current) => current || report.recommended);
    } catch (error) {
      setCatalogError(presentError(error));
    } finally {
      setCatalogLoading(false);
    }
  };

  const ensureVersionCatalog = (index: number): void => {
    if (steps[index]?.id === "version" && desktop && !catalog && !catalogLoading) {
      void loadVersions();
    }
  };

  const submit = async (): Promise<void> => {
    const invalid = steps
      .map((candidate, index) => ({ index, errors: errorsForStep(candidate) }))
      .find((entry) => entry.errors.length > 0);
    if (invalid) {
      setRequestedStep(invalid.index);
      setAttempted(true);
      return;
    }
    setSubmitted(true);
    if (mode === "import") {
      await useProjectStore.getState().importWorld(name, importSource, importStrategy, destination);
      return;
    }
    const options: CreateProjectOptions = {};
    if (selectedProfile) options.profileId = selectedProfile.id;
    if (mode === "flat" && flatPreset) options.flat = flatPreset;
    await useProjectStore.getState().create(name, mode, destination, options);
  };

  const retry = (): void => {
    void submit();
  };

  const goTo = (index: number): void => {
    if (index < 0 || index >= steps.length || index > furthest) return;
    setRequestedStep(index);
    setAttempted(false);
    ensureVersionCatalog(index);
  };

  const next = (): void => {
    if (errors.length > 0) {
      setAttempted(true);
      return;
    }
    const target = Math.min(activeStep + 1, steps.length - 1);
    setRequestedStep(target);
    setFurthestStep((value) => Math.max(value, target));
    setAttempted(false);
    ensureVersionCatalog(target);
  };

  const chooseMode = (nextMode: CreationMode): void => {
    setMode(nextMode);
    setRequestedStep(0);
    setFurthestStep(0);
    setAttempted(false);
    setComponentStep(null);
  };

  const changeImportSource = (value: string): void => {
    setImportSource(value);
    setImportStrategy("importAsIs");
    setComponentStep(null);
    useProjectStore.getState().clearWorldAnalysis();
  };

  const changeImportSourceKind = (value: ImportSourceKind): void => {
    setImportSourceKind(value);
    setImportSource("");
    setImportStrategy("importAsIs");
    setComponentStep(null);
    useProjectStore.getState().clearWorldAnalysis();
  };

  const browseImportSource = async (): Promise<void> => {
    try {
      const { open: openSource } = await import("@tauri-apps/plugin-dialog");
      const selected = await openSource({
        directory: importSourceKind === "folder",
        multiple: false,
        title:
          importSourceKind === "archive"
            ? "Select a Java world archive"
            : "Select a Java world folder",
        ...(importSourceKind === "archive"
          ? { filters: [{ name: "World archive", extensions: ["zip"] }] }
          : {}),
      });
      if (typeof selected === "string" && selected.length > 0) {
        changeImportSource(selected);
      }
    } catch (pickerError) {
      pushToast({
        kind: "error",
        title: "Source picker unavailable",
        description: String(pickerError),
      });
    }
  };

  const cancel = (): void => setOpen(false);

  return (
    <section
      aria-labelledby="project-wizard-title"
      className="fixed inset-0 z-50 flex flex-col bg-background text-foreground"
    >
      <header className="flex flex-wrap items-center gap-3 border-b px-4 py-3 sm:px-6">
        <img src={logoUrl} alt="" className="size-8 rounded-md" />
        <div className="min-w-0">
          <h1 id="project-wizard-title" className="truncate text-base font-semibold">
            New project
          </h1>
          <p className="truncate text-xs text-muted-foreground">{stepDescription}</p>
        </div>
        <Badge variant="outline" className="ml-auto tabular-nums">
          Step {activeStep + 1} of {steps.length}
        </Badge>
        <Button type="button" size="sm" variant="ghost" disabled={busy} onClick={cancel}>
          <X data-icon="inline-start" />
          Cancel
        </Button>
      </header>

      <div className="border-b px-4 py-3 sm:px-6">
        <StepNav steps={steps} active={activeStep} furthest={furthest} onSelect={goTo} />
      </div>

      <main className="min-h-0 flex-1 overflow-y-auto">
        <div className="mx-auto w-full max-w-3xl px-4 py-6 sm:px-6">
          {phase === "preparing" ? (
            <Card className="gap-0 py-0">
              <CardContent className="flex flex-col items-center gap-3 p-10 text-center">
                <Loader2 className="size-6 animate-spin text-primary" />
                <p className="text-sm font-medium">{prepareMessage ?? "Preparing the project…"}</p>
                <p className="max-w-md text-sm leading-relaxed text-muted-foreground">
                  The world, component checks and project metadata are prepared before the editor
                  mounts. Keep this window open.
                </p>
              </CardContent>
            </Card>
          ) : phase === "error" && failure && submitted ? (
            <FailurePanel
              message={failure.message}
              hint={failure.hint}
              retryDisabled={busy}
              onRetry={retry}
            />
          ) : (
            <div className="space-y-6">
              <div>
                <h2 className="text-lg font-semibold">{step.title}</h2>
                <p className="mt-1 text-sm leading-relaxed text-muted-foreground">
                  {stepDescription}
                </p>
              </div>

              {step.id === "basics" ? (
                <BasicsStep
                  name={name}
                  mode={mode}
                  folder={projectFolder}
                  folderSuffix={folderSuffix}
                  projectsRoot={projectsRoot}
                  disabled={busy}
                  onName={setName}
                  onFolder={setProjectFolder}
                  onMode={chooseMode}
                />
              ) : null}

              {step.id === "version" ? (
                <ProjectVersionStep
                  catalog={catalog}
                  loading={catalogLoading}
                  error={catalogError}
                  desktop={desktop}
                  selected={effectiveVersion}
                  locked={mode === "import"}
                  onSelect={(value) => {
                    setSelectedVersion(value);
                    setComponentStep(null);
                  }}
                  onRetry={() => void loadVersions()}
                />
              ) : null}

              {step.id === "source" ? (
                mode === "import" ? (
                  <ImportSourceStep
                    sourceKind={importSourceKind}
                    source={importSource}
                    analysis={analysis}
                    analysisLoading={analysisLoading}
                    strategy={importStrategy}
                    disabled={busy}
                    onSourceKind={changeImportSourceKind}
                    onSource={changeImportSource}
                    onBrowse={() => void browseImportSource()}
                    onAnalyze={() => void useProjectStore.getState().analyzeWorld(importSource)}
                    onStrategy={setImportStrategy}
                  />
                ) : mode === "vanilla" ? (
                  <VanillaWorldWizard
                    version={effectiveVersion}
                    onOpenWorld={(runId, projectName) =>
                      importGeneratedWorld(projectName, runId, destination)
                    }
                  />
                ) : mode === "flat" ? (
                  <FlatSourceStep
                    version={effectiveVersion}
                    profile={selectedProfile}
                    preset={flatPreset}
                    defaults={flatDefaults}
                    disabled={busy}
                    onChange={setFlatOverride}
                  />
                ) : (
                  <EmptySourceStep version={effectiveVersion} profile={selectedProfile} />
                )
              ) : null}

              {step.id === "components" ? (
                <ProjectComponentsStep
                  key={componentTargetKey ?? "no-target"}
                  targetKey={componentTargetKey ?? "no-target"}
                  mode={mode}
                  profile={selectedProfile}
                  desktop={desktop}
                  disabled={busy}
                  onStateChange={setComponentStep}
                />
              ) : null}

              {step.id === "review" ? (
                <ProjectReviewStep
                  name={name}
                  mode={mode}
                  version={effectiveVersion}
                  destination={projectFolderName(projectFolder, folderSuffix)}
                  projectsRoot={projectsRoot}
                  analysis={analysis}
                  flat={flatPreset}
                  flatFootprint={flatDefaults?.limits.footprint ?? 33}
                  importStrategy={importStrategy}
                  profile={selectedProfile}
                  components={componentStep}
                />
              ) : null}

              {attempted && errors.length > 0 ? (
                <Alert variant="destructive" role="alert">
                  <TriangleAlert />
                  <AlertTitle>Check this step</AlertTitle>
                  <AlertDescription>
                    {errors.map((error) => (
                      <p key={error}>{error}</p>
                    ))}
                  </AlertDescription>
                </Alert>
              ) : null}

              {step.id === "source" && mode === "import" && !desktop ? (
                <Alert>
                  <TriangleAlert />
                  <AlertTitle>Desktop core required</AlertTitle>
                  <AlertDescription>
                    World analysis and import need the desktop app.
                  </AlertDescription>
                </Alert>
              ) : null}

              {step.id === "source" && mode === "vanilla" ? (
                <p className="text-xs leading-relaxed text-muted-foreground">
                  Generation uses the Java runtime and official server prepared in Components. It
                  accepts the EULA explicitly and imports the finished world as a read-only source.
                  The seed names the project; the folder chosen in Basics is kept.
                </p>
              ) : null}
            </div>
          )}
        </div>
      </main>

      <Separator />
      <footer className="border-t px-4 py-3 sm:px-6">
        <div className="mx-auto flex w-full max-w-3xl flex-wrap items-center gap-3">
          <div className="min-w-40 flex-1">
            <div className="mb-1.5 flex items-center justify-between text-xs text-muted-foreground">
              <span>
                Step {activeStep + 1} of {steps.length}
              </span>
              <span className="tabular-nums">{Math.round(progress)}%</span>
            </div>
            <Progress value={progress} aria-label="Project creation progress" />
          </div>

          <div className="flex items-center gap-2">
            <Button
              type="button"
              size="sm"
              variant="outline"
              disabled={activeStep === 0 || busy}
              onClick={() => goTo(activeStep - 1)}
            >
              <ArrowLeft data-icon="inline-start" />
              Back
            </Button>
            {activeStep < steps.length - 1 ? (
              <Button type="button" size="sm" disabled={busy} onClick={next}>
                Next
                <ArrowRight data-icon="inline-end" />
              </Button>
            ) : mode !== "vanilla" ? (
              <Button
                type="button"
                size="sm"
                disabled={busy || analysisLoading}
                onClick={() => void submit()}
              >
                <Sparkles data-icon="inline-start" />
                Create project
              </Button>
            ) : null}
          </div>
        </div>
      </footer>
    </section>
  );
}
