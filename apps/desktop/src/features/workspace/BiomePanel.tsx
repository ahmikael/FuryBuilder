import { useEffect, useMemo, useState } from "react";
import { Leaf, Loader2, ScanLine } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { applyEditorOperation } from "@/editor/commands";
import { boundsVolume, isValidBiomeId, MAX_EDIT_VOLUME, planBiome } from "@/editor/session";
import { isTauriRuntime, normalizeError } from "@/lib/ipc";
import { previewOperation, readBiome } from "@/lib/world-api";
import { useToolStore } from "@/state/tool-store";
import { useWorldStore } from "@/state/world-store";

/**
 * M9.11 biome painter for the confirmed Java 1.21.4 layout. It paints the 4x4x4 biome
 * cells covering the selection as one reversible operation and never invents biome data
 * for a world that has none.
 */
const COMMON_BIOMES = [
  "minecraft:plains",
  "minecraft:forest",
  "minecraft:desert",
  "minecraft:taiga",
  "minecraft:jungle",
  "minecraft:savanna",
  "minecraft:snowy_plains",
  "minecraft:swamp",
  "minecraft:badlands",
  "minecraft:ocean",
  "minecraft:river",
  "minecraft:the_void",
  "minecraft:nether_wastes",
  "minecraft:soul_sand_valley",
  "minecraft:crimson_forest",
  "minecraft:warped_forest",
  "minecraft:basalt_deltas",
  "minecraft:the_end",
  "minecraft:end_highlands",
  "minecraft:end_midlands",
];

export function BiomePanel() {
  const selection = useToolStore((state) => state.selection);
  const job = useToolStore((state) => state.job);
  const dimension = useWorldStore((state) => state.dimension);
  const revision = useWorldStore((state) => state.revision);
  const [biome, setBiome] = useState("minecraft:forest");
  const [lookup, setLookup] = useState<{
    key: string;
    biome: string | null;
    error: string | null;
  } | null>(null);
  const [preview, setPreview] = useState<{
    changedBlocks: number;
    affectedBlocks: number;
    warnings: string[];
  } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [previewing, setPreviewing] = useState(false);
  const editing = isTauriRuntime();
  const busy = Boolean(job) || previewing;

  const lookupKey = selection
    ? `${dimension}@${revision}:${selection.min.x},${selection.min.y},${selection.min.z}`
    : null;
  useEffect(() => {
    if (!editing || !selection || !lookupKey) return;
    let cancelled = false;
    readBiome(dimension, selection.min)
      .then((value) => {
        if (!cancelled) setLookup({ key: lookupKey, biome: value, error: null });
      })
      .catch((cause: unknown) => {
        if (!cancelled) {
          setLookup({
            key: lookupKey,
            biome: null,
            error: normalizeError(cause).message,
          });
        }
      });
    return () => {
      cancelled = true;
    };
  }, [dimension, editing, lookupKey, selection]);
  const currentBiome = lookup?.key === lookupKey ? lookup.biome : null;
  const currentError = lookup?.key === lookupKey ? lookup.error : null;

  const operation = useMemo(
    () => planBiome({ dimension, selection, biome }),
    [biome, dimension, selection],
  );
  const volume = operation ? boundsVolume(operation.bounds) : 0;
  const tooLarge = volume > MAX_EDIT_VOLUME;
  const validBiome = isValidBiomeId(biome);

  function resetPreview(): void {
    setPreview(null);
    setError(null);
  }

  async function previewBiome(): Promise<void> {
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

  async function applyBiome(): Promise<void> {
    if (!operation) return;
    setPreview(null);
    setError(null);
    await applyEditorOperation(operation);
  }

  return (
    <div className="flex flex-wrap items-start gap-4 text-sm">
      <div className="flex w-80 flex-col gap-1.5">
        <div className="flex items-center gap-2">
          <Leaf className="size-3.5 text-primary" />
          <span className="font-medium">Biome painter</span>
          <span className="rounded bg-muted px-1 py-0.5 text-xs uppercase text-muted-foreground">
            Java 1.21.4
          </span>
        </div>
        <label className="flex flex-col gap-0.5">
          <span className="text-muted-foreground">Biome id</span>
          <Input
            aria-label="Biome id"
            list="fury-common-biomes"
            className="h-6 px-1 font-mono text-xs"
            value={biome}
            disabled={busy}
            onChange={(event) => {
              setBiome(event.target.value);
              resetPreview();
            }}
          />
          <datalist id="fury-common-biomes">
            {COMMON_BIOMES.map((entry) => (
              <option key={entry} value={entry} />
            ))}
          </datalist>
        </label>
        <div className="text-sm leading-relaxed text-muted-foreground">
          A namespaced lowercase id such as <span className="font-mono">minecraft:forest</span>.
          Every 4×4×4 cell touched by the selection is painted; cells outside it and all block
          states stay unchanged.
        </div>
      </div>

      <div className="w-56 space-y-3 rounded-xl border bg-card p-4">
        <div className="flex items-center justify-between gap-2">
          <span className="text-muted-foreground">Current biome</span>
          <span className="truncate font-mono" data-testid="current-biome">
            {selection ? (currentBiome ?? "no data") : "—"}
          </span>
        </div>
        <div className="text-sm leading-relaxed text-muted-foreground">
          {currentError
            ? `Current biome unavailable: ${currentError}`
            : "Read from the editable world at the selection minimum corner."}
        </div>
        {!validBiome ? (
          <div className="text-xs text-status-warn">
            Enter a namespaced id like minecraft:plains.
          </div>
        ) : null}
      </div>

      <div className="w-64 space-y-3 rounded-xl border bg-card p-4">
        <div className="flex items-center justify-between gap-2">
          <span className="text-muted-foreground">Selection</span>
          <span className="font-mono">
            {selection ? `${boundsVolume(selection).toLocaleString()} blocks` : "not set"}
          </span>
        </div>
        {operation ? (
          <div className="font-mono text-xs text-muted-foreground">
            Y {operation.bounds.min.y}..{operation.bounds.max.y} · {volume.toLocaleString()} blocks
          </div>
        ) : (
          <div className="text-xs text-status-warn">
            Use the Box select tool, then choose a biome id.
          </div>
        )}
        {tooLarge ? (
          <div className="text-xs text-status-error">
            Selection exceeds the {MAX_EDIT_VOLUME.toLocaleString()} block limit.
          </div>
        ) : null}
        <div className="flex flex-wrap gap-2 pt-0.5">
          <Button
            size="sm"
            variant="outline"
            disabled={!editing || !operation || busy || tooLarge}
            aria-label="Preview biome"
            onClick={() => void previewBiome()}
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
            aria-label="Apply biome"
            onClick={() => void applyBiome()}
          >
            {job ? <Loader2 className="mr-1 size-3 animate-spin" /> : null}
            Paint
          </Button>
        </div>
        {preview ? (
          <div
            className="rounded-lg border border-status-ok/30 bg-status-ok/5 p-3 text-sm text-status-ok"
            data-testid="biome-preview"
          >
            {preview.changedBlocks.toLocaleString()} biome block(s) would change out of{" "}
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
          Requires source biome data (imported 1.21.4 world or generated base). Export writes
          verified cells into a new world copy; no vanilla worldgen is simulated.
        </div>
      </div>
    </div>
  );
}
