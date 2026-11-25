import {
  AlertTriangle,
  ArrowDown,
  ArrowUp,
  FileArchive,
  FolderOpen,
  RefreshCw,
  X,
} from "lucide-react";
import { useEffect, useState } from "react";

import type { ProjectPackEdit, ProjectPackStack, ProjectPackStatus } from "@/assets/types";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { loadProjectPackStack, updateProjectPackStack } from "@/lib/assets-api";
import { normalizeError } from "@/lib/ipc";

const stateLabels: Record<ProjectPackStatus["state"], string> = {
  ready: "Ready",
  missing: "Missing",
  changed: "Changed",
  incompatible: "Incompatible",
  invalid: "Invalid",
};

export function ProjectResourcePacks({ folder }: { folder: string }) {
  const [stack, setStack] = useState<ProjectPackStack>();
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(true);

  useEffect(() => {
    let disposed = false;
    void loadProjectPackStack(folder)
      .then((stack) => {
        if (!disposed) setStack(stack);
      })
      .catch((cause) => {
        if (!disposed) setError(normalizeError(cause).message);
      })
      .finally(() => {
        if (!disposed) setBusy(false);
      });
    return () => {
      disposed = true;
    };
  }, [folder]);

  const run = async (action: () => Promise<ProjectPackStack | undefined>) => {
    setBusy(true);
    setError(null);
    try {
      const next = await action();
      if (next) setStack(next);
    } catch (cause) {
      setError(normalizeError(cause).message);
    } finally {
      setBusy(false);
    }
  };

  const edit = (edit: ProjectPackEdit) => run(() => updateProjectPackStack(folder, edit));
  const choose = (directory: boolean) =>
    run(async () => {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const path = await open({
        title: directory ? "Choose a resource pack folder" : "Choose a resource pack ZIP",
        directory,
        multiple: false,
        ...(directory ? {} : { filters: [{ name: "Resource pack ZIP", extensions: ["zip"] }] }),
      });
      return typeof path === "string"
        ? updateProjectPackStack(folder, { kind: "add", path })
        : undefined;
    });

  const canAdd = !busy && stack?.enabled && stack.packs.length < stack.maxPacks;
  return (
    <Card aria-busy={busy}>
      <CardHeader>
        <CardTitle>Resource packs</CardTitle>
        <CardDescription>
          Packs apply only to this project. Higher packs override lower ones. Changes apply
          immediately and do not change the world or block catalog.
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        {stack ? (
          <p className="text-sm text-muted-foreground">
            Minecraft {stack.minecraftVersion}
            {stack.targetFormat ? ` · resource pack format ${stack.targetFormat}` : ""}
            {` · ${stack.packs.length}/${stack.maxPacks} packs`}
          </p>
        ) : null}
        <div className="flex flex-wrap gap-2">
          <Button size="sm" variant="outline" disabled={!canAdd} onClick={() => void choose(false)}>
            <FileArchive data-icon="inline-start" /> Add ZIP
          </Button>
          <Button size="sm" variant="outline" disabled={!canAdd} onClick={() => void choose(true)}>
            <FolderOpen data-icon="inline-start" /> Add folder
          </Button>
          <Button
            size="sm"
            variant="ghost"
            disabled={busy}
            onClick={() => void run(() => loadProjectPackStack(folder, true))}
          >
            <RefreshCw data-icon="inline-start" /> Check pack files
          </Button>
        </div>
        {busy ? (
          <p className="text-sm text-muted-foreground" role="status">
            Checking resource packs…
          </p>
        ) : null}
        {error ? (
          <Alert variant="destructive">
            <AlertTriangle />
            <AlertTitle>Could not update resource packs</AlertTitle>
            <AlertDescription>{error}</AlertDescription>
          </Alert>
        ) : null}
        {stack && !stack.enabled ? (
          <p className="text-sm text-muted-foreground">
            Resource packs require pinned Minecraft client assets.
          </p>
        ) : null}
        {stack?.enabled && stack.packs.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            No packs. The project uses vanilla client assets.
          </p>
        ) : null}
        {stack?.packs.length ? (
          <ol className="flex flex-col gap-2" aria-label="Resource packs in priority order">
            {stack.packs.map((pack, index) => {
              const format = pack.supportedFormats ?? pack.packFormat;
              return (
                <li
                  key={pack.sha1}
                  className="flex flex-wrap items-start gap-2 rounded-md border p-2"
                >
                  <div className="min-w-0 flex-1">
                    <span className="text-sm font-medium break-words">
                      {index + 1}. {pack.name}
                    </span>
                    <Badge
                      variant={pack.state === "ready" ? "secondary" : "destructive"}
                      className="ml-2"
                    >
                      {stateLabels[pack.state]}
                    </Badge>
                    {format !== null ? (
                      <p className="text-xs text-muted-foreground">Pack format {format}</p>
                    ) : null}
                    {pack.message ? (
                      <p className="text-xs text-muted-foreground">
                        {pack.message}. This pack is skipped.
                      </p>
                    ) : null}
                  </div>
                  <div className="flex gap-1">
                    <Button
                      size="icon-sm"
                      variant="ghost"
                      aria-label={`Move ${pack.name} up`}
                      disabled={busy || index === 0}
                      onClick={() => void edit({ kind: "move", sha1: pack.sha1, earlier: true })}
                    >
                      <ArrowUp />
                    </Button>
                    <Button
                      size="icon-sm"
                      variant="ghost"
                      aria-label={`Move ${pack.name} down`}
                      disabled={busy || index === stack.packs.length - 1}
                      onClick={() => void edit({ kind: "move", sha1: pack.sha1, earlier: false })}
                    >
                      <ArrowDown />
                    </Button>
                    <Button
                      size="icon-sm"
                      variant="ghost"
                      aria-label={`Remove ${pack.name}`}
                      disabled={busy}
                      onClick={() => void edit({ kind: "remove", sha1: pack.sha1 })}
                    >
                      <X />
                    </Button>
                  </div>
                </li>
              );
            })}
          </ol>
        ) : null}
      </CardContent>
    </Card>
  );
}
