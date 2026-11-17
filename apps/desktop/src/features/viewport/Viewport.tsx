import { useCallback, useEffect, useRef, useState, type FormEvent } from "react";
import {
  Box,
  Camera,
  ChevronDown,
  ChevronRight,
  Grid3X3,
  Loader2,
  MousePointer2,
  Orbit,
  Plane,
  Scan,
  TriangleAlert,
  Waypoints,
} from "lucide-react";
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
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { invokeCommand, isTauriRuntime } from "@/lib/ipc";
import type { WorldInfo } from "@/lib/contracts";
import { createDemoSnapshot } from "@/renderer/demo";
import { ViewportRenderer, type CameraMode } from "@/renderer/viewport-renderer";
import type { ChunkPos, ChunkSnapshotBatch, RenderSnapshot } from "@/renderer/types";
import type { CaptureResult } from "@/renderer/observation";
import {
  MAX_LOAD_RADIUS,
  MIN_LOAD_RADIUS,
  clampLoadRadius,
  useWorldStore,
} from "@/state/world-store";
import { viewportBus } from "@/features/viewport/viewport-bus";
import { cn } from "@/lib/utils";

function detectWebGl2(): boolean {
  try {
    const canvas = document.createElement("canvas");
    return canvas.getContext("webgl2") !== null;
  } catch {
    return false;
  }
}

function previewSummary(snapshot: RenderSnapshot): {
  blockCount: number;
  sectionCount: number;
} {
  let blockCount = 0;
  let sectionCount = 0;
  for (const chunk of snapshot.chunks) {
    for (const section of chunk.sections) {
      blockCount += section.nonAir;
      sectionCount += 1;
    }
  }
  return { blockCount, sectionCount };
}

async function loadPreview(renderer: ViewportRenderer): Promise<void> {
  const snapshot = createDemoSnapshot(useWorldStore.getState().loadRadius);
  const summary = previewSummary(snapshot);
  renderer.setSnapshot(snapshot);
  useWorldStore.getState().applySummary({
    source: "preview",
    dimension: snapshot.dimension,
    revision: snapshot.revision,
    blockCount: summary.blockCount,
    sectionCount: summary.sectionCount,
    chunkCount: snapshot.chunks.filter((chunk) => chunk.loaded).length,
  });
}

async function loadCore(renderer: ViewportRenderer, radius: number): Promise<void> {
  let info = await invokeCommand<WorldInfo>("world_info");
  if (info.blockCount === 0) {
    info = await invokeCommand<WorldInfo>("load_demo_world");
  }
  const snapshot = await invokeCommand<RenderSnapshot>("render_snapshot", {
    dimension: "minecraft:overworld",
    center: { x: 0, z: 0 },
    radius,
  });
  renderer.setSnapshot(snapshot);
  useWorldStore.getState().applySummary({
    source: "core",
    dimension: snapshot.dimension,
    revision: snapshot.revision,
    blockCount: info.blockCount,
    sectionCount: info.sectionCount,
    chunkCount: info.chunkCount,
  });
}

async function initialLoad(renderer: ViewportRenderer): Promise<void> {
  const store = useWorldStore.getState();
  store.setStatus("loading");
  try {
    if (isTauriRuntime()) {
      await loadCore(renderer, store.loadRadius);
    } else {
      await loadPreview(renderer);
    }
    useWorldStore.getState().setStatus("ready");
    renderer.setGridVisible(useWorldStore.getState().gridVisible);
    renderer.setChunkBordersVisible(useWorldStore.getState().chunkBordersVisible);
  } catch (error) {
    useWorldStore
      .getState()
      .setStatus("error", error instanceof Error ? error.message : String(error));
  }
}

async function reloadRadius(renderer: ViewportRenderer, radius: number): Promise<void> {
  const store = useWorldStore.getState();
  store.setStatus("loading");
  try {
    if (isTauriRuntime()) {
      await loadCore(renderer, radius);
    } else {
      await loadPreview(renderer);
    }
    useWorldStore.getState().setStatus("ready");
  } catch (error) {
    useWorldStore
      .getState()
      .setStatus("error", error instanceof Error ? error.message : String(error));
  }
}

