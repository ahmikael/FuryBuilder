import { useEffect, useMemo, useState } from "react";
import { Grid3X3, Loader2, ScanLine } from "lucide-react";
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
import { boundsVolume, MAX_EDIT_VOLUME, planHeightmap } from "@/editor/session";
import type { BlockDefinition } from "@/editor/types";
import { isTauriRuntime, normalizeError } from "@/lib/ipc";
import { fetchBlockCatalog, previewOperation } from "@/lib/world-api";
import type { BlockState } from "@/renderer/types";
import { useToolStore } from "@/state/tool-store";
import { useWorldStore } from "@/state/world-store";

/**
 * M9.10 deterministic local heightmap generator. It builds a bounded heightmap patch from
 * explicit parameters and seed, then writes it as one reversible operation. This is a local
 * construction tool, not a vanilla world generator.
 */
export function HeightmapPanel() {
  const selection = useToolStore((state) => state.selection);
  const surface = useToolStore((state) => state.selectedBlock);
  const job = useToolStore((state) => state.job);
  const dimension = useWorldStore((state) => state.dimension);
  const [seed, setSeed] = useState(0);
  const [scale, setScale] = useState(8);
  const [octaves, setOctaves] = useState(3);
  const [amplitude, setAmplitude] = useState(8);
  const [baseY, setBaseY] = useState("");
  const [surfaceDepth, setSurfaceDepth] = useState(1);
  const [fillerName, setFillerName] = useState("minecraft:dirt");
  const [catalog, setCatalog] = useState<BlockDefinition[] | null>(null);
  const [catalogError, setCatalogError] = useState<string | null>(null);
  const [preview, setPreview] = useState<{
    changedBlocks: number;
    affectedBlocks: number;
    warnings: string[];
  } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [previewing, setPreviewing] = useState(false);
  const editing = isTauriRuntime();
  const busy = Boolean(job) || previewing;

  useEffect(() => {
    if (!editing) return;
    let cancelled = false;
    fetchBlockCatalog()
      .then((entries) => {
        if (!cancelled) {
          setCatalog(entries);
          setCatalogError(null);
        }
      })
      .catch((cause: unknown) => {
        if (!cancelled) setCatalogError(normalizeError(cause).message);
      });
    return () => {
      cancelled = true;
    };
  }, [editing]);

  const filler = useMemo<BlockState>(() => {
    const definition = catalog?.find((entry) => entry.name === fillerName) ?? null;
    return {
      name: fillerName,
      properties: definition ? { ...definition.properties } : {},
    };
  }, [catalog, fillerName]);

  const parsedBaseY = baseY.trim() === "" ? null : Number(baseY);
  const operation = useMemo(
    () =>
      planHeightmap({
        dimension,
        selection,
        seed,
        scale,
        octaves,
        amplitude,
        baseY: parsedBaseY !== null && Number.isFinite(parsedBaseY) ? parsedBaseY : null,
        surfaceDepth,
        surface,
        filler,
      }),
    [
      dimension,
      selection,
      seed,
      scale,
      octaves,
      amplitude,
      parsedBaseY,
      surfaceDepth,
      surface,
      filler,
    ],
  );
  const volume = operation ? boundsVolume(operation.bounds) : 0;
  const tooLarge = volume > MAX_EDIT_VOLUME;

  function resetPreview(): void {
    setPreview(null);
    setError(null);
  }

  async function previewHeightmap(): Promise<void> {
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

  async function applyHeightmap(): Promise<void> {
    if (!operation) return;
    setPreview(null);
    setError(null);
    await applyEditorOperation(operation);
  }

  return (
    <div className="flex flex-wrap items-start gap-4 text-sm">
      <div className="flex w-80 flex-col gap-1.5">
        <div className="flex items-center gap-2">
          <Grid3X3 className="size-3.5 text-primary" />
          <span className="font-medium">Heightmap generator</span>
          <Badge variant="secondary" className="uppercase">
            deterministic
          </Badge>
        </div>
        <div className="grid grid-cols-3 gap-1.5">
          <label className="flex flex-col gap-0.5">
            <span className="text-muted-foreground">Seed</span>
            <Input
              aria-label="Heightmap seed"
              type="number"
              min={0}
              value={seed}
              disabled={busy}
              onChange={(event) => {
                setSeed(Number(event.target.value));
                resetPreview();
              }}
            />
          </label>
          <label className="flex flex-col gap-0.5">
            <span className="text-muted-foreground">Scale</span>
            <Input
              aria-label="Heightmap scale"
              type="number"
              min={1}
              max={64}
              value={scale}
              disabled={busy}
              onChange={(event) => {
                setScale(Number(event.target.value));
                resetPreview();
              }}
            />
          </label>
          <label className="flex flex-col gap-0.5">
            <span className="text-muted-foreground">Octaves</span>
            <Input
              aria-label="Heightmap octaves"
              type="number"
              min={1}
              max={6}
              value={octaves}
              disabled={busy}
              onChange={(event) => {
                setOctaves(Number(event.target.value));
                resetPreview();
              }}
            />
          </label>
          <label className="flex flex-col gap-0.5">
            <span className="text-muted-foreground">Amplitude</span>
            <Input
              aria-label="Heightmap amplitude"
              type="number"
              min={1}
              max={256}
              value={amplitude}
              disabled={busy}
              onChange={(event) => {
                setAmplitude(Number(event.target.value));
                resetPreview();
              }}
            />
          </label>
          <label className="flex flex-col gap-0.5">
            <span className="text-muted-foreground">Base Y</span>
            <Input
              aria-label="Heightmap base Y"
              type="number"
              placeholder={selection ? String(selection.min.y) : "auto"}
              value={baseY}
              disabled={busy}
              onChange={(event) => {
                setBaseY(event.target.value);
                resetPreview();
              }}
            />
          </label>
          <label className="flex flex-col gap-0.5">
            <span className="text-muted-foreground">Surface depth</span>
            <Input
              aria-label="Heightmap surface depth"
              type="number"
              min={1}
              max={32}
              value={surfaceDepth}
              disabled={busy}
              onChange={(event) => {
                setSurfaceDepth(Number(event.target.value));
                resetPreview();
              }}
            />
          </label>
        </div>
        <div className="text-sm leading-relaxed text-muted-foreground">
          World-anchored integer noise. Surface uses the palette material{" "}
          <span className="font-mono text-foreground">{surface.name}</span>; filler is below it.
          Blocks above the generated surface inside the bounds are cleared.
        </div>
      </div>

      <div className="w-44 space-y-3 rounded-xl border bg-card p-4">
        <label className="flex flex-col gap-1">
          <span className="text-muted-foreground">Filler material</span>
          <Select
            items={
              catalog?.map((entry) => ({ value: entry.name, label: entry.name })) ?? [
                { value: "minecraft:dirt", label: "minecraft:dirt" },
              ]
            }
            value={fillerName}
            onValueChange={(value) => {
              setFillerName(value ?? "minecraft:dirt");
              resetPreview();
            }}
            disabled={busy || !editing}
          >
            <SelectTrigger className="w-fit" aria-label="Heightmap filler">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {catalog?.map((entry) => (
                <SelectItem key={entry.name} value={entry.name}>
                  {entry.name}
                </SelectItem>
              )) ?? <SelectItem value="minecraft:dirt">minecraft:dirt</SelectItem>}
            </SelectContent>
          </Select>
        </label>
        {catalogError ? (
          <div className="text-xs text-status-warn">Catalog unavailable: {catalogError}</div>
        ) : null}
        <div className="text-sm leading-relaxed text-muted-foreground">
          Same seed, parameters and world coordinates always produce the same patch.
        </div>
      </div>

      <div className="w-64 space-y-3 rounded-xl border bg-card p-4">
        <div className="flex items-center justify-between gap-2">
          <span className="text-muted-foreground">Footprint</span>
          <span className="font-mono">
            {selection ? `${boundsVolume(selection).toLocaleString()} selected` : "not set"}
          </span>
        </div>
        {operation ? (
          <div className="font-mono text-xs text-muted-foreground">
            bounds Y {operation.bounds.min.y}..{operation.bounds.max.y} · {volume.toLocaleString()}{" "}
            blocks
          </div>
        ) : (
          <div className="text-xs text-status-warn">
            Use the Box select tool on the terrain, then return here.
          </div>
        )}
        {tooLarge ? (
          <div className="text-xs text-status-error">
            Footprint exceeds the {MAX_EDIT_VOLUME.toLocaleString()} block limit; reduce the
            selection or amplitude.
          </div>
        ) : null}
        <div className="flex flex-wrap gap-2 pt-0.5">
          <Button
            size="sm"
            variant="outline"
            disabled={!editing || !operation || busy || tooLarge}
            aria-label="Preview heightmap"
            onClick={() => void previewHeightmap()}
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
            disabled={!editing || !operation || busy || tooLarge}
            aria-label="Apply heightmap"
            onClick={() => void applyHeightmap()}
          >
            {job ? <Loader2 className="mr-1 size-3 animate-spin" /> : null}
            Generate
          </Button>
        </div>
        {preview ? (
          <div
            className="rounded-lg border border-status-ok/30 bg-status-ok/5 p-3 text-sm text-status-ok"
            data-testid="heightmap-preview"
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
        <div className="text-xs text-muted-foreground">
          Local tool only; no vanilla worldgen parity claim.
        </div>
      </div>
    </div>
  );
}
