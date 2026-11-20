import { useEffect, useState } from "react";
import { Loader2, ShieldAlert } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  getMcpAccessStatus,
  listenForMcpConfirmations,
  submitMcpConfirmation,
  type McpConfirmationRequest,
} from "@/lib/mcp-access";
import { isTauriRuntime } from "@/lib/ipc";

function positionLabel(position: { x: number; y: number; z: number }): string {
  return `${position.x}, ${position.y}, ${position.z}`;
}

function boundsLabel(bounds: {
  min: { x: number; y: number; z: number };
  max: { x: number; y: number; z: number };
}): string {
  return `${positionLabel(bounds.min)} → ${positionLabel(bounds.max)}`;
}

/**
 * Mandatory human approval for MCP writes. It renders regardless of which editor panel is
 * open; if this host is not mounted, writes that need approval fail at the endpoint.
 */
export function McpConfirmationHost() {
  const [queue, setQueue] = useState<McpConfirmationRequest[]>([]);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    let stop: (() => void) | undefined;
    void listenForMcpConfirmations((request) => {
      setQueue((current) =>
        current.some((entry) => entry.requestId === request.requestId)
          ? current
          : [...current, request],
      );
    }).then((unlisten) => {
      if (active) stop = unlisten;
      else unlisten();
    });
    // A WebView reload loses the event but not the pending request in Rust.
    if (isTauriRuntime()) {
      void getMcpAccessStatus()
        .then((status) => {
          if (!active) return;
          setQueue((current) => {
            const known = new Set(current.map((entry) => entry.requestId));
            const restored = status.pendingConfirmations.filter(
              (entry) => !known.has(entry.requestId) && entry.expiresAtMs > Date.now() - 1000,
            );
            return [...current, ...restored];
          });
        })
        .catch((error: unknown) => {
          // Browser preview has no desktop command; the live listener still works.
          console.debug("FuryBuilder could not restore pending MCP confirmations", error);
        });
    }
    return () => {
      active = false;
      stop?.();
    };
  }, []);

  const current = queue[0];

  useEffect(() => {
    if (!current) return;
    const remaining = current.expiresAtMs - Date.now();
    const timer = window.setTimeout(
      () => {
        setQueue((queue) => queue.filter((entry) => entry.requestId !== current.requestId));
      },
      Math.max(remaining, 0) + 1000,
    );
    return () => window.clearTimeout(timer);
  }, [current]);

  async function respond(approved: boolean) {
    if (!current) return;
    setSubmitting(true);
    setError(null);
    try {
      await submitMcpConfirmation(current.requestId, approved);
      setQueue((queue) => queue.filter((entry) => entry.requestId !== current.requestId));
    } catch (submitError) {
      setError(submitError instanceof Error ? submitError.message : String(submitError));
    } finally {
      setSubmitting(false);
    }
  }

  const summary = current?.summary;
  const operation = summary?.operation;

  return (
    <Dialog open={Boolean(current)} onOpenChange={() => undefined}>
      <DialogContent className="max-w-lg" showCloseButton={false}>
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <ShieldAlert className="size-4 text-status-warn" />
            Approve an AI edit?
          </DialogTitle>
          <DialogDescription>
            An MCP client wants to change this project. Nothing has been written yet.
            {queue.length > 1 ? ` (${queue.length} requests waiting)` : null}
          </DialogDescription>
        </DialogHeader>

        {summary ? (
          <div className="space-y-2 rounded border border-border bg-background/40 p-3 font-mono text-[11px]">
            <div className="flex justify-between gap-3">
              <span className="text-muted-foreground">Action</span>
              <span className="text-status-warn">{summary.kind}</span>
            </div>
            <div className="flex justify-between gap-3">
              <span className="text-muted-foreground">Project</span>
              <span>{summary.projectId}</span>
            </div>
            <div className="flex justify-between gap-3">
              <span className="text-muted-foreground">Request</span>
              <span className="text-[10px]">{current?.requestId}</span>
            </div>
            <div className="flex justify-between gap-3">
              <span className="text-muted-foreground">Revision</span>
              <span>{summary.worldRevision}</span>
            </div>
            <div className="flex justify-between gap-3">
              <span className="text-muted-foreground">Access</span>
              <span>{summary.accessMode}</span>
            </div>

            {operation ? (
              <>
                <div className="flex justify-between gap-3">
                  <span className="text-muted-foreground">Operation</span>
                  <span>{operation.type}</span>
                </div>
                <div className="flex justify-between gap-3">
                  <span className="text-muted-foreground">Dimension</span>
                  <span>{operation.dimension}</span>
                </div>
                {operation.bounds ? (
                  <div className="flex justify-between gap-3">
                    <span className="text-muted-foreground">Bounds</span>
                    <span>{boundsLabel(operation.bounds)}</span>
                  </div>
                ) : null}
                {operation.pos ? (
                  <div className="flex justify-between gap-3">
                    <span className="text-muted-foreground">Position</span>
                    <span>{positionLabel(operation.pos)}</span>
                  </div>
                ) : null}
                {operation.state ? (
                  <div className="flex justify-between gap-3">
                    <span className="text-muted-foreground">Block</span>
                    <span className="text-right">{operation.state}</span>
                  </div>
                ) : null}
                {operation.from && operation.to ? (
                  <div className="flex justify-between gap-3">
                    <span className="text-muted-foreground">Replace</span>
                    <span className="text-right">
                      {operation.from} → {operation.to}
                    </span>
                  </div>
                ) : null}
                {operation.changes !== undefined ? (
                  <div className="flex justify-between gap-3">
                    <span className="text-muted-foreground">Changes</span>
                    <span>{operation.changes}</span>
                  </div>
                ) : null}
              </>
            ) : null}

            {summary.operationId ? (
              <div className="flex justify-between gap-3">
                <span className="text-muted-foreground">Undo operation</span>
                <span>{summary.operationId}</span>
              </div>
            ) : null}
            {summary.dimension ? (
              <div className="flex justify-between gap-3">
                <span className="text-muted-foreground">Dimension</span>
                <span>{summary.dimension}</span>
              </div>
            ) : null}
            {summary.bounds ? (
              <div className="flex justify-between gap-3">
                <span className="text-muted-foreground">Bounds</span>
                <span>{boundsLabel(summary.bounds)}</span>
              </div>
            ) : null}

            {summary.affectedBlocks !== undefined ? (
              <div className="flex justify-between gap-3">
                <span className="text-muted-foreground">Affected / changed</span>
                <span>
                  {summary.affectedBlocks} / {summary.changedBlocks}
                </span>
              </div>
            ) : null}
            {summary.blocks !== undefined && summary.blocks !== null ? (
              <div className="flex justify-between gap-3">
                <span className="text-muted-foreground">Blocks restored</span>
                <span>{summary.blocks.toLocaleString()}</span>
              </div>
            ) : null}
            {operation?.sample?.length ? (
              <div className="space-y-0.5 border-t border-border pt-1.5">
                <span className="text-muted-foreground">Sample</span>
                {operation.sample.map((change) => (
                  <div key={positionLabel(change.pos)} className="text-[10px]">
                    {positionLabel(change.pos)} · {change.state}
                  </div>
                ))}
                {operation.sampleTruncated ? (
                  <div className="text-[10px] text-muted-foreground">
                    … more changes are included in the operation
                  </div>
                ) : null}
              </div>
            ) : null}
          </div>
        ) : null}

        <p className="text-[10px] text-muted-foreground">
          Approve only if you expected this request. Denying or closing leaves the world unchanged.
          The request expires after 90 seconds.
        </p>
        {error ? <p className="text-[11px] text-status-warn">{error}</p> : null}

        <DialogFooter>
          <Button
            variant="outline"
            size="sm"
            disabled={submitting}
            onClick={() => void respond(false)}
          >
            Deny
          </Button>
          <Button size="sm" disabled={submitting} onClick={() => void respond(true)}>
            {submitting ? <Loader2 className="animate-spin" /> : null}
            Approve and apply
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
