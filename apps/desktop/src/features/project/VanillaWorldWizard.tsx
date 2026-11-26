import { useCallback, useEffect, useState } from "react";
import {
  Ban,
  CheckCircle2,
  Coffee,
  FolderOpen,
  Gauge,
  Loader2,
  Play,
  RefreshCw,
  Server,
  ShieldCheck,
  Trash2,
  TriangleAlert,
} from "lucide-react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Progress } from "@/components/ui/progress";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { ComponentCard } from "@/features/components";
import { displayRuntimePath, formatBytes } from "@/lib/runtime-api";
import {
  acceptServerEula,
  cancelServerGeneration,
  inspectServerDatapacks,
  serverGenerationEstimate,
  serverGenerationStatus,
  startServerGeneration,
  vanillaSetupCheck,
} from "@/lib/server-api";
import { cn } from "@/lib/utils";
import type { ComponentMode, ComponentState, ComponentStatus } from "@/lib/component-types";
import type {
  ComponentStepItem,
  ComponentStepSummary,
} from "@/features/project/ProjectComponentsStep";
import type {
  DatapackInspection,
  GenerationEstimate,
  GenerationReport,
  GenerationRequest,
  GenerationSnapshot,
  GenerationStatusReport,
  VanillaSetupReport,
  WorldPreset,
} from "@/runtime/server-types";

const EULA_URL = "https://aka.ms/MinecraftEULA";

const WORLD_PRESETS: { value: WorldPreset; label: string }[] = [
  { value: "normal", label: "normal" },
  { value: "flat", label: "flat" },
  { value: "large_biomes", label: "large biomes" },
  { value: "amplified", label: "amplified" },
];

type Busy = "load" | "eula" | "datapacks" | "start" | "cancel" | "import" | null;

interface VanillaWorldWizardProps {
  /** Verified profile selected for this project; generation is bound to exactly this version. */
  profileId?: string | undefined;
  /** Verified Java/assets/server facts from the Components step. Display only. */
  components?: ComponentStepSummary | null;
  onOpenWorld?: (runId: string, name: string) => Promise<boolean>;
}

function summaryStatus(item: ComponentStepItem, kind: "java" | "server"): ComponentStatus {
  return {
    kind,
    required: true,
    ...(item.mode ? { mode: item.mode } : {}),
    state: item.state,
    ...(item.version ? { version: item.version } : {}),
    ...(item.bytes !== null ? { bytes: item.bytes } : {}),
    ...(item.hash
      ? { hash: { algorithm: kind === "java" ? "sha256" : "sha1", value: item.hash } }
      : {}),
    ...(item.path ? { path: item.path } : {}),
  };
}

function runtimeState(ready: boolean, status: string): ComponentState {
  if (ready) return "ready";
  switch (status) {
    case "unsupported_arch":
    case "not_java":
    case "outdated":
      return "incompatible";
    case "unreadable":
      return "error";
    default:
      return "unverified";
  }
}

function setupJavaStatus(setup: VanillaSetupReport | null): ComponentStatus {
  if (!setup?.java) return { kind: "java", required: true, state: "missing" };
  const mode: ComponentMode = setup.java.managed ? "managed" : "manual";
  return {
    kind: "java",
    required: true,
    mode,
    state: runtimeState(setup.javaReady, setup.java.status),
    ...(setup.java.version ? { version: setup.java.version } : {}),
    path: setup.java.path,
    ...(setup.java.error ? { error: setup.java.error } : {}),
    ...(setup.java.warnings.length ? { notes: setup.java.warnings } : {}),
  };
}

function setupServerStatus(setup: VanillaSetupReport | null): ComponentStatus {
  if (!setup?.server) return { kind: "server", required: true, state: "missing" };
  return {
    kind: "server",
    required: true,
    mode: setup.server.managed ? "managed" : "manual",
    state: setup.serverReady ? "ready" : "incompatible",
    ...(setup.server.version ? { version: setup.server.version } : {}),
    path: setup.server.path,
    ...(setup.server.sha1 ? { hash: { algorithm: "sha1", value: setup.server.sha1 } } : {}),
    ...(setup.server.size !== null ? { bytes: setup.server.size } : {}),
    ...(setup.server.error ? { error: setup.server.error } : {}),
    ...(setup.server.warnings.length ? { notes: setup.server.warnings } : {}),
  };
}

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

