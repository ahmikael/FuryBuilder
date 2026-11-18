import { useCallback, useEffect, useState } from "react";
import {
  Ban,
  CheckCircle2,
  Download,
  FolderOpen,
  HardDriveDownload,
  Loader2,
  Play,
  RefreshCw,
  Server,
  ShieldCheck,
  Trash2,
  TriangleAlert,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  discoverJavaRuntimes,
  displayRuntimePath,
  formatBytes,
  selectJavaRuntime,
} from "@/lib/runtime-api";
import {
  acceptServerEula,
  cancelServerGeneration,
  discoverMinecraftServers,
  inspectServerDatapacks,
  installMinecraftServer,
  latestMinecraftServerRelease,
  selectMinecraftServerJar,
  serverGenerationStatus,
  startServerGeneration,
  vanillaSetupCheck,
} from "@/lib/server-api";
import { cn } from "@/lib/utils";
import type { RuntimeDiscoveryReport } from "@/runtime/types";
import type {
  DatapackInspection,
  GenerationReport,
  GenerationRequest,
  GenerationSnapshot,
  GenerationStatusReport,
  ServerDiscoveryReport,
  VanillaSetupReport,
  WorldPreset,
} from "@/runtime/server-types";

const EULA_URL = "https://aka.ms/MinecraftEULA";

type Busy =
  | "load"
  | "eula"
  | "java"
  | "server-select"
  | "server-download"
  | "datapacks"
  | "start"
  | "cancel"
  | null;

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

