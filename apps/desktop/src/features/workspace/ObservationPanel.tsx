import { useState } from "react";
import { AlertTriangle, Crosshair, Loader2, ScanLine } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { observeRegion } from "@/lib/observation-api";
import { isTauriRuntime, normalizeError } from "@/lib/ipc";
import {
  OBSERVATION_MODES,
  observationBoundsLabel,
  observationModeLabel,
  type Observation,
  type ObservationMode,
  type ObservationRequest,
  type ObservationSliceAxis,
} from "@/project/observation-types";
import { useToolStore } from "@/state/tool-store";
import { useWorldStore } from "@/state/world-store";

/**
 * Deterministic spatial observations rendered by the Rust Core. The preview is a derived
 * map at an exact world revision; exact block queries remain the authority, and the panel
 * never writes blocks or metadata.
 */
export function ObservationPanel() {
  const selection = useToolStore((state) => state.selection);
  const dimension = useWorldStore((state) => state.dimension);
  const revision = useWorldStore((state) => state.revision);
  const [mode, setMode] = useState<ObservationMode>("material_map");
  const [sliceAxis, setSliceAxis] = useState<ObservationSliceAxis>("y");
  const [sliceCoordinate, setSliceCoordinate] = useState(64);
  const [fromRevision, setFromRevision] = useState(Math.max(0, revision - 1));
  const [observation, setObservation] = useState<Observation | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const editing = isTauriRuntime();

  if (!editing) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-1.5 text-center text-[11px] text-muted-foreground">
        <ScanLine className="size-5 text-muted-foreground/60" />
        <p className="max-w-md leading-relaxed">
          Observation maps are rendered and bounded by the Rust Core. Open the desktop app to
          inspect top-down, height, material, biome, cross-section and revision-diff maps.
        </p>
      </div>
    );
  }

  const changeMode = (next: ObservationMode) => {
    setMode(next);
    setObservation(null);
    setError(null);
  };

  const renderObservation = async () => {
    if (!selection) {
      setError("Select a box in the viewport first.");
      return;
    }
    setBusy(true);
    setError(null);
    const request: ObservationRequest = {
      dimension,
      bounds: { min: selection.min, max: selection.max },
      mode,
    };
    if (mode === "cross_section") {
      request.slice = { axis: sliceAxis, coordinate: sliceCoordinate };
    }
    if (mode === "revision_diff") {
      request.fromRevision = fromRevision;
    }
    try {
      setObservation(await observeRegion(request));
    } catch (cause) {
      setError(normalizeError(cause).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex min-h-0 flex-1 gap-2 text-[11px]" data-testid="observation-panel">
      <div className="scrollbar-thin flex min-h-0 w-80 shrink-0 flex-col gap-1.5 overflow-y-auto rounded border border-border bg-background/40 p-2">
        <div className="flex items-center gap-1.5">
          <ScanLine className="size-3 text-muted-foreground" />
          <span className="text-[10px] font-semibold tracking-wide uppercase">Observation</span>
          <span className="ml-auto font-mono text-[8px] text-muted-foreground">
            schema 1 · revision {revision}
          </span>
        </div>

        <label className="flex flex-col gap-0.5 text-[9px] text-muted-foreground">
          <span>Mode</span>
          <select
            className="h-6 rounded border border-border bg-background px-1 text-[10px]"
            value={mode}
            aria-label="Observation mode"
            onChange={(event) => changeMode(event.target.value as ObservationMode)}
          >
            {OBSERVATION_MODES.map((entry) => (
              <option key={entry.value} value={entry.value}>
                {entry.label}
              </option>
            ))}
          </select>
        </label>
        <p className="text-[9px] leading-relaxed text-muted-foreground">
          {OBSERVATION_MODES.find((entry) => entry.value === mode)?.description}
        </p>

        <div className="rounded border border-border/70 p-1.5">
          <div className="flex items-center gap-1 text-[9px] text-muted-foreground">
            <Crosshair className="size-3" /> Selection bounds
          </div>
          <p className="mt-0.5 break-all font-mono text-[9px]">
            {selection ? observationBoundsLabel(selection) : "Select a box in the viewport."}
          </p>
        </div>

        {mode === "cross_section" ? (
          <div className="grid grid-cols-2 gap-1">
            <label className="flex flex-col gap-0.5 text-[9px] text-muted-foreground">
              <span>Slice axis</span>
              <select
                className="h-6 rounded border border-border bg-background px-1 text-[10px]"
                value={sliceAxis}
                aria-label="Slice axis"
                onChange={(event) => setSliceAxis(event.target.value as ObservationSliceAxis)}
              >
                <option value="x">X plane</option>
                <option value="y">Y plane</option>
                <option value="z">Z plane</option>
              </select>
            </label>
            <label className="flex flex-col gap-0.5 text-[9px] text-muted-foreground">
              <span>Slice coordinate</span>
              <Input
                className="h-6"
                type="number"
                value={sliceCoordinate}
                aria-label="Slice coordinate"
                onChange={(event) => setSliceCoordinate(Number(event.target.value))}
              />
            </label>
          </div>
        ) : null}

        {mode === "revision_diff" ? (
          <label className="flex flex-col gap-0.5 text-[9px] text-muted-foreground">
            <span>From revision</span>
            <Input
              className="h-6"
              type="number"
              min={0}
              max={revision}
              value={fromRevision}
              aria-label="From revision"
              onChange={(event) => setFromRevision(Number(event.target.value))}
            />
          </label>
        ) : null}

        <Button
          size="sm"
          variant="outline"
          disabled={busy}
          onClick={() => void renderObservation()}
        >
          {busy ? (
            <Loader2 className="mr-1 size-3 animate-spin" />
          ) : (
            <ScanLine className="mr-1 size-3" />
          )}
          Render observation
        </Button>
        <p className="text-[9px] text-muted-foreground">
          Read-only. Up to 512×512 pixels and 8,000,000 scanned blocks; revision diff fails closed
          when the requested revision is no longer retained in history.
        </p>
        {error ? <p className="text-[10px] text-status-error">{error}</p> : null}
      </div>

      <div className="scrollbar-thin flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto rounded border border-border bg-background/40 p-2">
        {observation ? (
          <>
            <div className="flex flex-wrap items-center gap-1.5">
              <ScanLine className="size-3 text-muted-foreground" />
              <span className="text-[11px] font-semibold">
                {observationModeLabel(observation.mode)}
              </span>
              <span className="rounded border border-border px-1 font-mono text-[8px] text-muted-foreground">
                {observation.dimension.replace("minecraft:", "")}
              </span>
              <span className="ml-auto font-mono text-[8px] text-muted-foreground">
                world revision {observation.worldRevision}
              </span>
            </div>

            <img
              src={observation.image}
              alt={`${observationModeLabel(observation.mode)} observation`}
              className="max-h-64 w-full rounded border border-border bg-black/30 object-contain"
              style={{ imageRendering: "pixelated" }}
            />

            <div className="grid grid-cols-2 gap-x-3 gap-y-1 font-mono text-[9px] text-muted-foreground">
              <span>
                image {observation.width}×{observation.height}
              </span>
              <span>{observation.blocksPerPixel} block / pixel</span>
              <span>{observation.sampledUnits.toLocaleString()} sampled units</span>
              <span>{observation.nonAirUnits.toLocaleString()} non-air units</span>
              <span className="col-span-2">
                bounds {observationBoundsLabel(observation.bounds)}
              </span>
              {observation.fromRevision !== null && observation.fromRevision !== undefined ? (
                <span className="col-span-2">
                  compared revision {observation.fromRevision} → {observation.worldRevision}
                </span>
              ) : null}
              {observation.mode === "revision_diff" ? (
                <span className="col-span-2">
                  added {observation.counts.added.toLocaleString()} · removed{" "}
                  {observation.counts.removed.toLocaleString()} · changed{" "}
                  {observation.counts.changed.toLocaleString()}
                </span>
              ) : null}
              <span className="col-span-2">
                scanned {observation.scannedBlocks.toLocaleString()} blocks · partial{" "}
                {observation.partial ? "yes" : "no"}
              </span>
            </div>

            <div className="space-y-1">
              <div className="flex items-center gap-1.5">
                <span className="text-[10px] font-semibold tracking-wide uppercase">Legend</span>
                {observation.legendTruncated ? (
                  <span className="font-mono text-[8px] text-status-warning">
                    truncated to 16 entries
                  </span>
                ) : null}
              </div>
              {observation.legend.length === 0 ? (
                <p className="text-[9px] text-muted-foreground">No legend entries.</p>
              ) : (
                <div className="grid grid-cols-2 gap-x-3 gap-y-0.5">
                  {observation.legend.map((entry) => (
                    <div key={entry.key} className="flex min-w-0 items-center gap-1">
                      <span
                        className="size-2.5 shrink-0 rounded-sm border border-black/30"
                        style={{ background: entry.color }}
                        aria-hidden="true"
                      />
                      <span className="truncate text-[9px]" title={entry.label}>
                        {entry.label}
                      </span>
                      <span className="ml-auto font-mono text-[8px] text-muted-foreground">
                        {entry.count.toLocaleString()}
                      </span>
                    </div>
                  ))}
                </div>
              )}
            </div>

            {observation.warnings.length > 0 ? (
              <div className="rounded border border-status-warning/40 bg-status-warning/5 p-1.5">
                {observation.warnings.map((warning) => (
                  <p
                    key={warning}
                    className="flex items-start gap-1 text-[9px] text-status-warning"
                  >
                    <AlertTriangle className="mt-0.5 size-3 shrink-0" /> {warning}
                  </p>
                ))}
              </div>
            ) : null}

            <div className="space-y-0.5">
              {observation.notes.map((note) => (
                <p key={note} className="text-[9px] text-muted-foreground">
                  {note}
                </p>
              ))}
            </div>
          </>
        ) : (
          <div className="flex h-full flex-col items-center justify-center gap-1 text-center text-[10px] text-muted-foreground">
            <ScanLine className="size-5 text-muted-foreground/50" />
            <p>Select a region and render a deterministic map from the authoritative revision.</p>
          </div>
        )}
      </div>
    </div>
  );
}
