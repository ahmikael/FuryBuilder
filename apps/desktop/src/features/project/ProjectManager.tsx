import { useEffect, useMemo, useState } from "react";
import {
  AlertTriangle,
  Archive,
  CheckCircle2,
  ChevronDown,
  Clock,
  FolderOpen,
  HardDrive,
  History,
  Loader2,
  Pencil,
  Plus,
  RefreshCw,
  RotateCcw,
  Save,
  ArrowLeftRight,
  ShieldAlert,
  Sparkles,
  Trash2,
  X,
} from "lucide-react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from "@/components/ui/empty";
import { Field, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Separator } from "@/components/ui/separator";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { isTauriRuntime } from "@/lib/ipc";
import { StructureExchange } from "@/features/project/StructureExchange";
import { ProjectResourcePacks } from "@/features/project/ProjectResourcePacks";
import { VanillaParityPanel } from "@/features/project/VanillaParityPanel";
import { VanillaWorldWizard } from "@/features/project/VanillaWorldWizard";
import { ProjectWizard } from "@/features/project/ProjectWizard";
import { isManagerTab, projectIsDirty, useProjectStore } from "@/state/project-store";
import { pushToast } from "@/state/toast-store";
import { useWorldStore } from "@/state/world-store";
import {
  CREATION_MODES,
  creationModeLabel,
  formatBytes,
  formatRelativeTime,
  importVersionForSource,
  projectSummaryWarnings,
} from "@/project/types";
import { cn } from "@/lib/utils";

type PendingAction =
  | { kind: "open"; folder: string; name: string }
  | { kind: "close" }
  | { kind: "create" }
  | null;

function ProjectRow({
  name,
  meta,
  active,
  recoverable,
  warnings,
  onOpen,
  onRename,
  onCheckpoints,
}: {
  name: string;
  meta: string;
  active: boolean;
  recoverable: boolean;
  warnings: string[];
  onOpen: () => void;
  onRename: () => void;
  onCheckpoints: () => void;
}) {
  return (
    <div
      data-testid={`project-row-${name}`}
      className={cn(
        "rounded-xl border p-3 transition-colors",
        active ? "border-primary/50 bg-primary/5" : "bg-card hover:bg-muted/40",
      )}
    >
      <div className="flex items-start gap-3">
        <div className="mt-0.5 grid size-9 shrink-0 place-items-center rounded-lg border bg-background">
          <Archive className="size-4 text-muted-foreground" />
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <span className="truncate text-sm font-medium">{name}</span>
            {active ? <Badge>Open</Badge> : null}
            {recoverable ? <Badge variant="secondary">Recoverable</Badge> : null}
          </div>
          <div className="mt-1 truncate font-mono text-xs text-muted-foreground">{meta}</div>
          {warnings.length > 0 ? (
            <div className="mt-1.5 text-sm leading-relaxed text-status-warn">
              {warnings.join(" · ")}
            </div>
          ) : null}
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <Button size="sm" onClick={onOpen}>
            <FolderOpen data-icon="inline-start" />
            Open
          </Button>
          <Button size="icon-sm" variant="ghost" aria-label={`Rename ${name}`} onClick={onRename}>
            <Pencil />
          </Button>
          <Button
            size="icon-sm"
            variant="ghost"
            aria-label={`Checkpoints for ${name}`}
            onClick={onCheckpoints}
          >
            <History />
          </Button>
        </div>
      </div>
    </div>
  );
}

