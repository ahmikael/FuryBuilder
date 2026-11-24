import { useCallback, useEffect, useState, type ReactNode } from "react";
import {
  CheckCircle2,
  Coffee,
  FolderOpen,
  Image,
  Loader2,
  PackageCheck,
  RefreshCw,
  Server,
  ShieldCheck,
  Trash2,
  TriangleAlert,
} from "lucide-react";

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
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
import {
  componentInventory,
  listenComponentChanged,
  openComponentFolder,
  removeComponent,
  verifyComponentInventory,
} from "@/lib/components-api";
import type {
  ComponentInventory,
  ComponentKind,
  ComponentVerification,
} from "@/lib/component-types";
import { presentError, type ErrorPresentation } from "@/lib/error-messages";
import { displayRuntimePath, formatBytes } from "@/lib/runtime-api";

type ItemAction = "verify" | "remove";
type BusyAction = { kind: ComponentKind; id: string; action: ItemAction } | null;
type ConfirmRemoval = { kind: ComponentKind; id: string; label: string } | null;

interface InventoryGroupProps {
  title: string;
  icon: ReactNode;
  count: number;
  totalBytes: number;
  root: string;
  empty: string;
  problems: string[];
  opening: boolean;
  onOpenFolder: () => void;
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
  opening,
  onOpenFolder,
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
          <Button
            variant="outline"
            size="sm"
            onClick={onOpenFolder}
            disabled={opening}
            aria-label={`Open ${title} folder`}
          >
            {opening ? (
              <Loader2 data-icon="inline-start" className="animate-spin" />
            ) : (
              <FolderOpen data-icon="inline-start" />
            )}
            Open folder
          </Button>
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

interface InventoryItemProps {
  title: string;
  meta: ReactNode;
  path: string;
  size: string;
  verification?: ComponentVerification;
  busy: ItemAction | null;
  locked: boolean;
  onVerify: () => void;
  onAskRemove: () => void;
}

function InventoryItem({
  title,
  meta,
  path,
  size,
  verification,
  busy,
  locked,
  onVerify,
  onAskRemove,
}: InventoryItemProps) {
  return (
    <li className="min-w-0 rounded-lg border bg-background p-3">
      <div className="flex flex-wrap items-center gap-2 text-sm">
        <span className="font-medium">{title}</span>
        {meta}
        {verification ? (
          <Badge variant={verification.verified ? "secondary" : "destructive"}>
            {verification.verified ? "Verified" : "Needs attention"}
          </Badge>
        ) : null}
        <span className="ml-auto font-mono text-xs text-muted-foreground">{size}</span>
      </div>
      <p
        className="truncate font-mono text-xs text-muted-foreground"
        title={displayRuntimePath(path)}
      >
        {displayRuntimePath(path)}
      </p>
      <div className="flex flex-wrap items-center gap-2 pt-1">
        <Button
          variant="outline"
          size="sm"
          onClick={onVerify}
          disabled={locked}
          aria-label={`Verify ${title}`}
        >
          {busy === "verify" ? (
            <Loader2 data-icon="inline-start" className="animate-spin" />
          ) : (
            <ShieldCheck data-icon="inline-start" />
          )}
          Verify
        </Button>
        <Button
          variant="ghost"
          size="sm"
          onClick={onAskRemove}
          disabled={locked}
          aria-label={`Remove ${title}`}
        >
          <Trash2 data-icon="inline-start" />
          Remove
        </Button>
      </div>
      {verification && !verification.verified ? (
        <p className="text-sm text-status-warn">
          {verification.error ?? "The artifact did not pass local verification."}
        </p>
      ) : null}
      {verification?.notes?.length ? (
        <p className="text-xs leading-relaxed text-muted-foreground">
          {verification.notes.join(" ")}
        </p>
      ) : null}
    </li>
  );
}

function itemKey(kind: ComponentKind, id: string): string {
  return `${kind}:${id}`;
}

function serverInventoryId(version: string): string {
  return `server-managed-${version}`;
}

/**
 * Maintenance view of the shared component cache: size, path, local verification, removal
 * and the cache folder. Selecting a download, a manual path or placeholder textures belongs
 * to the project wizard; Settings never provisions a project from here.
 */
export function ComponentInventorySection() {
  const [inventory, setInventory] = useState<ComponentInventory | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<ErrorPresentation | null>(null);
  const [reloadKey, setReloadKey] = useState(0);
  const [action, setAction] = useState<BusyAction>(null);
  const [opening, setOpening] = useState<ComponentKind | null>(null);
  const [confirmRemoval, setConfirmRemoval] = useState<ConfirmRemoval>(null);
  const [verification, setVerification] = useState<Record<string, ComponentVerification>>({});
  const [actionError, setActionError] = useState<ErrorPresentation | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

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
        setLoadError(null);
      })
      .catch((cause: unknown) => {
        if (cancelled) return;
        setInventory(null);
        setLoadError(presentError(cause));
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

  const busyFor = (kind: ComponentKind, id: string): ItemAction | null =>
    action?.kind === kind && action.id === id ? action.action : null;

  const openFolder = async (kind: ComponentKind): Promise<void> => {
    setOpening(kind);
    setActionError(null);
    try {
      await openComponentFolder(kind);
    } catch (cause) {
      setActionError(presentError(cause));
    } finally {
      setOpening(null);
    }
  };

  const verify = async (kind: ComponentKind, id: string, label: string): Promise<void> => {
    setAction({ kind, id, action: "verify" });
    setActionError(null);
    setNotice(null);
    try {
      const next = await verifyComponentInventory(kind, id);
      setVerification((current) => ({
        ...current,
        [itemKey(next.kind, next.id)]: next,
      }));
      setNotice(
        next.verified
          ? `${label} passed local verification.`
          : `${label} did not pass local verification.`,
      );
    } catch (cause) {
      setActionError(presentError(cause));
    } finally {
      setAction(null);
    }
  };

  const remove = async (kind: ComponentKind, id: string, label: string): Promise<void> => {
    setAction({ kind, id, action: "remove" });
    setActionError(null);
    setNotice(null);
    try {
      const result = await removeComponent(kind, id);
      setConfirmRemoval(null);
      setVerification((current) => {
        const next = { ...current };
        delete next[itemKey(kind, id)];
        return next;
      });
      setNotice(
        result.removedManaged
          ? `Removed ${label} from the FuryBuilder cache.`
          : `Cleared the selected ${label}. The file outside the cache was not touched.`,
      );
      reload();
    } catch (cause) {
      setConfirmRemoval(null);
      setActionError(presentError(cause));
    } finally {
      setAction(null);
    }
  };

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
        shared local cache. Verify an artifact against its recorded checksum, remove it, or open its
        cache folder here. Choosing a download, a manual path or placeholder textures happens in the
        project wizard.
      </p>

      {loading && !inventory ? (
        <p className="flex items-center gap-2 text-sm text-muted-foreground">
          <Loader2 className="size-4 animate-spin" /> Reading the local component cache…
        </p>
      ) : loadError ? (
        <Alert>
          <TriangleAlert />
          <AlertTitle>Component inventory unavailable</AlertTitle>
          <AlertDescription>
            <p className="text-status-warn">{loadError.message}</p>
            {loadError.hint ? <p>{loadError.hint}</p> : null}
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
          {actionError ? (
            <Alert>
              <TriangleAlert />
              <AlertTitle>Component action failed</AlertTitle>
              <AlertDescription>
                <p className="text-status-warn">{actionError.message}</p>
                {actionError.hint ? <p>{actionError.hint}</p> : null}
              </AlertDescription>
            </Alert>
          ) : null}
          {notice ? (
            <Alert>
              <CheckCircle2 />
              <AlertTitle>Component cache updated</AlertTitle>
              <AlertDescription>{notice}</AlertDescription>
            </Alert>
          ) : null}

          <InventoryGroup
            title="Java runtimes"
            icon={<Coffee className="size-4" />}
            count={inventory.java.runtimes.length}
            totalBytes={inventory.java.totalBytes}
            root={inventory.java.root}
            empty="No managed Java runtimes. The project wizard can download or link one when a project needs Java."
            problems={inventory.java.problems}
            opening={opening === "java"}
            onOpenFolder={() => void openFolder("java")}
          >
            {inventory.java.runtimes.map((runtime) => {
              const label = `Java ${runtime.version}`;
              return (
                <InventoryItem
                  key={runtime.id}
                  title={label}
                  meta={
                    <>
                      <Badge variant="outline">Java {runtime.major}</Badge>
                      <span className="font-mono text-xs text-muted-foreground">
                        {runtime.vendor}
                      </span>
                    </>
                  }
                  path={runtime.javaPath}
                  size={formatBytes(runtime.bytes)}
                  verification={verification[itemKey("java", runtime.id)]}
                  busy={busyFor("java", runtime.id)}
                  locked={action !== null}
                  onVerify={() => void verify("java", runtime.id, label)}
                  onAskRemove={() => setConfirmRemoval({ kind: "java", id: runtime.id, label })}
                />
              );
            })}
          </InventoryGroup>

          <InventoryGroup
            title="Minecraft client JARs"
            icon={<Image className="size-4" />}
            count={inventory.assets.clients.length}
            totalBytes={inventory.assets.totalBytes}
            root={inventory.assets.root}
            empty="No managed client JARs. The project wizard can download or link the exact-version client assets."
            problems={inventory.assets.problems}
            opening={opening === "assets"}
            onOpenFolder={() => void openFolder("assets")}
          >
            {inventory.assets.clients.map((client) => {
              const label = `Minecraft ${client.version} client JAR`;
              return (
                <InventoryItem
                  key={client.id}
                  title={label}
                  meta={<Badge variant="outline">DataVersion {client.dataVersion}</Badge>}
                  path={client.jarPath}
                  size={formatBytes(client.size)}
                  verification={verification[itemKey("assets", client.id)]}
                  busy={busyFor("assets", client.id)}
                  locked={action !== null}
                  onVerify={() => void verify("assets", client.id, label)}
                  onAskRemove={() => setConfirmRemoval({ kind: "assets", id: client.id, label })}
                />
              );
            })}
          </InventoryGroup>

          <InventoryGroup
            title="Official server JARs"
            icon={<Server className="size-4" />}
            count={inventory.servers.servers.length}
            totalBytes={inventory.servers.totalBytes}
            root={inventory.servers.root}
            empty="No managed server JARs. Vanilla World provisions the exact-version official server in the project wizard."
            problems={inventory.servers.problems}
            opening={opening === "server"}
            onOpenFolder={() => void openFolder("server")}
          >
            {inventory.servers.servers.map((server) => {
              const label = `Minecraft ${server.version} server JAR`;
              const id = serverInventoryId(server.version);
              return (
                <InventoryItem
                  key={server.version}
                  title={label}
                  meta={
                    <>
                      <Badge variant="outline">DataVersion {server.dataVersion}</Badge>
                      <span className="font-mono text-xs text-muted-foreground">
                        Java {server.javaMajor}
                      </span>
                    </>
                  }
                  path={server.jarPath}
                  size={formatBytes(server.size)}
                  verification={verification[itemKey("server", id)]}
                  busy={busyFor("server", id)}
                  locked={action !== null}
                  onVerify={() => void verify("server", id, label)}
                  onAskRemove={() => setConfirmRemoval({ kind: "server", id, label })}
                />
              );
            })}
          </InventoryGroup>
        </div>
      ) : null}

      <Dialog
        open={confirmRemoval !== null}
        onOpenChange={(open) => {
          if (!open) setConfirmRemoval(null);
        }}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Remove cached component?</DialogTitle>
            <DialogDescription>
              {confirmRemoval
                ? `${confirmRemoval.label} will be deleted from the FuryBuilder cache. The project wizard can download or link it again later.`
                : "The selected artifact will be deleted from the FuryBuilder cache."}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button
              variant="outline"
              onClick={() => setConfirmRemoval(null)}
              disabled={action?.action === "remove"}
            >
              Cancel
            </Button>
            <Button
              variant="destructive"
              onClick={() => {
                if (confirmRemoval) {
                  void remove(confirmRemoval.kind, confirmRemoval.id, confirmRemoval.label);
                }
              }}
              disabled={action?.action === "remove"}
            >
              {action?.action === "remove" ? (
                <Loader2 data-icon="inline-start" className="animate-spin" />
              ) : (
                <Trash2 data-icon="inline-start" />
              )}
              Remove
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </section>
  );
}
