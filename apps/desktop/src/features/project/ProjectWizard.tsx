import { useEffect, useMemo, useState } from "react";
import {
  ArrowLeft,
  ArrowRight,
  Check,
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
import { VanillaWorldWizard } from "@/features/project/VanillaWorldWizard";
import { isTauriRuntime } from "@/lib/ipc";
import { fetchProjectDefaults } from "@/lib/project-api";
import {
  CREATION_MODES,
  creationModeLabel,
  type CreationMode,
  type JavaWorldAnalysis,
  type ProjectDefaults,
} from "@/project/types";
import { useProjectStore } from "@/state/project-store";
import { pushToast } from "@/state/toast-store";

interface WizardStep {
  id: "basics" | "source" | "review";
  title: string;
  description: string;
}

const BASICS_STEP: WizardStep = {
  id: "basics",
  title: "Basics",
  description: "Name the project, choose its folder and pick a starting world.",
};

const SOURCE_STEP: WizardStep = {
  id: "source",
  title: "Source",
  description: "Describe or inspect the world data that the project starts with.",
};

const REVIEW_STEP: WizardStep = {
  id: "review",
  title: "Review",
  description: "Check the summary before the project is created.",
};

function wizardSteps(mode: CreationMode): WizardStep[] {
  return mode === "vanilla" ? [BASICS_STEP, SOURCE_STEP] : [BASICS_STEP, SOURCE_STEP, REVIEW_STEP];
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

function stepErrors(
  step: WizardStep,
  name: string,
  mode: CreationMode,
  analysis: JavaWorldAnalysis | null,
  folder: string,
  projectFolder: string,
  folderSuffix: string,
): string[] {
  if (step.id === "basics") {
    const errors: string[] = [];
    if (mode !== "vanilla" && name.trim().length === 0) errors.push("Enter a project name.");
    const invalidFolder = folderError(projectFolder, folderSuffix);
    if (invalidFolder) errors.push(invalidFolder);
    return errors;
  }
  if (step.id === "source" && mode === "import") {
    const errors: string[] = [];
    if (folder.trim().length === 0) errors.push("Choose a Java world folder.");
    if (!analysis) errors.push("Analyze the world before continuing.");
    if (analysis && !analysis.supported)
      errors.push("This world has blockers and cannot be imported.");
    return errors;
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
  folder,
  analysis,
  analysisLoading,
  disabled,
  onFolder,
  onBrowse,
  onAnalyze,
}: {
  folder: string;
  analysis: JavaWorldAnalysis | null;
  analysisLoading: boolean;
  disabled: boolean;
  onFolder: (value: string) => void;
  onBrowse: () => void;
  onAnalyze: () => void;
}) {
  return (
    <div className="space-y-4">
      <Field>
        <FieldLabel htmlFor="wizard-world-folder">Java world folder</FieldLabel>
        <div className="flex flex-wrap items-center gap-2">
          <Input
            id="wizard-world-folder"
            className="min-w-56 flex-1 font-mono text-xs"
            value={folder}
            placeholder="…/.minecraft/saves/My World"
            disabled={disabled}
            onChange={(event) => onFolder(event.target.value)}
          />
          <Button type="button" size="sm" variant="outline" disabled={disabled} onClick={onBrowse}>
            <FolderOpen data-icon="inline-start" />
            Browse
          </Button>
          <Button
            type="button"
            size="sm"
            variant="outline"
            disabled={disabled || analysisLoading || folder.trim().length === 0}
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
          Read-only analysis validates level.dat, the layout and every chunk before import. The
          source folder is never written.
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
            <span className="ml-auto font-mono text-xs text-muted-foreground">
              {analysis.regions} region(s) · {analysis.chunks} chunk(s)
            </span>
          </div>
          {analysis.levelName ? (
            <p className="text-sm text-muted-foreground">
              Level: <span className="font-medium text-foreground">{analysis.levelName}</span>
            </p>
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
        </div>
      ) : (
        <Card className="gap-0 py-0">
          <CardContent className="flex items-start gap-3 p-4 text-sm text-muted-foreground">
            <Sparkles className="mt-0.5 size-4 shrink-0 text-primary" />
            <p>
              Pick a world folder and analyze it. Import stays disabled until the version, layout
              and chunks pass verification.
            </p>
          </CardContent>
        </Card>
      )}
    </div>
  );
}

function SimpleSourceStep({ mode }: { mode: "empty" | "flat" }) {
  return (
    <Card className="gap-0 py-0">
      <CardContent className="space-y-2 p-5">
        <p className="text-sm font-medium">{creationModeLabel(mode)}</p>
        <p className="text-sm leading-relaxed text-muted-foreground">
          {mode === "empty"
            ? "A blank world is created with only air. Every block is placed by you or a tool."
            : "A flat platform around the origin is created. This is a FuryBuilder starting shape, not vanilla world generation."}
        </p>
        <p className="text-xs leading-relaxed text-muted-foreground">
          The supported Minecraft version and project format are selected when the project is
          created.
        </p>
      </CardContent>
    </Card>
  );
}

function ReviewStep({
  name,
  mode,
  folder,
  projectFolder,
  folderSuffix,
  projectsRoot,
  analysis,
}: {
  name: string;
  mode: CreationMode;
  folder: string;
  projectFolder: string;
  folderSuffix: string;
  projectsRoot: string | null;
  analysis: JavaWorldAnalysis | null;
}) {
  const destination = projectFolderName(projectFolder, folderSuffix);
  return (
    <div className="space-y-4">
      <Card className="gap-0 py-0">
        <CardContent className="p-0">
          <dl className="divide-y">
            <div className="flex flex-wrap items-center gap-3 px-4 py-3">
              <dt className="w-28 text-sm text-muted-foreground">Name</dt>
              <dd className="min-w-0 flex-1 truncate text-sm font-medium">
                {name.trim() || "Named by the generated world"}
              </dd>
            </div>
            <div className="flex flex-wrap items-center gap-3 px-4 py-3">
              <dt className="w-28 text-sm text-muted-foreground">Mode</dt>
              <dd className="text-sm">{creationModeLabel(mode)}</dd>
            </div>
            <div className="flex flex-wrap items-center gap-3 px-4 py-3">
              <dt className="w-28 text-sm text-muted-foreground">Folder</dt>
              <dd className="min-w-0 flex-1 truncate font-mono text-xs">
                {destination || "Named after the project"}
              </dd>
            </div>
            <div className="flex flex-wrap items-center gap-3 px-4 py-3">
              <dt className="w-28 text-sm text-muted-foreground">Location</dt>
              <dd className="min-w-0 flex-1 break-all font-mono text-xs">
                {projectsRoot ?? "Default library folder"}
              </dd>
            </div>
            <div className="flex flex-wrap items-center gap-3 px-4 py-3">
              <dt className="w-28 text-sm text-muted-foreground">Source</dt>
              <dd className="min-w-0 flex-1 truncate font-mono text-xs">
                {mode === "import"
                  ? folder
                  : mode === "flat"
                    ? "Flat starting platform"
                    : "Empty world"}
              </dd>
            </div>
            {analysis ? (
              <div className="flex flex-wrap items-center gap-3 px-4 py-3">
                <dt className="w-28 text-sm text-muted-foreground">Detected</dt>
                <dd className="font-mono text-xs">
                  {analysis.versionName ?? "unknown version"} · DataVersion{" "}
                  {analysis.dataVersion ?? "unknown"} · {analysis.chunks} chunk(s)
                </dd>
              </div>
            ) : null}
          </dl>
        </CardContent>
      </Card>
      <p className="text-sm leading-relaxed text-muted-foreground">
        {mode === "import"
          ? "The source folder is copied byte for byte and stays read-only. Edits live in the project overlay."
          : "The project is saved with the current project schema and the supported target profile."}
      </p>
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
  const [worldFolder, setWorldFolder] = useState("");
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
  const errors = stepErrors(step, name, mode, analysis, worldFolder, projectFolder, folderSuffix);
  const progress = ((activeStep + 1) / steps.length) * 100;
  const stepDescription =
    step.id === "basics" && mode === "vanilla"
      ? "Choose the project folder and review what world generation needs."
      : step.description;

  const submit = async (): Promise<void> => {
    setSubmitted(true);
    if (mode === "import") {
      await useProjectStore.getState().importWorld(name, worldFolder, destination);
      return;
    }
    await useProjectStore.getState().create(name, mode, destination);
  };

  const retry = (): void => {
    void submit();
  };

  const goTo = (index: number): void => {
    if (index < 0 || index >= steps.length || index > furthest) return;
    setRequestedStep(index);
    setAttempted(false);
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
  };

  const chooseMode = (nextMode: CreationMode): void => {
    setMode(nextMode);
    setRequestedStep(0);
    setFurthestStep(0);
    setAttempted(false);
  };

  const changeFolder = (value: string): void => {
    setWorldFolder(value);
    useProjectStore.getState().clearWorldAnalysis();
  };

  const browseWorldFolder = async (): Promise<void> => {
    try {
      const { open: openFolder } = await import("@tauri-apps/plugin-dialog");
      const selected = await openFolder({
        directory: true,
        multiple: false,
        title: "Select a Java world folder",
      });
      if (typeof selected === "string" && selected.length > 0) {
        changeFolder(selected);
      }
    } catch (pickerError) {
      pushToast({
        kind: "error",
        title: "Folder picker unavailable",
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

              {step.id === "source" ? (
                mode === "import" ? (
                  <ImportSourceStep
                    folder={worldFolder}
                    analysis={analysis}
                    analysisLoading={analysisLoading}
                    disabled={busy}
                    onFolder={changeFolder}
                    onBrowse={() => void browseWorldFolder()}
                    onAnalyze={() => void useProjectStore.getState().analyzeWorld(worldFolder)}
                  />
                ) : mode === "vanilla" ? (
                  <VanillaWorldWizard
                    onOpenWorld={(runId, projectName) =>
                      importGeneratedWorld(projectName, runId, destination)
                    }
                  />
                ) : (
                  <SimpleSourceStep mode={mode} />
                )
              ) : null}

              {step.id === "review" ? (
                <ReviewStep
                  name={name}
                  mode={mode}
                  folder={worldFolder}
                  projectFolder={projectFolder}
                  folderSuffix={folderSuffix}
                  projectsRoot={projectsRoot}
                  analysis={analysis}
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
                  Generation prepares Java and the official server, accepts the EULA explicitly and
                  imports the finished world as a read-only source. The seed names the project; the
                  folder chosen in Basics is kept.
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
