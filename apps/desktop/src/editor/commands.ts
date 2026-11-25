import type { BlockChange, BoxBounds, EditorOperation, EditorTool } from "@/editor/types";
import { clipboardPasteBounds, LARGE_PASTE_THRESHOLD, planRegionTiles } from "@/editor/clipboard";
import {
  AIR_STATE,
  MAX_EDIT_VOLUME,
  adjacentPosition,
  boundsFromPositions,
  boundsVolume,
  linePoints,
  normalizeBounds,
  operationLabel,
  pasteChanges,
  planBlockTool,
  TOOL_LABELS,
} from "@/editor/session";
import { normalizeError, isTauriRuntime } from "@/lib/ipc";
import {
  applyOperation,
  cancelWorldJob,
  fetchWorldInfo,
  newIdempotencyKey,
  previewOperation,
  readRegion,
  redoOperation,
  startWorldJob,
  undoOperation,
  worldJobStatus,
  type WorldJobSnapshot,
} from "@/lib/world-api";
import type { BlockState, ChunkPos } from "@/renderer/types";
import type { RayHit } from "@/renderer/raycast";
import { useHistoryStore } from "@/state/history-store";
import { projectAllowsEditor, useProjectStore } from "@/state/project-store";
import { pushToast } from "@/state/toast-store";
import { isJobCancelled, useToolStore } from "@/state/tool-store";
import { useWorldStore } from "@/state/world-store";
import { refreshViewportChunks, reloadViewport } from "@/features/viewport/viewport-bus";

function requireCore(): boolean {
  if (!isTauriRuntime()) {
    pushToast({
      kind: "warning",
      title: "Desktop core required",
      description:
        "Editing writes through the Rust world core. Run the desktop app (pnpm tauri dev) to edit blocks.",
    });
    return false;
  }
  const project = useProjectStore.getState();
  if (!projectAllowsEditor(project.phase, project.current)) {
    pushToast({
      kind: "warning",
      title: "No project ready",
      description: "Create or open a project before editing blocks.",
    });
    return false;
  }
  return true;
}

function reportError(error: unknown, title = "Edit failed"): void {
  const backendError = normalizeError(error);
  pushToast({
    kind: "error",
    title,
    description: `${backendError.message}${backendError.code !== "unknown" ? ` [${backendError.code}]` : ""}`,
  });
}

async function refreshWorldSummary(): Promise<void> {
  try {
    const info = await fetchWorldInfo();
    useWorldStore.getState().applySummary({
      revision: info.revision,
      blockCount: info.blockCount,
      sectionCount: info.sectionCount,
      chunkCount: info.chunkCount,
      dimensions: info.dimensions,
    });
  } catch {
    // The write already succeeded; a stale summary is preferable to a false error toast.
  }
}

function afterWrite(): void {
  void refreshWorldSummary();
  void useHistoryStore.getState().refresh();
  void useProjectStore.getState().refreshSession();
}

async function reloadAfterConflict(): Promise<void> {
  reloadViewport();
  afterWrite();
}

export interface ExternalWorldChange {
  dirtyChunks?: ChunkPos[];
  reload?: boolean;
}

/**
 * Synchronizes the editor after a write that bypassed the UI command path (the MCP bridge
 * or a completed MCP job). The Rust core is already authoritative; this refreshes the
 * summary, history, session marker and the affected Three.js chunks. A job snapshot without
 * dirty chunks falls back to a bounded full reload instead of showing a stale frame.
 */
export function applyExternalWorldChange(change: ExternalWorldChange): void {
  const project = useProjectStore.getState();
  if (!projectAllowsEditor(project.phase, project.current)) return;
  if (change.reload || !change.dirtyChunks || change.dirtyChunks.length === 0) {
    reloadViewport();
  } else {
    refreshViewportChunks(change.dirtyChunks);
  }
  afterWrite();
}

export interface ApplyOptions {
  label?: string;
  silentSuccess?: boolean;
  selectionAfter?: BoxBounds | null;
}

/**
 * Full write path: server-validated preview → atomic apply → revisioned summary → dirty
 * chunk refresh → history refresh. The preview step is cancellable; a dispatched write is
 * atomic and is never reported as cancelled mid-flight.
 */
