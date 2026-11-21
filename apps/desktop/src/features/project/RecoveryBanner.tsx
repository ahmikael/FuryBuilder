import { AlertTriangle, CheckCircle2, RotateCcw, Save, X } from "lucide-react";
import { useState } from "react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { useProjectStore } from "@/state/project-store";

function formatRecoveredAt(ms: number): string | null {
  if (!Number.isFinite(ms) || ms <= 0) return null;
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(ms));
}

/**
 * Persistent crash-recovery status. A verification failure is shown just as clearly as a
 * successful recovery, and destructive actions are always explicit.
 */
export function RecoveryBanner() {
  const recovery = useProjectStore((state) => state.recovery);
  const savedRevision = useProjectStore((state) => state.current?.savedRevision ?? null);
  const save = useProjectStore((state) => state.save);
  const discardRecovery = useProjectStore((state) => state.discardRecovery);
  const dismissRecovery = useProjectStore((state) => state.dismissRecovery);
  const [busy, setBusy] = useState<"save" | "discard" | null>(null);

  if (!recovery) return null;

  const available = recovery.available;
  const when = formatRecoveredAt(recovery.recoveredAtMs);
  const autosaveRevision = recovery.autosaveRevision ?? recovery.recoveredRevision;

  const saveRecovered = async (): Promise<void> => {
    setBusy("save");
    try {
      await save();
    } finally {
      setBusy(null);
    }
  };

  const discard = async (): Promise<void> => {
    setBusy("discard");
    try {
      await discardRecovery();
    } finally {
      setBusy(null);
    }
  };

  return (
    <Alert
      role="alert"
      aria-label="Crash recovery"
      className="rounded-none border-x-0 border-t-0 border-status-warn/40 bg-status-warn/10 px-4 py-3"
    >
      {available ? (
        <CheckCircle2 className="text-status-warn" />
      ) : (
        <AlertTriangle className="text-status-warn" />
      )}
      <AlertTitle className="text-sm">
        {available ? "Recovered unsaved changes" : "Crash recovery could not be applied"}
      </AlertTitle>
      <AlertDescription>
        <p className="text-sm text-status-warn">{recovery.reason}</p>
        <p className="mt-1 text-sm text-muted-foreground">
          {available ? (
            <>
              Autosave revision {autosaveRevision}
              {when ? ` from ${when}` : ""} is loaded in memory.
              {savedRevision !== null
                ? ` The project on disk is revision ${savedRevision}.`
                : ""}{" "}
              Save to publish the recovered work.
            </>
          ) : (
            <>
              Opened the last saved revision {recovery.recoveredRevision}
              {savedRevision !== null && savedRevision !== recovery.recoveredRevision
                ? ` (manifest revision ${savedRevision})`
                : ""}
              . The unverified autosave was not loaded; it stays on disk for diagnostics.
            </>
          )}
        </p>
        <div className="mt-3 flex flex-wrap items-center gap-2">
          {available ? (
            <>
              <Button size="sm" onClick={() => void saveRecovered()} disabled={busy !== null}>
                <Save data-icon="inline-start" />
                {busy === "save" ? "Saving..." : "Save now"}
              </Button>
              <Button
                size="sm"
                variant="outline"
                onClick={() => void discard()}
                disabled={busy !== null}
              >
                <RotateCcw data-icon="inline-start" />
                {busy === "discard" ? "Discarding..." : "Discard recovered changes"}
              </Button>
            </>
          ) : null}
          <Button size="sm" variant="ghost" onClick={dismissRecovery} disabled={busy !== null}>
            <X data-icon="inline-start" />
            {available ? "Later" : "Dismiss"}
          </Button>
        </div>
      </AlertDescription>
    </Alert>
  );
}
