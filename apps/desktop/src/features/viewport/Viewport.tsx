import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState, type FormEvent, type ReactNode } from "react";
import {
  Box,
  Camera,
  ChevronDown,
  ChevronRight,
  ClipboardPaste,
  Copy,
  Eraser,
  FolderOpen,
  Grid3X3,
  Loader2,
  MousePointer2,
  Orbit,
  PaintBucket,
  Paintbrush,
  Plane,
  Plus,
  Redo2,
  Replace,
  Scan,
  Spline,
  SquareDashed,
  TriangleAlert,
  Trash2,
  Undo2,
  Waypoints,
  X,
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
import { listenForRendererRequests } from "@/lib/renderer-bridge";
import { loadAssetBundle } from "@/lib/assets-api";
import { blockStateKey, type AssetStats } from "@/assets/model-assets";
import type { WorldInfo } from "@/lib/contracts";
import { createDemoSnapshot } from "@/renderer/demo";
import { ViewportRenderer, type CameraMode } from "@/renderer/viewport-renderer";
import type { BlockPosition, ChunkPos, ChunkSnapshotBatch, RenderSnapshot } from "@/renderer/types";
import type { CaptureResult } from "@/renderer/observation";
import type { RayHit } from "@/renderer/raycast";
import {
  MAX_LOAD_RADIUS,
  MIN_LOAD_RADIUS,
  clampLoadRadius,
  useWorldStore,
} from "@/state/world-store";
import { useToolStore } from "@/state/tool-store";
import { projectIsDirty, useProjectStore } from "@/state/project-store";
import {
  applyActiveTool,
  copySelection,
  deleteSelection,
  handleToolClick,
  pasteClipboard,
  pickBlockAsBrush,
  redoWorld,
  refreshGhost,
  setReplaceSource,
  undoWorld,
} from "@/editor/commands";
import type { EditorTool } from "@/editor/types";
import { MAX_BRUSH_RADIUS, MIN_BRUSH_RADIUS } from "@/editor/session";
import { useHistoryStore } from "@/state/history-store";
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
  const dimension = useWorldStore.getState().dimension;
  const info = await invokeCommand<WorldInfo>("world_info");
  const snapshot = await invokeCommand<RenderSnapshot>("render_snapshot", {
    dimension,
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
      void useHistoryStore.getState().refresh();
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

interface ContextMenuState {
  x: number;
  y: number;
  hit: RayHit | null;
}

const TOOL_ITEMS: Array<{
  tool: EditorTool;
  label: string;
  shortcut: string;
  icon: typeof MousePointer2;
}> = [
  { tool: "select", label: "Select / inspect", shortcut: "1", icon: MousePointer2 },
  { tool: "place", label: "Place block", shortcut: "2", icon: Plus },
  { tool: "remove", label: "Remove block", shortcut: "3", icon: Eraser },
  { tool: "brush", label: "Brush", shortcut: "4", icon: Paintbrush },
  { tool: "box", label: "Box select", shortcut: "5", icon: SquareDashed },
  { tool: "fill", label: "Fill selection", shortcut: "6", icon: PaintBucket },
  { tool: "line", label: "Line", shortcut: "7", icon: Spline },
  { tool: "replace", label: "Replace in selection", shortcut: "8", icon: Replace },
];

export function Viewport() {
  const containerRef = useRef<HTMLDivElement>(null);
  const rendererRef = useRef<ViewportRenderer | null>(null);
  const pointerDown = useRef<{ x: number; y: number } | null>(null);
  const rightDown = useRef<{ x: number; y: number } | null>(null);
  const hoverFrame = useRef<number | null>(null);
  const [webgl2] = useState(() => detectWebGl2());
  const [captureOpen, setCaptureOpen] = useState(false);
  const [gotoOpen, setGotoOpen] = useState(false);
  const [detailsOpen, setDetailsOpen] = useState(true);
  const [captureImage, setCaptureImage] = useState<string | null>(null);
  const [goto, setGoto] = useState({ x: "0", y: "70", z: "0" });
  const [contextMenu, setContextMenu] = useState<ContextMenuState | null>(null);
  const [assetStats, setAssetStats] = useState<AssetStats | null>(null);
  const [assetError, setAssetError] = useState<string | null>(null);
  const [sceneEpoch, setSceneEpoch] = useState(0);
  const assetSignature = useRef("");
  const assetBusy = useRef(false);
  const tauri = isTauriRuntime();
  const project = useProjectStore((state) => state.current);
  const setManagerOpen = useProjectStore((state) => state.setManagerOpen);
  const editingEnabled = tauri && Boolean(project?.folder);

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

  const tool = useToolStore((state) => state.tool);
  const brushRadius = useToolStore((state) => state.brushRadius);
  const selectedBlock = useToolStore((state) => state.selectedBlock);
  const replaceFrom = useToolStore((state) => state.replaceFrom);
  const boxSelection = useToolStore((state) => state.selection);
  const ghost = useToolStore((state) => state.ghost);
  const job = useToolStore((state) => state.job);
  const clipboard = useToolStore((state) => state.clipboard);
  const pendingPaste = useToolStore((state) => state.pendingPaste);
  const lastOperation = useToolStore((state) => state.lastOperation);
  const canUndo = useHistoryStore((state) => state.canUndo);
  const canRedo = useHistoryStore((state) => state.canRedo);
  const projectDirty = projectIsDirty(project, revision);
  const autosaved = projectDirty && project?.autosavedRevision === revision;

  const refreshAssets = useCallback(async (force = false): Promise<void> => {
    const renderer = rendererRef.current;
    if (!renderer || !isTauriRuntime() || assetBusy.current) return;
    const states = renderer.world.uniqueBlockStates();
    if (states.length === 0) return;
    const signature = states.map(blockStateKey).sort().join("|");
    if (!force && signature === assetSignature.current) return;
    assetBusy.current = true;
    try {
      const bundle = await loadAssetBundle(
        states.map((state) => ({
          name: state.name,
          properties: state.properties ?? {},
        })),
      );
      await renderer.setAssetBundle(bundle);
      assetSignature.current = signature;
      setAssetStats(renderer.assetStats);
      setAssetError(null);
    } catch (cause) {
      // Keep the documented placeholder fallback; never invent Minecraft textures.
      assetSignature.current = signature;
      renderer.clearAssetBundle();
      setAssetStats(null);
      setAssetError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      assetBusy.current = false;
    }
  }, []);

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
        onHover: (hit) => {
          useToolStore.getState().setHover(hit, useToolStore.getState().ghost);
          refreshGhost();
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
    let rendererDisposed = false;
    let stopRendererRequests: (() => void) | null = null;
    if (tauri) {
      void listenForRendererRequests(renderer).then((stop) => {
        if (rendererDisposed) stop();
        else stopRendererRequests = stop;
      });
    }
    if (import.meta.env.DEV) {
      (window as unknown as { __furyViewport?: ViewportRenderer }).__furyViewport = renderer;
    }
    const syncOverlays = (): void => {
      const state = useToolStore.getState();
      renderer.setSelectionBox(state.selection);
      renderer.setGhostBox(state.ghost?.bounds ?? null, state.ghost?.kind ?? "place");
    };
    syncOverlays();
    const unsubscribe = useToolStore.subscribe(syncOverlays);

    const observer = new ResizeObserver(() => renderer.resize());
    observer.observe(container);
    void initialLoad(renderer).then(() => {
      if (rendererRef.current !== renderer) return;
      // React StrictMode mounts effects twice in development. The first renderer may
      // finish loading after it was disposed, so only the current renderer may signal
      // that a snapshot is ready for asset resolution.
      setSceneEpoch((value) => value + 1);
      if (tauri) void useProjectStore.getState().resume();
    });

    const onRefresh = (event: Event): void => {
      const detail = (event as CustomEvent<ChunkPos[]>).detail;
      if (Array.isArray(detail)) void refreshChunks(renderer, detail);
    };
    const onReload = (): void => {
      void reloadRadius(renderer, useWorldStore.getState().loadRadius);
    };
    const onFocusPosition = (event: Event): void => {
      const detail = (event as CustomEvent<BlockPosition>).detail;
      if (detail && [detail.x, detail.y, detail.z].every((value) => Number.isFinite(value))) {
        renderer.focusOn(detail);
      }
    };
    viewportBus.addEventListener("refresh-chunks", onRefresh);
    viewportBus.addEventListener("reload-world", onReload);
    viewportBus.addEventListener("focus-position", onFocusPosition);

    return () => {
      rendererDisposed = true;
      stopRendererRequests?.();
      viewportBus.removeEventListener("refresh-chunks", onRefresh);
      viewportBus.removeEventListener("reload-world", onReload);
      viewportBus.removeEventListener("focus-position", onFocusPosition);
      unsubscribe();
      observer.disconnect();
      renderer.dispose();
      rendererRef.current = null;
      if (import.meta.env.DEV) {
        const debugWindow = window as unknown as { __furyViewport?: ViewportRenderer };
        if (debugWindow.__furyViewport === renderer) delete debugWindow.__furyViewport;
      }
      useWorldStore.getState().setStatus("idle");
    };
  }, [webgl2, tauri]);

  useEffect(() => {
    if (!contextMenu) return;
    const close = (event: Event): void => {
      if (event instanceof KeyboardEvent) {
        if (event.key === "Escape") setContextMenu(null);
        return;
      }
      const target = event.target;
      if (target instanceof Element && target.closest('[role="menu"]')) return;
      setContextMenu(null);
    };
    window.addEventListener("pointerdown", close);
    window.addEventListener("keydown", close);
    return () => {
      window.removeEventListener("pointerdown", close);
      window.removeEventListener("keydown", close);
    };
  }, [contextMenu]);

  useEffect(() => {
    refreshGhost();
  }, [tool, brushRadius, boxSelection, selectedBlock, replaceFrom]);

  useEffect(() => {
    if (!tauri || status !== "ready") return;
    void refreshAssets();
  }, [tauri, status, revision, sceneEpoch, refreshAssets]);

  useEffect(() => {
    if (!tauri) return;
    let disposed = false;
    let unlisten: (() => void) | null = null;
    void listen("furybuilder-assets-changed", () => {
      if (disposed) return;
      assetSignature.current = "";
      void refreshAssets(true);
    })
      .then((stop) => {
        if (disposed) stop();
        else unlisten = stop;
      })
      .catch(() => {
        // The event API is only present inside Tauri; the placeholder stays usable.
      });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [tauri, refreshAssets]);

  useEffect(
    () => () => {
      if (hoverFrame.current !== null) cancelAnimationFrame(hoverFrame.current);
      hoverFrame.current = null;
    },
    [],
  );

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

  const closeContextMenu = (): void => setContextMenu(null);

  const menuPlace = (): void => {
    if (!contextMenu?.hit) return;
    useToolStore.getState().setTool("place");
    void handleToolClick(contextMenu.hit);
    closeContextMenu();
  };

  const menuRemove = (): void => {
    if (!contextMenu?.hit) return;
    useToolStore.getState().setTool("remove");
    void handleToolClick(contextMenu.hit);
    closeContextMenu();
  };

  const menuPick = (): void => {
    pickBlockAsBrush(contextMenu?.hit ?? null);
    closeContextMenu();
  };

  const menuReplaceSource = (): void => {
    setReplaceSource(contextMenu?.hit ?? null);
    closeContextMenu();
  };

  const menuBoxCorner = (): void => {
    const hit = contextMenu?.hit;
    if (!hit) return;
    useToolStore.getState().setTool("box");
    useToolStore.getState().setAnchor(hit.pos);
    useToolStore.getState().setGhost(null);
    closeContextMenu();
  };

  const menuFocus = (): void => {
    const hit = contextMenu?.hit;
    if (hit) rendererRef.current?.focusOn(hit.pos);
    closeContextMenu();
  };

  const position = cameraPose?.position.map((value) => Math.round(value)) ?? [0, 0, 0];
  const positionLabel = `${position[0]}, ${position[1]}, ${position[2]}`;
  const toolLabel = TOOL_ITEMS.find((item) => item.tool === tool)?.label ?? "Tool";
  const canApplySelection = Boolean(boxSelection);

  return (
    <main className="relative min-h-0 min-w-0 flex-1 overflow-hidden bg-[oklch(0.17_0.014_260)]">
      <div
        ref={containerRef}
        className="absolute inset-0"
        onPointerDown={(event) => {
          if (event.button === 0) {
            pointerDown.current = { x: event.clientX, y: event.clientY };
          }
          if (event.button === 2) {
            rightDown.current = { x: event.clientX, y: event.clientY };
          }
        }}
        onPointerMove={(event) => {
          if (event.buttons !== 0) return;
          const target = event.currentTarget;
          const clientX = event.clientX;
          const clientY = event.clientY;
          if (hoverFrame.current !== null) cancelAnimationFrame(hoverFrame.current);
          hoverFrame.current = requestAnimationFrame(() => {
            hoverFrame.current = null;
            const renderer = rendererRef.current;
            if (!renderer || !target.isConnected) return;
            renderer.hoverAt(clientX, clientY);
          });
        }}
        onPointerLeave={() => {
          if (hoverFrame.current !== null) cancelAnimationFrame(hoverFrame.current);
          hoverFrame.current = null;
          useToolStore.getState().setHover(null, null);
          rendererRef.current?.setSelection(null);
        }}
        onPointerUp={(event) => {
          if (event.button !== 0) return;
          const start = pointerDown.current;
          pointerDown.current = null;
          if (!start) return;
          if (Math.hypot(event.clientX - start.x, event.clientY - start.y) > 5) return;
          const renderer = rendererRef.current;
          if (!renderer) return;
          const hit = renderer.pickAt(event.clientX, event.clientY);
          if (!hit && tool !== "fill" && tool !== "replace") return;
          if (event.altKey) {
            pickBlockAsBrush(hit);
            return;
          }
          if (!editingEnabled) return;
          void handleToolClick(hit);
        }}
        onDoubleClick={(event) => {
          if (tool !== "select") return;
          const renderer = rendererRef.current;
          if (!renderer) return;
          const hit = renderer.pickAt(event.clientX, event.clientY);
          if (hit) renderer.focusOn(hit.pos);
        }}
        onContextMenu={(event) => {
          event.preventDefault();
          const start = rightDown.current;
          rightDown.current = null;
          if (start && Math.hypot(event.clientX - start.x, event.clientY - start.y) > 5) {
            return;
          }
          const renderer = rendererRef.current;
          if (!renderer) return;
          const hit = renderer.pickAt(event.clientX, event.clientY);
          const rect = containerRef.current?.getBoundingClientRect() ?? null;
          if (!rect) return;
          setContextMenu({
            x: Math.max(4, Math.min(event.clientX - rect.left, rect.width - 232)),
            y: Math.max(4, Math.min(event.clientY - rect.top, rect.height - 340)),
            hit,
          });
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

      {webgl2 && tauri && !project?.folder && status === "ready" ? (
        <div className="absolute inset-0 z-10 grid place-items-center bg-black/35 p-6">
          <div className="w-full max-w-md space-y-3 rounded-lg border border-white/10 bg-card/95 p-5 text-center shadow-2xl">
            <div className="mx-auto grid size-12 place-items-center rounded-lg border border-primary/30 bg-primary/10">
              <FolderOpen className="size-6 text-primary" />
            </div>
            <div>
              <h2 className="text-sm font-semibold">No project open</h2>
              <p className="mt-1 text-[11px] leading-relaxed text-muted-foreground">
                Create an Empty Project or a flat base, or open an existing{" "}
                <span className="font-mono">.furybuilder</span> folder. Edits are saved atomically
                with checksums and autosave.
              </p>
            </div>
            <div className="flex justify-center gap-2">
              <Button size="sm" onClick={() => setManagerOpen(true)}>
                <Plus className="mr-1 size-3.5" /> New project
              </Button>
              <Button size="sm" variant="outline" onClick={() => setManagerOpen(true)}>
                <FolderOpen className="mr-1 size-3.5" /> Open project
              </Button>
            </div>
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

      <div className="absolute top-2 left-1/2 z-30 flex -translate-x-1/2 flex-col items-center gap-1.5">
        <div className="flex items-center gap-0.5 rounded-lg border border-white/10 bg-black/60 p-1 shadow-lg backdrop-blur-sm">
          {TOOL_ITEMS.map((item) => {
            const Icon = item.icon;
            const active = tool === item.tool;
            return (
              <Tooltip key={item.tool}>
                <TooltipTrigger asChild>
                  <Button
                    variant="ghost"
                    size="icon"
                    aria-label={item.label}
                    aria-pressed={active}
                    className={cn(
                      "size-7 rounded",
                      active
                        ? "bg-primary/25 text-primary hover:bg-primary/30"
                        : "text-white/65 hover:bg-white/10 hover:text-white",
                    )}
                    disabled={!editingEnabled}
                    onClick={() => useToolStore.getState().setTool(item.tool)}
                  >
                    <Icon />
                  </Button>
                </TooltipTrigger>
                <TooltipContent>
                  {item.label} · {item.shortcut}
                </TooltipContent>
              </Tooltip>
            );
          })}
          <Separator orientation="vertical" className="mx-0.5 h-5 bg-white/15" />
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                variant="ghost"
                size="icon"
                aria-label="Undo"
                className="size-7 rounded text-white/65 hover:bg-white/10 hover:text-white"
                disabled={!editingEnabled || Boolean(job) || !canUndo}
                onClick={() => void undoWorld()}
              >
                <Undo2 />
              </Button>
            </TooltipTrigger>
            <TooltipContent>Undo · Ctrl+Z</TooltipContent>
          </Tooltip>
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                variant="ghost"
                size="icon"
                aria-label="Redo"
                className="size-7 rounded text-white/65 hover:bg-white/10 hover:text-white"
                disabled={!editingEnabled || Boolean(job) || !canRedo}
                onClick={() => void redoWorld()}
              >
                <Redo2 />
              </Button>
            </TooltipTrigger>
            <TooltipContent>Redo · Ctrl+Shift+Z</TooltipContent>
          </Tooltip>
        </div>

        {editingEnabled ? (
          <div className="flex items-center gap-1.5 rounded-md border border-white/10 bg-black/55 px-2 py-1 font-mono text-[9px] text-white/55">
            <span className="text-white/80">{toolLabel}</span>
            {tool === "brush" ? (
              <label className="flex items-center gap-1">
                radius
                <select
                  aria-label="Brush radius"
                  value={brushRadius}
                  onChange={(event) =>
                    useToolStore.getState().setBrushRadius(Number(event.target.value))
                  }
                  className="rounded border border-white/15 bg-black/40 px-1 py-0.5"
                >
                  {Array.from(
                    { length: MAX_BRUSH_RADIUS - MIN_BRUSH_RADIUS + 1 },
                    (_, index) => MIN_BRUSH_RADIUS + index,
                  ).map((radius) => (
                    <option key={radius} value={radius}>
                      {radius} ({Math.pow(radius * 2 + 1, 3)} max)
                    </option>
                  ))}
                </select>
              </label>
            ) : null}
            <span className="text-primary/90">{selectedBlock.name.replace("minecraft:", "")}</span>
            {tool === "replace" ? (
              <span className="text-white/45">
                from {replaceFrom?.name.replace("minecraft:", "") ?? "—"}
              </span>
            ) : null}
            {boxSelection ? (
              <span className="text-sky-300">
                sel {boxSelection.min.x},{boxSelection.min.y},{boxSelection.min.z} →{" "}
                {boxSelection.max.x},{boxSelection.max.y},{boxSelection.max.z}
              </span>
            ) : null}
            {ghost ? <span className="text-white/40">{ghost.label}</span> : null}
          </div>
        ) : (
          <div className="rounded-md border border-status-warn/30 bg-black/60 px-2 py-1 font-mono text-[9px] text-status-warn">
            {tauri
              ? "No project open — create or open a project to edit blocks"
              : "Preview mode — block editing requires the desktop core"}
          </div>
        )}

        {job ? (
          <div className="flex items-center gap-2 rounded-md border border-primary/30 bg-black/70 px-2 py-1 font-mono text-[9px] text-white/75">
            <Loader2 className="size-3 animate-spin text-primary" />
            {job.label}
            {job.progress !== null ? ` · ${job.progress}%` : null}
            {job.cancelRequested ? " · cancelling…" : null}
            {job.cancellable && !job.cancelRequested ? (
              <button
                type="button"
                className="ml-1 rounded border border-white/20 px-1.5 py-0.5 text-[9px] text-white/70 hover:bg-white/10"
                onClick={() => useToolStore.getState().requestCancel()}
              >
                Cancel
              </button>
            ) : null}
          </div>
        ) : null}

        {editingEnabled && (tool === "fill" || tool === "replace") ? (
          <div className="flex items-center gap-1 rounded-md border border-white/10 bg-black/55 px-1.5 py-1">
            <Button
              size="sm"
              variant="ghost"
              className="h-6 px-2 text-[10px] text-white/75 hover:bg-white/10"
              disabled={!canApplySelection || Boolean(job)}
              onClick={() => void applyActiveTool()}
            >
              {tool === "fill" ? (
                <PaintBucket className="mr-1 size-3" />
              ) : (
                <Replace className="mr-1 size-3" />
              )}
              Apply {tool}
            </Button>
            <Button
              size="sm"
              variant="ghost"
              className="h-6 px-2 text-[10px] text-white/75 hover:bg-white/10"
              disabled={!canApplySelection || Boolean(job)}
              onClick={() => void deleteSelection()}
            >
              <Trash2 className="mr-1 size-3" /> Delete
            </Button>
          </div>
        ) : null}
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
            <span className="text-right">{metrics?.renderedSections ?? "-"}</span>
            <span>Mesh queue</span>
            <span className="text-right">{metrics?.pendingMeshJobs ?? "-"}</span>
            <span>Evicted</span>
            <span className="text-right">{metrics?.evictedChunks ?? "-"}</span>
            <span>Assets</span>
            <span
              className={cn(
                "text-right",
                assetError ? "text-status-warn" : assetStats ? "text-status-ok" : "text-white/40",
              )}
              title={assetError ?? undefined}
            >
              {assetStats
                ? `${assetStats.models} models · ${assetStats.textures} tex`
                : "placeholder"}
            </span>
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
            <span>Save</span>
            <span
              className={cn(
                "text-right",
                !project?.folder
                  ? "text-white/40"
                  : projectDirty
                    ? "text-status-warn"
                    : "text-status-ok",
              )}
            >
              {!project?.folder
                ? "no project"
                : projectDirty
                  ? autosaved
                    ? `autosaved r${project.autosavedRevision}`
                    : "unsaved"
                  : `saved r${project.savedRevision}`}
            </span>
            {lastOperation ? (
              <>
                <span>Last op</span>
                <span className="text-right">
                  {lastOperation.changedBlocks.toLocaleString()} blk · rev {lastOperation.revision}
                </span>
              </>
            ) : null}
          </div>
        ) : null}
      </div>

      <div className="pointer-events-none absolute right-2 bottom-2 z-20 max-w-[45%] rounded border border-white/10 bg-black/45 px-2.5 py-1 font-mono text-[9px] text-white/45">
        {editingEnabled
          ? "WASD fly · RMB look · 1-8 tools · Alt-click pick · Ctrl+Z undo · Ctrl+S save · RMB menu"
          : tauri
            ? "WASD fly · RMB look · open a project to edit · Ctrl+S save"
            : "WASD fly · RMB look · LMB pick · 1-8 select tool · Ctrl+Z undo"}
      </div>

      {boxSelection && editingEnabled ? (
        <div className="pointer-events-none absolute bottom-2 left-1/2 z-20 rounded border border-sky-400/40 bg-black/60 px-2 py-1 font-mono text-[10px] text-sky-200">
          box {boxSelection.max.x - boxSelection.min.x + 1}×
          {boxSelection.max.y - boxSelection.min.y + 1}×
          {boxSelection.max.z - boxSelection.min.z + 1} ·{" "}
          {(
            (boxSelection.max.x - boxSelection.min.x + 1) *
            (boxSelection.max.y - boxSelection.min.y + 1) *
            (boxSelection.max.z - boxSelection.min.z + 1)
          ).toLocaleString()}{" "}
          blocks
        </div>
      ) : null}

      {selection ? (
        <div className="pointer-events-none absolute bottom-10 left-1/2 z-20 max-w-[60%] -translate-x-1/2 truncate rounded border border-primary/40 bg-black/60 px-2 py-1 font-mono text-[10px] text-white/75">
          selected {selection.pos.x}, {selection.pos.y}, {selection.pos.z} ·{" "}
          {selection.state.name.replace("minecraft:", "")}
          {selection.state.properties && Object.keys(selection.state.properties).length > 0
            ? ` [${Object.entries(selection.state.properties)
                .map(([key, value]) => `${key}=${value}`)
                .join(",")}]`
            : ""}
        </div>
      ) : null}

      {contextMenu ? (
        <div
          className="absolute z-40 w-56 overflow-hidden rounded-md border border-border bg-card/95 text-[11px] shadow-2xl backdrop-blur"
          style={{ left: contextMenu.x, top: contextMenu.y }}
          role="menu"
        >
          <div className="border-b border-border px-2.5 py-1.5 font-mono text-[9px] text-muted-foreground">
            {contextMenu.hit
              ? `${contextMenu.hit.pos.x}, ${contextMenu.hit.pos.y}, ${contextMenu.hit.pos.z} · ${contextMenu.hit.state.name.replace("minecraft:", "")}`
              : "Empty space"}
          </div>
          <ContextItem
            icon={<Plus />}
            label="Place block"
            hint="2"
            disabled={!contextMenu.hit || !editingEnabled}
            onClick={menuPlace}
          />
          <ContextItem
            icon={<Eraser />}
            label="Remove block"
            hint="3"
            disabled={!contextMenu.hit || !editingEnabled}
            onClick={menuRemove}
          />
          <ContextItem
            icon={<Copy />}
            label="Pick block as brush"
            hint="Alt+click"
            disabled={!contextMenu.hit || !editingEnabled}
            onClick={menuPick}
          />
          <ContextItem
            icon={<Replace />}
            label="Set replace source"
            disabled={!contextMenu.hit || !editingEnabled}
            onClick={menuReplaceSource}
          />
          <ContextItem
            icon={<SquareDashed />}
            label="Set box corner"
            disabled={!contextMenu.hit || !editingEnabled}
            onClick={menuBoxCorner}
          />
          <div className="my-1 h-px bg-border" />
          <ContextItem
            icon={<Copy />}
            label="Copy selection"
            hint="Ctrl+C"
            disabled={!boxSelection || !editingEnabled}
            onClick={() => {
              closeContextMenu();
              void copySelection();
            }}
          />
          <ContextItem
            icon={<ClipboardPaste />}
            label={
              pendingPaste
                ? `Confirm paste (${pendingPaste.blocks})`
                : `Paste${clipboard ? ` (${clipboard.blocks.length})` : ""}`
            }
            hint="Ctrl+V"
            disabled={!clipboard || !editingEnabled}
            onClick={() => {
              closeContextMenu();
              void pasteClipboard();
            }}
          />
          <ContextItem
            icon={<PaintBucket />}
            label="Fill selection"
            hint="Enter"
            disabled={!boxSelection || !editingEnabled}
            onClick={() => {
              closeContextMenu();
              void applyActiveTool("fill");
            }}
          />
          <ContextItem
            icon={<Trash2 />}
            label="Delete selection"
            hint="Del"
            disabled={!boxSelection || !editingEnabled}
            onClick={() => {
              closeContextMenu();
              void deleteSelection();
            }}
          />
          <div className="my-1 h-px bg-border" />
          <ContextItem
            icon={<Scan />}
            label="Focus camera"
            disabled={!contextMenu.hit}
            onClick={menuFocus}
          />
          <ContextItem icon={<X />} label="Close menu" hint="Esc" onClick={closeContextMenu} />
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
    </main>
  );
}

function ContextItem({
  icon,
  label,
  hint,
  disabled,
  onClick,
}: {
  icon: ReactNode;
  label: string;
  hint?: string;
  disabled?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      role="menuitem"
      disabled={disabled}
      onClick={onClick}
      className={cn(
        "flex w-full items-center gap-2 px-2.5 py-1.5 text-left",
        disabled
          ? "cursor-not-allowed text-muted-foreground/50"
          : "hover:bg-accent hover:text-accent-foreground",
      )}
    >
      <span className="grid size-3.5 place-items-center [&>svg]:size-3.5">{icon}</span>
      <span className="flex-1">{label}</span>
      {hint ? <span className="font-mono text-[9px] text-muted-foreground">{hint}</span> : null}
    </button>
  );
}
