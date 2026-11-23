import { useCallback, useEffect, useState } from "react";
import {
  CheckCircle2,
  Download,
  FolderOpen,
  HardDrive,
  Loader2,
  RefreshCw,
  ShieldAlert,
  ShieldCheck,
  Trash2,
} from "lucide-react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from "@/components/ui/empty";
import { Progress } from "@/components/ui/progress";
import {
  clearJavaRuntime,
  clearManagedJavaRuntimes,
  discoverJavaRuntimes,
  displayRuntimePath,
  formatBytes,
  installTemurinRuntime,
  latestTemurinRuntime,
  listenRuntimeChanged,
  listenRuntimeDownload,
  selectJavaRuntime,
} from "@/lib/runtime-api";
import { cn } from "@/lib/utils";
import type {
  JavaRuntimeReport,
  RuntimeDiscoveryReport,
  RuntimeProgress,
  RuntimeReleaseInfo,
} from "@/runtime/types";

type Busy = "load" | "refresh" | "select" | "clear" | "release" | "install" | "managed" | null;

const STATUS_LABELS: Record<JavaRuntimeReport["status"], string> = {
  compatible: "compatible",
  outdated: "outdated",
  newer: "newer than target",
  unsupported_arch: "unsupported architecture",
  not_java: "not Java",
  unreadable: "unreadable",
};

function runtimeLabel(runtime: JavaRuntimeReport): string {
  const version = runtime.version ?? "unknown version";
  const vendor = runtime.vendor ?? "unknown vendor";
  return `${version} · ${vendor}`;
}

function progressLabel(progress: RuntimeProgress): string {
  switch (progress.phase) {
    case "metadata":
      return "Checking Adoptium metadata";
    case "downloading":
      return "Downloading verified Temurin JRE";
    case "verified":
      return "SHA-256 verified";
    case "extracting":
    case "extracted":
      return "Extracting safely";
    case "probing":
      return "Probing the extracted Java";
    case "complete":
      return "Installed and selected";
    default:
      return progress.phase;
  }
}

