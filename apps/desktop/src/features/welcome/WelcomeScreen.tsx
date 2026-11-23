import { useEffect, useState } from "react";
import {
  Archive,
  FolderOpen,
  Import,
  Loader2,
  Plus,
  RefreshCw,
  RotateCcw,
  TriangleAlert,
} from "lucide-react";
import logoUrl from "@/assets/branding/furybuilder-logo.png";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardAction,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from "@/components/ui/empty";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Separator } from "@/components/ui/separator";
import type { BackendState } from "@/hooks/use-backend";
import { isTauriRuntime } from "@/lib/ipc";
import type { ProjectSummary } from "@/project/types";
import {
  creationModeLabel,
  formatBytes,
  formatRelativeTime,
  projectSummaryWarnings,
} from "@/project/types";
import { useProjectStore } from "@/state/project-store";
import { pushToast } from "@/state/toast-store";

const RECENT_LIMIT = 6;

function recentProjects(projects: ProjectSummary[]): ProjectSummary[] {
  return [...projects]
    .sort(
      (left, right) =>
        right.lastSavedAtMs - left.lastSavedAtMs || right.updatedAtMs - left.updatedAtMs,
    )
    .slice(0, RECENT_LIMIT);
}

function RecentProject({
  project,
  disabled,
  onOpen,
}: {
  project: ProjectSummary;
  disabled: boolean;
  onOpen: () => void;
}) {
  const warnings = projectSummaryWarnings(project);
  return (
    <li className="flex flex-col gap-3 rounded-lg border p-3 sm:flex-row sm:items-center">
      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-center gap-2">
          <span className="truncate text-sm font-medium">{project.name}</span>
          {project.recoverable ? <Badge variant="secondary">Recoverable</Badge> : null}
          {warnings.length > 0 ? <Badge variant="outline">Check project</Badge> : null}
        </div>
        <p className="mt-1 truncate font-mono text-xs text-muted-foreground">
          {creationModeLabel(project.creationMode)} · Minecraft Java{" "}
          {project.targetMinecraftVersion} · DV {project.targetDataVersion} ·{" "}
          {formatBytes(project.bytes)} · saved {formatRelativeTime(project.lastSavedAtMs)}
        </p>
        {project.recoverable ? (
          <p className="mt-1 text-xs leading-relaxed text-status-warn">
            A verified autosave is newer than the saved revision. Recover opens it; the editor lets
            you save or discard it.
          </p>
        ) : null}
        {warnings.length > 0 ? (
          <p className="mt-1 text-xs leading-relaxed text-status-warn">{warnings.join(" · ")}</p>
        ) : null}
      </div>
      <Button
        size="sm"
        variant="outline"
        aria-label={`${project.recoverable ? "Recover" : "Open"} ${project.name}`}
        disabled={disabled}
        onClick={onOpen}
      >
        {project.recoverable ? (
          <RotateCcw data-icon="inline-start" />
        ) : (
          <FolderOpen data-icon="inline-start" />
        )}
        {project.recoverable ? "Recover" : "Open"}
      </Button>
    </li>
  );
}

function PreparingProject({ message }: { message: string | null }) {
  return (
    <main className="flex min-h-0 flex-1 items-center justify-center p-6">
      <div
        role="status"
        aria-label="Preparing project"
        aria-live="polite"
        className="flex max-w-sm flex-col items-center gap-3 text-center"
      >
        <img src={logoUrl} alt="" aria-hidden="true" draggable={false} className="h-10 w-auto" />
        <Loader2 className="size-5 animate-spin text-muted-foreground" />
        <p className="text-sm font-medium">{message ?? "Preparing the project…"}</p>
        <p className="text-sm leading-relaxed text-muted-foreground">
          The viewport stays closed until the project and its components are ready.
        </p>
      </div>
    </main>
  );
}

