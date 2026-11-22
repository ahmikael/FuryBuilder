import { useEffect, useState } from "react";
import { Loader2, ShieldAlert } from "lucide-react";

import { Alert, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Table, TableBody, TableCell, TableRow } from "@/components/ui/table";
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

  const details: Array<[string, string]> = [];
  if (summary) {
    details.push(
      ["Project", summary.projectId],
      ["Request", current?.requestId ?? ""],
      ["Revision", String(summary.worldRevision)],
      ["Access", summary.accessMode],
    );
    if (operation) {
      details.push(["Operation", operation.type], ["Dimension", operation.dimension]);
      if (operation.bounds) details.push(["Bounds", boundsLabel(operation.bounds)]);
      if (operation.pos) details.push(["Position", positionLabel(operation.pos)]);
      if (operation.state) details.push(["Block", operation.state]);
      if (operation.from && operation.to) {
        details.push(["Replace", `${operation.from} \u2192 ${operation.to}`]);
      }
      if (operation.changes !== undefined) {
        details.push(["Changes", String(operation.changes)]);
      }
    }
    if (summary.operationId) details.push(["Undo operation", summary.operationId]);
    if (summary.dimension) details.push(["Dimension", summary.dimension]);
    if (summary.bounds) details.push(["Bounds", boundsLabel(summary.bounds)]);
    if (summary.affectedBlocks !== undefined) {
      details.push(["Affected / changed", `${summary.affectedBlocks} / ${summary.changedBlocks}`]);
    }
    if (summary.blocks !== undefined && summary.blocks !== null) {
      details.push(["Blocks restored", summary.blocks.toLocaleString()]);
    }
  }

  return (
    <Dialog open={Boolean(current)} onOpenChange={() => undefined}>
      <DialogContent
        className="max-h-[90vh] max-w-3xl gap-6 overflow-y-auto"
        showCloseButton={false}
      >
        <DialogHeader>
          <DialogTitle className="flex flex-wrap items-center gap-2 text-xl">
            <ShieldAlert className="size-5 text-status-warn" />
            Approve an AI edit?
            {summary ? (
              <Badge variant="outline" className="font-mono">
                {summary.kind}
              </Badge>
            ) : null}
          </DialogTitle>
          <DialogDescription>
            An MCP client wants to change this project. Nothing has been written yet.
            {queue.length > 1 ? ` (${queue.length} requests waiting)` : null}
          </DialogDescription>
        </DialogHeader>

        {summary ? (
          <Card className="gap-0 overflow-hidden py-0">
            <CardContent className="p-0">
              <Table>
                <TableBody>
                  {details.map(([label, value], index) => (
                    <TableRow key={`${label}-${index}`}>
                      <TableCell className="w-44 text-muted-foreground">{label}</TableCell>
                      <TableCell className="text-right font-mono text-xs break-all whitespace-normal">
                        {value}
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </CardContent>
            {operation?.sample?.length ? (
              <div className="space-y-1.5 border-t px-4 py-3">
                <p className="text-xs font-medium text-muted-foreground">Sample</p>
                <div className="grid gap-1 font-mono text-xs">
                  {operation.sample.map((change) => (
                    <div key={positionLabel(change.pos)} className="flex justify-between gap-4">
                      <span className="text-muted-foreground">{positionLabel(change.pos)}</span>
                      <span>{change.state}</span>
                    </div>
                  ))}
                </div>
                {operation.sampleTruncated ? (
                  <p className="text-xs text-muted-foreground">
                    … more changes are included in the operation
                  </p>
                ) : null}
              </div>
            ) : null}
          </Card>
        ) : null}

        <p className="text-sm leading-relaxed text-muted-foreground">
          Approve only if you expected this request. Denying or closing leaves the world unchanged.
          The request expires after 90 seconds.
        </p>
        {error ? (
          <Alert variant="destructive">
            <ShieldAlert />
            <AlertTitle>{error}</AlertTitle>
          </Alert>
        ) : null}

        <DialogFooter>
          <Button variant="outline" disabled={submitting} onClick={() => void respond(false)}>
            Deny
          </Button>
          <Button disabled={submitting} onClick={() => void respond(true)}>
            {submitting ? <Loader2 data-icon="inline-start" className="animate-spin" /> : null}
            Approve and apply
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
