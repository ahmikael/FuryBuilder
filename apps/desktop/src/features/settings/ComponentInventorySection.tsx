import { useCallback, useEffect, useState, type ReactNode } from "react";
import {
  Coffee,
  Image,
  Loader2,
  PackageCheck,
  RefreshCw,
  Server,
  TriangleAlert,
} from "lucide-react";

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { componentInventory, listenComponentChanged } from "@/lib/components-api";
import type { ComponentInventory } from "@/lib/component-types";
import { presentError, type ErrorPresentation } from "@/lib/error-messages";
import { displayRuntimePath, formatBytes } from "@/lib/runtime-api";

interface InventoryGroupProps {
  title: string;
  icon: ReactNode;
  count: number;
  totalBytes: number;
  root: string;
  empty: string;
  problems: string[];
  children: ReactNode;
}

function InventoryGroup({
  title,
  icon,
  count,
  totalBytes,
  root,
  empty,
  problems,
  children,
}: InventoryGroupProps) {
  return (
    <Card role="group" aria-label={title} className="min-w-0 gap-0 py-0">
      <CardContent className="space-y-4 p-6">
        <div className="flex flex-wrap items-center gap-2">
          {icon}
          <h4 className="text-sm font-semibold">{title}</h4>
          <Badge variant="secondary" className="ml-auto font-mono">
            {count} · {formatBytes(totalBytes)}
          </Badge>
        </div>
        {count === 0 ? (
          <p className="text-sm leading-relaxed text-muted-foreground">{empty}</p>
        ) : (
          <ul className="space-y-3">{children}</ul>
        )}
        <p
          className="truncate font-mono text-xs text-muted-foreground"
          title={displayRuntimePath(root)}
        >
          {displayRuntimePath(root)}
        </p>
        {problems.map((problem) => (
          <p key={problem} className="text-sm text-status-warn">
            {problem}
          </p>
        ))}
      </CardContent>
    </Card>
  );
}

/**
 * Read-only view of FuryBuilder-owned component caches. Selecting a download, a manual path
 * or placeholder textures belongs to the project wizard; Settings only inspects and maintains
 * the shared artifacts, never provisions a project from here.
 */