export async function applyEditorOperation(
  operation: EditorOperation,
  options: ApplyOptions = {},
): Promise<{ revision: number; changedBlocks: number } | null> {
  if (!requireCore()) return null;
  const toolState = useToolStore.getState();
  if (toolState.job) {
    pushToast({
      kind: "info",
      title: "Another operation is running",
      description: "Wait for it to finish or cancel the preview first.",
    });
    return null;
  }

  const label = options.label ?? operationLabel(operation);
  let preview;
  const previewJob = useToolStore.getState().beginJob(`Validating ${label.toLowerCase()}`, true);
  try {
    preview = await previewOperation(operation);
  } catch (error) {
    useToolStore.getState().finishJob(previewJob);
    reportError(error, "Operation rejected");
    return null;
  }
  if (isJobCancelled(previewJob)) {
    useToolStore.getState().finishJob(previewJob);
    pushToast({ kind: "info", title: "Preview cancelled", description: "Nothing was written." });
    return null;
  }
  useToolStore.getState().finishJob(previewJob);
  if (preview.warnings.length > 0) {
    pushToast({
      kind: "warning",
      title: "Operation warning",
      description: preview.warnings.join(" "),
    });
  }

  const applyJob = useToolStore.getState().beginJob(`Applying ${label.toLowerCase()}`);
  try {
    const result = await applyOperation(operation, useWorldStore.getState().revision, undefined);
    useToolStore.getState().finishJob(applyJob);

    useWorldStore.getState().applySummary({
      revision: result.worldRevision,
      source: "core",
    });
    if (options.selectionAfter !== undefined) {
      useToolStore.getState().setSelection(options.selectionAfter);
    }

    if (result.changedBlocks === 0) {
      pushToast({
        kind: "info",
        title: "No blocks changed",
        description: "The target already contained the requested state.",
      });
      afterWrite();
      return { revision: result.worldRevision, changedBlocks: 0 };
    }

    useToolStore.getState().setLastOperation({
      operationId: result.operationId,
      changedBlocks: result.changedBlocks,
      revision: result.worldRevision,
      warnings: result.warnings,
    });
    if (!options.silentSuccess) {
      pushToast({
        kind: "success",
        title: `${label} applied`,
        description: `${result.changedBlocks.toLocaleString()} block(s) changed · revision ${result.worldRevision}`,
      });
    }
    refreshViewportChunks(result.dirtyChunks);
    afterWrite();
    return { revision: result.worldRevision, changedBlocks: result.changedBlocks };
  } catch (error) {
    useToolStore.getState().finishJob(applyJob);
    const backendError = normalizeError(error);
    if (backendError.code === "revision_mismatch") {
      pushToast({
        kind: "warning",
        title: "World changed underneath",
        description: "Reloading the loaded area from the Rust core. Repeat the edit if needed.",
      });
      await reloadAfterConflict();
    } else {
      reportError(error);
    }
    return null;
  }
}

function toolContextFor(hit: RayHit | null, overrideTool?: EditorTool) {
  const state = useToolStore.getState();
  return {
    tool: overrideTool ?? state.tool,
    dimension: useWorldStore.getState().dimension,
    hit,
    anchor: state.anchor,
    selection: state.selection,
    brushRadius: state.brushRadius,
    selected: state.selectedBlock,
    replaceFrom: state.replaceFrom,
  };
}

/**
 * Main world click. Box selection is a two-click state machine; lines use two clicks;
 * other tools resolve to one atomic operation.
 */
export async function handleToolClick(hit: RayHit | null): Promise<void> {
  if (!requireCore()) return;
  const state = useToolStore.getState();
  if (state.tool === "select") return;
  if (!hit && state.tool !== "fill" && state.tool !== "replace") {
    if (state.tool === "box" && state.anchor) {
      pushToast({ kind: "info", title: "Aim at a loaded block to close the box." });
    }
    return;
  }

  if (state.tool === "box") {
    if (!hit) return;
    if (!state.anchor) {
      state.setAnchor(hit.pos);
      state.setGhost(null);
      refreshGhost();
      pushToast({
        kind: "info",
        title: "Box corner set",
        description: "Click the opposite corner to finish the selection.",
      });
      return;
    }
    const bounds = normalizeBounds(state.anchor, hit.pos);
    const volume = boundsVolume(bounds);
    if (volume > MAX_EDIT_VOLUME) {
      pushToast({
        kind: "error",
        title: "Selection too large",
        description: `${volume.toLocaleString()} blocks exceeds the ${MAX_EDIT_VOLUME.toLocaleString()} block limit.`,
      });
      return;
    }
    state.setSelection(bounds);
    state.setAnchor(null);
    state.setGhost(null);
    pushToast({
      kind: "success",
      title: `Selected ${volume.toLocaleString()} blocks`,
      description: `${bounds.min.x},${bounds.min.y},${bounds.min.z} → ${bounds.max.x},${bounds.max.y},${bounds.max.z}`,
    });
    return;
  }

  if (state.tool === "line" && !state.anchor) {
    if (!hit) return;
    const start = adjacentPosition(hit.pos, hit.face);
    state.setAnchor(start);
    refreshGhost();
    pushToast({
      kind: "info",
      title: "Line start set",
      description: "Click the end point to place the line.",
    });
    return;
  }

  if (state.tool === "fill" || state.tool === "replace") {
    await applyActiveTool();
    return;
  }

  const operation = planBlockTool(toolContextFor(hit));
  if (!operation) {
    if (state.tool === "line" && state.anchor) state.setAnchor(null);
    return;
  }
  const isLine = state.tool === "line";
  await applyEditorOperation(operation, isLine ? {} : { selectionAfter: state.selection });
  if (isLine) state.setAnchor(null);
  state.setGhost(null);
}