export function VanillaWorldWizard() {
  const [setup, setSetup] = useState<VanillaSetupReport | null>(null);
  const [runtimes, setRuntimes] = useState<RuntimeDiscoveryReport | null>(null);
  const [servers, setServers] = useState<ServerDiscoveryReport | null>(null);
  const [status, setStatus] = useState<GenerationStatusReport | null>(null);
  const [busy, setBusy] = useState<Busy>("load");
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [eulaChecked, setEulaChecked] = useState(false);
  const [consentChecked, setConsentChecked] = useState(false);
  const [javaChoice, setJavaChoice] = useState("");
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

  const refresh = useCallback(async (discoverCandidates: boolean) => {
    const [nextSetup, nextStatus, nextRuntimes, nextServers] = await Promise.all([
      vanillaSetupCheck(),
      serverGenerationStatus(),
      discoverCandidates ? discoverJavaRuntimes() : Promise.resolve(null),
      discoverMinecraftServers(),
    ]);
    setSetup(nextSetup);
    setStatus(nextStatus);
    if (nextRuntimes) {
      setRuntimes(nextRuntimes);
      const firstCompatible = nextRuntimes.candidates.find(
        (candidate) => candidate.status === "compatible",
      );
      setJavaChoice((current) => current || firstCompatible?.path || "");
    }
    setServers(nextServers);
  }, []);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        await refresh(true);
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

  const active: GenerationSnapshot | null = status?.active ?? null;
  const last = status?.last ?? null;
  const running = Boolean(active?.running);
  const selectedJava = setup?.java ?? null;
  const compatibleCandidates =
    runtimes?.candidates.filter((candidate) => candidate.status === "compatible") ?? [];
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
      await refresh(false);
    });
  }

  async function chooseJava() {
    await runBusy("java", async () => {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const path = await open({
        title: "Choose a Java 21 executable",
        multiple: false,
        directory: false,
        filters: [{ name: "Java executable", extensions: ["exe"] }],
      });
      if (typeof path !== "string") return;
      const report = await selectJavaRuntime(path);
      if (report.status !== "compatible") {
        throw new Error(
          `Java ${report.version ?? "unknown"} is not compatible: ${report.warnings.join("; ")}`,
        );
      }
      setNotice(`Java ${report.version ?? "21"} selected and probed.`);
      await refresh(false);
    });
  }

  async function applyCandidate() {
    if (!javaChoice) return;
    await runBusy("java", async () => {
      const report = await selectJavaRuntime(javaChoice);
      if (report.status !== "compatible") {
        throw new Error(`The selected candidate is not compatible: ${report.warnings.join("; ")}`);
      }
      setNotice(`Java ${report.version ?? "21"} selected and probed.`);
      await refresh(false);
    });
  }

  async function selectServer() {
    await runBusy("server-select", async () => {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const path = await open({
        title: "Choose the official Minecraft 1.21.4 server.jar",
        multiple: false,
        directory: false,
        filters: [{ name: "Minecraft server JAR", extensions: ["jar"] }],
      });
      if (typeof path !== "string") return;
      const report = await selectMinecraftServerJar(path);
      if (!report.ready) {
        throw new Error(report.error ?? "The selected server JAR did not pass verification.");
      }
      setNotice("Official 1.21.4 server JAR selected and verified.");
      await refresh(false);
    });
  }

  async function downloadServer() {
    await runBusy("server-download", async () => {
      const release = await latestMinecraftServerRelease();
      const installed = await installMinecraftServer(release.minecraftVersion);
      setNotice(
        `Minecraft ${installed.server.version} server JAR downloaded from Mojang, SHA-1 verified and selected (${formatBytes(installed.server.size)}).`,
      );
      await refresh(false);
    });
  }

  async function acceptEula() {
    if (!eulaChecked) return;
    await runBusy("eula", async () => {
      const eula = await acceptServerEula(true);
      setEulaChecked(false);
      setNotice(
        `EULA accepted for Minecraft ${eula.minecraftVersion} (${eula.jarSha1.slice(0, 12)}…).`,
      );
      await refresh(false);
    });
  }

  async function browseDatapacks() {
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
      const inspected = await inspectServerDatapacks(fresh);
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
      const snapshot = await startServerGeneration(request);
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
      await refresh(false);
    });
  }

  return (
    <div className="space-y-2 rounded border border-border bg-background/40 p-2">
      <div className="flex items-center justify-between gap-2">
        <div className="flex items-center gap-1.5 text-[10px] font-semibold tracking-wide uppercase">
          <Server className="size-3.5" /> Vanilla World setup
        </div>
        <Button
          variant="ghost"
          size="sm"
          className="h-6 px-1.5"
          onClick={() => void reloadSetup()}
          disabled={busy !== null}
          aria-label="Refresh Vanilla World setup"
        >
          {busy === "load" ? <Loader2 className="animate-spin" /> : <RefreshCw />}
        </Button>
      </div>
      <p className="text-[10px] leading-relaxed text-muted-foreground">
        The world is generated by Mojang&apos;s official 1.21.4 server, not by a FuryBuilder
        generator. Target: Minecraft Java{" "}
        <span className="font-mono">{setup?.targetMinecraftVersion ?? "1.21.4"}</span> · DV{" "}
        <span className="font-mono">{setup?.targetDataVersion ?? 4189}</span> · Java{" "}
        <span className="font-mono">{setup?.targetJavaMajor ?? 21}</span>.
      </p>

      <div className={cn("space-y-1 rounded border p-2", statusTone(Boolean(setup?.ready)))}>
        <div className="flex flex-wrap items-center gap-1.5 text-[10px]">
          {setup?.ready ? (
            <CheckCircle2 className="size-3.5" />
          ) : (
            <TriangleAlert className="size-3.5" />
          )}
          <span className="font-semibold">
            {setup?.ready ? "Ready to generate" : "Setup is incomplete"}
          </span>
          <span className="text-muted-foreground">
            Java {selectedJava?.version ?? "not selected"} · JAR{" "}
            {setup?.server?.version ?? "not selected"} ·{" "}
            {setup?.eulaMatchesSelected ? "EULA accepted" : "EULA required"}
          </span>
        </div>

        <div className="grid gap-1.5 md:grid-cols-3">
          <div className="space-y-1 rounded border border-border/60 bg-background/60 p-1.5">
            <div className="flex items-center gap-1 text-[10px] font-medium">
              <HardDriveDownload className="size-3" /> Java 21
            </div>
            {setup?.javaReady ? (
              <div className="font-mono text-[9px] break-all text-muted-foreground">
                {selectedJava?.version} · {displayRuntimePath(selectedJava?.path ?? "")}
              </div>
            ) : (
              <div className="space-y-1">
                <select
                  aria-label="Compatible Java candidate"
                  className="h-6 w-full rounded border border-border bg-background px-1 font-mono text-[9px]"
                  value={javaChoice}
                  onChange={(event) => setJavaChoice(event.target.value)}
                  disabled={busy !== null || compatibleCandidates.length === 0}
                >
                  <option value="">
                    {compatibleCandidates.length === 0
                      ? "No compatible Java candidate"
                      : "Choose Java 21 candidate"}
                  </option>
                  {compatibleCandidates.map((candidate) => (
                    <option key={candidate.path} value={candidate.path}>
                      {candidate.version} · {displayRuntimePath(candidate.path)}
                    </option>
                  ))}
                </select>
                <div className="flex flex-wrap gap-1">
                  <Button
                    size="sm"
                    variant="outline"
                    className="h-6 px-1.5 text-[9px]"
                    onClick={() => void applyCandidate()}
                    disabled={!javaChoice || busy !== null}
                    aria-label="Use selected Java candidate"
                  >
                    Use
                  </Button>
                  <Button
                    size="sm"
                    variant="outline"
                    className="h-6 px-1.5 text-[9px]"
                    onClick={() => void chooseJava()}
                    disabled={busy !== null}
                    aria-label="Choose Java executable"
                  >
                    <FolderOpen /> Choose
                  </Button>
                </div>
              </div>
            )}
          </div>

          <div className="space-y-1 rounded border border-border/60 bg-background/60 p-1.5">
            <div className="flex items-center gap-1 text-[10px] font-medium">
              <Server className="size-3" /> Loader JAR
            </div>
            {setup?.serverReady ? (
              <div className="space-y-0.5 font-mono text-[9px] text-muted-foreground">
                <div>
                  {setup.server?.version} ·{" "}
                  {setup.server?.size ? formatBytes(setup.server.size) : "?"}
                </div>
                <div className="truncate" title={setup.server?.sha1 ?? undefined}>
                  SHA-1 {setup.server?.sha1?.slice(0, 12)}…
                </div>
              </div>
            ) : (
              <div className="flex flex-wrap gap-1">
                <Button
                  size="sm"
                  variant="outline"
                  className="h-6 px-1.5 text-[9px]"
                  onClick={() => void downloadServer()}
                  disabled={busy !== null}
                  aria-label="Download official Minecraft server JAR"
                >
                  {busy === "server-download" ? <Loader2 className="animate-spin" /> : <Download />}
                  Official
                </Button>
                <Button
                  size="sm"
                  variant="outline"
                  className="h-6 px-1.5 text-[9px]"
                  onClick={() => void selectServer()}
                  disabled={busy !== null}
                  aria-label="Choose server JAR"
                >
                  <FolderOpen /> Choose
                </Button>
              </div>
            )}
            {servers?.selected?.error ? (
              <div className="text-[9px] text-status-warn">{servers.selected.error}</div>
            ) : null}
          </div>

          <div className="space-y-1 rounded border border-border/60 bg-background/60 p-1.5">
            <div className="flex items-center gap-1 text-[10px] font-medium">
              <ShieldCheck className="size-3" /> Minecraft EULA
            </div>
            {setup?.eulaMatchesSelected ? (
              <div className="text-[9px] text-status-ok">Accepted for this exact server JAR.</div>
            ) : (
              <div className="space-y-1">
                <a
                  className="text-[9px] underline underline-offset-2"
                  href={EULA_URL}
                  target="_blank"
                  rel="noreferrer"
                >
                  Read the official EULA
                </a>
                <label className="flex items-start gap-1 text-[9px]">
                  <input
                    type="checkbox"
                    className="mt-0.5"
                    checked={eulaChecked}
                    onChange={(event) => setEulaChecked(event.target.checked)}
                    aria-label="I have read and accept the Minecraft EULA"
                  />
                  <span>I accept the Minecraft EULA for this JAR.</span>
                </label>
                <Button
                  size="sm"
                  variant="outline"
                  className="h-6 px-1.5 text-[9px]"
                  onClick={() => void acceptEula()}
                  disabled={!eulaChecked || busy !== null || !setup?.serverReady}
                  aria-label="Accept EULA"
                >
                  {busy === "eula" ? <Loader2 className="animate-spin" /> : <CheckCircle2 />}
                  Accept
                </Button>
              </div>
            )}
          </div>
        </div>

        {setup?.blockers.length ? (
          <div className="space-y-0.5 text-[9px]">
            {setup.blockers.map((blocker) => (
              <div key={blocker}>• {blocker}</div>
            ))}
          </div>
        ) : null}
      </div>

      <div className="grid grid-cols-2 gap-1.5 sm:grid-cols-4">
        <label className="space-y-0.5 text-[10px]">
          <span className="text-muted-foreground">Seed</span>
          <input
            className="h-6 w-full rounded border border-border bg-background px-1.5 font-mono text-[10px]"
            value={seed}
            maxLength={64}
            onChange={(event) => setSeed(event.target.value)}
            disabled={running}
            aria-label="Vanilla generation seed"
          />
        </label>
        <label className="space-y-0.5 text-[10px]">
          <span className="text-muted-foreground">Preset</span>
          <select
            className="h-6 w-full rounded border border-border bg-background px-1 text-[10px]"
            value={preset}
            onChange={(event) => setPreset(event.target.value as WorldPreset)}
            disabled={running}
            aria-label="Vanilla world preset"
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
            aria-label="Vanilla generation radius in chunks"
          />
        </label>
        <label className="flex items-end gap-1.5 pb-1 text-[10px]">
          <input
            type="checkbox"
            checked={generateStructures}
            onChange={(event) => setGenerateStructures(event.target.checked)}
            disabled={running}
            aria-label="Generate structures"
          />
          <span>Generate structures</span>
        </label>
        <label className="space-y-0.5 text-[10px]">
          <span className="text-muted-foreground">Center X</span>
          <input
            type="number"
            className="h-6 w-full rounded border border-border bg-background px-1.5 font-mono text-[10px]"
            value={centerX}
            onChange={(event) => setCenterX(event.target.value)}
            disabled={running}
            aria-label="Vanilla generation center X"
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
            aria-label="Vanilla generation center Z"
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
            aria-label="Vanilla generation time limit seconds"
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
            aria-label="Vanilla generation memory limit"
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
            aria-label="Vanilla generation view distance"
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
            aria-label="Vanilla generation CPU threads"
          />
        </label>
      </div>

      <div className="space-y-1 rounded border border-border bg-background/40 p-2">
        <div className="flex flex-wrap items-center gap-1.5">
          <span className="text-[10px] font-medium">Data packs</span>
          <span className="text-[9px] text-muted-foreground">
            Format <span className="font-mono">{setup?.serverDatapackFormat ?? 61}</span> · local
            .zip files are validated and copied into the isolated world only
          </span>
          <Button
            size="sm"
            variant="outline"
            className="ml-auto h-6 px-1.5 text-[9px]"
            onClick={() => void browseDatapacks()}
            disabled={busy !== null || running || datapacks.length >= 16}
            aria-label="Add data packs"
          >
            {busy === "datapacks" ? <Loader2 className="animate-spin" /> : <FolderOpen />}
            Add
          </Button>
        </div>
        {datapacks.length === 0 ? (
          <div className="text-[9px] text-muted-foreground">
            No data packs. Vanilla generation runs with the official data pack only.
          </div>
        ) : (
          <div className="space-y-0.5">
            {datapacks.map((datapack) => (
              <div
                key={datapack.path}
                className={cn(
                  "flex items-center gap-1.5 rounded px-1.5 py-0.5 font-mono text-[9px]",
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
                  className="h-5 shrink-0 px-1"
                  onClick={() =>
                    setDatapacks((current) => current.filter((item) => item.path !== datapack.path))
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
          <div className="text-[9px] text-status-warn">
            Remove invalid data packs before starting generation.
          </div>
        ) : null}
      </div>

      <label className="flex items-start gap-1.5 rounded border border-border bg-background/40 p-2 text-[10px]">
        <input
          type="checkbox"
          className="mt-0.5"
          checked={consentChecked}
          onChange={(event) => setConsentChecked(event.target.checked)}
          disabled={running}
          aria-label="Confirm local official server run"
        />
        <span>
          I explicitly allow FuryBuilder to start the selected official server JAR with the selected
          Java 21 runtime for this run. It listens on loopback only; no external player can connect.
        </span>
      </label>

      <div className="flex flex-wrap items-center gap-1.5">
        <Button
          size="sm"
          onClick={() => void start()}
          disabled={
            busy !== null ||
            running ||
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
        <span className="flex items-center gap-1 text-[9px] text-muted-foreground">
          <ShieldCheck className="size-3" /> loopback only · process job guarded ·{" "}
          {validDatapacks.length} data pack(s)
        </span>
      </div>

      {active ? (
        <div className="space-y-1 rounded border border-border p-2 text-[10px]">
          <div className="flex flex-wrap items-center gap-1.5">
            <span
              className={cn(
                "rounded px-1.5 py-0.5 font-mono text-[9px]",
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
              {seconds(active.elapsedMs)}
            </span>
          </div>
          <div className="flex flex-wrap gap-x-3 font-mono text-[9px] text-muted-foreground">
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
        <div className="space-y-1 rounded border border-border p-2 text-[10px]">
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
          <div className="flex flex-wrap gap-x-3 text-[9px]">
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
          <div className="truncate font-mono text-[9px] text-muted-foreground">
            {displayRuntimePath(last.worldDir)}
          </div>
          {last.snapshot.error ? (
            <div className="text-status-warn">{last.snapshot.error}</div>
          ) : null}
          <p className="text-[9px] text-muted-foreground">
            The generated world remains in the isolated run directory. Importing it into the Rust
            World Core without losing metadata is the next roadmap task (M9.7).
          </p>
        </div>
      ) : null}

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
