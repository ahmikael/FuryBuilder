import { useState } from "react";
import { Loader2, ScanLine, ShieldAlert, ShieldCheck } from "lucide-react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { checkVanillaParity } from "@/lib/project-api";
import type { VanillaParityReport, VanillaParityStatus } from "@/project/types";
import { cn } from "@/lib/utils";

interface VanillaParityPanelProps {
  folder: string;
}

const STATUS_LABEL: Record<VanillaParityStatus, string> = {
  matched: "Block parity matched",
  mismatched: "Block parity differs",
  refused: "Parity check refused",
};

const STATUS_CLASS: Record<VanillaParityStatus, string> = {
  matched: "bg-status-ok/15 text-status-ok",
  mismatched: "bg-status-error/15 text-status-error",
  refused: "bg-status-warn/15 text-status-warn",
};

/**
 * M9.8: compare the saved generated base with the original generator's reference chunks.
 * The backend refuses edited snapshots, mismatched generator settings and unsupported
 * reference layouts instead of reporting a misleading match.
 */
export function VanillaParityPanel({ folder }: VanillaParityPanelProps) {
  const [busy, setBusy] = useState(false);
  const [report, setReport] = useState<VanillaParityReport | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function verify(): Promise<void> {
    setBusy(true);
    setError(null);
    try {
      setReport(await checkVanillaParity(folder));
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  }

  const comparison = report?.comparison ?? null;
  return (
    <section className="space-y-4 rounded-xl border bg-card p-4">
      <div className="flex flex-wrap items-center gap-2">
        <ShieldCheck className="size-4 text-primary" />
        <span className="text-base font-semibold">Generated base verification</span>
        {report ? (
          <Badge variant="outline" className={cn("uppercase", STATUS_CLASS[report.status])}>
            {STATUS_LABEL[report.status]}
          </Badge>
        ) : (
          <Badge variant="secondary" className="uppercase">
            not checked
          </Badge>
        )}
        <Button
          size="sm"
          variant="outline"
          className="ml-auto"
          onClick={() => void verify()}
          disabled={busy}
          aria-label="Verify generated base"
        >
          {busy ? (
            <Loader2 className="mr-1 size-3 animate-spin" />
          ) : (
            <ScanLine className="mr-1 size-3" />
          )}
          Verify reference chunks
        </Button>
      </div>
      <p className="text-sm leading-relaxed text-muted-foreground">
        Compares the saved imported base with the original generator's requested overworld chunks.
        Only an untouched base (revision 1) can be checked; the generated source stays read-only.
      </p>

      {error ? (
        <Alert variant="destructive" className="p-4">
          <ShieldAlert />
          <AlertTitle>Parity check failed</AlertTitle>
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      ) : null}

      {report ? (
        <div className="space-y-3">
          <div className="flex flex-wrap items-center gap-2 font-mono text-xs text-muted-foreground">
            <span>seed {report.seed}</span>
            <span>preset {report.preset}</span>
            <span>
              center {report.centerX},{report.centerZ}
            </span>
            <span>radius {report.radiusChunks}</span>
            <span>
              reference{" "}
              {report.referenceKind === "generatedRun" ? "original run" : "project source copy"}
            </span>
            <span>rev {report.snapshotRevision}</span>
          </div>
          {comparison ? (
            <>
              <div className="text-sm text-muted-foreground" data-testid="vanilla-parity-counts">
                checked{" "}
                <span className="font-mono text-foreground">
                  {comparison.checkedChunks} of {comparison.sampledChunks}
                </span>{" "}
                sampled reference chunk(s) ({comparison.requestedChunks} requested) ·{" "}
                <span className="font-mono text-foreground">
                  {comparison.checkedSections.toLocaleString()}
                </span>{" "}
                sections ·{" "}
                <span className="font-mono text-foreground">
                  {comparison.checkedBlocks.toLocaleString()}
                </span>{" "}
                block positions ·{" "}
                <span
                  className={cn(
                    "font-mono",
                    comparison.mismatchCount === 0 ? "text-status-ok" : "text-status-error",
                  )}
                >
                  {comparison.mismatchCount.toLocaleString()}
                </span>{" "}
                difference(s)
              </div>
              {comparison.mismatches.length > 0 ? (
                <div className="scrollbar-thin max-h-28 space-y-1 overflow-y-auto rounded-lg border border-status-error/30 bg-status-error/5 p-3 font-mono text-xs text-status-error">
                  {comparison.mismatches.slice(0, 8).map((mismatch) => (
                    <div key={`${mismatch.x},${mismatch.y},${mismatch.z}`}>
                      ({mismatch.x}, {mismatch.y}, {mismatch.z}) reference {mismatch.reference} ·
                      project {mismatch.project}
                    </div>
                  ))}
                </div>
              ) : null}
              {comparison.missingProjectChunks.length > 0 ? (
                <div className="text-sm text-status-error">
                  {comparison.missingProjectChunks.length} reference chunk(s) have no matching
                  editable chunk.
                </div>
              ) : null}
            </>
          ) : null}
          {report.refusals.length > 0 ? (
            <div className="space-y-1 rounded-lg border border-status-warn/30 bg-status-warn/5 p-3 text-sm text-status-warn">
              {report.refusals.map((refusal) => (
                <div key={refusal}>{refusal}</div>
              ))}
            </div>
          ) : null}
          {report.fromAutosave ? (
            <div className="text-sm text-status-warn">
              The verified autosave was checked instead of the last manual save.
            </div>
          ) : null}
          {report.limitations.length > 0 ? (
            <details className="rounded-lg border bg-muted/30 p-4">
              <summary className="cursor-pointer text-sm font-medium">
                Parity scope and limitations
              </summary>
              <ul className="mt-2 list-disc space-y-1 pl-4 text-xs text-muted-foreground">
                {report.limitations.map((limitation) => (
                  <li key={limitation}>{limitation}</li>
                ))}
              </ul>
            </details>
          ) : null}
        </div>
      ) : null}
    </section>
  );
}
