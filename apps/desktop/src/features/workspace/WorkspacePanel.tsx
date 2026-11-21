import { useState } from "react";
import {
  Eye,
  EyeOff,
  Layers,
  Loader2,
  Lock,
  Plus,
  Shield,
  ShieldCheck,
  Trash2,
  Unlock,
} from "lucide-react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { isTauriRuntime } from "@/lib/ipc";
import {
  withActiveLayer,
  withActiveMask,
  withLayerUpdate,
  withNewLayer,
  withNewRegion,
  withoutLayer,
  withoutRegion,
} from "@/project/workspace";
import type { WorkspacePolicy } from "@/project/types";
import { useToolStore } from "@/state/tool-store";
import { useWorldStore } from "@/state/world-store";
import { useWorkspaceStore } from "@/state/workspace-store";
import { cn } from "@/lib/utils";

function SectionTitle({
  icon: Icon,
  title,
  hint,
}: {
  icon: typeof Layers;
  title: string;
  hint?: string;
}) {
  return (
    <div className="mb-1 flex items-center gap-2">
      <Icon className="size-4 text-muted-foreground" />
      <span className="text-base font-semibold">{title}</span>
      {hint ? <span className="text-xs text-muted-foreground">{hint}</span> : null}
    </div>
  );
}

/**
 * Layers, an active edit mask and protected regions. The Rust Core re-validates and
 * persists the whole policy; this panel never treats a local click as authoritative
 * until the update succeeds. Undo/redo deliberately bypasses these rules.
 */