export function WelcomeScreen({ backend }: { backend: BackendState }) {
  const projects = useProjectStore((state) => state.projects);
  const warnings = useProjectStore((state) => state.warnings);
  const error = useProjectStore((state) => state.error);
  const phase = useProjectStore((state) => state.phase);
  const prepareMessage = useProjectStore((state) => state.prepareMessage);
  const failure = useProjectStore((state) => state.failure);
  const loading = useProjectStore((state) => state.loading);
  const openManager = useProjectStore((state) => state.openManager);
  const desktop = isTauriRuntime();
  const [listing, setListing] = useState(desktop);
  const [picking, setPicking] = useState(false);

  useEffect(() => {
    if (!desktop) return;
    let cancelled = false;
    void (async () => {
      await useProjectStore.getState().refreshList();
      if (!cancelled) setListing(false);
    })();
    return () => {
      cancelled = true;
    };
  }, [desktop]);

  if (phase === "preparing") {
    return <PreparingProject message={prepareMessage} />;
  }

  const coreOffline = backend.status === "offline";
  const busy = listing || picking || loading;
  const actionsDisabled = busy || (desktop && coreOffline);
  const recent = recentProjects(projects);
  const recoverableCount = recent.filter((project) => project.recoverable).length;

  const refresh = async (): Promise<void> => {
    setListing(true);
    await useProjectStore.getState().refreshList();
    setListing(false);
  };

  const pickProject = async (): Promise<void> => {
    if (!desktop) {
      openManager("projects");
      return;
    }
    setPicking(true);
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = await open({
        directory: true,
        multiple: false,
        title: "Open a FuryBuilder project folder",
      });
      if (typeof selected === "string" && selected.length > 0) {
        await useProjectStore.getState().open(selected);
      }
    } catch (pickerError) {
      pushToast({
        kind: "error",
        title: "Folder picker unavailable",
        description: String(pickerError),
      });
    } finally {
      setPicking(false);
    }
  };

  return (
    <main className="flex min-h-0 flex-1 items-center justify-center overflow-y-auto bg-muted/20 p-6">
      <div className="grid w-full max-w-5xl gap-6 lg:grid-cols-[minmax(0,0.9fr)_minmax(0,1.1fr)] lg:items-center">
        <section className="flex flex-col gap-5">
          <div className="flex items-center gap-3">
            <img
              src={logoUrl}
              alt=""
              aria-hidden="true"
              draggable={false}
              className="h-12 w-auto"
            />
            <div>
              <h1 className="font-heading text-2xl font-semibold tracking-tight">FuryBuilder</h1>
              <p className="text-sm text-muted-foreground">Minecraft Java world editor</p>
            </div>
          </div>

          <p className="max-w-md text-sm leading-relaxed text-muted-foreground">
            Build, paint and analyze Java worlds in local projects. Every save is verified, every
            source stays untouched, and nothing is downloaded until you ask for it.
          </p>

          <div className="flex flex-wrap gap-2">
            <Button disabled={actionsDisabled} onClick={() => openManager("create", "empty")}>
              <Plus data-icon="inline-start" />
              New project
            </Button>
            <Button variant="outline" disabled={actionsDisabled} onClick={() => void pickProject()}>
              {picking ? (
                <Loader2 data-icon="inline-start" className="animate-spin" />
              ) : (
                <FolderOpen data-icon="inline-start" />
              )}
              Open project
            </Button>
            <Button
              variant="outline"
              disabled={actionsDisabled}
              onClick={() => openManager("create", "import")}
            >
              <Import data-icon="inline-start" />
              Import world
            </Button>
          </div>

          {desktop && coreOffline ? (
            <Alert>
              <TriangleAlert />
              <AlertTitle>Desktop core offline</AlertTitle>
              <AlertDescription>
                {backend.error ??
                  "The Rust backend is not reachable. Restart the desktop app before opening a project."}
              </AlertDescription>
            </Alert>
          ) : null}

          {!desktop ? (
            <Alert>
              <TriangleAlert />
              <AlertTitle>Desktop core required</AlertTitle>
              <AlertDescription>
                This browser preview cannot create, open or save projects. Run the desktop build
                with <span className="font-mono text-xs">pnpm tauri dev</span>.
              </AlertDescription>
            </Alert>
          ) : null}

          {phase === "error" && failure ? (
            <Alert variant="destructive">
              <TriangleAlert />
              <AlertTitle>Project could not be prepared</AlertTitle>
              <AlertDescription>
                <p>{failure.message}</p>
                {failure.hint ? <p className="mt-1">{failure.hint}</p> : null}
                <div className="mt-3 flex flex-wrap gap-2">
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={busy}
                    onClick={() => void useProjectStore.getState().resume()}
                  >
                    <RotateCcw data-icon="inline-start" />
                    Retry
                  </Button>
                  <Button
                    size="sm"
                    variant="ghost"
                    onClick={() => useProjectStore.getState().clearFailure()}
                  >
                    Dismiss
                  </Button>
                </div>
              </AlertDescription>
            </Alert>
          ) : null}

          {phase !== "error" && error ? (
            <Alert variant="destructive">
              <TriangleAlert />
              <AlertTitle>Projects could not be loaded</AlertTitle>
              <AlertDescription>
                <p>{error}</p>
                <Button
                  size="sm"
                  variant="outline"
                  className="mt-3"
                  disabled={listing || !desktop}
                  onClick={() => void refresh()}
                >
                  <RefreshCw data-icon="inline-start" />
                  Try again
                </Button>
              </AlertDescription>
            </Alert>
          ) : null}
        </section>

        <Card size="sm" className="min-h-0">
          <CardHeader>
            <CardTitle>Recent projects</CardTitle>
            <CardDescription>Continue where you left off.</CardDescription>
            <CardAction>
              <Button
                variant="ghost"
                size="icon-sm"
                aria-label="Refresh projects"
                disabled={listing || !desktop}
                onClick={() => void refresh()}
              >
                <RefreshCw className={listing ? "animate-spin" : undefined} />
              </Button>
            </CardAction>
          </CardHeader>
          <Separator />
          <CardContent className="flex min-h-0 flex-col gap-3 pt-3">
            {listing ? (
              <p className="flex items-center gap-2 py-6 text-sm text-muted-foreground">
                <Loader2 className="size-4 animate-spin" />
                Loading projects…
              </p>
            ) : recent.length === 0 ? (
              <Empty className="border p-6">
                <EmptyHeader>
                  <EmptyMedia variant="icon">
                    <Archive />
                  </EmptyMedia>
                  <EmptyTitle>No projects yet</EmptyTitle>
                  <EmptyDescription>
                    Create an empty or flat project, or import an existing Java world. Projects stay
                    on this machine.
                  </EmptyDescription>
                </EmptyHeader>
              </Empty>
            ) : (
              <ScrollArea className="max-h-[52vh]">
                <ul className="flex flex-col gap-2 pr-2">
                  {recent.map((project) => (
                    <RecentProject
                      key={project.folder}
                      project={project}
                      disabled={busy}
                      onOpen={() => void useProjectStore.getState().open(project.folder)}
                    />
                  ))}
                </ul>
              </ScrollArea>
            )}

            {recoverableCount > 0 ? (
              <Alert>
                <TriangleAlert />
                <AlertTitle>Unsaved recovery data</AlertTitle>
                <AlertDescription>
                  {recoverableCount === 1
                    ? "One recent project has"
                    : `${recoverableCount} recent projects have`}{" "}
                  a verified autosave newer than the saved revision. Recover opens that revision;
                  save or discard it from the editor banner.
                </AlertDescription>
              </Alert>
            ) : null}

            {warnings.length > 0 ? (
              <Alert>
                <TriangleAlert />
                <AlertTitle>Project library warnings</AlertTitle>
                <AlertDescription>
                  <ul className="list-disc pl-4">
                    {warnings.map((warning) => (
                      <li key={warning}>{warning}</li>
                    ))}
                  </ul>
                </AlertDescription>
              </Alert>
            ) : null}
          </CardContent>
        </Card>
      </div>
    </main>
  );
}
