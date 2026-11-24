import { CheckCircle2, Circle, Loader2, RefreshCw, TriangleAlert, XCircle } from "lucide-react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Progress } from "@/components/ui/progress";
import type { ComponentStepState } from "@/features/project/ProjectComponentsStep";
import type { PrepareProgress } from "@/lib/project-api";
import { formatBytes, type CreationMode } from "@/project/types";
import type { ProjectFailure } from "@/state/project-store";

type StageKind = "done" | "running" | "pending" | "failed" | "warning";

interface StageRow {
  key: string;
  label: string;
  detail: string;
  kind: StageKind;
}

const PROJECT_STAGES: Record<"empty-flat" | "import" | "vanilla", string[]> = {
  "empty-flat": ["metadata", "verify", "ready"],
  import: ["source", "world", "metadata", "copy", "verify", "ready"],
  vanilla: ["source", "world", "metadata", "copy", "verify", "ready"],
};

const STAGE_LABELS: Record<string, string> = {
  metadata: "Project metadata",
  verify: "Verify and open",
  ready: "Ready",
  source: "Source world",
  world: "Editable blocks",
  copy: "Source copy",
};

function stageKind(
  stage: string,
  progress: PrepareProgress[],
  failed: boolean,
  lastRunning: string | null,
): StageKind {
  const events = progress.filter((event) => event.stage === stage);
  const latest = events.at(-1);
  if (latest?.state === "done") return "done";
  if (latest?.state === "running") {
    if (failed && lastRunning === stage) return "failed";
    return "running";
  }
  if (failed && lastRunning === stage) return "failed";
  return "pending";
}

function stageDetail(
  stage: string,
  progress: PrepareProgress[],
  failed: boolean,
  lastRunning: string | null,
): string {
  const latest = progress.filter((event) => event.stage === stage).at(-1);
  if (latest?.state === "running" && latest.total > 0) {
    return `${formatBytes(latest.completed)} of ${formatBytes(latest.total)} · ${latest.message}`;
  }
  if (latest?.message) return latest.message;
  if (failed && lastRunning === stage) return "Stopped before this stage finished.";
  if (latest?.state === "done") return "Done.";
  return "Waiting.";
}

function StageIcon({ kind }: { kind: StageKind }) {
  switch (kind) {
    case "done":
      return <CheckCircle2 className="size-4 text-primary" aria-hidden="true" />;
    case "running":
      return <Loader2 className="size-4 animate-spin text-primary" aria-hidden="true" />;
    case "failed":
      return <XCircle className="size-4 text-destructive" aria-hidden="true" />;
    case "warning":
      return <TriangleAlert className="size-4 text-muted-foreground" aria-hidden="true" />;
    default:
      return <Circle className="size-4 text-muted-foreground/60" aria-hidden="true" />;
  }
}

function ComponentStages({
  components,
  mode,
}: {
  components: ComponentStepState | null;
  mode: CreationMode;
}) {
  const summary = components?.summary;
  if (!summary) {
    return (
      <li className="flex items-start gap-3 text-sm text-muted-foreground">
        <TriangleAlert className="mt-0.5 size-4" aria-hidden="true" />
        <span>
          Component details were not captured; return to Components if a download is missing.
        </span>
      </li>
    );
  }
  const rows: Array<{ key: string; label: string; detail: string; ready: boolean }> = [];
  rows.push({
    key: "java",
    label: "Java runtime",
    detail: summary.java
      ? summary.java.state === "ready"
        ? `${summary.java.version ?? "verified"} · ready`
        : summary.java.state
      : mode === "vanilla"
        ? "Required before generation"
        : "Optional for editing",
    ready: summary.java?.state === "ready",
  });
  rows.push({
    key: "assets",
    label: "Client assets",
    detail: summary.assets
      ? summary.assets.mode === "placeholder"
        ? "Placeholder textures; no verified catalog"
        : `${summary.assets.version ?? "Client JAR"} · ${summary.assets.state}`
      : "Not captured",
    ready: summary.assets?.state === "ready",
  });
  if (summary.catalog) {
    rows.push({
      key: "catalog",
      label: "Content catalog",
      detail: `${summary.catalog.blocks} blocks · ${summary.catalog.items} items`,
      ready: true,
    });
  }
  if (mode === "vanilla") {
    rows.push({
      key: "server",
      label: "Official server",
      detail: summary.server
        ? `${summary.server.version ?? "Official server JAR"} · ${summary.server.state}`
        : "Required before generation",
      ready: summary.server?.state === "ready",
    });
  }
  return (
    <>
      {rows.map((row) => (
        <li key={row.key} className="flex items-start gap-3">
          <StageIcon kind={row.ready ? "done" : "warning"} />
          <div className="min-w-0">
            <p className="text-sm font-medium">{row.label}</p>
            <p className="text-xs text-muted-foreground">{row.detail}</p>
          </div>
        </li>
      ))}
    </>
  );
}