export function ComponentInventorySection() {
  const [inventory, setInventory] = useState<ComponentInventory | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<ErrorPresentation | null>(null);
  const [reloadKey, setReloadKey] = useState(0);

  const reload = useCallback(() => {
    setLoading(true);
    setReloadKey((value) => value + 1);
  }, []);

  useEffect(() => {
    let cancelled = false;
    void componentInventory()
      .then((next) => {
        if (cancelled) return;
        setInventory(next);
        setError(null);
      })
      .catch((cause: unknown) => {
        if (cancelled) return;
        setInventory(null);
        setError(presentError(cause));
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
    let stop: (() => void) | null = null;
    void listenComponentChanged(() => {
      if (!disposed) reload();
    })
      .then((unlisten) => {
        if (disposed) unlisten();
        else stop = unlisten;
      })
      .catch(() => {
        // A browser preview has no Tauri event bridge; the inventory still renders.
      });
    return () => {
      disposed = true;
      stop?.();
    };
  }, [reload]);

  return (
    <section aria-labelledby="components-inventory-heading" className="min-w-0 space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3
          id="components-inventory-heading"
          className="flex items-center gap-2 text-base font-semibold"
        >
          <PackageCheck className="size-4" /> Installed components
        </h3>
        <Button
          variant="ghost"
          size="icon-sm"
          onClick={reload}
          disabled={loading}
          aria-label="Refresh component inventory"
        >
          {loading ? <Loader2 className="animate-spin" /> : <RefreshCw />}
        </Button>
      </div>
      <p className="text-sm leading-relaxed text-muted-foreground">
        FuryBuilder-owned Java runtimes, Minecraft client JARs and official server JARs in the
        shared local cache. Settings only lists these artifacts; choosing a download, a manual path
        or placeholder textures happens in the project wizard.
      </p>

      {loading && !inventory ? (
        <p className="flex items-center gap-2 text-sm text-muted-foreground">
          <Loader2 className="size-4 animate-spin" /> Reading the local component cache…
        </p>
      ) : error ? (
        <Alert>
          <TriangleAlert />
          <AlertTitle>Component inventory unavailable</AlertTitle>
          <AlertDescription>
            <p className="text-status-warn">{error.message}</p>
            {error.hint ? <p>{error.hint}</p> : null}
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
      ) : inventory ? (
        <div className="space-y-4">
          <InventoryGroup
            title="Java runtimes"
            icon={<Coffee className="size-4" />}
            count={inventory.java.runtimes.length}
            totalBytes={inventory.java.totalBytes}
            root={inventory.java.root}
            empty="No managed Java runtimes. The project wizard can download or link one when a project needs Java."
            problems={inventory.java.problems}
          >
            {inventory.java.runtimes.map((runtime) => (
              <li key={runtime.id} className="min-w-0 rounded-lg border bg-background p-3">
                <div className="flex flex-wrap items-center gap-2 text-sm">
                  <span className="font-medium">Java {runtime.version}</span>
                  <Badge variant="outline">Java {runtime.major}</Badge>
                  <span className="font-mono text-xs text-muted-foreground">{runtime.vendor}</span>
                  <span className="ml-auto font-mono text-xs text-muted-foreground">
                    {formatBytes(runtime.bytes)}
                  </span>
                </div>
                <p
                  className="truncate font-mono text-xs text-muted-foreground"
                  title={displayRuntimePath(runtime.javaPath)}
                >
                  {displayRuntimePath(runtime.javaPath)}
                </p>
              </li>
            ))}
          </InventoryGroup>

          <InventoryGroup
            title="Minecraft client JARs"
            icon={<Image className="size-4" />}
            count={inventory.assets.clients.length}
            totalBytes={inventory.assets.totalBytes}
            root={inventory.assets.root}
            empty="No managed client JARs. The project wizard can download or link the exact-version client assets."
            problems={inventory.assets.problems}
          >
            {inventory.assets.clients.map((client) => (
              <li key={client.id} className="min-w-0 rounded-lg border bg-background p-3">
                <div className="flex flex-wrap items-center gap-2 text-sm">
                  <span className="font-medium">Minecraft {client.version}</span>
                  <Badge variant="outline">DataVersion {client.dataVersion}</Badge>
                  <span className="ml-auto font-mono text-xs text-muted-foreground">
                    {formatBytes(client.size)}
                  </span>
                </div>
                <p
                  className="truncate font-mono text-xs text-muted-foreground"
                  title={displayRuntimePath(client.jarPath)}
                >
                  {displayRuntimePath(client.jarPath)}
                </p>
              </li>
            ))}
          </InventoryGroup>

          <InventoryGroup
            title="Official server JARs"
            icon={<Server className="size-4" />}
            count={inventory.servers.servers.length}
            totalBytes={inventory.servers.totalBytes}
            root={inventory.servers.root}
            empty="No managed server JARs. Vanilla World provisions the exact-version official server in the project wizard."
            problems={inventory.servers.problems}
          >
            {inventory.servers.servers.map((server) => (
              <li key={server.version} className="min-w-0 rounded-lg border bg-background p-3">
                <div className="flex flex-wrap items-center gap-2 text-sm">
                  <span className="font-medium">Minecraft {server.version}</span>
                  <Badge variant="outline">DataVersion {server.dataVersion}</Badge>
                  <span className="font-mono text-xs text-muted-foreground">
                    Java {server.javaMajor}
                  </span>
                  <span className="ml-auto font-mono text-xs text-muted-foreground">
                    {formatBytes(server.size)}
                  </span>
                </div>
                <p
                  className="truncate font-mono text-xs text-muted-foreground"
                  title={displayRuntimePath(server.jarPath)}
                >
                  {displayRuntimePath(server.jarPath)}
                </p>
              </li>
            ))}
          </InventoryGroup>
        </div>
      ) : null}
    </section>
  );
}
