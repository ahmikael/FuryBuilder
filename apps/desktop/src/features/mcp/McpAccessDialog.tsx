import { useEffect, useMemo, useState } from "react";
import { Loader2, ShieldCheck, ShieldAlert } from "lucide-react";

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Field, FieldLabel } from "@/components/ui/field";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
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
    <DialogContent className="max-h-[90vh] max-w-4xl gap-6 overflow-y-auto">
      <DialogHeader>
        <DialogTitle className="flex items-center gap-2 text-xl">
          {editing ? (
            <ShieldAlert className="size-5 text-status-warn" />
          ) : (
            <ShieldCheck className="size-5 text-status-ok" />
          )}
          AI / MCP access
          <Badge variant={editing ? "secondary" : "outline"} className="ml-1">
            {editing ? "Edit enabled" : "Read-only"}
          </Badge>
        </DialogTitle>
        <DialogDescription>
          Controls what an external MCP client can read and change. The bridge cannot raise its own
          access; this window is the authority.
        </DialogDescription>
      </DialogHeader>

      {loading ? (
        <div className="flex items-center gap-2 py-6 text-sm text-muted-foreground">
          <Loader2 className="size-4 animate-spin" /> Loading policy…
        </div>
      ) : (
        <div className="space-y-6">
          <section className="space-y-3">
            <h3 className="text-sm font-semibold">Mode</h3>
            <ToggleGroup
              variant="outline"
              spacing={2}
              value={[form.mode]}
              onValueChange={(value) => {
                const mode = value[0];
                if (
                  mode === "read_only" ||
                  mode === "edit_in_selection" ||
                  mode === "full_project"
                ) {
                  setForm((current) => ({ ...current, mode }));
                }
              }}
              className="flex-wrap"
            >
              {(["read_only", "edit_in_selection", "full_project"] as const).map((mode) => (
                <ToggleGroupItem key={mode} value={mode}>
                  {ACCESS_MODE_LABELS[mode]}
                </ToggleGroupItem>
              ))}
            </ToggleGroup>
            <p className="text-sm leading-relaxed text-muted-foreground">
              Read-only is the default. Edit-in-selection limits every write and undo to a single
              dimension and box. Full project keeps the hard block limits.
            </p>
          </section>

          {form.mode === "edit_in_selection" ? (
            <Card className="gap-0 py-0">
              <CardContent className="space-y-4 p-6">
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <h3 className="text-sm font-semibold">Selection</h3>
                  <Button size="sm" variant="outline" onClick={useCurrentSelection}>
                    Use current viewport selection
                  </Button>
                </div>
                <div className="flex flex-wrap items-end gap-3">
                  <Field className="min-w-56 flex-1">
                    <FieldLabel htmlFor="mcp-dimension">Dimension</FieldLabel>
                    <Input
                      id="mcp-dimension"
                      value={form.dimension}
                      onChange={(event) => update("dimension", event.target.value)}
                      spellCheck={false}
                    />
                  </Field>
                  <Badge variant="secondary" className="mb-1.5 font-mono">
                    {volume === null ? "invalid box" : `${volume} blocks`}
                  </Badge>
                </div>
                <div className="grid grid-cols-2 gap-3 sm:grid-cols-3">
                  {(
                    [
                      ["minX", "Min X"],
                      ["minY", "Min Y"],
                      ["minZ", "Min Z"],
                      ["maxX", "Max X"],
                      ["maxY", "Max Y"],
                      ["maxZ", "Max Z"],
                    ] as const
                  ).map(([key, label]) => (
                    <Field key={key}>
                      <FieldLabel htmlFor={`mcp-${key}`}>{label}</FieldLabel>
                      <Input
                        id={`mcp-${key}`}
                        value={form[key]}
                        inputMode="numeric"
                        onChange={(event) => update(key, event.target.value)}
                      />
                    </Field>
                  ))}
                </div>
              </CardContent>
            </Card>
          ) : null}

          <Card className="gap-0 py-0">
            <CardContent className="space-y-4 p-6">
              <h3 className="text-sm font-semibold">Limits</h3>
              <div className="grid gap-4 sm:grid-cols-2">
                <Field>
                  <FieldLabel htmlFor="mcp-max-blocks">Max blocks per operation</FieldLabel>
                  <Input
                    id="mcp-max-blocks"
                    value={form.maxBlocks}
                    inputMode="numeric"
                    onChange={(event) => update("maxBlocks", event.target.value)}
                  />
                </Field>
                <Field>
                  <FieldLabel htmlFor="mcp-max-batch">Max changes per batch</FieldLabel>
                  <Input
                    id="mcp-max-batch"
                    value={form.maxBatchChanges}
                    inputMode="numeric"
                    onChange={(event) => update("maxBatchChanges", event.target.value)}
                  />
                </Field>
              </div>
            </CardContent>
          </Card>

          <Card className="gap-0 py-0">
            <CardContent className="p-6">
              <div className="flex items-start gap-3">
                <Checkbox
                  id="mcp-confirmation"
                  className="mt-0.5"
                  checked={form.requireConfirmation}
                  onCheckedChange={(checked) => update("requireConfirmation", checked === true)}
                  aria-label="Ask me to approve every apply and undo"
                />
                <div className="space-y-1">
                  <p className="text-sm font-medium">Ask me to approve every apply and undo</p>
                  <p className="text-sm leading-relaxed text-muted-foreground">
                    Keep this on. The write waits for your answer and fails if you deny or do not
                    respond.
                  </p>
                </div>
              </div>
            </CardContent>
          </Card>

          {editing ? (
            <Alert>
              <ShieldAlert />
              <AlertTitle>External clients can modify this project</AlertTitle>
              <AlertDescription>
                Edit access is currently enabled. Switch back to Read-only when you are finished.
              </AlertDescription>
            </Alert>
          ) : null}

          {error ? (
            <Alert variant="destructive">
              <ShieldAlert />
              <AlertTitle>Could not save the policy</AlertTitle>
              <AlertDescription>{error}</AlertDescription>
            </Alert>
          ) : null}
          {saved ? (
            <Alert>
              <ShieldCheck />
              <AlertTitle>Policy saved</AlertTitle>
              <AlertDescription>New requests use it immediately.</AlertDescription>
            </Alert>
          ) : null}
        </div>
      )}

      <Separator />
      <DialogFooter className="w-full items-center justify-between sm:justify-between">
        <span className="text-xs text-muted-foreground">
          Changes apply to the running desktop only.
        </span>
        <div className="flex gap-2">
          <Button variant="outline" size="sm" onClick={onClose}>
            Close
          </Button>
          <Button size="sm" onClick={() => void save()} disabled={loading || saving}>
            {saving ? <Loader2 data-icon="inline-start" className="animate-spin" /> : null}
            Save policy
          </Button>
        </div>
      </DialogFooter>
    </DialogContent>
  );
}
