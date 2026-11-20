import {
  AlertTriangle,
  CheckCircle2,
  Download,
  Loader2,
  RefreshCw,
  ShieldCheck,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { canCheckForUpdates } from "@/lib/update-api";
import { cn } from "@/lib/utils";
import { useUpdateStore, type UpdateStatus } from "@/state/update-store";

interface UpdatesSectionProps {
  currentVersion: string | null;
}

const STATUS_LABELS: Record<UpdateStatus, string> = {
  idle: "Not checked yet",
  checking: "Checking GitHub Releases...",
  "up-to-date": "Up to date",
  available: "Update available",
  downloading: "Downloading update...",
  installing: "Installing and restarting...",
  error: "Update failed",
};

function formatCheckedAt(value: number | null): string {
  if (!value) return "Never";
  return new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" }).format(
    new Date(value),
  );
}

/**
 * Manual update controls plus the same state used by the startup check and the
 * non-blocking banner. Release downloads are verified by the Tauri updater
 * signature before the installer can run.
 */
export function UpdatesSection({ currentVersion }: UpdatesSectionProps) {
  const status = useUpdateStore((state) => state.status);
  const available = useUpdateStore((state) => state.available);
  const progress = useUpdateStore((state) => state.progress);
  const error = useUpdateStore((state) => state.error);
  const lastCheckedAt = useUpdateStore((state) => state.lastCheckedAt);
  const check = useUpdateStore((state) => state.check);
  const install = useUpdateStore((state) => state.install);
  const dismiss = useUpdateStore((state) => state.dismiss);

  const supported = canCheckForUpdates();
  const busy = status === "checking" || status === "downloading" || status === "installing";
  const canInstall = Boolean(available) && (status === "available" || status === "error");

  return (
    <section aria-labelledby="updates-heading" className="min-w-0 space-y-2">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3
          id="updates-heading"
          className="flex items-center gap-1.5 text-[11px] font-semibold tracking-wide uppercase"
        >
          <Download className="size-3.5" /> Updates
        </h3>
        <span className="flex items-center gap-1 text-[10px] text-muted-foreground">
          <ShieldCheck className="size-3" /> Updater-signed packages
        </span>
      </div>
      <p className="text-[10px] text-muted-foreground">
        Installed builds check GitHub Releases four seconds after startup. A newer version is
        downloaded only after confirmation and is verified against the updater public key before
        Windows runs the installer. Projects and settings are preserved.
      </p>

      <div className="grid grid-cols-[120px_1fr] gap-x-3 gap-y-1 rounded border border-border bg-background/40 p-2 text-[11px]">
        <span className="text-muted-foreground">Current version</span>
        <span className="font-mono">{currentVersion ?? "unknown"}</span>
        <span className="text-muted-foreground">Status</span>
        <span
          className={cn(
            "flex items-center gap-1 font-mono",
            status === "error" && "text-status-error",
            (status === "up-to-date" || status === "available") && "text-status-ok",
          )}
        >
          {status === "checking" || status === "downloading" || status === "installing" ? (
            <Loader2 className="size-3 animate-spin" />
          ) : null}
          {status === "available" && available
            ? `${STATUS_LABELS[status]} (${available.version})`
            : STATUS_LABELS[status]}
        </span>
        <span className="text-muted-foreground">Last check</span>
        <span>{formatCheckedAt(lastCheckedAt)}</span>
        {available ? (
          <>
            <span className="text-muted-foreground">New version</span>
            <span className="font-mono">{available.version}</span>
          </>
        ) : null}
      </div>

      {available?.notes ? (
        <details className="rounded border border-border bg-background/40 p-2 text-[10px]">
          <summary className="cursor-pointer text-muted-foreground">Release notes</summary>
          <div className="mt-1 max-h-32 overflow-auto whitespace-pre-line">{available.notes}</div>
        </details>
      ) : null}

      {status === "downloading" ? (
        <div className="space-y-1">
          <div className="h-1.5 overflow-hidden rounded bg-background/70">
            <div
              className="h-full bg-status-ok transition-[width]"
              style={{ width: `${progress?.percent ?? 5}%` }}
            />
          </div>
          <p className="text-[10px] text-muted-foreground">
            {progress?.percent === null || progress?.percent === undefined
              ? "Downloading..."
              : `Downloaded ${progress.percent}%`}
          </p>
        </div>
      ) : null}

      {status === "error" ? (
        <p className="flex items-start gap-1.5 rounded border border-status-error/40 bg-status-error/10 px-2 py-1 text-[10px] text-status-error">
          <AlertTriangle className="mt-0.5 size-3 shrink-0" />
          {error ?? "The update could not be completed."}
        </p>
      ) : null}

      {status === "up-to-date" ? (
        <p className="flex items-center gap-1 text-[10px] text-status-ok">
          <CheckCircle2 className="size-3" /> This is the newest published version.
        </p>
      ) : null}

      <div className="flex flex-wrap gap-1.5">
        <Button
          size="sm"
          variant="outline"
          onClick={() => void check()}
          disabled={!supported || busy}
        >
          <RefreshCw className={status === "checking" ? "size-3.5 animate-spin" : "size-3.5"} />
          {status === "checking" ? "Checking..." : "Check for updates"}
        </Button>
        {canInstall ? (
          <Button size="sm" onClick={() => void install()} disabled={busy}>
            <Download className="size-3.5" />
            {status === "error" ? "Retry update" : "Install and restart"}
          </Button>
        ) : null}
        {available && !busy ? (
          <Button size="sm" variant="ghost" onClick={dismiss}>
            Later
          </Button>
        ) : null}
      </div>

      {!supported ? (
        <p className="text-[10px] text-muted-foreground">
          Updates are disabled in browser preview and development builds. Install the Windows
          package to enable automatic update checks.
        </p>
      ) : null}
    </section>
  );
}