export function ProjectManager() {
  const open = useProjectStore((state) => state.managerOpen);
  const setOpen = useProjectStore((state) => state.setManagerOpen);
  const projects = useProjectStore((state) => state.projects);
  const warnings = useProjectStore((state) => state.warnings);
  const current = useProjectStore((state) => state.current);
  const recovery = useProjectStore((state) => state.recovery);
  const checkpoints = useProjectStore((state) => state.checkpoints);
  const loading = useProjectStore((state) => state.loading);
  const error = useProjectStore((state) => state.error);
  const lastSave = useProjectStore((state) => state.lastSave);
  const worldAnalysis = useProjectStore((state) => state.worldAnalysis);
  const importGeneratedWorld = useProjectStore((state) => state.importGeneratedWorld);
  const analysisLoading = useProjectStore((state) => state.analysisLoading);
  const worldRevision = useWorldStore((state) => state.revision);
  const tab = useProjectStore((state) => state.managerTab);
  const setTab = useProjectStore((state) => state.setManagerTab);
  const mode = useProjectStore((state) => state.managerCreateMode);
  const setMode = useProjectStore((state) => state.setManagerCreateMode);
  const [name, setName] = useState("");
  const [worldFolder, setWorldFolder] = useState("");
  const [renameFolder, setRenameFolder] = useState<string | null>(null);
  const [renameValue, setRenameValue] = useState("");
  const [pending, setPending] = useState<PendingAction>(null);
  const [checkpointReason, setCheckpointReason] = useState("manual checkpoint");
  const dirty = projectIsDirty(current, worldRevision);
  const confirmedImportVersion = importVersionForSource(worldAnalysis, worldFolder);
  const importReady = mode !== "import" || confirmedImportVersion !== null;
  const activeFolder = current?.folder ?? null;
  const desktop = isTauriRuntime();

  useEffect(() => {
    if (!open || !desktop) return;
    void useProjectStore.getState().refreshList();
    void useProjectStore.getState().refreshSession();
    if (current?.folder) void useProjectStore.getState().loadCheckpoints(current.folder);
  }, [open, current?.folder, desktop]);

  const currentProject = useMemo(
    () => projects.find((project) => project.folder === activeFolder) ?? null,
    [projects, activeFolder],
  );
  const targetVersion = currentProject?.targetMinecraftVersion;
  const targetDataVersion = currentProject?.targetDataVersion;

  // Without an open project, the welcome screen owns opening and the full-screen wizard
  // owns creation. The project dialog must never cover the empty shell.
  if (!current?.folder) {
    return open && tab === "create" ? <ProjectWizard /> : null;
  }

  const requestOpen = (folder: string, projectName: string): void => {
    if (folder === activeFolder) {
      setOpen(false);
      return;
    }
    if (dirty) {
      setPending({ kind: "open", folder, name: projectName });
      return;
    }
    void useProjectStore.getState().open(folder);
  };

  const confirmPending = async (strategy: "save" | "discard"): Promise<void> => {
    const action = pending;
    if (!action) return;
    if (strategy === "save") {
      const saved = await useProjectStore.getState().save();
      if (!saved) return;
    }
    if (action.kind === "open") {
      await useProjectStore.getState().open(action.folder, { discardCurrent: true });
    } else if (action.kind === "close") {
      if (!(await useProjectStore.getState().close(false))) return;
      setOpen(false);
      setTab("projects");
    } else if (action.kind === "create") {
      if (!(await useProjectStore.getState().close(false))) return;
      setTab("create");
    }
    setPending(null);
  };

  const closeProject = async (): Promise<void> => {
    if (await useProjectStore.getState().close(false)) setOpen(false);
  };

  const submitImport = async (): Promise<void> => {
    if (!confirmedImportVersion) return;
    const imported = await useProjectStore
      .getState()
      .importWorld(name, worldFolder, "importAsIs", confirmedImportVersion);
    if (imported) {
      setName("");
      setWorldFolder("");
      setTab("projects");
    }
  };

  const submitCreate = async (): Promise<void> => {
    if (dirty) {
      setPending({ kind: "create" });
      return;
    }
    if (mode === "import") {
      await submitImport();
      return;
    }
    const created = await useProjectStore.getState().create(name, mode);
    if (created) {
      setName("");
      setTab("projects");
    }
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
        setWorldFolder(selected);
        useProjectStore.getState().clearWorldAnalysis();
      }
    } catch (pickerError) {
      pushToast({
        kind: "error",
        title: "Folder picker unavailable",
        description: String(pickerError),
      });
    }
  };

  const submitRename = async (): Promise<void> => {
    if (!renameFolder) return;
    const renamed = await useProjectStore.getState().rename(renameFolder, renameValue);
    if (renamed) {
      setRenameFolder(null);
      setRenameValue("");
    }
  };

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent className="max-h-[90vh] max-w-5xl gap-6 overflow-y-auto">
        <DialogHeader>
          <DialogTitle>Projects</DialogTitle>
          <DialogDescription>
            Projects are stored locally. Snapshots are verified before use.
          </DialogDescription>
        </DialogHeader>

        {!desktop ? (
          <Alert>
            <ShieldAlert />
            <AlertTitle>Desktop core required</AlertTitle>
            <AlertDescription>
              Browser preview can show the demo scene. Create, open and save projects in the desktop
              app.
            </AlertDescription>
          </Alert>
        ) : (
          <>
            {error ? (
              <Alert variant="destructive">
                <AlertTriangle />
                <AlertTitle>Project error</AlertTitle>
                <AlertDescription>{error}</AlertDescription>
              </Alert>
            ) : null}

            {recovery ? (
              <Alert>
                <ShieldAlert />
                <AlertTitle>Recovered unsaved work</AlertTitle>
                <AlertDescription>
                  <p>
                    {recovery.reason} Revision {recovery.recoveredRevision} was loaded from a
                    verified autosave. Save the project to keep it.
                  </p>
                  <div className="mt-3 flex flex-wrap gap-2">
                    <Button
                      size="sm"
                      variant="outline"
                      onClick={() => useProjectStore.getState().dismissRecovery()}
                    >
                      <CheckCircle2 data-icon="inline-start" />
                      Keep recovered data
                    </Button>
                    <Button
                      size="sm"
                      variant="ghost"
                      onClick={() => void useProjectStore.getState().discardRecovery()}
                    >
                      <Trash2 data-icon="inline-start" />
                      Discard and reload saved
                    </Button>
                  </div>
                </AlertDescription>
              </Alert>
            ) : null}

            {pending ? (
              <Alert>
                <AlertTriangle />
                <AlertTitle>Unsaved changes in “{current?.name ?? "project"}”</AlertTitle>
                <AlertDescription>
                  <p>
                    {pending.kind === "open"
                      ? `Save before opening “${pending.name}”, or discard the current edits.`
                      : "Save before leaving this project, or discard the current edits."}
                  </p>
                  <div className="mt-3 flex flex-wrap gap-2">
                    <Button
                      size="sm"
                      disabled={loading}
                      onClick={() => void confirmPending("save")}
                    >
                      <Save data-icon="inline-start" />
                      Save and continue
                    </Button>
                    <Button
                      size="sm"
                      variant="outline"
                      disabled={loading}
                      onClick={() => void confirmPending("discard")}
                    >
                      Discard changes
                    </Button>
                    <Button size="sm" variant="ghost" onClick={() => setPending(null)}>
                      Cancel
                    </Button>
                  </div>
                </AlertDescription>
              </Alert>
            ) : null}

            <Tabs
              value={tab}
              onValueChange={(value) => {
                if (isManagerTab(value)) setTab(value);
              }}
              className="min-h-0"
            >
              <TabsList className="grid w-full grid-cols-5">
                <TabsTrigger value="projects">
                  <FolderOpen data-icon="inline-start" />
                  Projects ({projects.length})
                </TabsTrigger>
                <TabsTrigger value="create">
                  <Plus data-icon="inline-start" />
                  New project
                </TabsTrigger>
                <TabsTrigger value="checkpoints">
                  <History data-icon="inline-start" />
                  Checkpoints
                </TabsTrigger>
                <TabsTrigger value="exchange">
                  <ArrowLeftRight data-icon="inline-start" />
                  Import / Export
                </TabsTrigger>
                <TabsTrigger value="settings">Project Settings</TabsTrigger>
              </TabsList>

              <TabsContent value="projects" className="m-0 space-y-4 pt-4">
                <div className="flex flex-wrap items-center gap-3 rounded-xl border bg-card p-3">
                  <HardDrive className="size-4 text-primary" />
                  <span className="text-sm font-medium">{current.name}</span>
                  <span className="font-mono text-xs text-muted-foreground">
                    rev {current.savedRevision}
                    {current.autosavedRevision !== null
                      ? ` · autosave r${current.autosavedRevision}`
                      : ""}
                  </span>
                  <Badge variant={dirty ? "secondary" : "default"}>
                    {dirty ? "Unsaved" : "Saved"}
                  </Badge>
                  <span className="ml-auto flex items-center gap-2">
                    <Button
                      size="sm"
                      variant="outline"
                      disabled={loading || !dirty}
                      onClick={() => void useProjectStore.getState().save()}
                    >
                      {loading ? (
                        <Loader2 data-icon="inline-start" className="animate-spin" />
                      ) : (
                        <Save data-icon="inline-start" />
                      )}
                      Save
                    </Button>
                    <Button
                      size="sm"
                      variant="ghost"
                      aria-label="Close project"
                      onClick={() => (dirty ? setPending({ kind: "close" }) : void closeProject())}
                    >
                      <X data-icon="inline-start" />
                      Close
                    </Button>
                  </span>
                </div>

                {current?.folder && currentProject?.creationMode === "vanilla" ? (
                  <VanillaParityPanel folder={current.folder} />
                ) : null}

                {lastSave ? (
                  <Alert>
                    <CheckCircle2 />
                    <AlertTitle>Snapshot saved</AlertTitle>
                    <AlertDescription className="font-mono text-xs">
                      {lastSave.snapshotFile} · {formatBytes(lastSave.bytesWritten)} · checksum{" "}
                      {lastSave.checksum.slice(0, 16)}… · {lastSave.project.summary.lastSaveKind}
                      {lastSave.backupCreated ? " · manifest backup" : ""}
                      {lastSave.prunedSnapshots > 0
                        ? ` · pruned ${lastSave.prunedSnapshots} snapshot(s)`
                        : ""}
                    </AlertDescription>
                  </Alert>
                ) : null}

                <div className="scrollbar-thin max-h-[42vh] space-y-2 overflow-y-auto pr-1">
                  {projects.length === 0 ? (
                    <Empty className="border p-8">
                      <EmptyHeader>
                        <EmptyMedia variant="icon">
                          <Archive />
                        </EmptyMedia>
                        <EmptyTitle>No projects yet</EmptyTitle>
                        <EmptyDescription>
                          Create a project to start editing blocks.
                        </EmptyDescription>
                      </EmptyHeader>
                      <Button size="sm" onClick={() => setTab("create")}>
                        <Plus data-icon="inline-start" />
                        Create an Empty Project
                      </Button>
                    </Empty>
                  ) : (
                    projects.map((project) => (
                      <ProjectRow
                        key={project.folder}
                        name={project.name}
                        active={project.folder === activeFolder}
                        recoverable={project.recoverable}
                        warnings={projectSummaryWarnings(project)}
                        meta={`${creationModeLabel(project.creationMode)} · ${project.targetMinecraftVersion} · DV ${project.targetDataVersion} · rev ${project.worldRevision} · ${formatBytes(project.bytes)} · saved ${formatRelativeTime(project.lastSavedAtMs)}`}
                        onOpen={() => requestOpen(project.folder, project.name)}
                        onRename={() => {
                          setRenameFolder(project.folder);
                          setRenameValue(project.name);
                        }}
                        onCheckpoints={() => {
                          void useProjectStore.getState().loadCheckpoints(project.folder);
                          setTab("checkpoints");
                        }}
                      />
                    ))
                  )}
                </div>

                {warnings.length > 0 ? (
                  <Alert>
                    <AlertTriangle />
                    <AlertTitle>Warnings</AlertTitle>
                    <AlertDescription>
                      <ul className="list-disc space-y-1 pl-4">
                        {warnings.map((warning) => (
                          <li key={warning}>{warning}</li>
                        ))}
                      </ul>
                    </AlertDescription>
                  </Alert>
                ) : null}

                {renameFolder ? (
                  <div className="flex flex-wrap items-center gap-2 rounded-xl border bg-card p-3">
                    <Input
                      className="min-w-48 flex-1"
                      value={renameValue}
                      aria-label="New project name"
                      onChange={(event) => setRenameValue(event.target.value)}
                    />
                    <Button
                      size="sm"
                      disabled={loading || renameValue.trim().length === 0}
                      onClick={() => void submitRename()}
                    >
                      Rename
                    </Button>
                    <Button size="sm" variant="ghost" onClick={() => setRenameFolder(null)}>
                      Cancel
                    </Button>
                  </div>
                ) : null}
              </TabsContent>

              <TabsContent value="create" className="m-0 space-y-4 pt-4">
                {mode !== "vanilla" ? (
                  <Field>
                    <FieldLabel htmlFor="project-name">Project name</FieldLabel>
                    <Input
                      id="project-name"
                      value={name}
                      placeholder="My Castle"
                      maxLength={64}
                      onChange={(event) => setName(event.target.value)}
                    />
                  </Field>
                ) : null}

                <ToggleGroup
                  value={[mode]}
                  onValueChange={(value) => {
                    const next = value[0];
                    if (
                      next === "empty" ||
                      next === "flat" ||
                      next === "import" ||
                      next === "vanilla"
                    ) {
                      setMode(next);
                    }
                  }}
                  spacing={2}
                  className="grid w-full grid-cols-2"
                >
                  {CREATION_MODES.map((entry) => (
                    <ToggleGroupItem
                      key={entry.mode}
                      value={entry.mode}
                      disabled={!entry.available || loading}
                      className="h-auto flex-col items-start gap-1.5 p-4 text-left"
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

                {mode === "vanilla" ? (
                  <VanillaWorldWizard
                    onOpenWorld={(runId, projectName) => importGeneratedWorld(projectName, runId)}
                  />
                ) : null}

                {mode === "import" ? (
                  <Card className="gap-0 py-0">
                    <CardContent className="space-y-4 p-4">
                      <Field>
                        <FieldLabel htmlFor="world-folder">Java world folder</FieldLabel>
                        <div className="flex flex-wrap items-center gap-2">
                          <Input
                            id="world-folder"
                            className="min-w-56 flex-1 font-mono text-xs"
                            value={worldFolder}
                            placeholder="…/.minecraft/saves/My World"
                            onChange={(event) => {
                              setWorldFolder(event.target.value);
                              useProjectStore.getState().clearWorldAnalysis();
                            }}
                          />
                          <Button
                            size="sm"
                            variant="outline"
                            onClick={() => void browseWorldFolder()}
                          >
                            <FolderOpen data-icon="inline-start" />
                            Browse
                          </Button>
                          <Button
                            size="sm"
                            variant="outline"
                            disabled={analysisLoading || worldFolder.trim().length === 0}
                            onClick={() =>
                              void useProjectStore.getState().analyzeWorld(worldFolder)
                            }
                          >
                            {analysisLoading ? (
                              <Loader2 data-icon="inline-start" className="animate-spin" />
                            ) : (
                              <RefreshCw data-icon="inline-start" />
                            )}
                            Analyze
                          </Button>
                        </div>
                      </Field>

                      {worldAnalysis ? (
                        <div className="space-y-4 rounded-xl border bg-background p-4">
                          <div className="flex flex-wrap items-center gap-2 text-sm">
                            <Badge
                              variant={worldAnalysis.supported ? "secondary" : "destructive"}
                              className="uppercase"
                            >
                              {worldAnalysis.supported ? "supported" : "blocked"}
                            </Badge>
                            <span className="font-mono text-xs">
                              DataVersion {worldAnalysis.dataVersion ?? "unknown"}
                              {worldAnalysis.versionName ? ` · ${worldAnalysis.versionName}` : ""}
                            </span>
                            <span className="text-muted-foreground">{worldAnalysis.layout}</span>
                            <span className="text-muted-foreground">
                              {worldAnalysis.selectionKind}
                              {worldAnalysis.completeSave ? " · complete save" : " · incomplete"}
                            </span>
                            <span className="ml-auto font-mono text-xs text-muted-foreground">
                              {worldAnalysis.regions} region(s) · {worldAnalysis.chunks} chunk(s)
                            </span>
                          </div>
                          {worldAnalysis.levelName ? (
                            <p className="text-sm text-muted-foreground">
                              Level:{" "}
                              <span className="font-medium text-foreground">
                                {worldAnalysis.levelName}
                              </span>
                            </p>
                          ) : null}
                          <div className="space-y-2">
                            <h4 className="text-sm font-medium">Dimensions</h4>
                            {worldAnalysis.dimensions.map((dimension) => (
                              <div
                                key={dimension.id}
                                className="flex flex-wrap items-center gap-2 rounded-lg border bg-card px-3 py-2 font-mono text-xs"
                              >
                                <span className="truncate">{dimension.id}</span>
                                {dimension.minY !== null && dimension.height !== null ? (
                                  <span className="text-muted-foreground">
                                    y {dimension.minY}..{dimension.minY + dimension.height - 1}
                                  </span>
                                ) : null}
                                <span className="ml-auto text-muted-foreground">
                                  {dimension.regionFiles} region(s) · {dimension.chunks} chunk(s)
                                </span>
                              </div>
                            ))}
                          </div>
                          {worldAnalysis.blockers.length > 0 ? (
                            <Alert variant="destructive">
                              <AlertTriangle />
                              <AlertTitle>Import blocked</AlertTitle>
                              <AlertDescription>
                                {worldAnalysis.blockers.map((blocker) => (
                                  <p key={blocker}>{blocker}</p>
                                ))}
                              </AlertDescription>
                            </Alert>
                          ) : null}
                          {worldAnalysis.warnings.length > 0 ? (
                            <Alert>
                              <AlertTriangle />
                              <AlertTitle>Warnings</AlertTitle>
                              <AlertDescription>
                                <div className="scrollbar-thin max-h-24 space-y-1 overflow-y-auto">
                                  {worldAnalysis.warnings.map((warning) => (
                                    <p key={warning}>{warning}</p>
                                  ))}
                                </div>
                              </AlertDescription>
                            </Alert>
                          ) : null}
                          <Collapsible className="rounded-xl border bg-card p-4">
                            <CollapsibleTrigger className="flex w-full cursor-pointer items-center justify-between gap-2 text-left text-sm font-medium [&[data-panel-open]>svg]:rotate-180">
                              Read / edit / write / passthrough support matrix
                              <ChevronDown className="size-4 shrink-0 text-muted-foreground transition-transform" />
                            </CollapsibleTrigger>
                            <CollapsibleContent>
                              <div className="mt-3 space-y-3">
                                <div className="flex flex-wrap gap-2">
                                  {worldAnalysis.supportMatrix.versions.map((version) => (
                                    <Badge
                                      key={version.id}
                                      variant={
                                        version.status === "supported" ? "secondary" : "outline"
                                      }
                                      title={version.note}
                                    >
                                      {version.minecraftVersions} · {version.status}
                                    </Badge>
                                  ))}
                                </div>
                                <div className="overflow-hidden rounded-lg border">
                                  <Table>
                                    <TableHeader>
                                      <TableRow>
                                        <TableHead>Data</TableHead>
                                        <TableHead>Read</TableHead>
                                        <TableHead>Edit</TableHead>
                                        <TableHead>Write</TableHead>
                                        <TableHead>Pass</TableHead>
                                      </TableRow>
                                    </TableHeader>
                                    <TableBody>
                                      {worldAnalysis.supportMatrix.categories.map((category) => (
                                        <TableRow key={category.id} title={category.note}>
                                          <TableCell>{category.label}</TableCell>
                                          <TableCell className="font-mono text-xs">
                                            {category.read}
                                          </TableCell>
                                          <TableCell className="font-mono text-xs">
                                            {category.edit}
                                          </TableCell>
                                          <TableCell className="font-mono text-xs">
                                            {category.write}
                                          </TableCell>
                                          <TableCell className="font-mono text-xs">
                                            {category.passthrough}
                                          </TableCell>
                                        </TableRow>
                                      ))}
                                    </TableBody>
                                  </Table>
                                </div>
                              </div>
                            </CollapsibleContent>
                          </Collapsible>
                        </div>
                      ) : (
                        <p className="text-sm leading-relaxed text-muted-foreground">
                          Read-only analysis validates level.dat, the classic layout, every region
                          and every chunk before import. The original folder is never written.
                        </p>
                      )}
                    </CardContent>
                  </Card>
                ) : null}

                <Card className="gap-0 py-0">
                  <CardContent className="flex flex-wrap items-center gap-3 p-4 text-sm">
                    <Sparkles className="size-4 text-primary" />
                    <span className="text-muted-foreground">Target version</span>
                    {targetVersion ? (
                      <Select
                        items={[
                          {
                            value: targetVersion,
                            label: `Minecraft Java ${targetVersion}${targetDataVersion ? ` · DataVersion ${targetDataVersion}` : ""}`,
                          },
                        ]}
                        value={targetVersion}
                        disabled
                      >
                        <SelectTrigger className="font-mono" aria-label="Target Java version">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value={targetVersion}>
                            Minecraft Java {targetVersion}
                            {targetDataVersion ? ` · DataVersion ${targetDataVersion}` : ""}
                          </SelectItem>
                        </SelectContent>
                      </Select>
                    ) : (
                      <span className="text-xs text-muted-foreground">
                        Selected when the project is created
                      </span>
                    )}
                    <span className="ml-auto text-xs text-muted-foreground">
                      Saved as <span className="font-mono">project.json v2</span>
                    </span>
                  </CardContent>
                </Card>

                <div className="flex flex-wrap items-start gap-3">
                  <Button
                    size="sm"
                    disabled={
                      mode === "vanilla" ||
                      loading ||
                      analysisLoading ||
                      name.trim().length === 0 ||
                      !importReady
                    }
                    onClick={() => void submitCreate()}
                    className={mode === "vanilla" ? "hidden" : undefined}
                  >
                    {loading || analysisLoading ? (
                      <Loader2 data-icon="inline-start" className="animate-spin" />
                    ) : (
                      <Plus data-icon="inline-start" />
                    )}
                    {mode === "import" ? "Import world" : "Create project"}
                  </Button>
                  <p className="max-w-xl text-sm leading-relaxed text-muted-foreground">
                    {mode === "vanilla"
                      ? "Generate an isolated official-world result above, then use “Open as project”. The generated folder becomes a read-only source copy with its seed, preset and data packs recorded."
                      : mode === "import"
                        ? "Analyze a supported world first. The source folder is copied byte for byte and stays read-only."
                        : "Folders get a unique suffix when the name already exists. Renaming changes the display name only."}
                  </p>
                </div>
              </TabsContent>

              <TabsContent value="checkpoints" className="m-0 space-y-4 pt-4">
                <div className="flex flex-wrap items-center gap-2">
                  <Input
                    className="min-w-56 flex-1"
                    value={checkpointReason}
                    aria-label="Checkpoint reason"
                    placeholder="Checkpoint reason"
                    onChange={(event) => setCheckpointReason(event.target.value)}
                  />
                  <Button
                    size="sm"
                    disabled={loading || !activeFolder}
                    onClick={() =>
                      void useProjectStore.getState().createCheckpoint(checkpointReason || "manual")
                    }
                  >
                    <Plus data-icon="inline-start" />
                    Create checkpoint
                  </Button>
                  <Button
                    size="sm"
                    variant="outline"
                    onClick={() =>
                      void useProjectStore.getState().loadCheckpoints(activeFolder ?? undefined)
                    }
                  >
                    <RotateCcw data-icon="inline-start" />
                    Refresh
                  </Button>
                </div>
                <p className="text-sm leading-relaxed text-muted-foreground">
                  Checkpoints are full verified snapshots kept for the current project (up to 5).
                  Restoring one replaces the open world and leaves it unsaved until you save.
                </p>
                {checkpoints.length === 0 ? (
                  <Empty className="border p-6">
                    <EmptyHeader>
                      <EmptyMedia variant="icon">
                        <History />
                      </EmptyMedia>
                      <EmptyTitle>No checkpoints</EmptyTitle>
                      <EmptyDescription>
                        No checkpoints for {currentProject?.name ?? activeFolder ?? "this project"}.
                      </EmptyDescription>
                    </EmptyHeader>
                  </Empty>
                ) : (
                  <div className="space-y-2">
                    {checkpoints.map((checkpoint) => (
                      <div
                        key={checkpoint.revision}
                        className="flex flex-wrap items-center gap-3 rounded-xl border bg-card p-3 font-mono text-xs"
                      >
                        <Clock className="size-4 text-muted-foreground" />
                        <span className="font-medium">rev {checkpoint.revision}</span>
                        <span className="text-muted-foreground">
                          {formatBytes(checkpoint.bytes)}
                        </span>
                        <span className="min-w-0 flex-1 truncate text-muted-foreground">
                          {checkpoint.reason}
                        </span>
                        <span className="text-muted-foreground">
                          {formatRelativeTime(checkpoint.savedAtMs)}
                        </span>
                        <Button
                          size="sm"
                          variant="outline"
                          disabled={loading}
                          onClick={() => {
                            if (
                              window.confirm(
                                `Restore checkpoint revision ${checkpoint.revision}? The open world will be replaced.`,
                              )
                            ) {
                              void useProjectStore
                                .getState()
                                .restoreCheckpoint(checkpoint.revision);
                            }
                          }}
                        >
                          Restore
                        </Button>
                      </div>
                    ))}
                  </div>
                )}
              </TabsContent>

              <TabsContent value="exchange" className="m-0 space-y-4 pt-4">
                <StructureExchange />
              </TabsContent>
              <TabsContent value="settings" className="m-0 pt-4">
                {desktop && activeFolder ? (
                  <ProjectResourcePacks key={activeFolder} folder={activeFolder} />
                ) : null}
              </TabsContent>
            </Tabs>

            <Separator />
            <div className="flex items-start gap-2 text-sm leading-relaxed text-muted-foreground">
              <HardDrive className="mt-0.5 size-3.5 shrink-0" />
              <span>
                Atomic save: temp file → decode + re-encode verification → checksum → publish →
                manifest. Autosave runs every 30 seconds while edits are unsaved and on window
                close.
              </span>
              {loading ? (
                <Loader2 className="ml-auto size-3.5 shrink-0 animate-spin text-primary" />
              ) : null}
            </div>
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}

export function useProjectAutosave(intervalMs = 30_000): void {
  useEffect(() => {
    const tick = (): void => {
      const state = useProjectStore.getState();
      if (state.phase !== "ready" || !state.current?.folder) return;
      const revision = useWorldStore.getState().revision;
      if (revision === state.current.savedRevision) return;
      if (state.current.autosavedRevision === revision) return;
      void state.autosave();
    };
    const timer = window.setInterval(tick, intervalMs);
    const onVisibility = (): void => {
      if (document.visibilityState === "hidden") tick();
    };
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [intervalMs]);
}
