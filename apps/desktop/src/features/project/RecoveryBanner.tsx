import { AlertTriangle, CheckCircle2, RotateCcw, Save, X } from "lucide-react";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
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
    <div
      role="alert"
      aria-label="Crash recovery"
      className={cn(
        "flex flex-wrap items-start gap-3 border-b px-3 py-2 text-[11px]",
        available
          ? "border-status-warn/40 bg-status-warn/10"
          : "border-border bg-muted/40 text-muted-foreground",
      )}
    >
      {available ? (
        <CheckCircle2 className="mt-0.5 size-4 shrink-0 text-status-warn" />
      ) : (
        <AlertTriangle className="mt-0.5 size-4 shrink-0 text-status-warn" />
      )}
      <div className="min-w-48 flex-1 space-y-0.5">
        <p className="font-medium text-foreground">
          {available ? "Recovered unsaved changes" : "Crash recovery could not be applied"}
        </p>
        <p className="leading-relaxed">{recovery.reason}</p>
        <p className="text-muted-foreground">
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
      </div>
      <div className="flex flex-wrap items-center gap-1.5">
        {available ? (
          <>
            <Button size="sm" onClick={() => void saveRecovered()} disabled={busy !== null}>
              <Save className="size-3.5" />
              {busy === "save" ? "Saving..." : "Save now"}
            </Button>
            <Button
              size="sm"
              variant="outline"
              onClick={() => void discard()}
              disabled={busy !== null}
            >
              <RotateCcw className="size-3.5" />
              {busy === "discard" ? "Discarding..." : "Discard recovered changes"}
            </Button>
          </>
        ) : null}
        <Button size="sm" variant="ghost" onClick={dismissRecovery} disabled={busy !== null}>
          <X className="size-3.5" />
          {available ? "Later" : "Dismiss"}
        </Button>
      </div>
    </div>
  );
}
