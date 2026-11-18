import { useCallback, useEffect, useState } from "react";
import {
  CheckCircle2,
  Download,
  FolderOpen,
  HardDrive,
  Loader2,
  RefreshCw,
  ShieldCheck,
  Trash2,
} from "lucide-react";
import { Button } from "@/components/ui/button";
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
        title: "Choose the Java 21 executable",
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
    setBusy("release");
    setError(null);
    setNotice(null);
    try {
      setRelease(await latestTemurinRuntime());
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
      const installed = await installTemurinRuntime(release.version);
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
    <section className="min-w-0 space-y-2">
      <div className="flex items-center justify-between">
        <h3 className="text-[11px] font-semibold tracking-wide uppercase">
          Java &amp; world generation
        </h3>
        <span className="text-[10px] text-muted-foreground">
          Java {discovery?.targetJavaMajor ?? 21} · MC{" "}
          {discovery?.targetMinecraftVersion ?? "1.21.4"}
        </span>
      </div>
      <p className="text-[10px] text-muted-foreground">
        Empty Project and editing do not require Java. A verified Java 21 runtime is needed only for
        Vanilla World generation. Nothing is installed globally.
      </p>

      {saved ? (
        <div className="space-y-1 rounded border border-border bg-background/40 p-2 text-[11px]">
          <div className="flex items-center gap-2">
            <CheckCircle2
              className={cn(
                "size-3.5",
                saved.status === "compatible" ? "text-status-ok" : "text-status-warn",
              )}
            />
            <span className="font-medium">Selected runtime</span>
            <span
              className={cn(
                "rounded px-1.5 py-0.5 font-mono text-[10px]",
                saved.status === "compatible"
                  ? "bg-status-ok/15 text-status-ok"
                  : "bg-status-warn/15 text-status-warn",
              )}
            >
              {STATUS_LABELS[saved.status]}
            </span>
            <span className="ml-auto font-mono text-[10px]">{runtimeLabel(saved)}</span>
          </div>
          <div className="truncate font-mono text-[10px] text-muted-foreground">
            {displayRuntimePath(saved.path)}
          </div>
          {saved.error ? <div className="text-[10px] text-status-warn">{saved.error}</div> : null}
          {saved.warnings.map((warning) => (
            <div key={warning} className="text-[10px] text-status-warn">
              {warning}
            </div>
          ))}
          <div className="flex gap-1.5 pt-1">
            <Button variant="outline" size="sm" onClick={clearSelection} disabled={busy !== null}>
              <Trash2 /> Clear selection
            </Button>
          </div>
        </div>
      ) : (
        <div className="rounded border border-dashed border-border p-2 text-[10px] text-muted-foreground">
          No Java runtime selected. Detected candidates appear below; incompatible runtimes are
          shown with a reason and cannot be selected.
        </div>
      )}

      <div className="space-y-1">
        <div className="flex items-center gap-1.5">
          <span className="text-[10px] font-medium">Detected runtimes</span>
          <Button
            variant="ghost"
            size="sm"
            className="h-6 px-1.5"
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
            <FolderOpen /> Choose java.exe
          </Button>
        </div>
        <div className="max-h-36 space-y-1 overflow-y-auto">
          {(discovery?.candidates ?? []).map((runtime) => {
            const current = saved?.path === runtime.path;
            return (
              <div
                key={runtime.path}
                className="flex items-center gap-2 rounded border border-border/70 px-2 py-1 text-[10px]"
              >
                <span
                  className={cn(
                    "size-1.5 shrink-0 rounded-full",
                    runtime.status === "compatible" ? "bg-status-ok" : "bg-status-warn",
                  )}
                />
                <span className="min-w-0 flex-1">
                  <span className="block truncate font-mono">
                    {displayRuntimePath(runtime.path)}
                  </span>
                  <span className="text-muted-foreground">
                    {runtime.version ?? "unknown"} · {runtime.vendor ?? "unknown"} ·{" "}
                    {runtime.source.replace("_", " ")} · {STATUS_LABELS[runtime.status]}
                  </span>
                </span>
                <Button
                  variant={current ? "secondary" : "outline"}
                  size="sm"
                  className="h-6 px-2"
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
            <div className="text-[10px] text-muted-foreground">Nothing detected automatically.</div>
          ) : null}
        </div>
        {discovery?.problems.map((problem) => (
          <div key={problem} className="text-[10px] text-status-warn">
            {problem}
          </div>
        ))}
        {discovery ? (
          <p className="text-[10px] text-muted-foreground">{discovery.manualHint}</p>
        ) : null}
      </div>

      <div className="space-y-1.5 rounded border border-border bg-background/40 p-2">
        <div className="flex items-center gap-1.5 text-[10px] font-medium">
          <ShieldCheck className="size-3.5 text-status-ok" />
          Managed Eclipse Temurin JRE
          <span className="ml-auto font-mono text-[10px] text-muted-foreground">
            {formatBytes(discovery?.managedBytes ?? 0)} cached
          </span>
        </div>
        <p className="text-[10px] text-muted-foreground">
          Downloaded only when you press the button, from the official Adoptium API and its GitHub
          release host. Size and SHA-256 are checked before extraction; the JRE stays inside
          FuryBuilder data.
        </p>
        {release ? (
          <div className="space-y-1 rounded border border-border p-2 text-[10px]">
            <div className="flex flex-wrap gap-x-3 gap-y-0.5">
              <span>
                Version <span className="font-mono">{release.version}</span>
              </span>
              <span>
                Size <span className="font-mono">{formatBytes(release.size)}</span>
              </span>
              <span className="font-mono">
                {release.os}/{release.arch} {release.imageType}
              </span>
            </div>
            <div className="truncate font-mono text-muted-foreground">SHA-256 {release.sha256}</div>
            <div className="truncate font-mono text-muted-foreground">{release.downloadUrl}</div>
            <div className="flex gap-1.5 pt-0.5">
              <Button
                size="sm"
                onClick={() => void installTemurin()}
                disabled={busy !== null}
                aria-label="Download and install Temurin"
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
          <Button
            variant="outline"
            size="sm"
            onClick={() => void checkAdoptium()}
            disabled={busy !== null}
          >
            {busy === "release" ? <Loader2 className="animate-spin" /> : <Download />}
            Check official Temurin download
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
        <div className="flex items-center gap-1.5">
          <Button
            variant="outline"
            size="sm"
            onClick={() => void clearManaged()}
            disabled={busy !== null || (discovery?.managed.length ?? 0) === 0}
          >
            <Trash2 /> Clear managed Java
          </Button>
          <span className="flex items-center gap-1 text-[10px] text-muted-foreground">
            <HardDrive className="size-3" />
            {discovery?.managed.length ?? 0} managed
          </span>
        </div>
        {discovery?.managed.map((runtime) => (
          <div
            key={runtime.id}
            className="truncate rounded border border-border/60 px-2 py-1 font-mono text-[10px]"
            title={runtime.path}
          >
            {runtime.version} · {runtime.os}/{runtime.arch} · {formatBytes(runtime.bytes)} ·{" "}
            {runtime.sha256.slice(0, 12)}…
          </div>
        ))}
      </div>

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
    </section>
  );
}
