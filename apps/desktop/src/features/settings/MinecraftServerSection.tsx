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
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Progress } from "@/components/ui/progress";
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
    <section className="min-w-0 space-y-5">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3 className="flex items-center gap-2 text-base font-semibold">
          <Server className="size-4" /> Minecraft server JAR
        </h3>
        <Badge variant="secondary" className="font-mono">
          MC {discovery?.targetMinecraftVersion ?? "1.21.4"} · DV{" "}
          {discovery?.targetDataVersion ?? 4189} · Java {discovery?.targetJavaMajor ?? 21}
        </Badge>
      </div>
      <p className="text-sm leading-relaxed text-muted-foreground">
        Used for Vanilla World generation. The official JAR downloads from Mojang only when
        requested; it is never bundled, executed here or covered by an automatically accepted EULA.
      </p>

      {saved ? (
        <Card className="gap-0 py-0">
          <CardContent className="space-y-3 p-6">
            <div className="flex flex-wrap items-center gap-2">
              <CheckCircle2
                className={cn("size-4", saved.ready ? "text-status-ok" : "text-status-warn")}
              />
              <span className="text-sm font-medium">Selected server JAR</span>
              <Badge variant={saved.ready ? "secondary" : "outline"} className="uppercase">
                {saved.ready ? "ready" : "needs attention"}
              </Badge>
              <span className="ml-auto font-mono text-xs text-muted-foreground">
                {sourceLabel(saved)}
              </span>
            </div>
            <div className="flex flex-wrap gap-x-4 gap-y-1 text-sm">
              <span>
                Version <span className="font-mono text-xs">{saved.version ?? "unknown"}</span>
              </span>
              <span>
                DataVersion{" "}
                <span className="font-mono text-xs">{saved.dataVersion ?? "unknown"}</span>
              </span>
              <span>
                Java <span className="font-mono text-xs">{saved.javaMajor ?? "unknown"}</span>
              </span>
              <span>
                Size{" "}
                <span className="font-mono text-xs">
                  {saved.size === null ? "unknown" : formatBytes(saved.size)}
                </span>
              </span>
              {saved.matchesOfficial !== null ? (
                <span className="font-mono text-xs">
                  {saved.matchesOfficial ? "matches Mojang" : "does not match Mojang"}
                </span>
              ) : null}
            </div>
            <p className="truncate font-mono text-xs text-muted-foreground">
              {displayRuntimePath(saved.path)}
            </p>
            {saved.sha1 ? (
              <p className="truncate font-mono text-xs text-muted-foreground">SHA-1 {saved.sha1}</p>
            ) : null}
            {saved.error ? <p className="text-sm text-status-warn">{saved.error}</p> : null}
            {saved.warnings.map((warning) => (
              <p key={warning} className="text-sm text-status-warn">
                {warning}
              </p>
            ))}
            <div className="flex flex-wrap gap-2 pt-1">
              <Button variant="outline" size="sm" onClick={chooseJar} disabled={busy !== null}>
                <FolderOpen data-icon="inline-start" />
                Replace server.jar
              </Button>
              <Button variant="outline" size="sm" onClick={clearSelection} disabled={busy !== null}>
                <Trash2 data-icon="inline-start" />
                Clear selection
              </Button>
            </div>
          </CardContent>
        </Card>
      ) : (
        <div className="space-y-3 rounded-xl border border-dashed p-6">
          <p className="text-sm text-muted-foreground">
            No server JAR selected. Empty Project and Import World do not need one.
          </p>
          <Button variant="outline" size="sm" onClick={chooseJar} disabled={busy !== null}>
            {busy === "select" ? (
              <Loader2 data-icon="inline-start" className="animate-spin" />
            ) : (
              <FolderOpen data-icon="inline-start" />
            )}
            Choose server.jar
          </Button>
        </div>
      )}

      <Card className="gap-0 py-0">
        <CardContent className="space-y-4 p-6">
          <div className="flex flex-wrap items-center gap-2">
            <ShieldCheck className="size-4 text-status-ok" />
            <h4 className="text-sm font-medium">Official Mojang server JAR</h4>
            <Button
              variant="ghost"
              size="icon-sm"
              className="ml-auto"
              onClick={reload}
              disabled={busy !== null}
              aria-label="Refresh server JAR status"
            >
              {busy === "load" ? <Loader2 className="animate-spin" /> : <RefreshCw />}
            </Button>
          </div>
          <p className="text-sm leading-relaxed text-muted-foreground">
            Resolved from <span className="font-mono text-xs">version_manifest_v2</span> → version
            JSON → <span className="font-mono text-xs">downloads.server</span>. SHA-1, size,
            internal <span className="font-mono text-xs">version.json</span> and the server entry
            point are checked before the JAR enters the cache.
          </p>
          {release ? (
            <div className="space-y-3 rounded-xl border bg-background p-4 text-sm">
              <div className="flex flex-wrap gap-x-4 gap-y-1">
                <span>
                  Minecraft <span className="font-mono text-xs">{release.minecraftVersion}</span>
                </span>
                <span>
                  Size <span className="font-mono text-xs">{formatBytes(release.size)}</span>
                </span>
                <span>
                  Java <span className="font-mono text-xs">{release.javaMajor}</span>
                </span>
                <span>
                  DataVersion{" "}
                  <span className="font-mono text-xs">{release.expectedDataVersion}</span>
                </span>
              </div>
              <p className="truncate font-mono text-xs text-muted-foreground">
                SHA-1 {release.sha1}
              </p>
              <p className="truncate font-mono text-xs text-muted-foreground">
                {release.downloadUrl}
              </p>
              <div className="flex flex-wrap gap-2 pt-1">
                <Button
                  size="sm"
                  onClick={installServer}
                  disabled={busy !== null}
                  aria-label="Download and install server JAR"
                >
                  {busy === "install" ? (
                    <Loader2 data-icon="inline-start" className="animate-spin" />
                  ) : (
                    <Download data-icon="inline-start" />
                  )}
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
              {busy === "release" ? (
                <Loader2 data-icon="inline-start" className="animate-spin" />
              ) : (
                <Download data-icon="inline-start" />
              )}
              Check official server JAR
            </Button>
          )}
          {progress ? (
            <div className="space-y-2">
              <div className="flex justify-between gap-3 text-xs">
                <span>{progressLabel(progress)}</span>
                {progressPercent !== null ? (
                  <span className="font-mono text-muted-foreground">{progressPercent}%</span>
                ) : null}
              </div>
              <Progress value={progressPercent ?? 15} />
            </div>
          ) : null}
          <div className="flex flex-wrap items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              onClick={clearManaged}
              disabled={busy !== null || (discovery?.managed.length ?? 0) === 0}
            >
              <Trash2 data-icon="inline-start" />
              Clear managed server JARs
            </Button>
            <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
              <HardDrive className="size-3.5" />
              {discovery?.managed.length ?? 0} cached · {formatBytes(discovery?.managedBytes ?? 0)}
            </span>
          </div>
          {(discovery?.managed ?? []).map((server) => (
            <div
              key={server.path}
              className="truncate rounded-lg border bg-background px-3 py-2 font-mono text-xs"
              title={server.jarPath}
            >
              {server.version} · DV {server.dataVersion} · {formatBytes(server.size)} ·{" "}
              {server.sha1.slice(0, 12)}…
            </div>
          ))}
          {discovery?.problems.map((problem) => (
            <p key={problem} className="text-sm text-status-warn">
              {problem}
            </p>
          ))}
          {discovery ? (
            <p className="text-xs leading-relaxed text-muted-foreground">{discovery.manualHint}</p>
          ) : null}
        </CardContent>
      </Card>

      <ServerGenerationPanel discovery={discovery} onDiscoveryChanged={reload} />

      {error ? (
        <Alert>
          <AlertDescription>
            <p className="font-medium text-status-warn">{error}</p>
            <p>
              Empty Project and Import World do not need this JAR. If the network is unavailable,
              retry later or choose a verified local server.jar.
            </p>
          </AlertDescription>
        </Alert>
      ) : null}
      {notice ? (
        <Alert>
          <CheckCircle2 />
          <AlertTitle>Server JAR updated</AlertTitle>
          <AlertDescription>{notice}</AlertDescription>
        </Alert>
      ) : null}
    </section>
  );
}
