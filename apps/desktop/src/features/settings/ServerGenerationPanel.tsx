import { useCallback, useEffect, useState } from "react";
import {
  Ban,
  CheckCircle2,
  ChevronDown,
  FlaskConical,
  Loader2,
  Play,
  RefreshCw,
  ShieldCheck,
  Trash2,
  TriangleAlert,
} from "lucide-react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Progress } from "@/components/ui/progress";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { displayRuntimePath, formatBytes } from "@/lib/runtime-api";
import {
  acceptServerEula,
  cancelServerGeneration,
  clearServerEula,
  clearServerGenerationRuns,
  serverGenerationRuns,
  serverGenerationStatus,
  startServerGeneration,
} from "@/lib/server-api";
import { cn } from "@/lib/utils";
import type {
  GenerationReport,
  GenerationRequest,
  GenerationRunsReport,
  GenerationSnapshot,
  GenerationStatusReport,
  ServerDiscoveryReport,
  WorldPreset,
} from "@/runtime/server-types";

type Busy = "load" | "eula" | "start" | "cancel" | "runs" | null;

interface ServerGenerationPanelProps {
  discovery: ServerDiscoveryReport | null;
  onDiscoveryChanged: () => void;
}

const EULA_URL = "https://aka.ms/MinecraftEULA";

const PRESET_ITEMS = [
  { value: "normal", label: "normal" },
  { value: "flat", label: "flat" },
  { value: "large_biomes", label: "large biomes" },
  { value: "amplified", label: "amplified" },
];

function stopReasonLabel(report: GenerationReport): string {
  switch (report.stopReason) {
    case "completed":
      return "completed";
    case "incomplete":
      return "incomplete";
    case "time_limit":
      return "time limit reached";
    case "cancelled":
      return "cancelled";
    case "crashed":
      return "server crashed";
    case "startup_failed":
      return "startup failed";
    case "isolation_failed":
      return "isolation failure";
    default:
      return report.stopReason;
  }
}

function formatSeconds(milliseconds: number): string {
  return `${Math.max(0, Math.round(milliseconds / 1000))}s`;
}

function acceptedAt(seconds: number): string {
  return new Date(seconds * 1000).toLocaleString();
}

