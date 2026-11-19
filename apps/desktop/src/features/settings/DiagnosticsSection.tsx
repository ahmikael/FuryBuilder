import {
  AlertTriangle,
  FileText,
  FolderOpen,
  Loader2,
  RefreshCw,
  ShieldCheck,
  Trash2,
} from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import {
  clearDiagnostics,
  fetchDiagnostics,
  openDiagnosticsFolder,
  type DiagnosticsInfo,
} from "@/lib/diagnostics-api";
import { presentError } from "@/lib/error-messages";

type BusyAction = "load" | "clear" | "open" | null;

function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`;
}

/**
 * Read/clear controls for the bounded local diagnostics log. The section never uploads
 * anything and never exposes world content; it only reports file sizes and locations.
 */
export function DiagnosticsSection() {
  const [info, setInfo] = useState<DiagnosticsInfo | null>(null);
  const [busy, setBusy] = useState<BusyAction>("load");
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const load = useCallback(async (): Promise<void> => {
    setBusy("load");
    try {
      setInfo(await fetchDiagnostics());
      setError(null);
    } catch (cause) {
      const presented = presentError(cause);
      setError(`${presented.message}${presented.hint ? ` ${presented.hint}` : ""}`);
      setInfo(null);
    } finally {
      setBusy(null);
    }
  }, []);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const next = await fetchDiagnostics();
        if (cancelled) return;
        setInfo(next);
        setError(null);
      } catch (cause) {
        if (cancelled) return;
        const presented = presentError(cause);
        setError(`${presented.message}${presented.hint ? ` ${presented.hint}` : ""}`);
        setInfo(null);
      } finally {
        if (!cancelled) setBusy(null);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const openFolder = async (): Promise<void> => {
    setBusy("open");
    try {
      await openDiagnosticsFolder();
      setError(null);
    } catch (cause) {
      const presented = presentError(cause);
      setError(`${presented.message}${presented.hint ? ` ${presented.hint}` : ""}`);
    } finally {
      setBusy(null);
    }
  };

  const clear = async (): Promise<void> => {
    setBusy("clear");
    setNotice(null);
    try {
      setInfo(await clearDiagnostics());
      setError(null);
      setNotice("Diagnostic logs cleared. New events will be recorded here.");
    } catch (cause) {
      const presented = presentError(cause);
      setError(`${presented.message}${presented.hint ? ` ${presented.hint}` : ""}`);
    } finally {
      setBusy(null);
    }
  };

  return (
    <section aria-labelledby="diagnostics-heading" className="min-w-0 space-y-2">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3
          id="diagnostics-heading"
          className="flex items-center gap-1.5 text-[11px] font-semibold tracking-wide uppercase"
        >
          <FileText className="size-3.5" /> Diagnostics
        </h3>
        <span className="flex items-center gap-1 text-[10px] text-muted-foreground">
          <ShieldCheck className="size-3" /> Local only, bounded
        </span>
      </div>
      <p className="text-[10px] text-muted-foreground">
        A rotating JSON Lines log records startup, project saves, recovery checks and sanitized
        interface errors. World data, tokens and absolute paths are not written. Autosave recovery
        is verified by checksum and round-trip decoding before any crash state is shown.
      </p>

      {error ? (
        <p className="flex items-start gap-1.5 rounded border border-status-warn/40 bg-status-warn/10 px-2 py-1 text-[10px] text-status-warn">
          <AlertTriangle className="mt-0.5 size-3 shrink-0" />
          {error}
        </p>
      ) : null}

      {info ? (
        <div className="space-y-1.5 rounded border border-border bg-background/40 p-2 text-[11px]">
          <div className="grid grid-cols-[110px_1fr] gap-x-3 gap-y-0.5">
            <span className="text-muted-foreground">App version</span>
            <span>{info.appVersion}</span>
            <span className="text-muted-foreground">Location</span>
            <span className="break-all" title={info.logDir}>
              {info.logDir}
            </span>
            <span className="text-muted-foreground">Files</span>
            <span>
              {info.files.length} / {info.maxFiles} · {formatBytes(info.totalBytes)}
            </span>
            <span className="text-muted-foreground">Rotation</span>
            <span>
              {formatBytes(info.maxBytesPerFile)} per file, {info.maxFiles} files maximum
            </span>
          </div>
          {info.files.length > 0 ? (
            <ul className="max-h-24 space-y-0.5 overflow-auto border-t border-border pt-1 font-mono text-[10px] text-muted-foreground">
              {info.files.map((file) => (
                <li key={file.name} className="flex justify-between gap-2">
                  <span className="truncate">{file.name}</span>
                  <span className="shrink-0">{formatBytes(file.bytes)}</span>
                </li>
              ))}
            </ul>
          ) : (
            <p className="border-t border-border pt-1 text-[10px] text-muted-foreground">
              No log entries yet.
            </p>
          )}
        </div>
      ) : busy === "load" ? (
        <p className="flex items-center gap-1 text-[10px] text-muted-foreground">
          <Loader2 className="size-3 animate-spin" /> Reading diagnostics…
        </p>
      ) : null}

      {notice ? (
        <p className="rounded border border-status-ok/40 bg-status-ok/10 px-2 py-1 text-[10px] text-status-ok">
          {notice}
        </p>
      ) : null}

      <div className="flex flex-wrap gap-1.5">
        <Button
          size="sm"
          variant="outline"
          aria-label="Refresh diagnostics"
          onClick={() => void load()}
          disabled={busy !== null}
        >
          <RefreshCw className={busy === "load" ? "size-3.5 animate-spin" : "size-3.5"} />
          Refresh
        </Button>
        <Button
          size="sm"
          variant="outline"
          aria-label="Open diagnostics folder"
          onClick={() => void openFolder()}
          disabled={busy !== null}
        >
          <FolderOpen className="size-3.5" />
          Open logs folder
        </Button>
        <Button
          size="sm"
          variant="outline"
          aria-label="Clear diagnostic logs"
          onClick={() => void clear()}
          disabled={busy !== null || !info || info.files.length === 0}
        >
          <Trash2 className="size-3.5" />
          {busy === "clear" ? "Clearing…" : "Clear logs"}
        </Button>
      </div>
    </section>
  );
}