export interface ProjectPrepareStepProps {
  mode: CreationMode;
  components: ComponentStepState | null;
  progress: PrepareProgress[];
  message: string | null;
  cancelling: boolean;
  failure: ProjectFailure | null;
  onCancel: () => void;
  onBack: () => void;
  onRetry: () => void;
  canCancel: boolean;
}

/**
 * Honest preparation checklist. Component rows come from the verified Components step,
 * project rows come from backend `furybuilder-project-prepare` events. The UI never
 * advances a stage on a timer; pending stays pending until the backend reports it.
 */
export function ProjectPrepareStep({
  mode,
  components,
  progress,
  message,
  cancelling,
  failure,
  onCancel,
  onBack,
  onRetry,
  canCancel,
}: ProjectPrepareStepProps) {
  const failed = failure !== null;
  const lastRunning =
    [...progress].reverse().find((event) => event.state === "running")?.stage ?? null;
  const projectStages =
    PROJECT_STAGES[mode === "import" ? "import" : mode === "vanilla" ? "vanilla" : "empty-flat"];
  const rows: StageRow[] = projectStages.map((stage) => ({
    key: stage,
    label: STAGE_LABELS[stage] ?? stage,
    detail: stageDetail(stage, progress, failed, lastRunning),
    kind: stageKind(stage, progress, failed, lastRunning),
  }));
  const done = rows.filter((row) => row.kind === "done").length;
  const percent = rows.length > 0 ? Math.round((done / rows.length) * 100) : 0;
  const copy = progress.filter((event) => event.stage === "copy").at(-1);
  const copyPercent =
    copy && copy.total > 0 ? Math.min(100, Math.round((copy.completed / copy.total) * 100)) : null;

  return (
    <div className="space-y-4">
      <Card className="gap-0 py-0">
        <CardHeader className="border-b py-4">
          <CardTitle className="flex items-center gap-2 text-base">
            {failed ? (
              <TriangleAlert className="size-4 text-destructive" aria-hidden="true" />
            ) : (
              <Loader2 className="size-4 animate-spin text-primary" aria-hidden="true" />
            )}
            Preparing the project
          </CardTitle>
          <p role="status" className="text-sm text-muted-foreground">
            {message ?? "The editor opens only after every stage completes."}
          </p>
          {!failed && (
            <div className="mt-2">
              <Button
                type="button"
                size="sm"
                variant="outline"
                disabled={cancelling || !canCancel}
                onClick={onCancel}
              >
                {cancelling ? (
                  <Loader2 data-icon="inline-start" className="animate-spin" />
                ) : (
                  <XCircle data-icon="inline-start" />
                )}
                {cancelling ? "Cancelling…" : "Cancel preparation"}
              </Button>
            </div>
          )}
        </CardHeader>
        <CardContent className="space-y-5 px-6 py-5">
          <div className="space-y-2">
            <div className="flex items-center justify-between text-xs text-muted-foreground">
              <span>{failed ? "Stopped" : "Progress"}</span>
              <span className="tabular-nums">{percent}%</span>
            </div>
            <Progress value={percent} aria-label="Project preparation progress" />
          </div>

          <div>
            <p className="mb-2 text-xs font-medium uppercase tracking-wide text-muted-foreground">
              Prepared components
            </p>
            <ul className="space-y-3">
              <ComponentStages components={components} mode={mode} />
            </ul>
          </div>

          <div>
            <p className="mb-2 text-xs font-medium uppercase tracking-wide text-muted-foreground">
              Project stages
            </p>
            <ul className="space-y-3">
              {rows.map((row) => (
                <li key={row.key} className="flex items-start gap-3">
                  <StageIcon kind={row.kind} />
                  <div className="min-w-0 flex-1">
                    <div className="flex flex-wrap items-center justify-between gap-2">
                      <p className="text-sm font-medium">{row.label}</p>
                      {row.kind === "running" ? (
                        <span className="text-xs text-muted-foreground">In progress</span>
                      ) : null}
                    </div>
                    <p className="text-xs text-muted-foreground">{row.detail}</p>
                    {row.key === "copy" && copyPercent !== null && row.kind === "running" ? (
                      <Progress
                        className="mt-2"
                        value={copyPercent}
                        aria-label="Source copy progress"
                      />
                    ) : null}
                  </div>
                </li>
              ))}
            </ul>
          </div>

          <p className="text-xs leading-relaxed text-muted-foreground">
            Cancel stops at the next safe point before the project opens. Installed components stay
            in the cache.
          </p>

          {failed && failure ? (
            <Alert variant="destructive" role="alert">
              <TriangleAlert />
              <AlertTitle>Preparation stopped</AlertTitle>
              <AlertDescription>
                <p>{failure.message}</p>
                {failure.hint && <p>{failure.hint}</p>}
              </AlertDescription>
            </Alert>
          ) : null}

          {failed && (
            <div className="flex flex-wrap items-center gap-2">
              <Button type="button" size="sm" onClick={onRetry}>
                <RefreshCw data-icon="inline-start" />
                Retry
              </Button>
              <Button type="button" size="sm" variant="outline" onClick={onBack}>
                {mode === "vanilla" ? "Back to source" : "Back to review"}
              </Button>
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
