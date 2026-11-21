import { AlertTriangle, Download, Loader2, RefreshCw, X } from "lucide-react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Progress } from "@/components/ui/progress";
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
    <Alert
      role="status"
      aria-label="Application update"
      variant={failed ? "destructive" : "default"}
      className="rounded-none border-x-0 border-t-0 px-4 py-3"
    >
      {failed ? <AlertTriangle /> : <Download />}
      <AlertTitle className="text-sm">
        {failed
          ? "The update could not be installed"
          : `FuryBuilder ${available.version} is available`}
      </AlertTitle>
      <AlertDescription>
        {failed ? (
          <p className="text-sm">{error ?? "Unknown update error."}</p>
        ) : busy ? (
          <div className="space-y-2">
            <p className="text-sm">
              {status === "installing"
                ? "Installing the update. FuryBuilder will close automatically."
                : percentLabel(progress?.percent ?? null)}
            </p>
            {status === "downloading" ? <Progress value={progress?.percent ?? 5} /> : null}
          </div>
        ) : (
          <p className="text-sm">
            The update package was published on GitHub Releases and its updater signature is
            verified before installation. Installing closes FuryBuilder and keeps projects and
            settings. Current version {available.currentVersion}.
          </p>
        )}
        <div className="mt-3 flex flex-wrap items-center gap-2">
          {failed ? (
            <Button size="sm" onClick={() => void install()} disabled={busy}>
              <RefreshCw data-icon="inline-start" />
              Retry
            </Button>
          ) : null}
          {status === "available" ? (
            <Button size="sm" onClick={() => void install()}>
              <Download data-icon="inline-start" />
              Install and restart
            </Button>
          ) : null}
          {busy ? (
            <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
              <Loader2 className="size-3.5 animate-spin" />
              {status === "installing" ? "Installing" : "Downloading"}
            </span>
          ) : null}
          {status !== "installing" ? (
            <Button size="sm" variant="ghost" onClick={dismiss} disabled={busy}>
              <X data-icon="inline-start" />
              Later
            </Button>
          ) : null}
        </div>
      </AlertDescription>
    </Alert>
  );
}