export function JavaRuntimeSection() {
  const [discovery, setDiscovery] = useState<RuntimeDiscoveryReport | null>(null);
  const [release, setRelease] = useState<RuntimeReleaseInfo | null>(null);
  const [progress, setProgress] = useState<RuntimeProgress | null>(null);
  const [busy, setBusy] = useState<Busy>("load");
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [reloadKey, setReloadKey] = useState(0);

  const reload = useCallback(() => setReloadKey((value) => value + 1), []);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const report = await discoverJavaRuntimes();
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
    void listenRuntimeDownload((value) => {
      if (!disposed) setProgress(value);
    })
      .then((stop) => {
        if (disposed) stop();
        else stopDownload = stop;
      })
      .catch(() => {
        // A browser preview has no Tauri event bridge; the section still renders.
      });
    void listenRuntimeChanged(() => {
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

  async function chooseJava() {
    setError(null);
    setNotice(null);
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = await open({
        title: discovery?.targetJavaMajor
          ? `Choose the Java ${discovery.targetJavaMajor} executable`
          : "Choose a Java executable",
        multiple: false,
        directory: false,
        filters: [{ name: "Java executable", extensions: ["exe"] }],
      });
      if (typeof selected !== "string") return;
      await selectJavaRuntime(selected);
      setNotice("Java selected and verified.");
      reload();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    }
  }

  async function applyCandidate(runtime: JavaRuntimeReport) {
    setBusy("select");
    setError(null);
    setNotice(null);
    try {
      await selectJavaRuntime(runtime.path);
      setNotice("Java selected and verified.");
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
      await clearJavaRuntime();
      setNotice("Saved Java selection cleared. Empty Project is unaffected.");
      reload();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(null);
    }
  }

  async function checkAdoptium() {
    const major = discovery?.targetJavaMajor;
    if (!major) return;
    setBusy("release");
    setError(null);
    setNotice(null);
    try {
      setRelease(await latestTemurinRuntime(major));
    } catch (cause) {
      setRelease(null);
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(null);
    }
  }

  async function installTemurin() {
    if (!release) return;
    setBusy("install");
    setError(null);
    setNotice(null);
    try {
      const installed = await installTemurinRuntime(release.major, release.version);
      setNotice(`Temurin ${installed.runtime.version} installed, SHA-256 verified and selected.`);
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
      const report = await clearManagedJavaRuntimes();
      setNotice(
        report.runtimes.length === 0
          ? "Managed Java cache cleared. External Java was not touched."
          : "Some managed runtimes could not be removed.",
      );
      reload();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(null);
    }
  }

  const saved = discovery?.saved ?? null;
  const progressPercent =
    progress && progress.total > 0
      ? Math.min(100, Math.round((progress.downloaded / progress.total) * 100))
      : null;

  return (
    <section className="space-y-5">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3 className="text-sm font-semibold">Java runtime</h3>
        <Badge variant="secondary" className="font-mono">
          Java {discovery?.targetJavaMajor ?? "?"} · MC{" "}
          {discovery?.targetMinecraftVersion ?? "unknown"}
        </Badge>
      </div>

      <p className="text-sm leading-relaxed text-muted-foreground">
        Empty Project and editing do not require Java. A verified Java runtime matching the project
        version is needed only for Vanilla World generation. Nothing is installed globally.
      </p>

      {saved ? (
        <Card className="gap-0 py-0">
          <CardContent className="space-y-3 p-4">
            <div className="flex flex-wrap items-center gap-2">
              <CheckCircle2
                className={cn(
                  "size-4",
                  saved.status === "compatible" ? "text-status-ok" : "text-status-warn",
                )}
              />
              <span className="text-sm font-medium">Selected runtime</span>
              <Badge variant={saved.status === "compatible" ? "secondary" : "outline"}>
                {STATUS_LABELS[saved.status]}
              </Badge>
              <span className="ml-auto font-mono text-xs text-muted-foreground">
                {runtimeLabel(saved)}
              </span>
            </div>
            <p className="truncate font-mono text-xs text-muted-foreground">
              {displayRuntimePath(saved.path)}
            </p>
            {saved.error ? <p className="text-sm text-status-warn">{saved.error}</p> : null}
            {saved.warnings.map((warning) => (
              <p key={warning} className="text-sm text-status-warn">
                {warning}
              </p>
            ))}
            <Button variant="outline" size="sm" onClick={clearSelection} disabled={busy !== null}>
              <Trash2 data-icon="inline-start" />
              Clear selection
            </Button>
          </CardContent>
        </Card>
      ) : (
        <Empty className="border p-6">
          <EmptyHeader>
            <EmptyMedia variant="icon">
              <HardDrive />
            </EmptyMedia>
            <EmptyTitle>No Java runtime selected</EmptyTitle>
            <EmptyDescription>
              Detected candidates appear below; incompatible runtimes are shown with a reason and
              cannot be selected.
            </EmptyDescription>
          </EmptyHeader>
        </Empty>
      )}

      <div className="space-y-3">
        <div className="flex flex-wrap items-center gap-2">
          <h4 className="text-sm font-medium">Detected runtimes</h4>
          <Button
            variant="ghost"
            size="icon-sm"
            onClick={reload}
            disabled={busy !== null}
            aria-label="Refresh detected Java runtimes"
          >
            {busy === "load" || busy === "refresh" ? (
              <Loader2 className="animate-spin" />
            ) : (
              <RefreshCw />
            )}
          </Button>
          <Button
            variant="outline"
            size="sm"
            className="ml-auto"
            onClick={chooseJava}
            disabled={busy !== null}
          >
            <FolderOpen data-icon="inline-start" />
            Choose java.exe
          </Button>
        </div>
        <div className="max-h-44 space-y-2 overflow-y-auto pr-1">
          {(discovery?.candidates ?? []).map((runtime) => {
            const current = saved?.path === runtime.path;
            return (
              <div
                key={runtime.path}
                className="flex flex-wrap items-center gap-3 rounded-xl border bg-card p-3"
              >
                <span
                  className={cn(
                    "size-2 shrink-0 rounded-full",
                    runtime.status === "compatible" ? "bg-status-ok" : "bg-status-warn",
                  )}
                />
                <div className="min-w-0 flex-1">
                  <p className="truncate font-mono text-xs">{displayRuntimePath(runtime.path)}</p>
                  <p className="mt-1 text-xs text-muted-foreground">
                    {runtime.version ?? "unknown"} · {runtime.vendor ?? "unknown"} ·{" "}
                    {runtime.source.replace("_", " ")} · {STATUS_LABELS[runtime.status]}
                  </p>
                </div>
                <Button
                  variant={current ? "secondary" : "outline"}
                  size="sm"
                  disabled={busy !== null || runtime.status !== "compatible" || current}
                  onClick={() => void applyCandidate(runtime)}
                  aria-label={`Use ${displayRuntimePath(runtime.path)}`}
                >
                  {current ? "In use" : "Use"}
                </Button>
              </div>
            );
          })}
          {discovery && discovery.candidates.length === 0 ? (
            <p className="rounded-xl border border-dashed p-4 text-sm text-muted-foreground">
              Nothing detected automatically.
            </p>
          ) : null}
        </div>
        {discovery?.problems.map((problem) => (
          <p key={problem} className="text-xs text-status-warn">
            {problem}
          </p>
        ))}
        {discovery ? (
          <p className="text-xs leading-relaxed text-muted-foreground">{discovery.manualHint}</p>
        ) : null}
      </div>

      <Card className="gap-0 py-0">
        <CardContent className="space-y-3 p-4">
          <div className="flex flex-wrap items-center gap-2">
            <ShieldCheck className="size-4 text-status-ok" />
            <h4 className="text-sm font-medium">Managed Eclipse Temurin JRE</h4>
            <span className="ml-auto font-mono text-xs text-muted-foreground">
              {formatBytes(discovery?.managedBytes ?? 0)} cached
            </span>
          </div>
          <p className="text-sm leading-relaxed text-muted-foreground">
            Downloaded only when you press the button, from the official Adoptium API and its GitHub
            release host. Size and SHA-256 are checked before extraction; the JRE stays inside
            FuryBuilder data.
          </p>
          {release ? (
            <div className="space-y-2 rounded-lg border bg-background p-3 text-sm">
              <div className="flex flex-wrap gap-x-4 gap-y-1">
                <span>
                  Version <span className="font-mono text-xs">{release.version}</span>
                </span>
                <span>
                  Size <span className="font-mono text-xs">{formatBytes(release.size)}</span>
                </span>
                <span className="font-mono text-xs">
                  {release.os}/{release.arch} {release.imageType}
                </span>
              </div>
              <p className="truncate font-mono text-xs text-muted-foreground">
                SHA-256 {release.sha256}
              </p>
              <p className="truncate font-mono text-xs text-muted-foreground">
                {release.downloadUrl}
              </p>
              <div className="flex flex-wrap gap-2 pt-1">
                <Button
                  size="sm"
                  onClick={() => void installTemurin()}
                  disabled={busy !== null}
                  aria-label="Download and install Temurin"
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
            <Button
              variant="outline"
              size="sm"
              onClick={() => void checkAdoptium()}
              disabled={busy !== null || !discovery}
            >
              {busy === "release" ? (
                <Loader2 data-icon="inline-start" className="animate-spin" />
              ) : (
                <Download data-icon="inline-start" />
              )}
              Check official Temurin download
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
              onClick={() => void clearManaged()}
              disabled={busy !== null || (discovery?.managed.length ?? 0) === 0}
            >
              <Trash2 data-icon="inline-start" />
              Clear managed Java
            </Button>
            <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
              <HardDrive className="size-3.5" />
              {discovery?.managed.length ?? 0} managed
            </span>
          </div>
          {discovery?.managed.map((runtime) => (
            <div
              key={runtime.id}
              className="truncate rounded-lg border bg-background px-3 py-2 font-mono text-xs"
              title={runtime.path}
            >
              Java {runtime.major} · {runtime.version} · {runtime.os}/{runtime.arch} ·{" "}
              {formatBytes(runtime.bytes)} · {runtime.sha256.slice(0, 12)}…
            </div>
          ))}
        </CardContent>
      </Card>

      {error ? (
        <Alert variant="destructive">
          <ShieldAlert />
          <AlertTitle>Java runtime error</AlertTitle>
          <AlertDescription>
            <p>{error}</p>
            <p>
              Empty Project and manual editing keep working without Java. If the network is
              unavailable, retry later or choose a local java.exe.
            </p>
          </AlertDescription>
        </Alert>
      ) : null}
      {notice ? (
        <Alert>
          <CheckCircle2 />
          <AlertTitle>Java runtime updated</AlertTitle>
          <AlertDescription>{notice}</AlertDescription>
        </Alert>
      ) : null}
    </section>
  );
}
