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
  Grid3X3,
  ImageOff,
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
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

import { invokeCommand, isTauriRuntime } from "@/lib/ipc";
import { listenForRendererRequests } from "@/lib/renderer-bridge";
import { loadProjectAssetBundle } from "@/lib/assets-api";
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
  resolveDimension,
  useWorldStore,
} from "@/state/world-store";
import { useToolStore } from "@/state/tool-store";
import { useEditorStore } from "@/state/editor-store";
import { projectAllowsEditor, useProjectStore } from "@/state/project-store";
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

const BRUSH_RADIUS_ITEMS = Array.from(
  { length: MAX_BRUSH_RADIUS - MIN_BRUSH_RADIUS + 1 },
  (_, index) => {
    const radius = MIN_BRUSH_RADIUS + index;
    return {
      value: String(radius),
      label: `${radius} (${Math.pow(radius * 2 + 1, 3)} max)`,
    };
  },
);

const VIEW_DISTANCE_ITEMS = Array.from(
  { length: MAX_LOAD_RADIUS - MIN_LOAD_RADIUS + 1 },
  (_, index) => {
    const radius = MIN_LOAD_RADIUS + index;
    return {
      value: String(radius),
      label: `${radius * 2 + 1}×${radius * 2 + 1}`,
    };
  },
);
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

/** World and render commands are only valid for a project the editor may mount. */
function worldCallsAllowed(): boolean {
  const state = useProjectStore.getState();
  return projectAllowsEditor(state.phase, state.current);
}

async function loadPreview(renderer: ViewportRenderer): Promise<void> {
  if (!worldCallsAllowed()) return;
  const snapshot = createDemoSnapshot(useWorldStore.getState().loadRadius);
  const summary = previewSummary(snapshot);
  const loadedChunks = snapshot.chunks.filter((chunk) => chunk.loaded).length;
  renderer.setSnapshot(snapshot);
  useWorldStore.getState().applySummary({
    source: "preview",
    dimension: snapshot.dimension,
    revision: snapshot.revision,
    blockCount: summary.blockCount,
    sectionCount: summary.sectionCount,
    chunkCount: loadedChunks,
    dimensions: [
      {
        id: snapshot.dimension,
        kind: "overworld",
        minY: snapshot.minY,
        maxY: snapshot.minY + snapshot.height - 1,
        height: snapshot.height,
        chunkCount: loadedChunks,
        sectionCount: summary.sectionCount,
        blockCount: summary.blockCount,
      },
    ],
  });
}

async function loadCore(renderer: ViewportRenderer, radius: number): Promise<void> {
  if (!worldCallsAllowed()) return;
  const info = await invokeCommand<WorldInfo>("world_info");
  const requested = useWorldStore.getState().dimension;
  const dimension = resolveDimension(requested, info.dimensions);
  if (dimension !== requested) {
    useWorldStore.getState().setDimension(dimension);
    useEditorStore.getState().setSelectedDimension(dimension);
  }
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
    dimensions: info.dimensions,
  });
}

