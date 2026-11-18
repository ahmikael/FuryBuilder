import { useCallback, useEffect, useState } from "react";
import {
  Ban,
  CheckCircle2,
  FlaskConical,
  Loader2,
  Play,
  RefreshCw,
  ShieldCheck,
  Trash2,
  TriangleAlert,
} from "lucide-react";
import { Button } from "@/components/ui/button";
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
    <div className="min-w-0 space-y-2 rounded border border-border bg-background/40 p-2">
      <div className="flex items-center justify-between">
        <h4 className="flex items-center gap-1.5 text-[10px] font-semibold tracking-wide uppercase">
          <FlaskConical className="size-3.5" /> Isolated vanilla generation
        </h4>
        <Button
          variant="ghost"
          size="sm"
          className="h-6 px-1.5"
          onClick={() => void refresh()}
          disabled={busy !== null}
          aria-label="Refresh generation status"
        >
          {busy === "load" ? <Loader2 className="animate-spin" /> : <RefreshCw />}
        </Button>
      </div>
      <p className="text-[10px] text-muted-foreground">
        Starts the official 1.21.4 server in a temporary run directory. It listens on{" "}
        <span className="font-mono">127.0.0.1</span> only, accepts no external players, has a
        wall-clock and memory limit, and is placed in a kill-on-close process job. The world is
        generated by Mojang&apos;s code, not by a FuryBuilder generator.
      </p>

      {!selected ? (
        <p className="rounded border border-dashed border-border px-2 py-1 text-[10px] text-muted-foreground">
          Select or download the verified 1.21.4 <span className="font-mono">server.jar</span> above
          first.
        </p>
      ) : !eulaAccepted ? (
        <div className="space-y-1.5 rounded border border-status-warn/40 bg-status-warn/5 p-2">
          <div className="flex items-center gap-1.5 text-[10px] font-medium text-status-warn">
            <TriangleAlert className="size-3.5" /> Minecraft EULA confirmation required
          </div>
          <p className="text-[10px] text-muted-foreground">
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
          <label className="flex items-start gap-1.5 text-[10px]">
            <input
              type="checkbox"
              className="mt-0.5"
              checked={eulaChecked}
              onChange={(event) => setEulaChecked(event.target.checked)}
              aria-label="I have read and accept the Minecraft EULA"
            />
            <span>
              I have read and accept the Minecraft EULA for this server JAR
              <span className="ml-1 font-mono text-muted-foreground">
                {selected.sha1?.slice(0, 12)}…
              </span>
            </span>
          </label>
          <Button
            size="sm"
            onClick={() => void acceptEula()}
            disabled={!eulaChecked || busy !== null}
            aria-label="Accept EULA"
          >
            {busy === "eula" ? <Loader2 className="animate-spin" /> : <CheckCircle2 />}
            Accept EULA
          </Button>
        </div>
      ) : (
        <div className="flex flex-wrap items-center gap-x-2 gap-y-1 rounded border border-status-ok/40 bg-status-ok/5 px-2 py-1 text-[10px]">
          <CheckCircle2 className="size-3.5 text-status-ok" />
          <span className="font-medium text-status-ok">EULA accepted</span>
          <span className="text-muted-foreground">
            {discovery?.eula ? acceptedAt(discovery.eula.acceptedAt) : ""}
          </span>
          <a
            className="underline underline-offset-2"
            href={EULA_URL}
            target="_blank"
            rel="noreferrer"
          >
            EULA text
          </a>
          <Button
            variant="ghost"
            size="sm"
            className="ml-auto h-6 px-1.5"
            onClick={() => void withdrawEula()}
            disabled={busy !== null || running}
            aria-label="Withdraw EULA acceptance"
          >
            <Trash2 /> Withdraw
          </Button>
        </div>
      )}

      <div className="grid grid-cols-2 gap-1.5 sm:grid-cols-3">
        <label className="space-y-0.5 text-[10px]">
          <span className="text-muted-foreground">Seed</span>
          <input
            className="h-6 w-full rounded border border-border bg-background px-1.5 font-mono text-[10px]"
            value={seed}
            maxLength={64}
            onChange={(event) => setSeed(event.target.value)}
            disabled={running}
            aria-label="Generation seed"
          />
        </label>
        <label className="space-y-0.5 text-[10px]">
          <span className="text-muted-foreground">Preset</span>
          <select
            className="h-6 w-full rounded border border-border bg-background px-1 text-[10px]"
            value={preset}
            onChange={(event) => setPreset(event.target.value as WorldPreset)}
            disabled={running}
            aria-label="World preset"
          >
            <option value="normal">normal</option>
            <option value="flat">flat</option>
            <option value="large_biomes">large biomes</option>
            <option value="amplified">amplified</option>
          </select>
        </label>
        <label className="space-y-0.5 text-[10px]">
          <span className="text-muted-foreground">Radius, chunks</span>
          <input
            type="number"
            min={1}
            max={16}
            className="h-6 w-full rounded border border-border bg-background px-1.5 font-mono text-[10px]"
            value={radius}
            onChange={(event) => setRadius(event.target.value)}
            disabled={running}
            aria-label="Generation radius in chunks"
          />
        </label>
        <label className="space-y-0.5 text-[10px]">
          <span className="text-muted-foreground">Center X</span>
          <input
            type="number"
            className="h-6 w-full rounded border border-border bg-background px-1.5 font-mono text-[10px]"
            value={centerX}
            onChange={(event) => setCenterX(event.target.value)}
            disabled={running}
            aria-label="Generation center X"
          />
        </label>
        <label className="space-y-0.5 text-[10px]">
          <span className="text-muted-foreground">Center Z</span>
          <input
            type="number"
            className="h-6 w-full rounded border border-border bg-background px-1.5 font-mono text-[10px]"
            value={centerZ}
            onChange={(event) => setCenterZ(event.target.value)}
            disabled={running}
            aria-label="Generation center Z"
          />
        </label>
        <label className="space-y-0.5 text-[10px]">
          <span className="text-muted-foreground">Time limit, s</span>
          <input
            type="number"
            min={10}
            max={3600}
            className="h-6 w-full rounded border border-border bg-background px-1.5 font-mono text-[10px]"
            value={timeLimit}
            onChange={(event) => setTimeLimit(event.target.value)}
            disabled={running}
            aria-label="Generation time limit seconds"
          />
        </label>
        <label className="space-y-0.5 text-[10px]">
          <span className="text-muted-foreground">Memory, MiB</span>
          <input
            type="number"
            min={512}
            max={4096}
            step={256}
            className="h-6 w-full rounded border border-border bg-background px-1.5 font-mono text-[10px]"
            value={memory}
            onChange={(event) => setMemory(event.target.value)}
            disabled={running}
            aria-label="Generation memory limit"
          />
        </label>
        <label className="space-y-0.5 text-[10px]">
          <span className="text-muted-foreground">View distance</span>
          <input
            type="number"
            min={2}
            max={8}
            className="h-6 w-full rounded border border-border bg-background px-1.5 font-mono text-[10px]"
            value={viewDistance}
            onChange={(event) => setViewDistance(event.target.value)}
            disabled={running}
            aria-label="Generation view distance"
          />
        </label>
        <label className="space-y-0.5 text-[10px]">
          <span className="text-muted-foreground">CPU threads</span>
          <input
            type="number"
            min={1}
            max={8}
            className="h-6 w-full rounded border border-border bg-background px-1.5 font-mono text-[10px]"
            value={cpuThreads}
            onChange={(event) => setCpuThreads(event.target.value)}
            disabled={running}
            aria-label="Generation CPU threads"
          />
        </label>
      </div>

      <div className="flex flex-wrap items-center gap-1.5">
        <Button
          size="sm"
          onClick={() => void start()}
          disabled={busy !== null || running || !eulaAccepted || !selected?.ready}
          aria-label="Start isolated generation"
        >
          {busy === "start" ? <Loader2 className="animate-spin" /> : <Play />}
          Start isolated generation
        </Button>
        <Button
          variant="outline"
          size="sm"
          onClick={() => void cancel()}
          disabled={busy !== null || !running}
          aria-label="Cancel generation"
        >
          {busy === "cancel" ? <Loader2 className="animate-spin" /> : <Ban />}
          Cancel
        </Button>
        <span className="flex items-center gap-1 text-[10px] text-muted-foreground">
          <ShieldCheck className="size-3" /> loopback only · {memory} MiB · {timeLimit}s
        </span>
      </div>

      {active ? (
        <div className="space-y-1 rounded border border-border p-2 text-[10px]">
          <div className="flex flex-wrap items-center gap-x-2 gap-y-0.5">
            <span
              className={cn(
                "rounded px-1.5 py-0.5 font-mono text-[10px]",
                running
                  ? "bg-primary/15 text-primary"
                  : active.phase === "completed"
                    ? "bg-status-ok/15 text-status-ok"
                    : "bg-status-warn/15 text-status-warn",
              )}
            >
              {active.phase}
            </span>
            <span className="font-medium">{active.message}</span>
            <span className="ml-auto font-mono text-muted-foreground">
              {formatSeconds(active.elapsedMs)}
            </span>
          </div>
          <div className="flex flex-wrap gap-x-3 gap-y-0.5 font-mono text-[10px] text-muted-foreground">
            <span>
              chunks {active.generatedChunks}/{active.expectedChunks}
            </span>
            <span>regions {active.regionFiles}</span>
            <span>{formatBytes(active.worldBytes)}</span>
            <span className="truncate">run {active.runId}</span>
          </div>
          <div className="h-1 overflow-hidden rounded bg-muted">
            <div
              className="h-full bg-primary transition-[width]"
              style={{ width: `${progress ?? 8}%` }}
            />
          </div>
          {active.error ? <div className="text-status-warn">{active.error}</div> : null}
        </div>
      ) : null}

      {last && !running ? (
        <details className="rounded border border-border p-2 text-[10px]" open>
          <summary className="cursor-pointer font-medium">Last isolated run</summary>
          <div className="mt-1 flex flex-wrap gap-x-3 gap-y-0.5">
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
          <div className="mt-0.5 flex flex-wrap gap-x-3 gap-y-0.5 font-mono text-[10px] text-muted-foreground">
            <span>seed {last.seed}</span>
            <span>preset {last.preset}</span>
            <span>
              center {last.centerX},{last.centerZ}
            </span>
            <span>bind {last.bindAddress ?? "unknown"}</span>
            <span>java {last.javaVersion}</span>
          </div>
          <div className="mt-0.5 truncate font-mono text-[10px] text-muted-foreground">
            {displayRuntimePath(last.worldDir)}
          </div>
          <div className="truncate font-mono text-[10px] text-muted-foreground">
            {displayRuntimePath(last.logPath)}
          </div>
          {last.snapshot.error ? (
            <div className="mt-0.5 text-status-warn">{last.snapshot.error}</div>
          ) : null}
          {last.logTail.length > 0 ? (
            <pre className="mt-1 max-h-24 overflow-auto rounded bg-muted/40 p-1 font-mono text-[9px] whitespace-pre-wrap">
              {last.logTail.slice(-12).join("\n")}
            </pre>
          ) : null}
        </details>
      ) : null}

      <div className="flex flex-wrap items-center gap-1.5">
        <Button
          variant="outline"
          size="sm"
          onClick={() => void clearRuns()}
          disabled={busy !== null || running || (runs?.runs.length ?? 0) === 0}
          aria-label="Clear generation runs"
        >
          <Trash2 /> Clear generation runs
        </Button>
        <span className="text-[10px] text-muted-foreground">
          {runs?.runs.length ?? 0} run(s) · {formatBytes(runs?.totalBytes ?? 0)}
        </span>
      </div>
      {runs?.runs.slice(0, 3).map((run) => (
        <div
          key={run.runId}
          className="truncate rounded border border-border/60 px-2 py-1 font-mono text-[10px]"
          title={run.path}
        >
          {run.runId} · {run.status} · {run.chunks}/{run.expectedChunks} chunks ·{" "}
          {formatBytes(run.worldBytes)}
        </div>
      ))}
      {runs?.problems.map((problem) => (
        <div key={problem} className="text-[10px] text-status-warn">
          {problem}
        </div>
      ))}

      {error ? (
        <p className="rounded border border-status-warn/40 bg-status-warn/10 px-2 py-1 text-[10px] text-status-warn">
          {error}
        </p>
      ) : null}
      {notice ? (
        <p className="rounded border border-status-ok/40 bg-status-ok/10 px-2 py-1 text-[10px] text-status-ok">
          {notice}
        </p>
      ) : null}
    </div>
  );
}