async function refreshChunks(renderer: ViewportRenderer, chunks: ChunkPos[]): Promise<void> {
  if (chunks.length === 0) return;
  try {
    if (isTauriRuntime()) {
      const batch = await invokeCommand<ChunkSnapshotBatch>("render_chunks", {
        dimension: useWorldStore.getState().dimension,
        chunks,
      });
      renderer.updateChunks(batch);
      useWorldStore.getState().applySummary({ revision: batch.revision });
    } else {
      await loadPreview(renderer);
    }
  } catch (error) {
    useWorldStore
      .getState()
      .setStatus("error", error instanceof Error ? error.message : String(error));
  }
}

export function Viewport() {
  const containerRef = useRef<HTMLDivElement>(null);
  const rendererRef = useRef<ViewportRenderer | null>(null);
  const pointerDown = useRef<{ x: number; y: number } | null>(null);
  const [webgl2] = useState(() => detectWebGl2());
  const [captureOpen, setCaptureOpen] = useState(false);
  const [gotoOpen, setGotoOpen] = useState(false);
  const [detailsOpen, setDetailsOpen] = useState(true);
  const [captureImage, setCaptureImage] = useState<string | null>(null);
  const [goto, setGoto] = useState({ x: "0", y: "70", z: "0" });

  const status = useWorldStore((state) => state.status);
  const error = useWorldStore((state) => state.error);
  const source = useWorldStore((state) => state.source);
  const dimension = useWorldStore((state) => state.dimension);
  const revision = useWorldStore((state) => state.revision);
  const loadRadius = useWorldStore((state) => state.loadRadius);
  const metrics = useWorldStore((state) => state.metrics);
  const cameraPose = useWorldStore((state) => state.cameraPose);
  const selection = useWorldStore((state) => state.selection);
  const lastCapture = useWorldStore((state) => state.lastCapture);
  const gridVisible = useWorldStore((state) => state.gridVisible);
  const chunkBordersVisible = useWorldStore((state) => state.chunkBordersVisible);

  useEffect(() => {
    const container = containerRef.current;
    if (!container || !webgl2) return;

    let renderer: ViewportRenderer;
    try {
      renderer = new ViewportRenderer(container, {
        onMetrics: (value) => useWorldStore.getState().setMetrics(value),
        onCameraPose: (pose) => useWorldStore.getState().setCameraPose(pose),
        onPick: (hit) => {
          useWorldStore.getState().setSelection(
            hit
              ? {
                  dimension: useWorldStore.getState().dimension,
                  pos: hit.pos,
                  state: hit.state,
                  face: hit.face,
                }
              : null,
          );
        },
        onContextLost: () =>
          useWorldStore
            .getState()
            .setStatus("error", "The WebGL context was lost. Reopen the viewport to recover."),
      });
    } catch (constructionError) {
      useWorldStore
        .getState()
        .setStatus(
          "error",
          constructionError instanceof Error
            ? constructionError.message
            : String(constructionError),
        );
      return;
    }

    rendererRef.current = renderer;
    if (import.meta.env.DEV) {
      (window as unknown as { __furyViewport?: ViewportRenderer }).__furyViewport = renderer;
    }
    const observer = new ResizeObserver(() => renderer.resize());
    observer.observe(container);
    void initialLoad(renderer);

    const onRefresh = (event: Event): void => {
      const detail = (event as CustomEvent<ChunkPos[]>).detail;
      if (Array.isArray(detail)) void refreshChunks(renderer, detail);
    };
    viewportBus.addEventListener("refresh-chunks", onRefresh);

    return () => {
      viewportBus.removeEventListener("refresh-chunks", onRefresh);
      observer.disconnect();
      renderer.dispose();
      rendererRef.current = null;
      if (import.meta.env.DEV) {
        const debugWindow = window as unknown as { __furyViewport?: ViewportRenderer };
        if (debugWindow.__furyViewport === renderer) delete debugWindow.__furyViewport;
      }
      useWorldStore.getState().setStatus("idle");
    };
  }, [webgl2]);

  const handleRadius = (radius: number): void => {
    const clamped = clampLoadRadius(radius);
    useWorldStore.getState().setLoadRadius(clamped);
    const renderer = rendererRef.current;
    if (renderer) void reloadRadius(renderer, clamped);
  };

  const toggleGrid = (): void => {
    const next = !useWorldStore.getState().gridVisible;
    useWorldStore.getState().setGridVisible(next);
    rendererRef.current?.setGridVisible(next);
  };

  const toggleBorders = (): void => {
    const next = !useWorldStore.getState().chunkBordersVisible;
    useWorldStore.getState().setChunkBordersVisible(next);
    rendererRef.current?.setChunkBordersVisible(next);
  };

  const setCameraMode = (mode: CameraMode): void => {
    rendererRef.current?.setCameraMode(mode);
  };

  const doCapture = (): void => {
    const renderer = rendererRef.current;
    if (!renderer) return;
    const result: CaptureResult = renderer.capture();
    useWorldStore.getState().setLastCapture(result.metadata);
    setCaptureImage(result.dataUrl);
    setCaptureOpen(true);
  };

  const downloadCapture = (): void => {
    if (!lastCapture || !captureImage) return;
    const anchor = document.createElement("a");
    anchor.href = captureImage;
    anchor.download = `furybuilder-${lastCapture.dimension.replace(":", "_")}-rev${lastCapture.worldRevision}.png`;
    anchor.click();
  };

  const copyMetadata = useCallback(async (): Promise<void> => {
    if (!lastCapture) return;
    try {
      await navigator.clipboard.writeText(JSON.stringify(lastCapture, null, 2));
    } catch {
      // Clipboard permissions may be unavailable; the JSON is visible in the dialog.
    }
  }, [lastCapture]);

  const submitGoto = (event: FormEvent): void => {
    event.preventDefault();
    const x = Number(goto.x);
    const y = Number(goto.y);
    const z = Number(goto.z);
    if (![x, y, z].every(Number.isFinite)) return;
    rendererRef.current?.focusOn({ x, y, z });
    setGotoOpen(false);
  };

  const openGoto = (): void => {
    const pose = useWorldStore.getState().cameraPose;
    const position = pose?.position ?? [0, 80, 0];
    setGoto({
      x: String(Math.round(position[0])),
      y: String(Math.round(position[1])),
      z: String(Math.round(position[2])),
    });
    setGotoOpen(true);
  };

  const position = cameraPose?.position.map((value) => Math.round(value)) ?? [0, 0, 0];
  const positionLabel = `${position[0]}, ${position[1]}, ${position[2]}`;

  return (
    <main className="relative min-h-0 min-w-0 flex-1 overflow-hidden bg-[oklch(0.17_0.014_260)]">
      <div
        ref={containerRef}
        className="absolute inset-0"
        onPointerDown={(event) => {
          if (event.button === 0) {
            pointerDown.current = { x: event.clientX, y: event.clientY };
          }
        }}
        onPointerUp={(event) => {
          if (event.button !== 0) return;
          const start = pointerDown.current;
          pointerDown.current = null;
          if (!start) return;
          if (Math.hypot(event.clientX - start.x, event.clientY - start.y) > 5) return;
          const renderer = rendererRef.current;
          if (renderer) renderer.pickAt(event.clientX, event.clientY);
        }}
        onDoubleClick={(event) => {
          const renderer = rendererRef.current;
          if (!renderer) return;
          const hit = renderer.pickAt(event.clientX, event.clientY);
          if (hit) renderer.focusOn(hit.pos);
        }}
        data-testid="viewport-canvas"
      />

      {!webgl2 ? (
        <div className="absolute inset-0 z-10 flex flex-col items-center justify-center gap-3 bg-[oklch(0.17_0.014_260)] text-center">
          <div className="grid size-16 place-items-center rounded-xl border border-white/10 bg-white/[0.03]">
            <Box className="size-8 text-primary/80" strokeWidth={1.2} />
          </div>
          <h1 className="text-sm font-semibold text-white/90">3D viewport</h1>
          <p className="max-w-sm text-[11px] leading-relaxed text-white/50">
            WebGL2 is unavailable in this WebView. Rendering is disabled, but world data and the
            command API still work.
          </p>
        </div>
      ) : null}

      {webgl2 && status === "loading" ? (
        <div className="pointer-events-none absolute inset-0 z-10 grid place-items-center bg-black/25">
          <div className="flex items-center gap-2 rounded-md border border-white/10 bg-black/50 px-3 py-2 text-[11px] text-white/70">
            <Loader2 className="size-3.5 animate-spin" /> Loading chunk snapshots…
          </div>
        </div>
      ) : null}

      {webgl2 && status === "error" ? (
        <div className="absolute inset-0 z-10 grid place-items-center bg-black/55 p-6">
          <div className="max-w-md space-y-2 rounded-lg border border-status-error/40 bg-card/95 p-4 text-center">
            <TriangleAlert className="mx-auto size-6 text-status-error" />
            <h2 className="text-sm font-semibold">Viewport unavailable</h2>
            <p className="text-[11px] leading-relaxed text-muted-foreground">
              {error ?? "The renderer could not be started."}
            </p>
            <Button
              size="sm"
              variant="outline"
              onClick={() => {
                const renderer = rendererRef.current;
                if (renderer) void initialLoad(renderer);
                else window.location.reload();
              }}
            >
              Retry
            </Button>
          </div>
        </div>
      ) : null}

      <div className="pointer-events-none absolute top-2 left-2 z-20 flex items-center gap-2">
        <span className="rounded border border-white/10 bg-black/40 px-2 py-1 font-mono text-[10px] text-white/65">
          {dimension === "minecraft:overworld"
            ? "Overworld"
            : dimension.replace("minecraft:", "").replace(/_/g, " ")}
          {" · "}
          {positionLabel}
        </span>
        <span className="rounded border border-white/10 bg-black/40 px-2 py-1 font-mono text-[10px] text-white/50">
          rev {revision}
        </span>
        <span
          className={cn(
            "rounded border px-2 py-1 font-mono text-[10px]",
            source === "core"
              ? "border-status-ok/30 bg-status-ok/10 text-status-ok"
              : "border-status-warn/30 bg-status-warn/10 text-status-warn",
          )}
        >
          {source === "core" ? "Rust core" : source === "preview" ? "Preview data" : "…"}
        </span>
      </div>

      <div className="absolute top-2 right-2 z-20 flex items-center gap-1">
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant="ghost"
              size="icon"
              aria-label="Fly camera"
              className="text-white/70 hover:bg-white/10"
              onClick={() => setCameraMode("fly")}
            >
              <Plane />
            </Button>
          </TooltipTrigger>
          <TooltipContent>Fly camera — WASD, Space/Ctrl, right-drag to look.</TooltipContent>
        </Tooltip>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant="ghost"
              size="icon"
              aria-label="Orbit camera"
              className="text-white/70 hover:bg-white/10"
              onClick={() => setCameraMode("orbit")}
            >
              <Orbit />
            </Button>
          </TooltipTrigger>
          <TooltipContent>Orbit camera — drag to rotate, wheel to zoom.</TooltipContent>
        </Tooltip>
        <Separator orientation="vertical" className="mx-1 h-5 bg-white/10" />
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant="ghost"
              size="icon"
              aria-label="Toggle grid"
              className={cn("hover:bg-white/10", gridVisible ? "text-primary" : "text-white/40")}
              onClick={toggleGrid}
            >
              <Grid3X3 />
            </Button>
          </TooltipTrigger>
          <TooltipContent>Toggle the ground grid.</TooltipContent>
        </Tooltip>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant="ghost"
              size="icon"
              aria-label="Toggle chunk borders"
              className={cn(
                "hover:bg-white/10",
                chunkBordersVisible ? "text-primary" : "text-white/40",
              )}
              onClick={toggleBorders}
            >
              <Waypoints />
            </Button>
          </TooltipTrigger>
          <TooltipContent>Toggle chunk borders.</TooltipContent>
        </Tooltip>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant="ghost"
              size="icon"
              aria-label="Go to coordinates"
              className="text-white/70 hover:bg-white/10"
              onClick={openGoto}
            >
              <MousePointer2 />
            </Button>
          </TooltipTrigger>
          <TooltipContent>Fly the camera to exact coordinates.</TooltipContent>
        </Tooltip>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant="ghost"
              size="icon"
              aria-label="Capture viewport"
              className="text-white/70 hover:bg-white/10"
              onClick={doCapture}
            >
              <Camera />
            </Button>
          </TooltipTrigger>
          <TooltipContent>Capture the viewport with observation metadata.</TooltipContent>
        </Tooltip>
      </div>

      <div className="absolute bottom-2 left-2 z-20 w-60 rounded-md border border-white/10 bg-black/55 text-white/70 backdrop-blur-sm">
        <button
          type="button"
          className="flex w-full items-center gap-1.5 px-2 py-1.5 text-left text-[10px] font-semibold tracking-wide uppercase"
          onClick={() => setDetailsOpen((value) => !value)}
        >
          {detailsOpen ? <ChevronDown className="size-3" /> : <ChevronRight className="size-3" />}
          Performance
          <span className="ml-auto font-mono text-[10px] normal-case">
            {metrics ? `${metrics.fps} fps` : "—"}
          </span>
        </button>
        {detailsOpen ? (
          <div className="grid grid-cols-[1fr_auto] gap-x-3 gap-y-0.5 border-t border-white/10 px-2 py-1.5 font-mono text-[9px]">
            <span>Frame</span>
            <span className="text-right">{metrics ? `${metrics.frameMs} ms` : "—"}</span>
            <span>Draw calls</span>
            <span className="text-right">{metrics?.drawCalls ?? "—"}</span>
            <span>Triangles</span>
            <span className="text-right">{metrics?.triangles.toLocaleString() ?? "—"}</span>
            <span>Meshes</span>
            <span className="text-right">{metrics?.meshCount ?? "—"}</span>
            <span>Geometries</span>
            <span className="text-right">{metrics?.geometries ?? "—"}</span>
            <span>Textures</span>
            <span className="text-right">{metrics?.textures ?? "—"}</span>
            <span>Chunks</span>
            <span className="text-right">{metrics?.loadedChunks ?? "—"}</span>
            <span>Sections</span>
            <span className="text-right">{metrics?.renderedSections ?? "—"}</span>
            <span>View distance</span>
            <span className="text-right">
              <select
                aria-label="View distance"
                className="rounded border border-white/10 bg-black/40 px-1 py-0.5 font-mono text-[9px] text-white/70"
                value={loadRadius}
                onChange={(event) => handleRadius(Number(event.target.value))}
              >
                {Array.from(
                  { length: MAX_LOAD_RADIUS - MIN_LOAD_RADIUS + 1 },
                  (_, index) => MIN_LOAD_RADIUS + index,
                ).map((radius) => (
                  <option key={radius} value={radius}>
                    {radius * 2 + 1}×{radius * 2 + 1}
                  </option>
                ))}
              </select>
            </span>
          </div>
        ) : null}
      </div>

      <div className="pointer-events-none absolute right-2 bottom-2 z-20 rounded border border-white/10 bg-black/45 px-2.5 py-1 font-mono text-[9px] text-white/45">
        WASD fly · RMB look · Shift boost · click pick · dbl-click focus
      </div>

      {selection ? (
        <div className="pointer-events-none absolute bottom-2 left-1/2 z-20 max-w-[60%] -translate-x-1/2 truncate rounded border border-primary/40 bg-black/60 px-2 py-1 font-mono text-[10px] text-white/75">
          selected {selection.pos.x}, {selection.pos.y}, {selection.pos.z} ·{" "}
          {selection.state.name.replace("minecraft:", "")}
          {selection.state.properties && Object.keys(selection.state.properties).length > 0
            ? ` [${Object.entries(selection.state.properties)
                .map(([key, value]) => `${key}=${value}`)
                .join(",")}]`
            : ""}
        </div>
      ) : null}

      <Dialog open={gotoOpen} onOpenChange={setGotoOpen}>
        <DialogContent className="max-w-sm">
          <DialogHeader>
            <DialogTitle>Go to coordinates</DialogTitle>
            <DialogDescription>
              Moves the camera to a position near the given block coordinates.
            </DialogDescription>
          </DialogHeader>
          <form onSubmit={submitGoto} className="space-y-3">
            <div className="grid grid-cols-3 gap-2">
              {(["x", "y", "z"] as const).map((axis) => (
                <label key={axis} className="space-y-1 text-[10px] text-muted-foreground">
                  <span className="font-mono uppercase">{axis}</span>
                  <Input
                    value={goto[axis]}
                    inputMode="numeric"
                    onChange={(event) =>
                      setGoto((current) => ({ ...current, [axis]: event.target.value }))
                    }
                    aria-label={`${axis} coordinate`}
                  />
                </label>
              ))}
            </div>
            <div className="flex justify-end gap-2">
              <Button type="button" variant="outline" size="sm" onClick={() => setGotoOpen(false)}>
                Cancel
              </Button>
              <Button type="submit" size="sm">
                Go
              </Button>
            </div>
          </form>
        </DialogContent>
      </Dialog>

      <Dialog open={captureOpen} onOpenChange={setCaptureOpen}>
        <DialogContent className="max-w-3xl">
          <DialogHeader>
            <DialogTitle>Viewport capture</DialogTitle>
            <DialogDescription>
              Observation metadata — revision {lastCapture?.worldRevision ?? "—"}, camera and
              completeness are recorded with the image.
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-2">
            <div className="overflow-hidden rounded border border-border bg-black/40">
              <img
                src={captureImage ?? ""}
                alt="Captured viewport"
                className="max-h-64 w-full object-contain"
              />
            </div>
            {lastCapture ? (
              <div className="grid grid-cols-2 gap-x-4 gap-y-1 rounded border border-border bg-background/40 p-2 font-mono text-[10px]">
                <span className="text-muted-foreground">Dimension</span>
                <span>{lastCapture.dimension}</span>
                <span className="text-muted-foreground">World revision</span>
                <span>{lastCapture.worldRevision}</span>
                <span className="text-muted-foreground">Resolution</span>
                <span>
                  {lastCapture.resolution[0]}×{lastCapture.resolution[1]}
                </span>
                <span className="text-muted-foreground">Camera</span>
                <span className="truncate">
                  {lastCapture.camera.position.map((value) => value.toFixed(1)).join(", ")}
                </span>
                <span className="text-muted-foreground">Completeness</span>
                <span className={lastCapture.partial ? "text-status-warn" : "text-status-ok"}>
                  {lastCapture.partial
                    ? `partial — ${lastCapture.missingChunks.length} chunk(s) missing`
                    : "complete for the loaded area"}
                </span>
                <span className="text-muted-foreground">Loaded chunks</span>
                <span>{lastCapture.loadedChunks.length}</span>
              </div>
            ) : null}
            {lastCapture ? (
              <details className="rounded border border-border bg-background/40 p-2">
                <summary className="cursor-pointer text-[10px] font-medium text-muted-foreground">
                  Legend and full metadata ({lastCapture.legend.length} states
                  {lastCapture.legendTruncated ? ", truncated" : ""})
                </summary>
                <div className="mt-2 grid max-h-32 grid-cols-2 gap-x-4 gap-y-0.5 overflow-y-auto font-mono text-[10px]">
                  {lastCapture.legend.map((entry) => (
                    <div key={entry.state} className="flex items-center gap-1.5">
                      <span
                        className="size-2.5 rounded-[2px] border border-black/30"
                        style={{ backgroundColor: entry.color }}
                      />
                      <span className="truncate">{entry.state}</span>
                      <span className="ml-auto text-muted-foreground">{entry.count}</span>
                    </div>
                  ))}
                </div>
                <pre className="scrollbar-thin mt-2 max-h-32 overflow-auto rounded bg-background/60 p-2 font-mono text-[9px] leading-relaxed text-muted-foreground">
                  {JSON.stringify(lastCapture, null, 2)}
                </pre>
              </details>
            ) : null}
          </div>
          <DialogFooter>
            <Button variant="outline" size="sm" onClick={() => void copyMetadata()}>
              Copy metadata
            </Button>
            <Button variant="outline" size="sm" onClick={downloadCapture}>
              Download PNG
            </Button>
            <Button size="sm" onClick={() => setCaptureOpen(false)}>
              Close
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {status === "ready" ? (
        <span className="pointer-events-none absolute top-11 left-2 z-10 rounded border border-white/5 bg-black/30 px-2 py-0.5 font-mono text-[9px] text-white/35">
          <Scan className="mr-1 inline size-3" />
          rev {revision} · {metrics?.geometries ?? 0} geometries
        </span>
      ) : null}
    </main>
  );
}