export function WorkspacePanel() {
  const policy = useWorkspaceStore((state) => state.policy);
  const apply = useWorkspaceStore((state) => state.apply);
  const saving = useWorkspaceStore((state) => state.saving);
  const error = useWorkspaceStore((state) => state.error);
  const selection = useToolStore((state) => state.selection);
  const dimension = useWorldStore((state) => state.dimension);
  const [layerName, setLayerName] = useState("");
  const [maskName, setMaskName] = useState("Allowed area");
  const [protectedName, setProtectedName] = useState("Protected area");
  const [actionError, setActionError] = useState<string | null>(null);
  const editing = isTauriRuntime();

  const commit = async (next: WorkspacePolicy, message: string) => {
    setActionError(null);
    const saved = await apply(next, message);
    if (!saved) setActionError("The Core rejected the policy change.");
    return saved;
  };

  if (!editing) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-2 text-center text-sm text-muted-foreground">
        <Layers className="size-5 text-muted-foreground/60" />
        <p className="max-w-md leading-relaxed">
          Layers, masks and protected regions are enforced by the Rust Core. Open the desktop app to
          configure them.
        </p>
      </div>
    );
  }

  const activeMask = policy.masks.find((mask) => mask.id === policy.activeMaskId) ?? null;

  return (
    <div className="flex min-h-0 flex-1 gap-4 text-sm" data-testid="workspace-panel">
      <div className="scrollbar-thin flex min-h-0 w-80 shrink-0 flex-col gap-2 overflow-y-auto rounded-xl border bg-card p-4">
        <SectionTitle icon={Layers} title="Layers" hint="active layer receives edits" />
        <div className="space-y-1">
          {policy.layers.map((layer) => {
            const active = layer.id === policy.activeLayerId;
            return (
              <div
                key={layer.id}
                className={cn(
                  "flex items-center gap-1 rounded border px-1.5 py-1",
                  active ? "border-primary/60 bg-primary/10" : "bg-card",
                )}
              >
                <button
                  type="button"
                  className="min-w-0 flex-1 truncate text-left font-mono text-xs"
                  title={active ? "Active layer" : "Make active"}
                  onClick={() =>
                    void commit(withActiveLayer(policy, layer.id), "Active layer changed")
                  }
                >
                  {active ? "● " : "○ "}
                  {layer.name}
                </button>
                <button
                  type="button"
                  title={layer.visible ? "Hide layer overlay" : "Show layer overlay"}
                  aria-label={layer.visible ? "Hide layer" : "Show layer"}
                  onClick={() =>
                    void commit(
                      withLayerUpdate(policy, layer.id, { visible: !layer.visible }),
                      "Layer visibility changed",
                    )
                  }
                >
                  {layer.visible ? (
                    <Eye className="size-3 text-muted-foreground" />
                  ) : (
                    <EyeOff className="size-3 text-muted-foreground/50" />
                  )}
                </button>
                <button
                  type="button"
                  title={layer.locked ? "Unlock layer" : "Lock layer"}
                  aria-label={layer.locked ? "Unlock layer" : "Lock layer"}
                  onClick={() =>
                    void commit(
                      withLayerUpdate(policy, layer.id, { locked: !layer.locked }),
                      layer.locked ? "Layer unlocked" : "Layer locked",
                    )
                  }
                >
                  {layer.locked ? (
                    <Lock className="size-3 text-status-warn" />
                  ) : (
                    <Unlock className="size-3 text-muted-foreground" />
                  )}
                </button>
                <button
                  type="button"
                  title="Delete layer"
                  aria-label="Delete layer"
                  disabled={policy.layers.length <= 1 || active}
                  onClick={() => void commit(withoutLayer(policy, layer.id), "Layer deleted")}
                  className="disabled:opacity-30"
                >
                  <Trash2 className="size-3 text-muted-foreground" />
                </button>
              </div>
            );
          })}
        </div>
        <div className="flex gap-2">
          <Input
            placeholder="Layer name"
            value={layerName}
            onChange={(event) => setLayerName(event.target.value)}
            aria-label="New layer name"
          />
          <Button
            size="sm"
            variant="outline"
            disabled={saving || !layerName.trim()}
            onClick={() => {
              const next = withNewLayer(policy, layerName);
              if (!next) {
                setActionError("Enter a unique name of at most 64 characters.");
                return;
              }
              void commit(next, "Layer created").then((saved) => {
                if (saved) setLayerName("");
              });
            }}
          >
            <Plus className="size-3" /> Add
          </Button>
        </div>
      </div>

      <div className="scrollbar-thin flex min-h-0 w-80 shrink-0 flex-col gap-2 overflow-y-auto rounded-xl border bg-card p-4">
        <SectionTitle icon={Shield} title="Edit mask" hint="operations must fit inside" />
        <Select
          items={[
            { value: "", label: "No mask" },
            ...policy.masks.map((mask) => ({
              value: mask.id,
              label: `${mask.name} (${mask.dimension.replace("minecraft:", "")})`,
            })),
          ]}
          value={policy.activeMaskId ?? ""}
          onValueChange={(value) =>
            void commit(withActiveMask(policy, value || null), "Active mask changed")
          }
        >
          <SelectTrigger className="w-fit" aria-label="Active mask">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="">No mask</SelectItem>
            {policy.masks.map((mask) => (
              <SelectItem key={mask.id} value={mask.id}>
                {mask.name} ({mask.dimension.replace("minecraft:", "")})
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <p className="text-sm text-muted-foreground">
          The mask only constrains its own dimension. Empty means unrestricted.
        </p>
        <div className="flex gap-2">
          <Input
            value={maskName}
            onChange={(event) => setMaskName(event.target.value)}
            aria-label="New mask name"
          />
          <Button
            size="sm"
            variant="outline"
            disabled={saving || !selection}
            title={selection ? "Use the current selection" : "Select a box first"}
            onClick={() => {
              if (!selection) return;
              const next = withNewRegion(policy, "mask", maskName, dimension, selection);
              if (!next) {
                setActionError("Enter a name and select a valid box.");
                return;
              }
              void commit(next, "Edit mask created");
            }}
          >
            <Plus className="size-3" /> From selection
          </Button>
        </div>
        {activeMask ? (
          <Button
            size="sm"
            variant="outline"
            disabled={saving}
            onClick={() =>
              void commit(withoutRegion(policy, "mask", activeMask.id), "Edit mask deleted")
            }
          >
            <Trash2 className="size-3" /> Delete active mask
          </Button>
        ) : null}

        <SectionTitle icon={ShieldCheck} title="Protected regions" hint="always fail closed" />
        <div className="space-y-1">
          {policy.protectedRegions.length === 0 ? (
            <p className="text-sm text-muted-foreground">Nothing is protected yet.</p>
          ) : (
            policy.protectedRegions.map((region) => (
              <div
                key={region.id}
                className="flex items-center gap-1 rounded border border-border/70 px-1.5 py-1"
              >
                <div className="min-w-0 flex-1">
                  <div className="truncate text-xs">{region.name}</div>
                  <div className="truncate font-mono text-xs text-muted-foreground">
                    {region.dimension.replace("minecraft:", "")} · ({region.bounds.min.x},
                    {region.bounds.min.y},{region.bounds.min.z})..({region.bounds.max.x},
                    {region.bounds.max.y},{region.bounds.max.z})
                  </div>
                </div>
                <button
                  type="button"
                  title="Remove protection"
                  aria-label={`Delete protected region ${region.name}`}
                  onClick={() =>
                    void commit(
                      withoutRegion(policy, "protected", region.id),
                      "Protected region deleted",
                    )
                  }
                >
                  <Trash2 className="size-3 text-muted-foreground" />
                </button>
              </div>
            ))
          )}
        </div>
        <div className="flex gap-2">
          <Input
            value={protectedName}
            onChange={(event) => setProtectedName(event.target.value)}
            aria-label="New protected region name"
          />
          <Button
            size="sm"
            variant="outline"
            disabled={saving || !selection}
            title={selection ? "Protect the current selection" : "Select a box first"}
            onClick={() => {
              if (!selection) return;
              const next = withNewRegion(policy, "protected", protectedName, dimension, selection);
              if (!next) {
                setActionError("Enter a name and select a valid box.");
                return;
              }
              void commit(next, "Protected region created");
            }}
          >
            <Plus className="size-3" /> Protect selection
          </Button>
        </div>
        <p className="text-sm text-muted-foreground">
          Protection blocks new operations and templates. Undo/redo restore previous snapshots and
          remain available.
        </p>
      </div>

      <div className="min-w-0 flex-1 rounded-xl border bg-card p-4">
        <SectionTitle icon={Layers} title="Policy state" />
        <div className="space-y-1 font-mono text-xs text-muted-foreground">
          <div>layers: {policy.layers.length} / 64</div>
          <div>masks: {policy.masks.length} / 64</div>
          <div>protected: {policy.protectedRegions.length} / 64</div>
          <div>active layer: {policy.activeLayerId ?? "none"}</div>
          <div>active mask: {policy.activeMaskId ?? "none"}</div>
        </div>
        {saving ? (
          <div className="mt-2 flex items-center gap-1 text-xs text-muted-foreground">
            <Loader2 className="size-3 animate-spin" /> Saving policy…
          </div>
        ) : null}
        {error || actionError ? (
          <p className="mt-2 text-xs text-status-error">{actionError ?? error}</p>
        ) : (
          <p className="mt-2 text-xs text-muted-foreground">
            Policy changes are stored in project.json and do not bump the world revision.
          </p>
        )}
      </div>
    </div>
  );
}
