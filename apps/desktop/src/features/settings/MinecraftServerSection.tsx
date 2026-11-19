import { useCallback, useEffect, useState } from "react";
import {
  CheckCircle2,
  Download,
  FolderOpen,
  HardDrive,
  Loader2,
  RefreshCw,
  Server,
  ShieldCheck,
  Trash2,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { displayRuntimePath, formatBytes } from "@/lib/runtime-api";
import { ServerGenerationPanel } from "@/features/settings/ServerGenerationPanel";
import {
  clearManagedMinecraftServers,
  clearMinecraftServerSelection,
  discoverMinecraftServers,
  installMinecraftServer,
  latestMinecraftServerRelease,
  listenServerChanged,
  listenServerDownload,
  selectMinecraftServerJar,
} from "@/lib/server-api";
import { cn } from "@/lib/utils";
import type {
  ServerDiscoveryReport,
  ServerJarReport,
  ServerProgress,
  ServerReleaseInfo,
} from "@/runtime/server-types";

type Busy = "load" | "select" | "clear" | "release" | "install" | "managed" | null;

function progressLabel(progress: ServerProgress): string {
  switch (progress.phase) {
    case "metadata":
      return "Checking Mojang version metadata";
    case "downloading":
      return "Downloading the official server JAR";
    case "verified":
      return "SHA-1 and size verified";
    case "inspecting":
      return "Inspecting version.json and server entry point";
    case "complete":
      return "Installed and selected";
    default:
      return progress.phase;
  }
}

function sourceLabel(report: ServerJarReport): string {
  return report.source === "managed" ? "managed cache" : "manual file";
}

export function MinecraftServerSection() {
  const [discovery, setDiscovery] = useState<ServerDiscoveryReport | null>(null);
  const [release, setRelease] = useState<ServerReleaseInfo | null>(null);
  const [progress, setProgress] = useState<ServerProgress | null>(null);
  const [busy, setBusy] = useState<Busy>("load");
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [reloadKey, setReloadKey] = useState(0);

  const reload = useCallback(() => setReloadKey((value) => value + 1), []);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const report = await discoverMinecraftServers();
        if (!cancelled) {
          setDiscovery(report);
          setError(null);
        }
      } catch (cause) {
        if (!cancelled) setError(cause instanceof Error ? cause.message : String(cause));
      } finally {
        if (!cancelled) setBusy((current) => (current === "load" ? null : current));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [reloadKey]);

  useEffect(() => {
    let disposed = false;
    let stopDownload: (() => void) | null = null;
    let stopChanged: (() => void) | null = null;
    void listenServerDownload((value) => {
      if (!disposed) setProgress(value);
    })
      .then((stop) => {
        if (disposed) stop();
        else stopDownload = stop;
      })
      .catch(() => {
        // A browser preview has no Tauri event bridge; the section still renders.
      });
    void listenServerChanged(() => {
      if (!disposed) reload();
    })
      .then((stop) => {
        if (disposed) stop();
        else stopChanged = stop;
      })
      .catch(() => {
        // A browser preview has no Tauri event bridge; the section still renders.
      });
    return () => {
      disposed = true;
      stopDownload?.();
      stopChanged?.();
    };
  }, [reload]);

  async function chooseJar() {
    setBusy("select");
    setError(null);
    setNotice(null);
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = await open({
        title: "Choose the official Minecraft 1.21.4 server.jar",
        multiple: false,
        directory: false,
        filters: [{ name: "Minecraft server JAR", extensions: ["jar"] }],
      });
      if (typeof selected !== "string") return;
      await selectMinecraftServerJar(selected);
      setNotice("Server JAR selected and verified.");
      reload();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(null);
    }
  }

  async function clearSelection() {
    setBusy("clear");
    setError(null);
    setNotice(null);
    try {
      await clearMinecraftServerSelection();
      setNotice("Saved server JAR selection cleared.");
      reload();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(null);
    }
  }

  async function checkMojang() {
    setBusy("release");
    setError(null);
    setNotice(null);
    try {
      setRelease(await latestMinecraftServerRelease());
    } catch (cause) {
      setRelease(null);
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(null);
    }
  }

  async function installServer() {
    if (!release) return;
    setBusy("install");
    setError(null);
    setNotice(null);
    try {
      const installed = await installMinecraftServer(release.minecraftVersion);
      setNotice(
        `Minecraft ${installed.server.version} server JAR installed, SHA-1 verified and selected.`,
      );
      setRelease(null);
      reload();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(null);
    }
  }

  async function clearManaged() {
    setBusy("managed");
    setError(null);
    setNotice(null);
    try {
      const report = await clearManagedMinecraftServers();
      setNotice(
        report.servers.length === 0
          ? "Managed server JAR cache cleared. Manual files were not touched."
          : "Some managed server JARs could not be removed.",
      );
      reload();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(null);
    }
  }

  const saved = discovery?.selected ?? null;
  const progressPercent =
    progress && progress.total > 0
      ? Math.min(100, Math.round((progress.downloaded / progress.total) * 100))
      : null;

  return (
    <section className="min-w-0 space-y-2">
      <div className="flex items-center justify-between">
        <h3 className="flex items-center gap-1.5 text-[11px] font-semibold tracking-wide uppercase">
          <Server className="size-3.5" /> Minecraft server JAR
        </h3>
        <span className="text-[10px] text-muted-foreground">
          MC {discovery?.targetMinecraftVersion ?? "1.21.4"} · DV{" "}
          {discovery?.targetDataVersion ?? 4189} · Java {discovery?.targetJavaMajor ?? 21}
        </span>
      </div>
      <p className="text-[10px] text-muted-foreground">
        Used only by the future Vanilla World generator. The official JAR is downloaded from
        Mojang&apos;s version manifest only after you ask for it. It is never bundled, executed here
        or covered by an automatically accepted EULA.
      </p>

      {saved ? (
        <div className="space-y-1 rounded border border-border bg-background/40 p-2 text-[11px]">
          <div className="flex items-center gap-2">
            <CheckCircle2
              className={cn("size-3.5", saved.ready ? "text-status-ok" : "text-status-warn")}
            />
            <span className="font-medium">Selected server JAR</span>
            <span
              className={cn(
                "rounded px-1.5 py-0.5 font-mono text-[10px]",
                saved.ready
                  ? "bg-status-ok/15 text-status-ok"
                  : "bg-status-warn/15 text-status-warn",
              )}
            >
              {saved.ready ? "ready" : "needs attention"}
            </span>
            <span className="ml-auto font-mono text-[10px]">{sourceLabel(saved)}</span>
          </div>
          <div className="flex flex-wrap gap-x-3 gap-y-0.5 text-[10px]">
            <span>
              Version <span className="font-mono">{saved.version ?? "unknown"}</span>
            </span>
            <span>
              DataVersion <span className="font-mono">{saved.dataVersion ?? "unknown"}</span>
            </span>
            <span>
              Java <span className="font-mono">{saved.javaMajor ?? "unknown"}</span>
            </span>
            <span>
              Size{" "}
              <span className="font-mono">
                {saved.size === null ? "unknown" : formatBytes(saved.size)}
              </span>
            </span>
            {saved.matchesOfficial !== null ? (
              <span className="font-mono">
                {saved.matchesOfficial ? "matches Mojang" : "does not match Mojang"}
              </span>
            ) : null}
          </div>
          <div className="truncate font-mono text-[10px] text-muted-foreground">
            {displayRuntimePath(saved.path)}
          </div>
          {saved.sha1 ? (
            <div className="truncate font-mono text-[10px] text-muted-foreground">
              SHA-1 {saved.sha1}
            </div>
          ) : null}
          {saved.error ? <div className="text-[10px] text-status-warn">{saved.error}</div> : null}
          {saved.warnings.map((warning) => (
            <div key={warning} className="text-[10px] text-status-warn">
              {warning}
            </div>
          ))}
          <div className="flex flex-wrap gap-1.5 pt-1">
            <Button variant="outline" size="sm" onClick={chooseJar} disabled={busy !== null}>
              <FolderOpen /> Replace server.jar
            </Button>
            <Button variant="outline" size="sm" onClick={clearSelection} disabled={busy !== null}>
              <Trash2 /> Clear selection
            </Button>
          </div>
        </div>
      ) : (
        <div className="space-y-1.5 rounded border border-dashed border-border p-2 text-[10px] text-muted-foreground">
          <p>No server JAR selected. Empty Project and Import World do not need one.</p>
          <Button variant="outline" size="sm" onClick={chooseJar} disabled={busy !== null}>
            {busy === "select" ? <Loader2 className="animate-spin" /> : <FolderOpen />}
            Choose server.jar
          </Button>
        </div>
      )}

      <div className="space-y-1.5 rounded border border-border bg-background/40 p-2">
        <div className="flex items-center gap-1.5 text-[10px] font-medium">
          <ShieldCheck className="size-3.5 text-status-ok" />
          Official Mojang server JAR
          <Button
            variant="ghost"
            size="sm"
            className="ml-auto h-6 px-1.5"
            onClick={reload}
            disabled={busy !== null}
            aria-label="Refresh server JAR status"
          >
            {busy === "load" ? <Loader2 className="animate-spin" /> : <RefreshCw />}
          </Button>
        </div>
        <p className="text-[10px] text-muted-foreground">
          Resolved from <span className="font-mono">version_manifest_v2</span> → version JSON →
          <span className="font-mono"> downloads.server</span>. SHA-1, size, internal{" "}
          <span className="font-mono">version.json</span> and the server entry point are checked
          before the JAR enters the cache.
        </p>
        {release ? (
          <div className="space-y-1 rounded border border-border p-2 text-[10px]">
            <div className="flex flex-wrap gap-x-3 gap-y-0.5">
              <span>
                Minecraft <span className="font-mono">{release.minecraftVersion}</span>
              </span>
              <span>
                Size <span className="font-mono">{formatBytes(release.size)}</span>
              </span>
              <span>
                Java <span className="font-mono">{release.javaMajor}</span>
              </span>
              <span>
                DataVersion <span className="font-mono">{release.expectedDataVersion}</span>
              </span>
            </div>
            <div className="truncate font-mono text-muted-foreground">SHA-1 {release.sha1}</div>
            <div className="truncate font-mono text-muted-foreground">{release.downloadUrl}</div>
            <div className="flex gap-1.5 pt-0.5">
              <Button
                size="sm"
                onClick={installServer}
                disabled={busy !== null}
                aria-label="Download and install server JAR"
              >
                {busy === "install" ? <Loader2 className="animate-spin" /> : <Download />}
                Download &amp; install
              </Button>
              <Button
                variant="ghost"
                size="sm"
                onClick={() => setRelease(null)}
                disabled={busy === "install"}
              >
                Cancel
              </Button>
            </div>
          </div>
        ) : (
          <Button variant="outline" size="sm" onClick={checkMojang} disabled={busy !== null}>
            {busy === "release" ? <Loader2 className="animate-spin" /> : <Download />}
            Check official server JAR
          </Button>
        )}
        {progress ? (
          <div className="space-y-0.5">
            <div className="flex justify-between text-[10px]">
              <span>{progressLabel(progress)}</span>
              {progressPercent !== null ? (
                <span className="font-mono">{progressPercent}%</span>
              ) : null}
            </div>
            <div className="h-1 overflow-hidden rounded bg-muted">
              <div
                className="h-full bg-primary transition-[width]"
                style={{ width: `${progressPercent ?? 15}%` }}
              />
            </div>
          </div>
        ) : null}
        <div className="flex flex-wrap items-center gap-1.5">
          <Button
            variant="outline"
            size="sm"
            onClick={clearManaged}
            disabled={busy !== null || (discovery?.managed.length ?? 0) === 0}
          >
            <Trash2 /> Clear managed server JARs
          </Button>
          <span className="flex items-center gap-1 text-[10px] text-muted-foreground">
            <HardDrive className="size-3" />
            {discovery?.managed.length ?? 0} cached · {formatBytes(discovery?.managedBytes ?? 0)}
          </span>
        </div>
        {(discovery?.managed ?? []).map((server) => (
          <div
            key={server.path}
            className="truncate rounded border border-border/60 px-2 py-1 font-mono text-[10px]"
            title={server.jarPath}
          >
            {server.version} · DV {server.dataVersion} · {formatBytes(server.size)} ·{" "}
            {server.sha1.slice(0, 12)}…
          </div>
        ))}
        {discovery?.problems.map((problem) => (
          <div key={problem} className="text-[10px] text-status-warn">
            {problem}
          </div>
        ))}
        {discovery ? (
          <p className="text-[10px] text-muted-foreground">{discovery.manualHint}</p>
        ) : null}
      </div>

      <ServerGenerationPanel discovery={discovery} onDiscoveryChanged={reload} />

      {error ? (
        <div className="space-y-0.5 rounded border border-status-warn/40 bg-status-warn/10 px-2 py-1 text-[10px] text-status-warn">
          <p>{error}</p>
          <p className="text-muted-foreground">
            Empty Project and Import World do not need this JAR. If the network is unavailable,
            retry later or choose a verified local server.jar.
          </p>
        </div>
      ) : null}
      {notice ? (
        <p className="rounded border border-status-ok/40 bg-status-ok/10 px-2 py-1 text-[10px] text-status-ok">
          {notice}
        </p>
      ) : null}
    </section>
  );
}