function seconds(milliseconds: number): string {
  return `${Math.max(0, Math.round(milliseconds / 1000))}s`;
}

function statusTone(ready: boolean): string {
  return ready
    ? "border-status-ok/40 bg-status-ok/5 text-status-ok"
    : "border-status-warn/40 bg-status-warn/5 text-status-warn";
}

export function VanillaWorldWizard({
  profileId,
  components = null,
  onOpenWorld,
}: VanillaWorldWizardProps) {
  const [setup, setSetup] = useState<VanillaSetupReport | null>(null);
  const [status, setStatus] = useState<GenerationStatusReport | null>(null);
  const [estimate, setEstimate] = useState<GenerationEstimate | null>(null);
  const [estimateError, setEstimateError] = useState<string | null>(null);
  const [busy, setBusy] = useState<Busy>("load");
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [eulaChecked, setEulaChecked] = useState(false);
  const [consentChecked, setConsentChecked] = useState(false);
  const [seed, setSeed] = useState("FuryBuilder");
  const [preset, setPreset] = useState<WorldPreset>("normal");
  const [generateStructures, setGenerateStructures] = useState(true);
  const [centerX, setCenterX] = useState("0");
  const [centerZ, setCenterZ] = useState("0");
  const [radius, setRadius] = useState("1");
  const [timeLimit, setTimeLimit] = useState("120");
  const [memory, setMemory] = useState("2048");
  const [viewDistance, setViewDistance] = useState("3");
  const [cpuThreads, setCpuThreads] = useState("2");
  const [datapacks, setDatapacks] = useState<DatapackInspection[]>([]);

  const refresh = useCallback(async () => {
    if (!profileId) {
      setSetup(null);
      setStatus(null);
      return;
    }
    const [nextSetup, nextStatus] = await Promise.all([
      vanillaSetupCheck(profileId),
      serverGenerationStatus(),
    ]);
    setSetup(nextSetup);
    setStatus(nextStatus);
  }, [profileId]);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        await refresh();
      } catch (cause) {
        if (!cancelled) {
          setError(cause instanceof Error ? cause.message : String(cause));
        }
      } finally {
        if (!cancelled) setBusy(null);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [refresh]);

  useEffect(() => {
    if (!status?.active?.running) return;
    const timer = window.setInterval(() => {
      void serverGenerationStatus()
        .then((next) => setStatus(next))
        .catch(() => undefined);
    }, 1000);
    return () => window.clearInterval(timer);
  }, [status?.active?.running]);

  const radiusValue = Number(radius);
  const timeLimitValue = Number(timeLimit);
  useEffect(() => {
    if (!setup) return;
    if (!Number.isInteger(radiusValue) || !Number.isInteger(timeLimitValue)) return;
    let cancelled = false;
    const timer = window.setTimeout(() => {
      void serverGenerationEstimate(radiusValue, timeLimitValue)
        .then((value) => {
          if (cancelled) return;
          setEstimate(value);
          setEstimateError(null);
        })
        .catch((cause) => {
          if (cancelled) return;
          setEstimateError(cause instanceof Error ? cause.message : String(cause));
        });
    }, 150);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [setup, radiusValue, timeLimitValue]);

  const active: GenerationSnapshot | null = status?.active ?? null;
  const last = status?.last && status.last.profileId === profileId ? status.last : null;
  const running = Boolean(active?.running);
  const limits = setup?.generationLimits ?? null;
  const targetMismatch = Boolean(profileId) && setup !== null && setup.profileId !== profileId;
  const currentEstimate =
    estimate &&
    estimate.radiusChunks === radiusValue &&
    estimate.timeLimitSeconds === timeLimitValue
      ? estimate
      : null;
  const diskBlocked = currentEstimate !== null && !currentEstimate.fitsOnDisk;
  const setupLocked = busy !== null || !profileId || targetMismatch;
  const availablePresets = setup?.availablePresets ?? [];
  const presetOptions = WORLD_PRESETS.filter((entry) => availablePresets.includes(entry.value));
  const activePreset = availablePresets.includes(preset) ? preset : (availablePresets[0] ?? preset);
  const javaStatus = components?.java
    ? summaryStatus(components.java, "java")
    : setupJavaStatus(setup);
  const serverStatus = components?.server
    ? summaryStatus(components.server, "server")
    : setupServerStatus(setup);
  const javaLabel = components?.java?.version ?? setup?.java?.version ?? "not selected";
  const serverLabel = components?.server?.version ?? setup?.server?.version ?? "not selected";
  const validDatapacks = datapacks.filter((datapack) => datapack.valid);
  const invalidDatapacks = datapacks.filter((datapack) => !datapack.valid);
  const progress =
    active && active.expectedChunks > 0
      ? Math.min(100, Math.round((active.generatedChunks / active.expectedChunks) * 100))
      : null;

  async function runBusy(kind: Busy, action: () => Promise<void>) {
    setBusy(kind);
    setError(null);
    setNotice(null);
    try {
      await action();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(null);
    }
  }

  async function reloadSetup() {
    await runBusy("load", async () => {
      await refresh();
    });
  }

  async function acceptEula() {
    if (!eulaChecked || !profileId) return;
    await runBusy("eula", async () => {
      const eula = await acceptServerEula(profileId, true);
      setEulaChecked(false);
      setNotice(
        `EULA accepted for Minecraft ${eula.minecraftVersion} (${eula.jarSha1.slice(0, 12)}…).`,
      );
      await refresh();
    });
  }

  async function browseDatapacks() {
    if (!profileId) return;
    await runBusy("datapacks", async () => {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = await open({
        title: "Choose local data pack .zip files",
        multiple: true,
        directory: false,
        filters: [{ name: "Minecraft data pack", extensions: ["zip"] }],
      });
      const paths = (Array.isArray(selected) ? selected : selected ? [selected] : []).filter(
        (path): path is string => typeof path === "string" && path.length > 0,
      );
      if (paths.length === 0) return;
      const existing = new Set(datapacks.map((datapack) => datapack.path));
      const fresh = paths.filter((path) => !existing.has(path));
      if (fresh.length === 0) return;
      const inspected = await inspectServerDatapacks(profileId, fresh);
      setDatapacks((current) => [...current, ...inspected]);
      const rejected = inspected.filter((datapack) => !datapack.valid).length;
      setNotice(
        rejected === 0
          ? `${inspected.length} data pack(s) validated.`
          : `${inspected.length - rejected} data pack(s) validated; ${rejected} rejected.`,
      );
    });
  }

  async function start() {
    if (!profileId) {
      setError("Choose a Minecraft version before starting Vanilla generation.");
      return;
    }
    if (diskBlocked && currentEstimate) {
      setError(
        `Not enough free disk space: ${formatBytes(currentEstimate.requiredDiskBytes)} required, ${formatBytes(currentEstimate.availableDiskBytes)} available.`,
      );
      return;
    }
    const request: GenerationRequest = {
      seed: seed.trim(),
      preset: activePreset,
      centerX: Number(centerX),
      centerZ: Number(centerZ),
      radiusChunks: Number(radius),
      timeLimitSeconds: Number(timeLimit),
      memoryMb: Number(memory),
      viewDistance: Number(viewDistance),
      cpuThreads: Number(cpuThreads),
      generateStructures,
      consent: consentChecked,
      datapacks: validDatapacks.map((datapack) => ({ path: datapack.path })),
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
    if (!request.consent) {
      setError("Confirm that the official server may run locally for this generation.");
      return;
    }
    await runBusy("start", async () => {
      const snapshot = await startServerGeneration(profileId, request);
      setStatus((current) => ({ active: snapshot, last: current?.last ?? null }));
      setConsentChecked(false);
      setNotice(
        "Isolated generation started. The server is loopback-only and the process tree is contained.",
      );
    });
  }

  async function cancel() {
    await runBusy("cancel", async () => {
      const requested = await cancelServerGeneration();
      setNotice(
        requested
          ? "Cancellation requested. The whole Java process tree will be terminated."
          : "No active generation run.",
      );
      await refresh();
    });
  }

  async function openGeneratedWorld() {
    if (!last || !last.worldComplete || !onOpenWorld) return;
    const name = `Vanilla ${last.seed}`;
    await runBusy("import", async () => {
      const opened = await onOpenWorld(last.snapshot.runId, name);
      if (!opened) {
        throw new Error("The generated world could not be opened as a project.");
      }
      setNotice(
        "Generated world imported. The generated folder is kept read-only and edits use the project overlay.",
      );
    });
  }

  return (
    <Card className="gap-0 py-0">
      <CardContent className="space-y-6 p-6">
        <div className="flex items-center justify-between gap-2">
          <div className="flex items-center gap-2 text-base font-semibold">
            <Server className="size-3.5" /> Vanilla World setup
          </div>
          <Button
            variant="ghost"
            size="sm"
            onClick={() => void reloadSetup()}
            disabled={busy !== null}
            aria-label="Refresh Vanilla World setup"
          >
            {busy === "load" ? <Loader2 className="animate-spin" /> : <RefreshCw />}
          </Button>
        </div>
        <p className="text-sm leading-relaxed text-muted-foreground">
          The world is generated by Mojang&apos;s official Minecraft server, not by a FuryBuilder
          generator. Target: Minecraft Java{" "}
          <span className="font-mono">{setup?.targetMinecraftVersion ?? "unknown"}</span> · DV{" "}
          <span className="font-mono">{setup?.targetDataVersion ?? "unknown"}</span> · Java{" "}
          <span className="font-mono">{setup?.targetJavaMajor ?? "unknown"}</span> · level-type{" "}
          <span className="font-mono">{setup?.levelTypeSyntax ?? "unknown"}</span>.
        </p>

        {!profileId ? (
          <Alert variant="destructive">
            <TriangleAlert />
            <AlertTitle>Choose a version first</AlertTitle>
            <AlertDescription>
              Vanilla generation is bound to the exact project version. Return to the Version step
              and select a supported release.
            </AlertDescription>
          </Alert>
        ) : null}

        {setup && !setup.generationSupported ? (
          <Alert variant="destructive">
            <TriangleAlert />
            <AlertTitle>Generation is not available for this version</AlertTitle>
            <AlertDescription>
              {setup.blockers[0] ??
                `This build cannot generate a Vanilla World for Minecraft ${setup.targetMinecraftVersion}.`}{" "}
              Create an Empty, Flat or Imported project instead.
            </AlertDescription>
          </Alert>
        ) : null}

        <div className={cn("space-y-4 rounded-xl border p-4", statusTone(Boolean(setup?.ready)))}>
          <div className="flex flex-wrap items-center gap-2 text-sm">
            {setup?.ready ? (
              <CheckCircle2 className="size-3.5" />
            ) : (
              <TriangleAlert className="size-3.5" />
            )}
            <span className="font-semibold">
              {setup?.ready ? "Ready to generate" : "Setup is incomplete"}
            </span>
            <span className="text-muted-foreground">
              Java {javaLabel} · JAR {serverLabel} ·{" "}
              {setup?.eulaMatchesSelected ? "EULA accepted" : "EULA required"}
            </span>
          </div>

          <div className="grid gap-4 md:grid-cols-2">
            <ComponentCard
              icon={<Coffee className="size-4" />}
              title={`Java ${setup?.targetJavaMajor ?? ""}`}
              description="Prepared and verified in the Components step. Go back there to change the runtime; generation never selects a different Java."
              status={javaStatus}
            />
            <ComponentCard
              icon={<Server className="size-4" />}
              title="Official server JAR"
              description="Prepared and verified in the Components step. Go back there to change the JAR; generation only runs the selected exact-version server."
              status={serverStatus}
            />
          </div>

          <div className="space-y-3 rounded-xl border bg-muted/30 p-4">
            <div className="flex items-center gap-2 text-sm font-medium">
              <ShieldCheck className="size-4" /> Minecraft EULA
            </div>
            {setup?.eulaMatchesSelected ? (
              <div className="text-xs text-status-ok">
                Accepted for this exact server JAR and Minecraft version.
              </div>
            ) : (
              <div className="space-y-1">
                <a
                  className="text-sm underline underline-offset-2"
                  href={EULA_URL}
                  target="_blank"
                  rel="noreferrer"
                >
                  Read the official EULA
                </a>
                <label className="flex items-start gap-2 text-sm">
                  <Checkbox
                    className="mt-0.5"
                    checked={eulaChecked}
                    onCheckedChange={(checked) => setEulaChecked(checked === true)}
                    disabled={setupLocked}
                    aria-label="I have read and accept the Minecraft EULA"
                  />
                  <span>I accept the Minecraft EULA for this JAR and version.</span>
                </label>
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() => void acceptEula()}
                  disabled={!eulaChecked || setupLocked || !setup?.serverReady}
                  aria-label="Accept EULA"
                >
                  {busy === "eula" ? <Loader2 className="animate-spin" /> : <CheckCircle2 />}
                  Accept
                </Button>
              </div>
            )}
          </div>

          {setup?.blockers.length ? (
            <div className="space-y-1 text-sm">
              {setup.blockers.map((blocker) => (
                <div key={blocker}>• {blocker}</div>
              ))}
            </div>
          ) : null}
        </div>

        <div className="grid grid-cols-2 gap-4 sm:grid-cols-4">
          <label className="space-y-1.5 text-sm">
            <span className="text-muted-foreground">Seed</span>
            <Input
              className="font-mono"
              value={seed}
              maxLength={64}
              onChange={(event) => setSeed(event.target.value)}
              disabled={running || setupLocked}
              aria-label="Vanilla generation seed"
            />
          </label>
          <label className="space-y-1.5 text-sm">
            <span className="text-muted-foreground">Preset</span>
            <Select
              items={presetOptions}
              value={activePreset}
              onValueChange={(value) => setPreset(value as WorldPreset)}
              disabled={running || setupLocked || presetOptions.length === 0}
            >
              <SelectTrigger className="w-full" aria-label="Vanilla world preset">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {presetOptions.map((entry) => (
                  <SelectItem key={entry.value} value={entry.value}>
                    {entry.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </label>
          <label className="space-y-1.5 text-sm">
            <span className="text-muted-foreground">Radius, chunks</span>
            <Input
              type="number"
              min={limits?.minRadiusChunks ?? 1}
              max={limits?.maxRadiusChunks ?? 16}
              className="font-mono"
              value={radius}
              onChange={(event) => setRadius(event.target.value)}
              disabled={running || setupLocked}
              aria-label="Vanilla generation radius in chunks"
            />
          </label>
          <label className="flex items-end gap-2 pb-1 text-sm">
            <Checkbox
              checked={generateStructures}
              onCheckedChange={(checked) => setGenerateStructures(checked === true)}
              disabled={running || setupLocked}
              aria-label="Generate structures"
            />
            <span>Generate structures</span>
          </label>
          <label className="space-y-1.5 text-sm">
            <span className="text-muted-foreground">Center X</span>
            <Input
              type="number"
              className="font-mono"
              value={centerX}
              onChange={(event) => setCenterX(event.target.value)}
              disabled={running || setupLocked}
              aria-label="Vanilla generation center X"
            />
          </label>
          <label className="space-y-1.5 text-sm">
            <span className="text-muted-foreground">Center Z</span>
            <Input
              type="number"
              className="font-mono"
              value={centerZ}
              onChange={(event) => setCenterZ(event.target.value)}
              disabled={running || setupLocked}
              aria-label="Vanilla generation center Z"
            />
          </label>
          <label className="space-y-1.5 text-sm">
            <span className="text-muted-foreground">Time limit, s</span>
            <Input
              type="number"
              min={limits?.minTimeLimitSeconds ?? 10}
              max={limits?.maxTimeLimitSeconds ?? 3600}
              className="font-mono"
              value={timeLimit}
              onChange={(event) => setTimeLimit(event.target.value)}
              disabled={running || setupLocked}
              aria-label="Vanilla generation time limit seconds"
            />
          </label>
          <label className="space-y-1.5 text-sm">
            <span className="text-muted-foreground">Memory, MiB</span>
            <Input
              type="number"
              min={limits?.minMemoryMb ?? 512}
              max={limits?.maxMemoryMb ?? 4096}
              step={256}
              className="font-mono"
              value={memory}
              onChange={(event) => setMemory(event.target.value)}
              disabled={running || setupLocked}
              aria-label="Vanilla generation memory limit"
            />
          </label>
          <label className="space-y-1.5 text-sm">
            <span className="text-muted-foreground">View distance</span>
            <Input
              type="number"
              min={limits?.minViewDistance ?? 2}
              max={limits?.maxViewDistance ?? 8}
              className="font-mono"
              value={viewDistance}
              onChange={(event) => setViewDistance(event.target.value)}
              disabled={running || setupLocked}
              aria-label="Vanilla generation view distance"
            />
          </label>
          <label className="space-y-1.5 text-sm">
            <span className="text-muted-foreground">CPU threads</span>
            <Input
              type="number"
              min={limits?.minCpuThreads ?? 1}
              max={limits?.maxCpuThreads ?? 8}
              className="font-mono"
              value={cpuThreads}
              onChange={(event) => setCpuThreads(event.target.value)}
              disabled={running || setupLocked}
              aria-label="Vanilla generation CPU threads"
            />
          </label>
        </div>

        <div className="space-y-3 rounded-xl border bg-muted/30 p-4">
          <div className="flex flex-wrap items-center gap-2">
            <span className="flex items-center gap-2 text-sm font-medium">
              <Gauge className="size-4" /> Generation estimate
            </span>
            {currentEstimate ? (
              <Badge variant={currentEstimate.fitsOnDisk ? "secondary" : "destructive"}>
                {currentEstimate.fitsOnDisk ? "Fits on disk" : "Not enough disk"}
              </Badge>
            ) : null}
          </div>
          {estimateError ? (
            <div className="text-sm text-status-warn">
              The disk estimate is unavailable: {estimateError}. The backend still checks free space
              before any server process starts.
            </div>
          ) : currentEstimate ? (
            <>
              <div className="grid grid-cols-2 gap-3 text-sm sm:grid-cols-4">
                <div>
                  <div className="text-xs text-muted-foreground">Expected area</div>
                  <div className="font-mono">
                    {currentEstimate.expectedChunks.toLocaleString()} chunks
                  </div>
                </div>
                <div>
                  <div className="text-xs text-muted-foreground">Estimated world</div>
                  <div className="font-mono">
                    {formatBytes(currentEstimate.estimatedWorldBytes)}
                  </div>
                </div>
                <div>
                  <div className="text-xs text-muted-foreground">Required free space</div>
                  <div className="font-mono">{formatBytes(currentEstimate.requiredDiskBytes)}</div>
                </div>
                <div>
                  <div className="text-xs text-muted-foreground">Free on run volume</div>
                  <div className="font-mono">{formatBytes(currentEstimate.availableDiskBytes)}</div>
                </div>
              </div>
              <p className="text-xs leading-relaxed text-muted-foreground">
                Generation is bounded by the {currentEstimate.timeLimitSeconds}s time limit; the
                server stops after it even if the requested area is incomplete. The world estimate
                is conservative and includes a safety margin.
              </p>
              {!currentEstimate.fitsOnDisk ? (
                <div className="text-sm text-status-warn">
                  Free at least {formatBytes(currentEstimate.requiredDiskBytes)} on the volume that
                  holds the isolated server runs, or reduce the radius.
                </div>
              ) : null}
            </>
          ) : (
            <p className="text-sm text-muted-foreground">
              Checking the requested radius and the run volume…
            </p>
          )}
        </div>

        <div className="space-y-3 rounded-xl border bg-muted/30 p-4">
          <div className="flex flex-wrap items-center gap-2">
            <span className="text-sm font-medium">Data packs</span>
            <span className="text-sm text-muted-foreground">
              Format <span className="font-mono">{setup?.serverDatapackFormat ?? "unknown"}</span> ·
              local .zip files are validated and copied into the isolated world only
            </span>
            <Button
              size="sm"
              variant="outline"
              className="ml-auto"
              onClick={() => void browseDatapacks()}
              disabled={
                busy !== null ||
                running ||
                setupLocked ||
                datapacks.length >= (limits?.maxDatapacks ?? 16)
              }
              aria-label="Add data packs"
            >
              {busy === "datapacks" ? <Loader2 className="animate-spin" /> : <FolderOpen />}
              Add
            </Button>
          </div>
          {datapacks.length === 0 ? (
            <div className="text-sm text-muted-foreground">
              No data packs. Vanilla generation runs with the official data pack only.
            </div>
          ) : (
            <div className="space-y-0.5">
              {datapacks.map((datapack) => (
                <div
                  key={datapack.path}
                  className={cn(
                    "flex items-center gap-2 rounded-lg border bg-card px-3 py-1.5 font-mono text-xs",
                    datapack.valid ? "bg-background/60" : "bg-status-warn/10 text-status-warn",
                  )}
                  title={datapack.path}
                >
                  <span className="truncate">{datapack.name}</span>
                  {datapack.valid ? (
                    <span className="ml-auto shrink-0 text-muted-foreground">
                      format {datapack.packFormat} ·{" "}
                      {datapack.size ? formatBytes(datapack.size) : "?"} ·{" "}
                      {datapack.sha256?.slice(0, 8)}…
                    </span>
                  ) : (
                    <span className="ml-auto max-w-64 truncate">{datapack.error}</span>
                  )}
                  <Button
                    variant="ghost"
                    size="sm"
                    className="shrink-0"
                    onClick={() =>
                      setDatapacks((current) =>
                        current.filter((item) => item.path !== datapack.path),
                      )
                    }
                    disabled={busy !== null || running}
                    aria-label={`Remove data pack ${datapack.name}`}
                  >
                    <Trash2 />
                  </Button>
                </div>
              ))}
            </div>
          )}
          {invalidDatapacks.length > 0 ? (
            <div className="text-sm text-status-warn">
              Remove invalid data packs before starting generation.
            </div>
          ) : null}
        </div>

        <label className="flex items-start gap-3 rounded-xl border bg-muted/30 p-4 text-sm">
          <Checkbox
            className="mt-0.5"
            checked={consentChecked}
            onCheckedChange={(checked) => setConsentChecked(checked === true)}
            disabled={running || setupLocked}
            aria-label="Confirm local official server run"
          />
          <span>
            I explicitly allow FuryBuilder to start the selected official server JAR with the
            selected Java {setup?.targetJavaMajor ?? ""} runtime for this run. It listens on
            loopback only; no external player can connect.
          </span>
        </label>

        <div className="flex flex-wrap items-center gap-2">
          <Button
            size="sm"
            onClick={() => void start()}
            disabled={
              busy !== null ||
              running ||
              setupLocked ||
              diskBlocked ||
              !setup?.ready ||
              !consentChecked ||
              invalidDatapacks.length > 0
            }
            aria-label="Start Vanilla World generation"
          >
            {busy === "start" ? <Loader2 className="animate-spin" /> : <Play />}
            Generate Vanilla World
          </Button>
          <Button
            variant="outline"
            size="sm"
            onClick={() => void cancel()}
            disabled={busy !== null || !running}
            aria-label="Cancel Vanilla World generation"
          >
            {busy === "cancel" ? <Loader2 className="animate-spin" /> : <Ban />}
            Cancel
          </Button>
          <span className="flex items-center gap-2 text-sm text-muted-foreground">
            <ShieldCheck className="size-4" /> loopback only · process job guarded ·{" "}
            {validDatapacks.length} data pack(s)
          </span>
        </div>

        {active ? (
          <div className="space-y-2 rounded-xl border bg-card p-4 text-sm">
            <div className="flex flex-wrap items-center gap-2">
              <Badge
                variant="secondary"
                className={cn(
                  "font-mono",
                  running
                    ? "bg-primary/15 text-primary"
                    : active.phase === "completed"
                      ? "bg-status-ok/15 text-status-ok"
                      : "bg-status-warn/15 text-status-warn",
                )}
              >
                {active.phase}
              </Badge>
              <span className="font-medium">{active.message}</span>
              <span className="ml-auto font-mono text-muted-foreground">
                {seconds(active.elapsedMs)}
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
            {active.error ? <div className="text-status-warn">{active.error}</div> : null}
          </div>
        ) : null}

        {last && !running ? (
          <div className="space-y-2 rounded-xl border bg-card p-4 text-sm">
            <div className="flex flex-wrap items-center gap-x-2 gap-y-0.5">
              <span className="font-medium">Last isolated generation</span>
              <span
                className={cn(
                  "font-mono",
                  last.worldComplete ? "text-status-ok" : "text-status-warn",
                )}
              >
                {stopReasonLabel(last)}
              </span>
              <span className="ml-auto font-mono text-muted-foreground">
                {seconds(last.snapshot.elapsedMs)}
              </span>
            </div>
            <div className="flex flex-wrap gap-x-4 gap-y-1 text-sm">
              <span>
                Minecraft <span className="font-mono">{last.minecraftVersion}</span>
              </span>
              <span>
                seed <span className="font-mono">{last.seed}</span>
              </span>
              <span>
                preset <span className="font-mono">{last.preset}</span>
              </span>
              <span>
                structures <span className="font-mono">{String(last.generateStructures)}</span>
              </span>
              <span>
                chunks <span className="font-mono">{last.snapshot.generatedChunks}</span>/
                <span className="font-mono">{last.snapshot.expectedChunks}</span>
              </span>
              <span>
                regions <span className="font-mono">{last.snapshot.regionFiles}</span>
              </span>
              <span>
                bind <span className="font-mono">{last.bindAddress ?? "unknown"}</span>
              </span>
              <span>
                guard{" "}
                <span className="font-mono">
                  {last.processTreeGuarded ? "job-object" : "fallback"}
                </span>
              </span>
              <span>
                data packs <span className="font-mono">{last.datapacks.length}</span>
              </span>
            </div>
            <div className="truncate font-mono text-xs text-muted-foreground">
              {displayRuntimePath(last.worldDir)}
            </div>
            {last.snapshot.error ? (
              <div className="text-status-warn">{last.snapshot.error}</div>
            ) : null}
            <div className="flex flex-wrap items-center gap-2">
              <Button
                size="sm"
                variant="outline"
                onClick={() => void openGeneratedWorld()}
                disabled={busy !== null || !last.worldComplete || !onOpenWorld}
                aria-label="Open generated world as a project"
              >
                {busy === "import" ? <Loader2 className="animate-spin" /> : <FolderOpen />}
                Open as project
              </Button>
              <span className="text-xs text-muted-foreground">
                {last.worldComplete
                  ? "The generated folder is copied byte-for-byte as an immutable source; seed, preset, data packs and generator identity are stored in project.json."
                  : "Only a completed run can be opened as a project."}
              </span>
            </div>
            <p className="text-xs text-muted-foreground">
              The generated world remains in the isolated run directory. Opening it creates a
              Vanilla World project backed by that read-only copy; edits are stored in the project
              overlay and exported as a new copy.
            </p>
          </div>
        ) : null}

        {error ? (
          <Alert variant="destructive">
            <TriangleAlert />
            <AlertTitle>Vanilla world setup error</AlertTitle>
            <AlertDescription>{error}</AlertDescription>
          </Alert>
        ) : null}
        {notice ? (
          <Alert>
            <CheckCircle2 />
            <AlertTitle>Vanilla world setup</AlertTitle>
            <AlertDescription>{notice}</AlertDescription>
          </Alert>
        ) : null}
      </CardContent>
    </Card>
  );
}