/** Applies fill/replace to the current selection using the active tool. */
export async function applyActiveTool(tool?: EditorTool): Promise<void> {
  if (!requireCore()) return;
  const state = useToolStore.getState();
  const active = tool ?? state.tool;
  if (active !== "fill" && active !== "replace") {
    pushToast({
      kind: "info",
      title: "Nothing to apply",
      description: "Choose Fill or Replace, then apply it to a selection.",
    });
    return;
  }
  if (!state.selection) {
    pushToast({
      kind: "warning",
      title: "Select a box first",
      description: "Use the Box select tool (5) to set the region, then fill or replace it.",
    });
    return;
  }
  if (active === "replace") {
    const source = state.replaceFrom ?? useWorldStore.getState().selection?.state ?? null;
    if (!source) {
      pushToast({
        kind: "warning",
        title: "No replace source",
        description: "Alt-click a world block or choose a palette block as the source.",
      });
      return;
    }
    if (source.name === state.selectedBlock.name) {
      pushToast({
        kind: "warning",
        title: "Source equals target",
        description: "Pick a different palette block before replacing.",
      });
      return;
    }
    state.setReplaceFrom(source);
  }
  const operation = planBlockTool(toolContextFor(null, active));
  if (!operation) {
    pushToast({ kind: "warning", title: "The current selection cannot be applied." });
    return;
  }
  await applyEditorOperation(operation, { selectionAfter: state.selection });
}

export async function deleteSelection(): Promise<void> {
  if (!requireCore()) return;
  const state = useToolStore.getState();
  if (!state.selection) {
    pushToast({ kind: "info", title: "Select a box first." });
    return;
  }
  await applyEditorOperation(
    {
      type: "fill_cuboid",
      dimension: useWorldStore.getState().dimension,
      bounds: state.selection,
      state: AIR_STATE,
    },
    { label: "Delete selection", selectionAfter: state.selection },
  );
}

export function pickBlockAsBrush(hit: RayHit | null): void {
  if (!hit) return;
  useToolStore.getState().setSelectedBlock(hit.state);
  pushToast({
    kind: "info",
    title: "Brush block updated",
    description: hit.state.name,
  });
}

export function setReplaceSource(hit: RayHit | null): void {
  if (!hit) return;
  useToolStore.getState().setReplaceFrom(hit.state);
  pushToast({
    kind: "info",
    title: "Replace source set",
    description: hit.state.name,
  });
}

