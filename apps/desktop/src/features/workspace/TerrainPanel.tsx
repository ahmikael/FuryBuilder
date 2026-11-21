import { useMemo, useState } from "react";
import { Loader2, Mountain, ScanLine } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { applyEditorOperation } from "@/editor/commands";
import {
  boundsVolume,
  planTerrainBrush,
  TERRAIN_BRUSH_LABELS,
  TERRAIN_BRUSH_ORDER,
  TERRAIN_BRUSH_VERTICAL_MARGIN,
  terrainBrushBounds,
} from "@/editor/session";
import type { TerrainBrushKind } from "@/editor/types";
import { isTauriRuntime, normalizeError } from "@/lib/ipc";
import { previewOperation } from "@/lib/world-api";
import { useToolStore } from "@/state/tool-store";
import { useWorldStore } from "@/state/world-store";

const TERRAIN_BRUSH_ITEMS = TERRAIN_BRUSH_ORDER.map((value) => ({
  value,
  label: TERRAIN_BRUSH_LABELS[value],
}));

/**
 * M9.9 terrain brushes. Every brush operates on the existing top surface inside the current
 * box selection, is previewed by the Rust core and applied as one reversible operation.
 */
export function TerrainPanel() {
  const selection = useToolStore((state) => state.selection);
  const material = useToolStore((state) => state.selectedBlock);
  const job = useToolStore((state) => state.job);
  const dimension = useWorldStore((state) => state.dimension);
  const [kind, setKind] = useState<TerrainBrushKind>("raise");
  const [amount, setAmount] = useState(1);
  const [seed, setSeed] = useState(0);
  const [step, setStep] = useState(4);
  const [targetY, setTargetY] = useState("");
  const [preview, setPreview] = useState<{
    changedBlocks: number;
    affectedBlocks: number;
    warnings: string[];
  } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [previewing, setPreviewing] = useState(false);
  const editing = isTauriRuntime();
  const busy = Boolean(job) || previewing;

  const parsedTargetY = targetY.trim() === "" ? null : Number(targetY);
  const brushBounds = selection ? terrainBrushBounds(selection) : null;
  const operation = useMemo(
    () =>
      planTerrainBrush({
        dimension,
        selection,
        kind,
        material,
        amount,
        seed,
        step,
        targetY: parsedTargetY !== null && Number.isFinite(parsedTargetY) ? parsedTargetY : null,
      }),
    [dimension, selection, kind, material, amount, seed, step, parsedTargetY],
  );

  function resetPreview(): void {
    setPreview(null);
    setError(null);
  }

  async function previewBrush(): Promise<void> {
    if (!operation) return;
    setPreviewing(true);
    setError(null);
    try {
      const result = await previewOperation(operation);
      setPreview({
        changedBlocks: result.changedBlocks,
        affectedBlocks: result.affectedBlocks,
        warnings: result.warnings,
      });
    } catch (cause) {
      setPreview(null);
      setError(normalizeError(cause).message);
    } finally {
      setPreviewing(false);
    }
  }

  async function applyBrush(): Promise<void> {
    if (!operation) return;
    setPreview(null);
    setError(null);
    await applyEditorOperation(operation);
  }

  return (
    <div className="flex flex-wrap items-start gap-4 text-sm">
      <div className="flex w-72 flex-col gap-1.5">
        <div className="flex items-center gap-2">
          <Mountain className="size-3.5 text-primary" />
          <span className="font-medium">Terrain brush</span>
          <Badge variant="secondary" className="uppercase">
            deterministic
          </Badge>
        </div>
        <label className="flex items-center justify-between gap-2">
          <span className="text-muted-foreground">Brush</span>
          <Select
            items={TERRAIN_BRUSH_ITEMS}
            value={kind}
            onValueChange={(value) => {
              setKind(value as TerrainBrushKind);
              resetPreview();
            }}
            disabled={busy}
          >
            <SelectTrigger className="w-fit" aria-label="Terrain brush kind">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {TERRAIN_BRUSH_ITEMS.map((entry) => (
                <SelectItem key={entry.value} value={entry.value}>
                  {entry.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </label>
        <div className="flex flex-wrap items-center gap-2">
          {kind !== "flatten" && kind !== "terrace" ? (
            <label className="flex items-center gap-1">
              <span className="text-muted-foreground">Amount</span>
              <Input
                aria-label="Terrain brush amount"
                type="number"
                min={1}
                max={16}
                className="w-14"
                value={amount}
                disabled={busy}
                onChange={(event) => {
                  setAmount(Number(event.target.value));
                  resetPreview();
                }}
              />
            </label>
          ) : null}
          {kind === "noise" ? (
            <label className="flex items-center gap-1">
              <span className="text-muted-foreground">Seed</span>
              <Input
                aria-label="Terrain brush seed"
                type="number"
                min={0}
                className="w-24"
                value={seed}
                disabled={busy}
                onChange={(event) => {
                  setSeed(Number(event.target.value));
                  resetPreview();
                }}
              />
            </label>
          ) : null}
          {kind === "terrace" ? (
            <label className="flex items-center gap-1">
              <span className="text-muted-foreground">Step</span>
              <Input
                aria-label="Terrace step"
                type="number"
                min={2}
                max={16}
                className="w-14"
                value={step}
                disabled={busy}
                onChange={(event) => {
                  setStep(Number(event.target.value));
                  resetPreview();
                }}
              />
            </label>
          ) : null}
          {kind === "flatten" ? (
            <label className="flex items-center gap-1">
              <span className="text-muted-foreground">Target Y</span>
              <Input
                aria-label="Flatten target Y"
                type="number"
                placeholder="median"
                className="w-20"
                value={targetY}
                disabled={busy}
                onChange={(event) => {
                  setTargetY(event.target.value);
                  resetPreview();
                }}
              />
            </label>
          ) : null}
        </div>
        <div className="text-sm leading-relaxed text-muted-foreground">
          Material <span className="font-mono text-foreground">{material.name}</span>. Adjusts the
          topmost non-air block of each selected column; empty columns are skipped. A selection is
          required.
        </div>
      </div>

      <div className="w-64 space-y-3 rounded-xl border bg-card p-4">
        <div className="flex items-center justify-between gap-2">
          <span className="text-muted-foreground">Selection</span>
          <span className="font-mono">
            {selection ? `${boundsVolume(selection).toLocaleString()} blocks` : "not set"}
          </span>
        </div>
        {selection ? (
          <div className="font-mono text-xs text-muted-foreground">
            {selection.min.x},{selection.min.y},{selection.min.z} → {selection.max.x},
            {selection.max.y},{selection.max.z}
            {brushBounds ? (
              <div>
                brush box Y {brushBounds.min.y}..{brushBounds.max.y} (+
                {TERRAIN_BRUSH_VERTICAL_MARGIN} air margin)
              </div>
            ) : null}
          </div>
        ) : (
          <div className="text-xs text-status-warn">
            Use the Box select tool on the terrain, then return here.
          </div>
        )}
        <div className="flex flex-wrap gap-2 pt-0.5">
          <Button
            size="sm"
            variant="outline"
            disabled={!editing || !operation || busy}
            aria-label="Preview terrain brush"
            onClick={() => void previewBrush()}
          >
            {previewing ? (
              <Loader2 className="mr-1 size-3 animate-spin" />
            ) : (
              <ScanLine className="mr-1 size-3" />
            )}
            Preview
          </Button>
          <Button
            size="sm"
            disabled={!editing || !operation || busy}
            aria-label="Apply terrain brush"
            onClick={() => void applyBrush()}
          >
            {job ? <Loader2 className="mr-1 size-3 animate-spin" /> : null}
            Apply brush
          </Button>
        </div>
        {preview ? (
          <div
            className="rounded-lg border border-status-ok/30 bg-status-ok/5 p-3 text-sm text-status-ok"
            data-testid="terrain-preview"
          >
            {preview.changedBlocks.toLocaleString()} block(s) would change out of{" "}
            {preview.affectedBlocks.toLocaleString()} selected.
            {preview.warnings.map((warning) => (
              <div key={warning} className="text-status-warn">
                {warning}
              </div>
            ))}
          </div>
        ) : null}
        {error ? (
          <div className="rounded-lg border border-status-error/30 bg-status-error/5 p-3 text-sm text-status-error">
            {error}
          </div>
        ) : null}
      </div>
    </div>
  );
}
