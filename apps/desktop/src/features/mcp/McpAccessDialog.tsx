import { useEffect, useMemo, useState } from "react";
import { Loader2, ShieldCheck, ShieldAlert } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Separator } from "@/components/ui/separator";
import {
  ACCESS_MODE_LABELS,
  getMcpAccessStatus,
  setMcpAccess,
  type McpAccessMode,
  type McpAccessPolicy,
} from "@/lib/mcp-access";
import { useToolStore } from "@/state/tool-store";
import { useWorldStore } from "@/state/world-store";
import { cn } from "@/lib/utils";

interface McpAccessDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

interface FormState {
  mode: McpAccessMode;
  dimension: string;
  minX: string;
  minY: string;
  minZ: string;
  maxX: string;
  maxY: string;
  maxZ: string;
  maxBlocks: string;
  maxBatchChanges: string;
  requireConfirmation: boolean;
}

const DEFAULT_FORM: FormState = {
  mode: "read_only",
  dimension: "minecraft:overworld",
  minX: "0",
  minY: "60",
  minZ: "0",
  maxX: "15",
  maxY: "75",
  maxZ: "15",
  maxBlocks: "50000",
  maxBatchChanges: "4096",
  requireConfirmation: true,
};

function formFromPolicy(policy: McpAccessPolicy): FormState {
  const selection = policy.selection;
  return {
    mode: policy.mode,
    dimension: selection?.dimension ?? DEFAULT_FORM.dimension,
    minX: String(selection?.bounds.min.x ?? DEFAULT_FORM.minX),
    minY: String(selection?.bounds.min.y ?? DEFAULT_FORM.minY),
    minZ: String(selection?.bounds.min.z ?? DEFAULT_FORM.minZ),
    maxX: String(selection?.bounds.max.x ?? DEFAULT_FORM.maxX),
    maxY: String(selection?.bounds.max.y ?? DEFAULT_FORM.maxY),
    maxZ: String(selection?.bounds.max.z ?? DEFAULT_FORM.maxZ),
    maxBlocks: String(policy.maxBlocks),
    maxBatchChanges: String(policy.maxBatchChanges),
    requireConfirmation: policy.requireConfirmation,
  };
}

function parseInteger(value: string): number | null {
  if (!/^-?\d+$/.test(value.trim())) return null;
  const parsed = Number(value);
  return Number.isSafeInteger(parsed) ? parsed : null;
}

function volumeLabel(form: FormState): string | null {
  const values = [form.minX, form.minY, form.minZ, form.maxX, form.maxY, form.maxZ].map(
    parseInteger,
  );
  if (values.some((value) => value === null)) return null;
  const [minX, minY, minZ, maxX, maxY, maxZ] = values as number[];
  if (minX > maxX || minY > maxY || minZ > maxZ) return null;
  const volume = (maxX - minX + 1) * (maxY - minY + 1) * (maxZ - minZ + 1);
  return Number.isSafeInteger(volume) ? volume.toLocaleString() : "too large";
}

export function McpAccessDialog({ open, onOpenChange }: McpAccessDialogProps) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      {open ? <McpAccessContent onClose={() => onOpenChange(false)} /> : null}
    </Dialog>
  );
}

