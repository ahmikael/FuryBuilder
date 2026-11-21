import {
  AlertTriangle,
  CheckCircle2,
  ChevronDown,
  Download,
  Loader2,
  RefreshCw,
  ShieldCheck,
} from "lucide-react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { Progress } from "@/components/ui/progress";
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
    <section aria-labelledby="updates-heading" className="min-w-0 space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3 id="updates-heading" className="flex items-center gap-2 text-base font-semibold">
          <Download className="size-4" /> Updates
        </h3>
        <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
          <ShieldCheck className="size-3.5" /> Updater-signed packages
        </span>
      </div>
      <p className="text-sm leading-relaxed text-muted-foreground">
        Installed builds check GitHub Releases four seconds after startup. A newer version is
        downloaded only after confirmation and is verified against the updater public key before
        Windows runs the installer. Projects and settings are preserved.
      </p>

      <Card className="gap-0 py-0">
        <CardContent className="grid grid-cols-[140px_1fr] gap-x-4 gap-y-2.5 p-6 text-sm">
          <span className="text-muted-foreground">Current version</span>
          <span className="font-mono">{currentVersion ?? "unknown"}</span>
          <span className="text-muted-foreground">Status</span>
          <span
            className={cn(
              "flex items-center gap-1.5 font-mono",
              status === "error" && "text-status-error",
              (status === "up-to-date" || status === "available") && "text-status-ok",
            )}
          >
            {status === "checking" || status === "downloading" || status === "installing" ? (
              <Loader2 className="size-3.5 animate-spin" />
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
        </CardContent>
      </Card>

      {available?.notes ? (
        <Collapsible className="rounded-xl border bg-card p-4 text-sm">
          <CollapsibleTrigger className="flex w-full cursor-pointer items-center justify-between gap-2 text-left text-muted-foreground [&[data-panel-open]>svg]:rotate-180">
            Release notes
            <ChevronDown className="size-4 shrink-0 transition-transform" />
          </CollapsibleTrigger>
          <CollapsibleContent>
            <div className="mt-2 max-h-40 overflow-auto whitespace-pre-line leading-relaxed">
              {available.notes}
            </div>
          </CollapsibleContent>
        </Collapsible>
      ) : null}

      {status === "downloading" ? (
        <div className="space-y-2">
          <Progress value={progress?.percent ?? 5} />
          <p className="text-sm text-muted-foreground">
            {progress?.percent === null || progress?.percent === undefined
              ? "Downloading..."
              : `Downloaded ${progress.percent}%`}
          </p>
        </div>
      ) : null}

      {status === "error" ? (
        <Alert variant="destructive" className="p-4">
          <AlertTriangle />
          <AlertTitle>Update failed</AlertTitle>
          <AlertDescription>{error ?? "The update could not be completed."}</AlertDescription>
        </Alert>
      ) : null}

      {status === "up-to-date" ? (
        <Alert className="p-4">
          <CheckCircle2 />
          <AlertTitle>You are up to date</AlertTitle>
          <AlertDescription>This is the newest published version.</AlertDescription>
        </Alert>
      ) : null}

      <div className="flex flex-wrap gap-2">
        <Button
          size="sm"
          variant="outline"
          onClick={() => void check()}
          disabled={!supported || busy}
        >
          <RefreshCw
            data-icon="inline-start"
            className={cn(status === "checking" && "animate-spin")}
          />
          {status === "checking" ? "Checking..." : "Check for updates"}
        </Button>
        {canInstall ? (
          <Button size="sm" onClick={() => void install()} disabled={busy}>
            <Download data-icon="inline-start" />
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
        <p className="text-xs leading-relaxed text-muted-foreground">
          Updates are disabled in browser preview and development builds. Install the Windows
          package to enable automatic update checks.
        </p>
      ) : null}
    </section>
  );
}
