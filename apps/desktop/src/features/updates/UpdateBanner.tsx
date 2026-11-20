import { AlertTriangle, Download, Loader2, RefreshCw, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { useUpdateStore } from "@/state/update-store";

function percentLabel(percent: number | null): string {
  return percent === null ? "Downloading update..." : `Downloading update... ${percent}%`;
}

/**
 * Non-blocking update notice. The startup check only presents the signed
 * release; downloading and replacing the running build stay explicit because
 * Windows closes the app during installation.
 */
export function UpdateBanner() {
  const status = useUpdateStore((state) => state.status);
  const available = useUpdateStore((state) => state.available);
  const progress = useUpdateStore((state) => state.progress);
  const error = useUpdateStore((state) => state.error);
  const install = useUpdateStore((state) => state.install);
  const dismiss = useUpdateStore((state) => state.dismiss);

  if (!available || !["available", "downloading", "installing", "error"].includes(status)) {
    return null;
  }

  const busy = status === "downloading" || status === "installing";
  const failed = status === "error";

  return (
    <div
      role="status"
      aria-label="Application update"
      className={cn(
        "flex flex-wrap items-start gap-3 border-b px-3 py-2 text-[11px]",
        failed
          ? "border-status-error/40 bg-status-error/10"
          : "border-status-ok/40 bg-status-ok/10",
      )}
    >
      {failed ? (
        <AlertTriangle className="mt-0.5 size-4 shrink-0 text-status-error" />
      ) : (
        <Download className="mt-0.5 size-4 shrink-0 text-status-ok" />
      )}
      <div className="min-w-48 flex-1 space-y-0.5">
        <p className="font-medium text-foreground">
          {failed
            ? "The update could not be installed"
            : `FuryBuilder ${available.version} is available`}
        </p>
        {failed ? (
          <p className="leading-relaxed">{error ?? "Unknown update error."}</p>
        ) : busy ? (
          <div className="space-y-1">
            <p className="leading-relaxed">
              {status === "installing"
                ? "Installing the update. FuryBuilder will close automatically."
                : percentLabel(progress?.percent ?? null)}
            </p>
            {status === "downloading" ? (
              <div className="h-1.5 overflow-hidden rounded bg-background/70">
                <div
                  className="h-full bg-status-ok transition-[width]"
                  style={{ width: `${progress?.percent ?? 5}%` }}
                />
              </div>
            ) : null}
          </div>
        ) : (
          <p className="leading-relaxed">
            The update package was published on GitHub Releases and its updater signature is
            verified before installation. Installing closes FuryBuilder and keeps projects and
            settings. Current version {available.currentVersion}.
          </p>
        )}
      </div>
      <div className="flex flex-wrap items-center gap-1.5">
        {failed ? (
          <Button size="sm" onClick={() => void install()} disabled={busy}>
            <RefreshCw className="size-3.5" />
            Retry
          </Button>
        ) : null}
        {status === "available" ? (
          <Button size="sm" onClick={() => void install()}>
            <Download className="size-3.5" />
            Install and restart
          </Button>
        ) : null}
        {busy ? (
          <span className="flex items-center gap-1 px-1.5 text-[10px] text-muted-foreground">
            <Loader2 className="size-3 animate-spin" />
            {status === "installing" ? "Installing" : "Downloading"}
          </span>
        ) : null}
        {status !== "installing" ? (
          <Button size="sm" variant="ghost" onClick={dismiss} disabled={busy}>
            <X className="size-3.5" />
            Later
          </Button>
        ) : null}
      </div>
    </div>
  );
}