async function initialLoad(renderer: ViewportRenderer): Promise<void> {
  const store = useWorldStore.getState();
  if (!worldCallsAllowed()) {
    store.setStatus("idle");
    return;
  }
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
  if (!worldCallsAllowed()) return;
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
  if (chunks.length === 0 || !worldCallsAllowed()) return;
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
  const [captureImage, setCaptureImage] = useState<string | null>(null);
  const [goto, setGoto] = useState({ x: "0", y: "70", z: "0" });
  const [contextMenu, setContextMenu] = useState<ContextMenuState | null>(null);
  const [assetStats, setAssetStats] = useState<AssetStats | null>(null);
  const [assetError, setAssetError] = useState<string | null>(null);
  const [packIssues, setPackIssues] = useState<string[]>([]);
  const [placeholderActive, setPlaceholderActive] = useState<boolean | null>(null);
  const [sceneEpoch, setSceneEpoch] = useState(0);
  const assetRequest = useRef({ signature: "", busy: false, pending: false, epoch: 0 });
  const tauri = isTauriRuntime();
  const project = useProjectStore((state) => state.current);
  const folder = project?.folder;
  const phase = useProjectStore((state) => state.phase);
  const canRender = projectAllowsEditor(phase, project);
  const editingEnabled = tauri && canRender;

  const status = useWorldStore((state) => state.status);
  const error = useWorldStore((state) => state.error);
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

  const refreshAssets = useCallback(async function refresh(force = false): Promise<void> {
    const request = assetRequest.current;
    if (force) {
      request.epoch += 1;
      request.signature = "";
    }
    const renderer = rendererRef.current;
    const folder = useProjectStore.getState().current?.folder;
    if (!renderer || !folder || !isTauriRuntime() || useWorldStore.getState().status !== "ready")
      return;
    if (request.busy) {
      request.pending = true;
      return;
    }
    const states = renderer.world.uniqueBlockStates();
    if (states.length === 0) return;
    const signature = JSON.stringify([folder, ...states.map(blockStateKey).sort()]);
    if (signature === request.signature) return;
    const epoch = request.epoch;
    const isCurrent = () =>
      request.epoch === epoch &&
      rendererRef.current === renderer &&
      useProjectStore.getState().current?.folder === folder;
    request.busy = true;
    try {
      const bundle = await loadProjectAssetBundle(
        folder,
        states.map((state) => ({
          name: state.name,
          properties: state.properties ?? {},
        })),
      );
      if (!isCurrent()) return;
      if (bundle === null) {
        renderer.clearAssetBundle();
      } else {
        await renderer.setAssetBundle(bundle, isCurrent);
      }
      if (!isCurrent()) return;
      request.signature = signature;
      setAssetStats(renderer.assetStats);
      setPackIssues(bundle?.packIssues ?? []);
      setAssetError(null);
      setPlaceholderActive(bundle === null);
    } catch (cause) {
      if (!isCurrent()) return;
      request.signature = signature;
      renderer.clearAssetBundle();
      setAssetStats(null);
      setPackIssues([]);
      setPlaceholderActive(true);
      setAssetError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      request.busy = false;
      if (request.pending) {
        request.pending = false;
        void refresh();
      }
    }
  }, []);

  useEffect(() => {
    const container = containerRef.current;
    if (!container || !webgl2 || !canRender) return;
    assetRequest.current.signature = "";
    setAssetStats(null);
    setPackIssues([]);
    setAssetError(null);
    setPlaceholderActive(null);

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
  }, [webgl2, tauri, canRender, folder]);

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
  }, [tauri, status, revision, sceneEpoch, folder, refreshAssets]);

  useEffect(() => {
    if (!tauri) return;
    let disposed = false;
    const stops: Array<() => void> = [];
    const track = (subscription: Promise<() => void>) => {
      void subscription
        .then((stop) => {
          if (disposed) stop();
          else stops.push(stop);
        })
        .catch(() => {
          // The event API is only present inside Tauri; the placeholder stays usable.
        });
    };
    track(
      listen("furybuilder-assets-changed", () => {
        if (!disposed) void refreshAssets(true);
      }),
    );
    track(
      listen<string>("furybuilder-project-packs-changed", ({ payload }) => {
        if (!disposed && payload === useProjectStore.getState().current?.folder) {
          void refreshAssets(true);
        }
      }),
    );
    return () => {
      disposed = true;
      for (const stop of stops) stop();
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

  if (!canRender) return null;

  return (
    <main className="relative min-h-0 min-w-0 flex-1 overflow-hidden bg-background">
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
        <div className="absolute inset-0 z-10 flex flex-col items-center justify-center gap-3 bg-background text-center">
          <div className="grid size-16 place-items-center rounded-xl border bg-card">
            <Box className="size-8 text-primary/80" strokeWidth={1.2} />
          </div>
          <h1 className="text-sm font-semibold">3D viewport</h1>
          <p className="max-w-sm text-sm leading-relaxed text-muted-foreground">
            WebGL2 is unavailable in this WebView. Rendering is disabled, but world data and the
            command API still work.
          </p>
        </div>
      ) : null}

      {webgl2 && status === "loading" ? (
        <div className="pointer-events-none absolute inset-0 z-10 grid place-items-center bg-background/40">
          <div className="flex items-center gap-2 rounded-lg border bg-card/95 px-3 py-2 text-xs text-muted-foreground shadow-sm">
            <Loader2 className="size-3.5 animate-spin" /> Loading chunk snapshots…
          </div>
        </div>
      ) : null}

      {webgl2 && status === "error" ? (
        <div className="absolute inset-0 z-10 grid place-items-center bg-background/70 p-6">
          <div className="max-w-md space-y-2 rounded-xl border border-status-error/40 bg-card p-4 text-center shadow-lg">
            <TriangleAlert className="mx-auto size-6 text-status-error" />
            <h2 className="text-sm font-semibold">Viewport unavailable</h2>
            <p className="text-sm leading-relaxed text-muted-foreground">
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

      <div className="pointer-events-none absolute top-3 left-3 z-20 flex flex-wrap items-center gap-2">
        <Badge variant="secondary" className="font-mono">
          {dimension === "minecraft:overworld"
            ? "Overworld"
            : dimension.replace("minecraft:", "").replace(/_/g, " ")}
          {" · "}
          {positionLabel}
        </Badge>
        {placeholderActive ? (
          <Badge
            variant="outline"
            className="gap-1"
            title={
              assetError ??
              "Original FuryBuilder placeholder textures; Minecraft block data is unchanged."
            }
          >
            <ImageOff />
            Placeholder textures
          </Badge>
        ) : null}
        {packIssues.length > 0 ? (
          <Badge variant="outline" className="pointer-events-auto" title={packIssues.join("\n")}>
            <TriangleAlert /> {packIssues.length} resource{" "}
            {packIssues.length === 1 ? "pack" : "packs"} skipped
          </Badge>
        ) : null}
      </div>

      <div className="absolute top-3 left-1/2 z-30 flex -translate-x-1/2 flex-col items-center gap-2">
        <div className="flex items-center gap-1 rounded-xl border bg-background/85 p-1 shadow-lg backdrop-blur-md">
          {TOOL_ITEMS.map((item) => {
            const Icon = item.icon;
            const active = tool === item.tool;
            return (
              <Tooltip key={item.tool}>
                <TooltipTrigger
                  render={
                    <Button
                      variant={active ? "secondary" : "ghost"}
                      size="icon"
                      aria-label={item.label}
                      aria-pressed={active}
                      className={cn(active && "bg-accent text-accent-foreground")}
                      disabled={!editingEnabled}
                      onClick={() => useToolStore.getState().setTool(item.tool)}
                    />
                  }
                >
                  <Icon />
                </TooltipTrigger>
                <TooltipContent>
                  {item.label} · {item.shortcut}
                </TooltipContent>
              </Tooltip>
            );
          })}
          <Separator orientation="vertical" className="mx-1 !h-6" />
          <Tooltip>
            <TooltipTrigger
              render={
                <Button
                  variant="ghost"
                  size="icon"
                  aria-label="Undo"
                  disabled={!editingEnabled || Boolean(job) || !canUndo}
                  onClick={() => void undoWorld()}
                />
              }
            >
              <Undo2 />
            </TooltipTrigger>
            <TooltipContent>Undo · Ctrl+Z</TooltipContent>
          </Tooltip>
          <Tooltip>
            <TooltipTrigger
              render={
                <Button
                  variant="ghost"
                  size="icon"
                  aria-label="Redo"
                  disabled={!editingEnabled || Boolean(job) || !canRedo}
                  onClick={() => void redoWorld()}
                />
              }
            >
              <Redo2 />
            </TooltipTrigger>
            <TooltipContent>Redo · Ctrl+Shift+Z</TooltipContent>
          </Tooltip>
        </div>

        {editingEnabled ? (
          <div className="flex items-center gap-2 rounded-lg border bg-background/85 px-3 py-1.5 font-mono text-xs text-muted-foreground shadow-sm backdrop-blur-md">
            <span className="font-sans font-medium text-foreground">{toolLabel}</span>
            {tool === "brush" ? (
              <label className="flex items-center gap-1.5">
                radius
                <Select
                  items={BRUSH_RADIUS_ITEMS}
                  value={String(brushRadius)}
                  onValueChange={(value) => {
                    if (value) useToolStore.getState().setBrushRadius(Number(value));
                  }}
                >
                  <SelectTrigger size="sm" aria-label="Brush radius" className="min-w-36">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {BRUSH_RADIUS_ITEMS.map((item) => (
                      <SelectItem key={item.value} value={item.value}>
                        {item.label}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </label>
            ) : null}
            <span className="text-primary">{selectedBlock.name.replace("minecraft:", "")}</span>
            {tool === "replace" ? (
              <span>from {replaceFrom?.name.replace("minecraft:", "") ?? "—"}</span>
            ) : null}
            {boxSelection ? (
              <span className="text-primary">
                sel {boxSelection.min.x},{boxSelection.min.y},{boxSelection.min.z} →{" "}
                {boxSelection.max.x},{boxSelection.max.y},{boxSelection.max.z}
              </span>
            ) : null}
            {ghost ? <span className="text-muted-foreground/70">{ghost.label}</span> : null}
          </div>
        ) : (
          <div className="rounded-lg border border-status-warn/30 bg-background/85 px-3 py-1.5 font-mono text-xs text-status-warn backdrop-blur-md">
            Preview only · open the desktop app to edit
          </div>
        )}

        {job ? (
          <div className="flex items-center gap-2 rounded-lg border border-primary/30 bg-background/85 px-3 py-1.5 font-mono text-xs shadow-sm backdrop-blur-md">
            <Loader2 className="size-3.5 animate-spin text-primary" />
            {job.label}
            {job.progress !== null ? ` · ${job.progress}%` : null}
            {job.cancelRequested ? " · cancelling…" : null}
            {job.cancellable && !job.cancelRequested ? (
              <Button
                size="xs"
                variant="outline"
                className="ml-1"
                onClick={() => useToolStore.getState().requestCancel()}
              >
                Cancel
              </Button>
            ) : null}
          </div>
        ) : null}

        {editingEnabled && (tool === "fill" || tool === "replace") ? (
          <div className="flex items-center gap-1.5 rounded-lg border bg-background/85 p-1 shadow-sm backdrop-blur-md">
            <Button
              size="sm"
              variant="secondary"
              disabled={!canApplySelection || Boolean(job)}
              onClick={() => void applyActiveTool()}
            >
              {tool === "fill" ? (
                <PaintBucket data-icon="inline-start" />
              ) : (
                <Replace data-icon="inline-start" />
              )}
              Apply {tool}
            </Button>
            <Button
              size="sm"
              variant="outline"
              disabled={!canApplySelection || Boolean(job)}
              onClick={() => void deleteSelection()}
            >
              <Trash2 data-icon="inline-start" />
              Delete
            </Button>
          </div>
        ) : null}
      </div>

      <div className="absolute top-3 right-3 z-20 flex items-center gap-1 rounded-xl border bg-background/85 p-1 shadow-lg backdrop-blur-md">
        <Tooltip>
          <TooltipTrigger
            render={
              <Button
                variant="ghost"
                size="icon"
                aria-label="Fly camera"
                onClick={() => setCameraMode("fly")}
              />
            }
          >
            <Plane />
          </TooltipTrigger>
          <TooltipContent>Fly camera — WASD, Space/Ctrl, right-drag to look.</TooltipContent>
        </Tooltip>
        <Tooltip>
          <TooltipTrigger
            render={
              <Button
                variant="ghost"
                size="icon"
                aria-label="Orbit camera"
                onClick={() => setCameraMode("orbit")}
              />
            }
          >
            <Orbit />
          </TooltipTrigger>
          <TooltipContent>Orbit camera — drag to rotate, wheel to zoom.</TooltipContent>
        </Tooltip>
        <Separator orientation="vertical" className="mx-1 !h-6" />
        <Tooltip>
          <TooltipTrigger
            render={
              <Button
                variant="ghost"
                size="icon"
                aria-label="Toggle grid"
                className={cn(gridVisible && "bg-accent text-accent-foreground")}
                onClick={toggleGrid}
              />
            }
          >
            <Grid3X3 />
          </TooltipTrigger>
          <TooltipContent>Toggle the ground grid.</TooltipContent>
        </Tooltip>
        <Tooltip>
          <TooltipTrigger
            render={
              <Button
                variant="ghost"
                size="icon"
                aria-label="Toggle chunk borders"
                className={cn(chunkBordersVisible && "bg-accent text-accent-foreground")}
                onClick={toggleBorders}
              />
            }
          >
            <Waypoints />
          </TooltipTrigger>
          <TooltipContent>Toggle chunk borders.</TooltipContent>
        </Tooltip>
        <Tooltip>
          <TooltipTrigger
            render={
              <Button
                variant="ghost"
                size="icon"
                aria-label="Go to coordinates"
                onClick={openGoto}
              />
            }
          >
            <MousePointer2 />
          </TooltipTrigger>
          <TooltipContent>Fly the camera to exact coordinates.</TooltipContent>
        </Tooltip>
        <Tooltip>
          <TooltipTrigger
            render={
              <Button
                variant="ghost"
                size="icon"
                aria-label="Capture viewport"
                onClick={doCapture}
              />
            }
          >
            <Camera />
          </TooltipTrigger>
          <TooltipContent>Capture the viewport with observation metadata.</TooltipContent>
        </Tooltip>
      </div>

      <Collapsible className="absolute bottom-3 left-3 z-20 w-64 overflow-hidden rounded-xl border bg-background/90 text-muted-foreground shadow-lg backdrop-blur-md">
        <CollapsibleTrigger className="flex w-full cursor-pointer items-center gap-2 px-3 py-2 text-left text-xs font-medium [&[data-panel-open]>svg]:rotate-90">
          <ChevronRight className="size-3.5 transition-transform" />
          Performance
          <span className="ml-auto font-mono text-xs text-foreground">
            {metrics ? `${metrics.fps} fps` : "—"}
          </span>
        </CollapsibleTrigger>
        <CollapsibleContent>
          <div className="grid grid-cols-[1fr_auto] gap-x-3 gap-y-1 border-t px-3 py-2 font-mono text-xs">
            <span>Frame</span>
            <span className="text-right text-foreground">
              {metrics ? `${metrics.frameMs} ms` : "—"}
            </span>
            <span>Draw calls</span>
            <span className="text-right text-foreground">{metrics?.drawCalls ?? "—"}</span>
            <span>Triangles</span>
            <span className="text-right text-foreground">
              {metrics?.triangles.toLocaleString() ?? "—"}
            </span>
            <span>Meshes</span>
            <span className="text-right text-foreground">{metrics?.meshCount ?? "—"}</span>
            <span>Geometries</span>
            <span className="text-right text-foreground">{metrics?.geometries ?? "—"}</span>
            <span>Textures</span>
            <span className="text-right text-foreground">{metrics?.textures ?? "—"}</span>
            <span>Chunks</span>
            <span className="text-right text-foreground">{metrics?.loadedChunks ?? "—"}</span>
            <span>Sections</span>
            <span className="text-right text-foreground">{metrics?.renderedSections ?? "-"}</span>
            <span>Mesh queue</span>
            <span className="text-right text-foreground">{metrics?.pendingMeshJobs ?? "-"}</span>
            <span>Evicted</span>
            <span className="text-right text-foreground">{metrics?.evictedChunks ?? "-"}</span>
            <span>Assets</span>
            <span
              className={cn(
                "text-right",
                assetError
                  ? "text-status-warn"
                  : assetStats
                    ? "text-status-ok"
                    : "text-muted-foreground",
              )}
              title={assetError ?? undefined}
            >
              {assetStats
                ? `${assetStats.models} models · ${assetStats.textures} tex`
                : "placeholder"}
            </span>
            <span className="self-center">View distance</span>
            <Select
              items={VIEW_DISTANCE_ITEMS}
              value={String(loadRadius)}
              onValueChange={(value) => {
                if (value) handleRadius(Number(value));
              }}
            >
              <SelectTrigger size="sm" aria-label="View distance" className="h-7 min-w-28">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {VIEW_DISTANCE_ITEMS.map((item) => (
                  <SelectItem key={item.value} value={item.value}>
                    {item.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            {lastOperation ? (
              <>
                <span>Last op</span>
                <span className="text-right text-foreground">
                  {lastOperation.changedBlocks.toLocaleString()} blk · rev {lastOperation.revision}
                </span>
              </>
            ) : null}
          </div>
        </CollapsibleContent>
      </Collapsible>

      <div className="pointer-events-none absolute right-3 bottom-3 z-20 max-w-[45%] rounded-lg border bg-background/85 px-3 py-1.5 font-mono text-xs text-muted-foreground backdrop-blur-md">
        {editingEnabled
          ? "WASD move · RMB look · 1–8 tools · Alt-click pick"
          : "Preview · WASD move · RMB look"}
      </div>

      {boxSelection && editingEnabled ? (
        <div className="pointer-events-none absolute bottom-3 left-1/2 z-20 rounded-lg border border-primary/40 bg-background/90 px-3 py-1.5 font-mono text-xs text-foreground shadow-sm backdrop-blur-md">
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
        <div className="pointer-events-none absolute bottom-12 left-1/2 z-20 max-w-[60%] -translate-x-1/2 truncate rounded-lg border border-primary/40 bg-background/90 px-3 py-1.5 font-mono text-xs text-foreground shadow-sm backdrop-blur-md">
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
          className="absolute z-40 w-64 overflow-hidden rounded-xl border bg-popover text-sm text-popover-foreground shadow-xl"
          style={{ left: contextMenu.x, top: contextMenu.y }}
          role="menu"
        >
          <div className="border-b px-3 py-2 font-mono text-xs text-muted-foreground">
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
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Go to coordinates</DialogTitle>
            <DialogDescription>Fly the camera near these block coordinates.</DialogDescription>
          </DialogHeader>
          <form onSubmit={submitGoto} className="space-y-3">
            <div className="grid grid-cols-3 gap-2">
              {(["x", "y", "z"] as const).map((axis) => (
                <label key={axis} className="space-y-1 text-xs text-muted-foreground">
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
        <DialogContent className="max-h-[90vh] max-w-4xl gap-6 overflow-y-auto">
          <DialogHeader>
            <DialogTitle>Viewport capture</DialogTitle>
            <DialogDescription>
              Observation metadata — revision {lastCapture?.worldRevision ?? "—"}, camera and
              completeness are recorded with the image.
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-2">
            <div className="overflow-hidden rounded-lg border bg-muted/40">
              <img
                src={captureImage ?? ""}
                alt="Captured viewport"
                className="max-h-64 w-full object-contain"
              />
            </div>
            {lastCapture ? (
              <div className="grid grid-cols-2 gap-x-4 gap-y-1 rounded border border-border bg-background/40 p-2 font-mono text-xs">
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
              <Collapsible className="rounded border border-border bg-background/40 p-2">
                <CollapsibleTrigger className="flex w-full cursor-pointer items-center justify-between gap-2 text-left text-xs font-medium text-muted-foreground [&[data-panel-open]>svg]:rotate-180">
                  <span>
                    Legend and full metadata ({lastCapture.legend.length} states
                    {lastCapture.legendTruncated ? ", truncated" : ""})
                  </span>
                  <ChevronDown className="size-3.5 shrink-0 transition-transform" />
                </CollapsibleTrigger>
                <CollapsibleContent>
                  <div className="mt-2 grid max-h-32 grid-cols-2 gap-x-4 gap-y-0.5 overflow-y-auto font-mono text-xs">
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
                  <pre className="scrollbar-thin mt-2 max-h-32 overflow-auto rounded bg-background/60 p-2 font-mono text-xs leading-relaxed text-muted-foreground">
                    {JSON.stringify(lastCapture, null, 2)}
                  </pre>
                </CollapsibleContent>
              </Collapsible>
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
        "flex w-full items-center gap-2.5 px-3 py-2 text-left",
        disabled
          ? "cursor-not-allowed text-muted-foreground/50"
          : "hover:bg-accent hover:text-accent-foreground",
      )}
    >
      <span className="grid size-4 place-items-center [&>svg]:size-4">{icon}</span>
      <span className="flex-1">{label}</span>
      {hint ? <span className="font-mono text-xs text-muted-foreground">{hint}</span> : null}
    </button>
  );
}