function samePosition(
  a: { x: number; y: number; z: number },
  b: { x: number; y: number; z: number },
): boolean {
  return a.x === b.x && a.y === b.y && a.z === b.z;
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

export async function copySelection(): Promise<void> {
  if (!requireCore()) return;
  const state = useToolStore.getState();
  const selection = state.selection;
  if (!selection) {
    pushToast({ kind: "warning", title: "Select a box to copy." });
    return;
  }
  if (state.job) {
    pushToast({ kind: "info", title: "Another operation is running." });
    return;
  }
  const volume = boundsVolume(selection);
  if (volume <= 0 || volume > MAX_EDIT_VOLUME) {
    pushToast({
      kind: "error",
      title: "Selection too large to copy",
      description: `${volume.toLocaleString()} blocks exceeds the ${MAX_EDIT_VOLUME.toLocaleString()} block editor limit.`,
    });
    return;
  }

  const dimension = useWorldStore.getState().dimension;
  const tiles = planRegionTiles(selection);
  const job = state.beginJob(`Copying ${volume.toLocaleString()} blocks`, true, 0);
  const blocks: BlockChange[] = [];
  try {
    for (let index = 0; index < tiles.length; index += 1) {
      if (isJobCancelled(job)) {
        useToolStore.getState().finishJob(job);
        pushToast({ kind: "info", title: "Copy cancelled", description: "Nothing was copied." });
        return;
      }
      const region = await readRegion(dimension, tiles[index]!);
      for (const change of region.blocks) {
        blocks.push({
          pos: {
            x: change.pos.x - selection.min.x,
            y: change.pos.y - selection.min.y,
            z: change.pos.z - selection.min.z,
          },
          state: change.state,
        });
      }
      useToolStore.getState().setJobProgress(job, Math.round(((index + 1) / tiles.length) * 100));
    }
    if (isJobCancelled(job)) {
      useToolStore.getState().finishJob(job);
      pushToast({ kind: "info", title: "Copy cancelled", description: "Nothing was copied." });
      return;
    }

    const size: [number, number, number] = [
      selection.max.x - selection.min.x + 1,
      selection.max.y - selection.min.y + 1,
      selection.max.z - selection.min.z + 1,
    ];
    useToolStore.getState().setClipboard({ dimension, size, blocks });
    useToolStore.getState().finishJob(job);
    pushToast({
      kind: "success",
      title: `Copied ${blocks.length.toLocaleString()} blocks`,
      description: `${size[0]}×${size[1]}×${size[2]} structure from ${dimension}${
        tiles.length > 1 ? ` in ${tiles.length} bounded tiles` : ""
      }`,
    });
  } catch (error) {
    useToolStore.getState().finishJob(job);
    reportError(error, "Copy failed");
  }
}

/**
 * Large pastes use the Rust Job API: preview, then a queued atomic batch whose progress
 * comes from the operation observer. Cancelling the job rolls back before any write, so
 * the editor never reports a partially pasted structure as cancelled.
 */
async function applyLargePaste(changes: BlockChange[], bounds: BoxBounds): Promise<void> {
  if (!requireCore()) return;
  const state = useToolStore.getState();
  if (state.job) {
    pushToast({ kind: "info", title: "Another operation is running." });
    return;
  }

  const dimension = useWorldStore.getState().dimension;
  const operation: EditorOperation = { type: "batch", dimension, changes };
  const count = changes.length;
  const job = state.beginJob(`Validating paste of ${count.toLocaleString()} blocks`, true, 0);
  let preview;
  try {
    preview = await previewOperation(operation);
  } catch (error) {
    useToolStore.getState().finishJob(job);
    reportError(error, "Paste preview failed");
    return;
  }
  if (isJobCancelled(job)) {
    useToolStore.getState().finishJob(job);
    pushToast({ kind: "info", title: "Paste cancelled", description: "Nothing was written." });
    return;
  }
  if (preview.changedBlocks === 0) {
    useToolStore.getState().finishJob(job);
    pushToast({
      kind: "info",
      title: "No blocks changed",
      description: "The target already contained the pasted structure.",
    });
    return;
  }
  if (preview.warnings.length > 0) {
    pushToast({
      kind: "warning",
      title: "Paste warning",
      description: preview.warnings.join(" "),
    });
  }
  useToolStore.getState().setJobProgress(job, 5);

  let snapshot: WorldJobSnapshot;
  try {
    snapshot = await startWorldJob(
      "apply_operation",
      { operation },
      useWorldStore.getState().revision,
      newIdempotencyKey(),
    );
  } catch (error) {
    useToolStore.getState().finishJob(job);
    reportError(error, "Large paste was not started");
    return;
  }
  useToolStore.getState().setJobProgress(job, snapshot.progress);

  let cancelSent = false;
  while (snapshot.state === "queued" || snapshot.state === "running") {
    if (!cancelSent && isJobCancelled(job)) {
      cancelSent = true;
      try {
        await cancelWorldJob(snapshot.jobId);
      } catch {
        // The next status poll reports the authoritative terminal state.
      }
    }
    await delay(200);
    try {
      snapshot = await worldJobStatus(snapshot.jobId);
    } catch (error) {
      useToolStore.getState().finishJob(job);
      reportError(error, "Could not read the paste job");
      return;
    }
    useToolStore.getState().setJobProgress(job, snapshot.progress);
  }

  useToolStore.getState().finishJob(job);
  const result = snapshot.result;
  if (snapshot.state === "succeeded" && result) {
    useWorldStore.getState().applySummary({
      revision: result.worldRevision,
      source: "core",
    });
    useToolStore.getState().setSelection(bounds);
    if (result.changedBlocks === 0) {
      pushToast({
        kind: "info",
        title: "No blocks changed",
        description: "The target already contained the pasted structure.",
      });
      afterWrite();
      return;
    }
    useToolStore.getState().setLastOperation({
      operationId: result.operationId,
      changedBlocks: result.changedBlocks,
      revision: result.worldRevision,
      warnings: result.warnings,
    });
    refreshViewportChunks(result.dirtyChunks);
    afterWrite();
    pushToast({
      kind: "success",
      title: "Large paste applied",
      description: `${result.changedBlocks.toLocaleString()} block(s) changed · revision ${result.worldRevision}`,
    });
    return;
  }
  if (snapshot.state === "cancelled") {
    pushToast({
      kind: "info",
      title: "Paste cancelled",
      description: "No changes were committed; the job rolled back before writing.",
    });
    return;
  }
  const backendError = normalizeError(snapshot.error ?? snapshot.message);
  pushToast({
    kind: "error",
    title: "Large paste failed",
    description: `${backendError.message}${backendError.code !== "unknown" ? ` [${backendError.code}]` : ""}`,
  });
}

export async function pasteClipboard(): Promise<void> {
  const state = useToolStore.getState();
  const clipboard = state.clipboard;
  if (!clipboard) {
    pushToast({ kind: "info", title: "Clipboard is empty", description: "Copy a box first." });
    return;
  }
  if (state.job) {
    pushToast({ kind: "info", title: "Another operation is running." });
    return;
  }
  const target =
    state.selection?.min ??
    (state.hover ? adjacentPosition(state.hover.pos, state.hover.face) : null);
  if (!target) {
    pushToast({
      kind: "warning",
      title: "No paste position",
      description: "Aim at a block or keep a box selection to choose the paste corner.",
    });
    return;
  }
  const changes = pasteChanges(clipboard, target);
  if (changes.length === 0) {
    pushToast({
      kind: "info",
      title: "Clipboard has no blocks",
      description: "Air is implied, so an all-air selection copies nothing.",
    });
    return;
  }

  const bounds = clipboardPasteBounds(clipboard, target);
  const pasteVolume = boundsVolume(bounds);
  if (pasteVolume <= 0 || pasteVolume > MAX_EDIT_VOLUME) {
    pushToast({
      kind: "error",
      title: "Paste too large",
      description: `${pasteVolume.toLocaleString()} blocks exceeds the ${MAX_EDIT_VOLUME.toLocaleString()} block editor limit.`,
    });
    return;
  }
  if (changes.length >= LARGE_PASTE_THRESHOLD) {
    if (!requireCore()) return;
    const pending = state.pendingPaste;
    if (!pending || !samePosition(pending.target, target)) {
      useToolStore.getState().setPendingPaste({ target, bounds, blocks: changes.length });
      refreshGhost();
      pushToast({
        kind: "info",
        title: "Large paste preview",
        description: `${changes.length.toLocaleString()} blocks into (${bounds.min.x},${bounds.min.y},${bounds.min.z}) → (${bounds.max.x},${bounds.max.y},${bounds.max.z}). Press Ctrl+V again or click Paste to apply; Esc cancels.`,
      });
      return;
    }
    useToolStore.getState().setPendingPaste(null);
    useToolStore.getState().setGhost(null);
    await applyLargePaste(changes, bounds);
    return;
  }

  useToolStore.getState().setPendingPaste(null);
  await applyEditorOperation(
    {
      type: "batch",
      dimension: useWorldStore.getState().dimension,
      changes,
    },
    { label: "Paste structure", selectionAfter: bounds },
  );
}

export async function undoWorld(): Promise<void> {
  if (!requireCore()) return;
  const state = useToolStore.getState();
  if (state.job) {
    pushToast({ kind: "info", title: "Another operation is running." });
    return;
  }
  const job = state.beginJob("Undoing");
  try {
    const result = await undoOperation();
    useToolStore.getState().finishJob(job);
    useWorldStore.getState().applySummary({ revision: result.worldRevision, source: "core" });
    useToolStore.getState().setLastOperation({
      operationId: result.operationId,
      changedBlocks: result.changedBlocks,
      revision: result.worldRevision,
      warnings: result.warnings,
    });
    refreshViewportChunks(result.dirtyChunks);
    afterWrite();
    pushToast({
      kind: "success",
      title: "Undo applied",
      description: `${result.changedBlocks.toLocaleString()} block(s) restored · revision ${result.worldRevision}`,
    });
  } catch (error) {
    useToolStore.getState().finishJob(job);
    reportError(error, "Undo failed");
  }
}

export async function redoWorld(): Promise<void> {
  if (!requireCore()) return;
  const state = useToolStore.getState();
  if (state.job) {
    pushToast({ kind: "info", title: "Another operation is running." });
    return;
  }
  const job = state.beginJob("Redoing");
  try {
    const result = await redoOperation();
    useToolStore.getState().finishJob(job);
    useWorldStore.getState().applySummary({ revision: result.worldRevision, source: "core" });
    useToolStore.getState().setLastOperation({
      operationId: result.operationId,
      changedBlocks: result.changedBlocks,
      revision: result.worldRevision,
      warnings: result.warnings,
    });
    refreshViewportChunks(result.dirtyChunks);
    afterWrite();
    pushToast({
      kind: "success",
      title: "Redo applied",
      description: `${result.changedBlocks.toLocaleString()} block(s) changed · revision ${result.worldRevision}`,
    });
  } catch (error) {
    useToolStore.getState().finishJob(job);
    reportError(error, "Redo failed");
  }
}

export function cancelInteraction(): void {
  const state = useToolStore.getState();
  if (state.job?.cancellable) {
    state.requestCancel();
    return;
  }
  if (state.pendingPaste) {
    state.setPendingPaste(null);
    state.setGhost(null);
    pushToast({ kind: "info", title: "Paste preview cancelled" });
    return;
  }
  if (state.anchor) {
    state.setAnchor(null);
    state.setGhost(null);
    pushToast({ kind: "info", title: "Pending click cancelled" });
    return;
  }
  if (state.selection) {
    state.setSelection(null);
    state.setGhost(null);
    pushToast({ kind: "info", title: "Selection cleared" });
    return;
  }
  state.setGhost(null);
}

export function describeActiveTool(): string {
  const state = useToolStore.getState();
  if (state.anchor) return `${TOOL_LABELS[state.tool]} — second click to finish`;
  if (state.selection)
    return `${TOOL_LABELS[state.tool]} — selection ${boundsVolume(state.selection).toLocaleString()} blocks`;
  return TOOL_LABELS[state.tool];
}

/** Rebuilds the local ghost after hover/anchor/selection changes. */
export function refreshGhost(): void {
  const state = useToolStore.getState();
  if (state.pendingPaste) {
    state.setGhost({
      bounds: state.pendingPaste.bounds,
      kind: "paste",
      blocks: state.pendingPaste.blocks,
      label: `Paste preview · ${state.pendingPaste.blocks.toLocaleString()} blocks`,
    });
    return;
  }
  const hit = state.hover;
  const selection = state.selection;
  const tool = state.tool;
  if (tool === "fill" || tool === "replace") {
    if (!selection) {
      state.setGhost(null);
      return;
    }
    const blocks = boundsVolume(selection);
    state.setGhost({
      bounds: selection,
      kind: tool,
      blocks,
      label: `${tool === "fill" ? "Fill" : "Replace"} ${blocks.toLocaleString()} blocks`,
    });
    return;
  }
  if (!hit) {
    state.setGhost(null);
    return;
  }
  if (tool === "box" && state.anchor) {
    const bounds = normalizeBounds(state.anchor, hit.pos);
    const blocks = boundsVolume(bounds);
    state.setGhost({
      bounds,
      kind: "box",
      blocks,
      label: `Select ${blocks.toLocaleString()} blocks`,
    });
    return;
  }
  if (tool === "line" && state.anchor) {
    const end = adjacentPosition(hit.pos, hit.face);
    const points = linePoints(state.anchor, end);
    const bounds = boundsFromPositions(points);
    if (bounds) {
      state.setGhost({
        bounds,
        kind: "line",
        blocks: points.length,
        label: `Line ${points.length} blocks`,
      });
    }
    return;
  }
  if (tool === "place" || tool === "remove") {
    const target = tool === "remove" ? hit.pos : adjacentPosition(hit.pos, hit.face);
    state.setGhost({
      bounds: { min: target, max: target },
      kind: tool,
      blocks: 1,
      label: tool === "remove" ? "Remove block" : "Place block",
    });
    return;
  }
  state.setGhost(null);
}

export function useBrushBlock(state: BlockState): void {
  useToolStore.getState().setSelectedBlock(state);
}