function McpAccessContent({ onClose }: { onClose: () => void }) {
  const toolSelection = useToolStore((state) => state.selection);
  const dimension = useWorldStore((state) => state.dimension);
  const [form, setForm] = useState<FormState>(DEFAULT_FORM);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    let cancelled = false;
    void getMcpAccessStatus()
      .then((status) => {
        if (cancelled) return;
        const currentSelection = useToolStore.getState().selection;
        const currentDimension = useWorldStore.getState().dimension;
        setForm(
          formFromPolicy({
            ...status.policy,
            selection:
              status.policy.selection ??
              (currentSelection ? { dimension: currentDimension, bounds: currentSelection } : null),
          }),
        );
      })
      .catch((loadError: unknown) => {
        if (!cancelled) {
          setError(loadError instanceof Error ? loadError.message : String(loadError));
        }
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const volume = useMemo(() => volumeLabel(form), [form]);

  function update<K extends keyof FormState>(key: K, value: FormState[K]) {
    setSaved(false);
    setForm((current) => ({ ...current, [key]: value }));
  }

  function useCurrentSelection() {
    if (!toolSelection) {
      setError("Select a region in the viewport first.");
      return;
    }
    setError(null);
    setForm((current) => ({
      ...current,
      dimension,
      minX: String(toolSelection.min.x),
      minY: String(toolSelection.min.y),
      minZ: String(toolSelection.min.z),
      maxX: String(toolSelection.max.x),
      maxY: String(toolSelection.max.y),
      maxZ: String(toolSelection.max.z),
    }));
  }

  async function save() {
    const maxBlocks = parseInteger(form.maxBlocks);
    const maxBatchChanges = parseInteger(form.maxBatchChanges);
    if (maxBlocks === null || maxBlocks < 1) {
      setError("Max blocks must be a positive integer.");
      return;
    }
    if (maxBatchChanges === null || maxBatchChanges < 1) {
      setError("Max batch changes must be a positive integer.");
      return;
    }
    let selection = null;
    if (form.mode === "edit_in_selection") {
      const values = [form.minX, form.minY, form.minZ, form.maxX, form.maxY, form.maxZ].map(
        parseInteger,
      );
      if (values.some((value) => value === null)) {
        setError("Selection coordinates must be integers.");
        return;
      }
      const [minX, minY, minZ, maxX, maxY, maxZ] = values as number[];
      if (minX > maxX || minY > maxY || minZ > maxZ) {
        setError("Selection minimum must not exceed maximum.");
        return;
      }
      selection = {
        dimension: form.dimension.trim(),
        bounds: {
          min: { x: minX, y: minY, z: minZ },
          max: { x: maxX, y: maxY, z: maxZ },
        },
      };
      if (!selection.dimension) {
        setError("Selection dimension must not be empty.");
        return;
      }
    }

    setSaving(true);
    setError(null);
    try {
      const policy = await setMcpAccess({
        mode: form.mode,
        selection,
        maxBlocks,
        maxBatchChanges,
        requireConfirmation: form.requireConfirmation,
      });
      setForm(formFromPolicy(policy));
      setSaved(true);
    } catch (saveError) {
      setError(saveError instanceof Error ? saveError.message : String(saveError));
    } finally {
      setSaving(false);
    }
  }

  const editing = form.mode !== "read_only";

  return (
    <DialogContent className="max-w-2xl">
      <DialogHeader>
        <DialogTitle className="flex items-center gap-2">
          {editing ? (
            <ShieldAlert className="size-4 text-status-warn" />
          ) : (
            <ShieldCheck className="size-4 text-status-ok" />
          )}
          AI / MCP access
        </DialogTitle>
        <DialogDescription>
          Controls what an external MCP client can read and change. The bridge cannot raise its own
          access; this window is the authority.
        </DialogDescription>
      </DialogHeader>

      {loading ? (
        <div className="flex items-center gap-2 py-6 text-xs text-muted-foreground">
          <Loader2 className="size-3.5 animate-spin" /> Loading policy…
        </div>
      ) : (
        <div className="space-y-4">
          <section className="space-y-1.5">
            <h3 className="text-[11px] font-semibold tracking-wide uppercase">Mode</h3>
            <div className="flex flex-wrap gap-1">
              {(["read_only", "edit_in_selection", "full_project"] as const).map((mode) => (
                <Button
                  key={mode}
                  size="sm"
                  variant={form.mode === mode ? "default" : "outline"}
                  onClick={() => setForm((current) => ({ ...current, mode }))}
                >
                  {ACCESS_MODE_LABELS[mode]}
                </Button>
              ))}
            </div>
            <p className="text-[10px] text-muted-foreground">
              Read-only is the default. Edit-in-selection limits every write and undo to a single
              dimension and box. Full project keeps the hard block limits.
            </p>
          </section>

          {form.mode === "edit_in_selection" ? (
            <section className="space-y-2">
              <div className="flex items-center justify-between gap-2">
                <h3 className="text-[11px] font-semibold tracking-wide uppercase">Selection</h3>
                <Button size="sm" variant="outline" onClick={useCurrentSelection}>
                  Use current viewport selection
                </Button>
              </div>
              <div className="grid grid-cols-[1fr_auto] gap-2">
                <label className="text-[10px] text-muted-foreground">
                  Dimension
                  <Input
                    value={form.dimension}
                    onChange={(event) => update("dimension", event.target.value)}
                    spellCheck={false}
                  />
                </label>
                <span className="self-end pb-1 font-mono text-[10px] text-muted-foreground">
                  {volume === null ? "invalid box" : `${volume} blocks`}
                </span>
              </div>
              <div className="grid grid-cols-6 gap-1.5">
                {(
                  [
                    ["minX", "min X"],
                    ["minY", "min Y"],
                    ["minZ", "min Z"],
                    ["maxX", "max X"],
                    ["maxY", "max Y"],
                    ["maxZ", "max Z"],
                  ] as const
                ).map(([key, label]) => (
                  <label key={key} className="text-[10px] text-muted-foreground">
                    {label}
                    <Input
                      value={form[key]}
                      inputMode="numeric"
                      onChange={(event) => update(key, event.target.value)}
                    />
                  </label>
                ))}
              </div>
            </section>
          ) : null}

          <section className="space-y-2">
            <h3 className="text-[11px] font-semibold tracking-wide uppercase">Limits</h3>
            <div className="grid grid-cols-2 gap-2">
              <label className="text-[10px] text-muted-foreground">
                Max blocks per operation
                <Input
                  value={form.maxBlocks}
                  inputMode="numeric"
                  onChange={(event) => update("maxBlocks", event.target.value)}
                />
              </label>
              <label className="text-[10px] text-muted-foreground">
                Max changes per batch
                <Input
                  value={form.maxBatchChanges}
                  inputMode="numeric"
                  onChange={(event) => update("maxBatchChanges", event.target.value)}
                />
              </label>
            </div>
          </section>

          <section className="space-y-1.5">
            <label className="flex items-start gap-2 rounded border border-border bg-background/40 p-2 text-[11px]">
              <input
                type="checkbox"
                checked={form.requireConfirmation}
                onChange={(event) => update("requireConfirmation", event.target.checked)}
                className="mt-0.5 accent-[var(--primary)]"
              />
              <span>
                Ask me to approve every apply and undo
                <span className="block text-[10px] text-muted-foreground">
                  Keep this on. The write waits for your answer and fails if you deny or do not
                  respond.
                </span>
              </span>
            </label>
          </section>

          {editing ? (
            <div className="rounded border border-status-warn/30 bg-status-warn/5 p-2 text-[10px] text-status-warn">
              External clients can modify this project while edit access is enabled. Switch back to
              Read-only when you are finished.
            </div>
          ) : null}

          {error ? <p className="text-[11px] text-status-warn">{error}</p> : null}
          {saved ? (
            <p className={cn("text-[11px]", "text-status-ok")}>
              Policy saved. New requests use it immediately.
            </p>
          ) : null}
        </div>
      )}

      <Separator />
      <DialogFooter className="items-center justify-between gap-2 sm:justify-between">
        <span className="text-[10px] text-muted-foreground">
          Changes apply to the running desktop only.
        </span>
        <div className="flex gap-1.5">
          <Button variant="outline" size="sm" onClick={onClose}>
            Close
          </Button>
          <Button size="sm" onClick={() => void save()} disabled={loading || saving}>
            {saving ? <Loader2 className="animate-spin" /> : null}
            Save policy
          </Button>
        </div>
      </DialogFooter>
    </DialogContent>
  );
}
