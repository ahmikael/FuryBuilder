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
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
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
    <section aria-labelledby="diagnostics-heading" className="min-w-0 space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3 id="diagnostics-heading" className="flex items-center gap-2 text-base font-semibold">
          <FileText className="size-4" /> Diagnostics
        </h3>
        <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
          <ShieldCheck className="size-3.5" /> Local only, bounded
        </span>
      </div>
      <p className="text-sm leading-relaxed text-muted-foreground">
        A rotating JSON Lines log records startup, project saves, recovery checks and sanitized
        interface errors. World data, tokens and absolute paths are not written. Autosave recovery
        is verified by checksum and round-trip decoding before any crash state is shown.
      </p>

      {error ? (
        <Alert>
          <AlertTriangle />
          <AlertTitle>Diagnostics unavailable</AlertTitle>
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      ) : null}

      {info ? (
        <Card className="gap-0 py-0">
          <CardContent className="space-y-4 p-6 text-sm">
            <div className="grid gap-x-4 gap-y-2 sm:grid-cols-[130px_1fr]">
              <span className="text-muted-foreground">App version</span>
              <span className="font-mono text-xs">{info.appVersion}</span>
              <span className="text-muted-foreground">Location</span>
              <span className="break-all font-mono text-xs" title={info.logDir}>
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
              <ul className="max-h-32 space-y-1 overflow-auto border-t pt-3 font-mono text-xs text-muted-foreground">
                {info.files.map((file) => (
                  <li key={file.name} className="flex justify-between gap-2">
                    <span className="truncate">{file.name}</span>
                    <span className="shrink-0">{formatBytes(file.bytes)}</span>
                  </li>
                ))}
              </ul>
            ) : (
              <p className="border-t pt-3 text-xs text-muted-foreground">No log entries yet.</p>
            )}
          </CardContent>
        </Card>
      ) : busy === "load" ? (
        <p className="flex items-center gap-2 text-sm text-muted-foreground">
          <Loader2 className="size-4 animate-spin" /> Reading diagnostics…
        </p>
      ) : null}

      {notice ? (
        <Alert>
          <ShieldCheck />
          <AlertTitle>Logs cleared</AlertTitle>
          <AlertDescription>{notice}</AlertDescription>
        </Alert>
      ) : null}

      <div className="flex flex-wrap gap-2">
        <Button
          size="sm"
          variant="outline"
          aria-label="Refresh diagnostics"
          onClick={() => void load()}
          disabled={busy !== null}
        >
          <RefreshCw
            data-icon="inline-start"
            className={busy === "load" ? "animate-spin" : undefined}
          />
          Refresh
        </Button>
        <Button
          size="sm"
          variant="outline"
          aria-label="Open diagnostics folder"
          onClick={() => void openFolder()}
          disabled={busy !== null}
        >
          <FolderOpen data-icon="inline-start" />
          Open logs folder
        </Button>
        <Button
          size="sm"
          variant="outline"
          aria-label="Clear diagnostic logs"
          onClick={() => void clear()}
          disabled={busy !== null || !info || info.files.length === 0}
        >
          <Trash2 data-icon="inline-start" />
          {busy === "clear" ? "Clearing…" : "Clear logs"}
        </Button>
      </div>
    </section>
  );
}