export function ServerGenerationPanel({
  discovery,
  onDiscoveryChanged,
}: ServerGenerationPanelProps) {
  const [status, setStatus] = useState<GenerationStatusReport | null>(null);
  const [runs, setRuns] = useState<GenerationRunsReport | null>(null);
  const [busy, setBusy] = useState<Busy>("load");
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [eulaChecked, setEulaChecked] = useState(false);
  const [seed, setSeed] = useState("FuryBuilder");
  const [preset, setPreset] = useState<WorldPreset>("normal");
  const [centerX, setCenterX] = useState("0");
  const [centerZ, setCenterZ] = useState("0");
  const [radius, setRadius] = useState("1");
  const [timeLimit, setTimeLimit] = useState("120");
  const [memory, setMemory] = useState("2048");
  const [viewDistance, setViewDistance] = useState("3");
  const [cpuThreads, setCpuThreads] = useState("2");

  const refresh = useCallback(async () => {
    try {
      const [nextStatus, nextRuns] = await Promise.all([
        serverGenerationStatus(),
        serverGenerationRuns(),
      ]);
      setStatus(nextStatus);
      setRuns(nextRuns);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    }
  }, []);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        await refresh();
      } finally {
        if (!cancelled) setBusy((current) => (current === "load" ? null : current));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [refresh]);

  useEffect(() => {
    if (!status?.active?.running) return;
    const timer = window.setInterval(() => {
      void refresh();
    }, 1000);
    return () => window.clearInterval(timer);
  }, [status?.active?.running, refresh]);

  const selected = discovery?.selected ?? null;
  const eulaAccepted = Boolean(discovery?.eulaMatchesSelected);
  const active: GenerationSnapshot | null = status?.active ?? null;
  const last = status?.last ?? null;
  const running = Boolean(active?.running);
  const progress =
    active && active.expectedChunks > 0
      ? Math.min(100, Math.round((active.generatedChunks / active.expectedChunks) * 100))
      : null;

  async function acceptEula() {
    if (!eulaChecked) return;
    setBusy("eula");
    setError(null);
    setNotice(null);
    try {
      const eula = await acceptServerEula(true);
      setEulaChecked(false);
      setNotice(
        `EULA accepted for Minecraft ${eula.minecraftVersion} (${eula.jarSha1.slice(0, 12)}…).`,
      );
      onDiscoveryChanged();
      await refresh();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(null);
    }
  }

  async function withdrawEula() {
    setBusy("eula");
    setError(null);
    setNotice(null);
    try {
      await clearServerEula();
      setNotice("EULA acceptance withdrawn. No server can start until you accept again.");
      onDiscoveryChanged();
      await refresh();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(null);
    }
  }

  async function start() {
    const request: GenerationRequest = {
      seed: seed.trim(),
      preset,
      centerX: Number(centerX),
      centerZ: Number(centerZ),
      radiusChunks: Number(radius),
      timeLimitSeconds: Number(timeLimit),
      memoryMb: Number(memory),
      viewDistance: Number(viewDistance),
      cpuThreads: Number(cpuThreads),
      generateStructures: true,
      consent: true,
      datapacks: [],
    };
    const numbers = [
      request.centerX,
      request.centerZ,
      request.radiusChunks,
      request.timeLimitSeconds,
      request.memoryMb,
      request.viewDistance,
      request.cpuThreads,
    ];
    if (numbers.some((value) => !Number.isFinite(value))) {
      setError("Every numeric field must be a finite number.");
      return;
    }
    setBusy("start");
    setError(null);
    setNotice(null);
    try {
      const snapshot = await startServerGeneration(request);
      setStatus({ active: snapshot, last: status?.last ?? null });
      setNotice("Isolated generation started. The server is bound to loopback only.");
      await refresh();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(null);
    }
  }

  async function cancel() {
    setBusy("cancel");
    setError(null);
    setNotice(null);
    try {
      const requested = await cancelServerGeneration();
      setNotice(
        requested
          ? "Cancellation requested. The process tree will be terminated if it does not stop."
          : "No active generation run.",
      );
      await refresh();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(null);
    }
  }

  async function clearRuns() {
    setBusy("runs");
    setError(null);
    setNotice(null);
    try {
      const report = await clearServerGenerationRuns();
      setRuns(report);
      setStatus(await serverGenerationStatus());
      setNotice("FuryBuilder-owned generation run directories were removed.");
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(null);
    }
  }

  return (
    <div className="min-w-0 space-y-5 rounded-xl border bg-card p-6">
      <div className="flex items-start justify-between gap-3">
        <div className="space-y-1.5">
          <h4 className="flex items-center gap-2 text-base font-semibold">
            <FlaskConical className="size-4" /> Isolated vanilla generation
          </h4>
          <p className="text-sm leading-relaxed text-muted-foreground">
            Starts the official 1.21.4 server in a temporary run directory. It listens on{" "}
            <span className="font-mono">127.0.0.1</span> only, accepts no external players, has a
            wall-clock and memory limit, and is placed in a kill-on-close process job. The world is
            generated by Mojang&apos;s code, not by a FuryBuilder generator.
          </p>
        </div>
        <Button
          variant="ghost"
          size="icon-sm"
          className="shrink-0"
          onClick={() => void refresh()}
          disabled={busy !== null}
          aria-label="Refresh generation status"
        >
          {busy === "load" ? <Loader2 className="animate-spin" /> : <RefreshCw />}
        </Button>
      </div>

      {!selected ? (
        <Alert className="p-4">
          <TriangleAlert />
          <AlertTitle>Server JAR required</AlertTitle>
          <AlertDescription>
            Select or download the verified 1.21.4 <span className="font-mono">server.jar</span>{" "}
            above first.
          </AlertDescription>
        </Alert>
      ) : !eulaAccepted ? (
        <Alert className="p-4">
          <TriangleAlert />
          <AlertTitle>Minecraft EULA confirmation required</AlertTitle>
          <AlertDescription>
            <p>
              FuryBuilder does not accept the Minecraft EULA for you. Read the official text at{" "}
              <a
                className="underline underline-offset-2"
                href={EULA_URL}
                target="_blank"
                rel="noreferrer"
              >
                {EULA_URL}
              </a>{" "}
              and confirm it for this exact JAR
              {discovery?.eula ? " (the previous acceptance was for a different file)" : ""}.
            </p>
            <div className="mt-3 flex items-start gap-2">
              <Checkbox
                id="eula-confirmation"
                className="mt-0.5"
                checked={eulaChecked}
                onCheckedChange={(checked) => setEulaChecked(checked === true)}
                aria-label="I have read and accept the Minecraft EULA"
              />
              <span className="text-sm leading-relaxed">
                I have read and accept the Minecraft EULA for this server JAR
                <span className="ml-1 font-mono text-xs text-muted-foreground">
                  {selected.sha1?.slice(0, 12)}…
                </span>
              </span>
            </div>
            <Button
              size="sm"
              className="mt-3"
              onClick={() => void acceptEula()}
              disabled={!eulaChecked || busy !== null}
              aria-label="Accept EULA"
            >
              {busy === "eula" ? (
                <Loader2 data-icon="inline-start" className="animate-spin" />
              ) : (
                <CheckCircle2 data-icon="inline-start" />
              )}
              Accept EULA
            </Button>
          </AlertDescription>
        </Alert>
      ) : (
        <Alert className="p-4">
          <CheckCircle2 className="text-status-ok" />
          <AlertTitle className="text-status-ok">EULA accepted</AlertTitle>
          <AlertDescription>
            <p>
              Accepted {discovery?.eula ? acceptedAt(discovery.eula.acceptedAt) : ""}. Read the{" "}
              <a
                className="underline underline-offset-2"
                href={EULA_URL}
                target="_blank"
                rel="noreferrer"
              >
                EULA text
              </a>
              .
            </p>
            <Button
              variant="ghost"
              size="sm"
              className="mt-2"
              onClick={() => void withdrawEula()}
              disabled={busy !== null || running}
              aria-label="Withdraw EULA acceptance"
            >
              <Trash2 data-icon="inline-start" />
              Withdraw
            </Button>
          </AlertDescription>
        </Alert>
      )}

      <div className="grid grid-cols-2 gap-4 sm:grid-cols-3">
        <Label className="flex-col items-start gap-1.5" htmlFor="generation-seed">
          <span className="text-xs text-muted-foreground">Seed</span>
          <Input
            id="generation-seed"
            className="font-mono text-xs"
            value={seed}
            maxLength={64}
            onChange={(event) => setSeed(event.target.value)}
            disabled={running}
            aria-label="Generation seed"
          />
        </Label>
        <Label className="flex-col items-start gap-1.5" htmlFor="world-preset">
          <span className="text-xs text-muted-foreground">Preset</span>
          <Select
            items={PRESET_ITEMS}
            value={preset}
            onValueChange={(value) => {
              if (value) setPreset(value as WorldPreset);
            }}
            disabled={running}
          >
            <SelectTrigger id="world-preset" className="w-full" aria-label="World preset">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {PRESET_ITEMS.map((item) => (
                <SelectItem key={item.value} value={item.value}>
                  {item.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </Label>
        <Label className="flex-col items-start gap-1.5" htmlFor="generation-radius">
          <span className="text-xs text-muted-foreground">Radius, chunks</span>
          <Input
            id="generation-radius"
            type="number"
            min={1}
            max={16}
            className="font-mono text-xs"
            value={radius}
            onChange={(event) => setRadius(event.target.value)}
            disabled={running}
            aria-label="Generation radius in chunks"
          />
        </Label>
        <Label className="flex-col items-start gap-1.5" htmlFor="generation-center-x">
          <span className="text-xs text-muted-foreground">Center X</span>
          <Input
            id="generation-center-x"
            type="number"
            className="font-mono text-xs"
            value={centerX}
            onChange={(event) => setCenterX(event.target.value)}
            disabled={running}
            aria-label="Generation center X"
          />
        </Label>
        <Label className="flex-col items-start gap-1.5" htmlFor="generation-center-z">
          <span className="text-xs text-muted-foreground">Center Z</span>
          <Input
            id="generation-center-z"
            type="number"
            className="font-mono text-xs"
            value={centerZ}
            onChange={(event) => setCenterZ(event.target.value)}
            disabled={running}
            aria-label="Generation center Z"
          />
        </Label>
        <Label className="flex-col items-start gap-1.5" htmlFor="generation-time-limit">
          <span className="text-xs text-muted-foreground">Time limit, s</span>
          <Input
            id="generation-time-limit"
            type="number"
            min={10}
            max={3600}
            className="font-mono text-xs"
            value={timeLimit}
            onChange={(event) => setTimeLimit(event.target.value)}
            disabled={running}
            aria-label="Generation time limit seconds"
          />
        </Label>
        <Label className="flex-col items-start gap-1.5" htmlFor="generation-memory">
          <span className="text-xs text-muted-foreground">Memory, MiB</span>
          <Input
            id="generation-memory"
            type="number"
            min={512}
            max={4096}
            step={256}
            className="font-mono text-xs"
            value={memory}
            onChange={(event) => setMemory(event.target.value)}
            disabled={running}
            aria-label="Generation memory limit"
          />
        </Label>
        <Label className="flex-col items-start gap-1.5" htmlFor="generation-view-distance">
          <span className="text-xs text-muted-foreground">View distance</span>
          <Input
            id="generation-view-distance"
            type="number"
            min={2}
            max={8}
            className="font-mono text-xs"
            value={viewDistance}
            onChange={(event) => setViewDistance(event.target.value)}
            disabled={running}
            aria-label="Generation view distance"
          />
        </Label>
        <Label className="flex-col items-start gap-1.5" htmlFor="generation-cpu-threads">
          <span className="text-xs text-muted-foreground">CPU threads</span>
          <Input
            id="generation-cpu-threads"
            type="number"
            min={1}
            max={8}
            className="font-mono text-xs"
            value={cpuThreads}
            onChange={(event) => setCpuThreads(event.target.value)}
            disabled={running}
            aria-label="Generation CPU threads"
          />
        </Label>
      </div>

      <div className="flex flex-wrap items-center gap-2">
        <Button
          onClick={() => void start()}
          disabled={busy !== null || running || !eulaAccepted || !selected?.ready}
          aria-label="Start isolated generation"
        >
          {busy === "start" ? (
            <Loader2 data-icon="inline-start" className="animate-spin" />
          ) : (
            <Play data-icon="inline-start" />
          )}
          Start isolated generation
        </Button>
        <Button
          variant="outline"
          onClick={() => void cancel()}
          disabled={busy !== null || !running}
          aria-label="Cancel generation"
        >
          {busy === "cancel" ? (
            <Loader2 data-icon="inline-start" className="animate-spin" />
          ) : (
            <Ban data-icon="inline-start" />
          )}
          Cancel
        </Button>
        <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
          <ShieldCheck className="size-3.5" /> loopback only · {memory} MiB · {timeLimit}s
        </span>
      </div>

      {active ? (
        <Card className="gap-0 py-0">
          <CardContent className="space-y-3 p-4">
            <div className="flex flex-wrap items-center gap-2">
              <Badge variant={running ? "secondary" : "outline"} className="uppercase">
                {active.phase}
              </Badge>
              <span className="text-sm font-medium">{active.message}</span>
              <span className="ml-auto font-mono text-xs text-muted-foreground">
                {formatSeconds(active.elapsedMs)}
              </span>
            </div>
            <div className="flex flex-wrap gap-x-4 gap-y-1 font-mono text-xs text-muted-foreground">
              <span>
                chunks {active.generatedChunks}/{active.expectedChunks}
              </span>
              <span>regions {active.regionFiles}</span>
              <span>{formatBytes(active.worldBytes)}</span>
              <span className="truncate">run {active.runId}</span>
            </div>
            <Progress value={progress ?? 8} />
            {active.error ? <p className="text-sm text-status-warn">{active.error}</p> : null}
          </CardContent>
        </Card>
      ) : null}

      {last && !running ? (
        <Collapsible className="rounded-xl border bg-card p-4 text-sm" defaultOpen>
          <CollapsibleTrigger className="flex w-full cursor-pointer items-center justify-between gap-2 text-left font-medium [&[data-panel-open]>svg]:rotate-180">
            Last isolated run
            <ChevronDown className="size-4 shrink-0 text-muted-foreground transition-transform" />
          </CollapsibleTrigger>
          <CollapsibleContent>
            <div className="mt-3 flex flex-wrap gap-x-4 gap-y-1">
              <span>
                Result{" "}
                <span
                  className={cn(
                    "font-mono",
                    last.worldComplete ? "text-status-ok" : "text-status-warn",
                  )}
                >
                  {stopReasonLabel(last)}
                </span>
              </span>
              <span>
                chunks <span className="font-mono">{last.snapshot.generatedChunks}</span>/
                <span className="font-mono">{last.snapshot.expectedChunks}</span>
              </span>
              <span>
                regions <span className="font-mono">{last.snapshot.regionFiles}</span>
              </span>
              <span>
                world <span className="font-mono">{formatBytes(last.snapshot.worldBytes)}</span>
              </span>
              <span>
                exit <span className="font-mono">{last.exitCode ?? "killed"}</span>
              </span>
              <span>
                guard{" "}
                <span className="font-mono">
                  {last.processTreeGuarded ? "job-object" : "fallback"}
                </span>
              </span>
            </div>
            <div className="mt-2 flex flex-wrap gap-x-4 gap-y-1 font-mono text-xs text-muted-foreground">
              <span>seed {last.seed}</span>
              <span>preset {last.preset}</span>
              <span>
                center {last.centerX},{last.centerZ}
              </span>
              <span>bind {last.bindAddress ?? "unknown"}</span>
              <span>java {last.javaVersion}</span>
            </div>
            <div className="mt-2 truncate font-mono text-xs text-muted-foreground">
              {displayRuntimePath(last.worldDir)}
            </div>
            <div className="truncate font-mono text-xs text-muted-foreground">
              {displayRuntimePath(last.logPath)}
            </div>
            {last.snapshot.error ? (
              <p className="mt-2 text-sm text-status-warn">{last.snapshot.error}</p>
            ) : null}
            {last.logTail.length > 0 ? (
              <pre className="mt-3 max-h-32 overflow-auto rounded-lg bg-muted/40 p-3 font-mono text-xs whitespace-pre-wrap">
                {last.logTail.slice(-12).join("\n")}
              </pre>
            ) : null}
          </CollapsibleContent>
        </Collapsible>
      ) : null}

      <div className="flex flex-wrap items-center gap-2">
        <Button
          variant="outline"
          size="sm"
          onClick={() => void clearRuns()}
          disabled={busy !== null || running || (runs?.runs.length ?? 0) === 0}
          aria-label="Clear generation runs"
        >
          <Trash2 data-icon="inline-start" />
          Clear generation runs
        </Button>
        <span className="text-xs text-muted-foreground">
          {runs?.runs.length ?? 0} run(s) · {formatBytes(runs?.totalBytes ?? 0)}
        </span>
      </div>
      {runs?.runs.slice(0, 3).map((run) => (
        <div
          key={run.runId}
          className="truncate rounded-lg border bg-background px-3 py-2 font-mono text-xs"
          title={run.path}
        >
          {run.runId} · {run.status} · {run.chunks}/{run.expectedChunks} chunks ·{" "}
          {formatBytes(run.worldBytes)}
        </div>
      ))}
      {runs?.problems.map((problem) => (
        <p key={problem} className="text-sm text-status-warn">
          {problem}
        </p>
      ))}

      {error ? (
        <Alert variant="destructive" className="p-4">
          <TriangleAlert />
          <AlertTitle>Generation error</AlertTitle>
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      ) : null}
      {notice ? (
        <Alert className="p-4">
          <CheckCircle2 />
          <AlertTitle>Generation update</AlertTitle>
          <AlertDescription>{notice}</AlertDescription>
        </Alert>
      ) : null}
    </div>
  );
}
