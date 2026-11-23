import { CheckCircle2, Loader2, RefreshCw, ShieldCheck, TriangleAlert } from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { listenRuntimeChanged } from "@/lib/runtime-api";
import { listenServerChanged, vanillaSetupCheck } from "@/lib/server-api";
import { cn } from "@/lib/utils";
import type { VanillaSetupReport } from "@/runtime/server-types";

function javaLabel(report: VanillaSetupReport): string {
  if (!report.java) return "Not selected";
  return `${report.java.version ?? "unknown"} · ${report.java.vendor ?? "unknown"} · ${report.java.status.replaceAll("_", " ")}`;
}

function serverLabel(report: VanillaSetupReport): string {
  if (!report.server) return "Not selected";
  return `Minecraft ${report.server.version ?? "unknown"} · DV ${report.server.dataVersion ?? "?"} · ${report.server.ready ? "ready" : "needs attention"}`;
}

function eulaLabel(report: VanillaSetupReport): string {
  if (!report.eulaMatchesSelected || report.eulaAcceptedAt === null) return "Not accepted";
  return `Accepted ${new Date(report.eulaAcceptedAt * 1000).toLocaleString()}`;
}

/**
 * Read-only roll-up of the verified Vanilla World dependencies. It never probes, downloads,
 * accepts the EULA or starts a process; the individual sections above own those actions.
 */
export function DependencyReadiness() {
  const [report, setReport] = useState<VanillaSetupReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [reloadKey, setReloadKey] = useState(0);

  const reload = useCallback(() => {
    setLoading(true);
    setError(null);
    setReloadKey((value) => value + 1);
  }, []);

  useEffect(() => {
    let cancelled = false;
    void vanillaSetupCheck()
      .then((next) => {
        if (cancelled) return;
        setReport(next);
        setError(null);
      })
      .catch((cause: unknown) => {
        if (cancelled) return;
        setReport(null);
        setError(cause instanceof Error ? cause.message : String(cause));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [reloadKey]);

  useEffect(() => {
    let disposed = false;
    let stopRuntime: (() => void) | null = null;
    let stopServer: (() => void) | null = null;
    void listenRuntimeChanged(() => {
      if (!disposed) reload();
    })
      .then((stop) => {
        if (disposed) stop();
        else stopRuntime = stop;
      })
      .catch(() => {
        // Browser preview has no Tauri event bridge; the card still renders.
      });
    void listenServerChanged(() => {
      if (!disposed) reload();
    })
      .then((stop) => {
        if (disposed) stop();
        else stopServer = stop;
      })
      .catch(() => {
        // Browser preview has no Tauri event bridge; the card still renders.
      });
    return () => {
      disposed = true;
      stopRuntime?.();
      stopServer?.();
    };
  }, [reload]);

  const target = report
    ? `Minecraft ${report.targetMinecraftVersion} · DV ${report.targetDataVersion} · Java ${report.targetJavaMajor} · datapack ${report.serverDatapackFormat}`
    : "Target version unavailable";

  return (
    <section className="min-w-0 space-y-4" aria-labelledby="vanilla-readiness-heading">
      <div className="flex flex-wrap items-center gap-2">
        <h3
          id="vanilla-readiness-heading"
          className="flex items-center gap-2 text-base font-semibold"
        >
          <ShieldCheck className="size-4" /> Vanilla World readiness
        </h3>
        <Badge variant="secondary" className="font-mono">
          {target}
        </Badge>
        <Button
          variant="ghost"
          size="icon-sm"
          className="ml-auto"
          onClick={reload}
          disabled={loading}
          aria-label="Refresh Vanilla World readiness"
        >
          {loading ? <Loader2 className="animate-spin" /> : <RefreshCw />}
        </Button>
      </div>
      <p className="text-sm leading-relaxed text-muted-foreground">
        Reads the verified local Java, server JAR and EULA state. It never downloads, accepts the
        EULA or starts a server. Empty Project and Import World do not need any of these.
      </p>

      {loading && !report ? (
        <p className="flex items-center gap-2 text-sm text-muted-foreground">
          <Loader2 className="size-4 animate-spin" /> Checking local dependencies…
        </p>
      ) : error ? (
        <Alert>
          <TriangleAlert />
          <AlertTitle>Readiness check unavailable</AlertTitle>
          <AlertDescription>
            <p className="text-status-warn">{error}</p>
            <p>
              This check reads local state only. Empty Project and Import World still work offline
              and do not require Java or the server JAR.
            </p>
            <Button
              variant="outline"
              size="sm"
              className="mt-3"
              onClick={reload}
              disabled={loading}
            >
              {loading ? (
                <Loader2 data-icon="inline-start" className="animate-spin" />
              ) : (
                <RefreshCw data-icon="inline-start" />
              )}
              Retry
            </Button>
          </AlertDescription>
        </Alert>
      ) : report ? (
        <Card className="gap-0 py-0">
          <CardContent className="space-y-4 p-6">
            <div className="flex flex-wrap items-center gap-2">
              {report.ready ? (
                <CheckCircle2 className="size-4 text-status-ok" />
              ) : (
                <TriangleAlert className="size-4 text-status-warn" />
              )}
              <span
                className={cn(
                  "text-sm font-medium",
                  report.ready ? "text-status-ok" : "text-status-warn",
                )}
              >
                {report.ready ? "Vanilla World is ready" : "Setup required"}
              </span>
              <span className="ml-auto text-xs text-muted-foreground">
                {report.ready
                  ? "Generation still starts only from the wizard."
                  : `${report.blockers.length} blocker(s)`}
              </span>
            </div>

            <div className="grid gap-x-4 gap-y-2 text-sm sm:grid-cols-[120px_1fr]">
              <span className="text-muted-foreground">Java</span>
              <span
                className={cn(
                  "truncate font-mono text-xs",
                  report.javaReady ? "text-status-ok" : "text-status-warn",
                )}
                title={javaLabel(report)}
              >
                {javaLabel(report)}
              </span>
              <span className="text-muted-foreground">Server JAR</span>
              <span
                className={cn(
                  "truncate font-mono text-xs",
                  report.serverReady ? "text-status-ok" : "text-status-warn",
                )}
                title={serverLabel(report)}
              >
                {serverLabel(report)}
              </span>
              <span className="text-muted-foreground">Minecraft EULA</span>
              <span
                className={cn(
                  "truncate font-mono text-xs",
                  report.eulaMatchesSelected ? "text-status-ok" : "text-status-warn",
                )}
              >
                {eulaLabel(report)}
              </span>
            </div>

            {report.blockers.length > 0 ? (
              <div className="space-y-2 rounded-lg border border-status-warn/40 bg-status-warn/5 p-3">
                {report.blockers.map((blocker) => (
                  <p key={blocker} className="flex gap-2 text-sm text-status-warn">
                    <TriangleAlert className="mt-0.5 size-3.5 shrink-0" />
                    <span>{blocker}</span>
                  </p>
                ))}
              </div>
            ) : (
              <p className="text-xs leading-relaxed text-muted-foreground">
                All prerequisites were verified locally. No dependency is downloaded automatically.
              </p>
            )}
          </CardContent>
        </Card>
      ) : null}
    </section>
  );
}
