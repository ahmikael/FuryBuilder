import { useEffect, useRef, useState } from "react";
import {
  Boxes,
  Loader2,
  Package,
  Plus,
  RefreshCw,
  ShieldAlert,
  Trash2,
  Upload,
} from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { reloadViewport } from "@/features/viewport/viewport-bus";
import { isTauriRuntime, normalizeError } from "@/lib/ipc";
import { readStructureFile } from "@/lib/structure-api";
import {
  deleteTemplate,
  importTemplate,
  insertTemplate,
  listTemplates,
  previewTemplate,
  saveSelectionTemplate,
} from "@/lib/template-api";
import { formatBytes } from "@/project/types";
import type { TemplateEntry, TemplateInsertPreview } from "@/project/template-types";
import { useEditorStore } from "@/state/editor-store";
import { useHistoryStore } from "@/state/history-store";
import { pushToast } from "@/state/toast-store";
import { useToolStore } from "@/state/tool-store";
import { useWorldStore } from "@/state/world-store";
import { useWorkspaceStore } from "@/state/workspace-store";

function describeError(error: unknown): string {
  return normalizeError(error).message;
}

/**
 * Project-local reusable Java .nbt templates. Saving a selection and importing a file
 * both go through the same verified library; insertion is one reversible Batch operation
 * and is still subject to layers, masks and protected regions in the Core.
 */
export function TemplateLibraryPanel() {
  const selection = useToolStore((state) => state.selection);
  const dimension = useWorldStore((state) => state.dimension);
  const activeLayerId = useWorkspaceStore((state) => state.policy.activeLayerId);
  const [templates, setTemplates] = useState<TemplateEntry[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [name, setName] = useState("Template");
  const [includeAir, setIncludeAir] = useState(false);
  const [preview, setPreview] = useState<{
    result: TemplateInsertPreview;
    key: string;
  } | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const fileInput = useRef<HTMLInputElement>(null);
  const editing = isTauriRuntime();
  const selected = templates.find((entry) => entry.id === selectedId) ?? null;
  const previewKey =
    selected && selection
      ? `${selected.id}:${dimension}:${selection.min.x},${selection.min.y},${selection.min.z}:${includeAir}`
      : null;
  const activePreview = preview && preview.key === previewKey ? preview.result : null;

  const refresh = async (selectId?: string) => {
    if (!editing) return;
    setLoading(true);
    try {
      const entries = await listTemplates();
      setTemplates(entries);
      setSelectedId((current) => selectId ?? current ?? entries[0]?.id ?? null);
      setError(null);
    } catch (cause) {
      setError(describeError(cause));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    if (!editing) return;
    let cancelled = false;
    listTemplates()
      .then((entries) => {
        if (cancelled) return;
        setTemplates(entries);
        setSelectedId((current) => current ?? entries[0]?.id ?? null);
        setError(null);
      })
      .catch((cause: unknown) => {
        if (!cancelled) setError(describeError(cause));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [editing]);

  if (!editing) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-2 text-center text-sm text-muted-foreground">
        <Boxes className="size-5 text-muted-foreground/60" />
        <p className="max-w-md leading-relaxed">
          Save, import and place templates in the desktop app.
        </p>
      </div>
    );
  }

  const saveSelection = async () => {
    if (!selection) {
      setError("Select a box in the viewport first.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const entry = await saveSelectionTemplate(name, dimension, selection);
      await refresh(entry.id);
      setName("Template");
      pushToast({
        kind: "success",
        title: "Template saved",
        description: `${entry.name} · ${entry.nonAirBlocks.toLocaleString()} non-air block(s)`,
      });
    } catch (cause) {
      setError(describeError(cause));
    } finally {
      setBusy(false);
    }
  };

  const importFile = async (file: File | undefined) => {
    if (!file) return;
    setBusy(true);
    setError(null);
    try {
      const payload = await readStructureFile(file);
      const entry = await importTemplate(name.trim() || file.name.replace(/\.nbt$/i, ""), payload);
      await refresh(entry.id);
      pushToast({
        kind: "success",
        title: "Template imported",
        description: `${entry.name} · ${entry.nonAirBlocks.toLocaleString()} non-air block(s)`,
      });
    } catch (cause) {
      setError(describeError(cause));
    } finally {
      setBusy(false);
      if (fileInput.current) fileInput.current.value = "";
    }
  };

  const previewInsertion = async () => {
    if (!selected || !selection) {
      setError("Select a template and a target box; insertion starts at the box minimum.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const result = await previewTemplate({
        templateId: selected.id,
        dimension,
        origin: [selection.min.x, selection.min.y, selection.min.z],
        includeAir,
      });
      if (previewKey) setPreview({ result, key: previewKey });
    } catch (cause) {
      setError(describeError(cause));
    } finally {
      setBusy(false);
    }
  };

  const applyInsertion = async () => {
    if (!selected || !activePreview || !selection) return;
    setBusy(true);
    setError(null);
    try {
      const outcome = await insertTemplate({
        templateId: selected.id,
        dimension,
        origin: [selection.min.x, selection.min.y, selection.min.z],
        includeAir,
        layerId: activeLayerId,
      });
      useEditorStore.getState().setSelectedDimension(dimension);
      useWorldStore.getState().applySummary({
        source: "core",
        dimension,
        revision: outcome.info.revision,
        blockCount: outcome.info.blockCount,
        sectionCount: outcome.info.sectionCount,
        chunkCount: outcome.info.chunkCount,
        dimensions: outcome.info.dimensions,
      });
      useToolStore.getState().markModified();
      useToolStore.getState().setSelection(null);
      void useHistoryStore.getState().refresh();
      reloadViewport();
      setPreview(null);
      pushToast({
        kind: "success",
        title: "Template inserted",
        description: outcome.operation
          ? `${outcome.operation.changedBlocks.toLocaleString()} block(s) changed · rev ${outcome.operation.worldRevision}`
          : "The template contained no writable positions.",
      });
    } catch (cause) {
      setError(describeError(cause));
    } finally {
      setBusy(false);
    }
  };

  const removeSelected = async () => {
    if (!selected) return;
    setBusy(true);
    setError(null);
    try {
      await deleteTemplate(selected.id);
      setSelectedId(null);
      await refresh();
      pushToast({ kind: "info", title: `Deleted template "${selected.name}"` });
    } catch (cause) {
      setError(describeError(cause));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex min-h-0 flex-1 gap-4 text-sm" data-testid="template-library">
      <div className="flex min-h-0 w-80 shrink-0 flex-col gap-3 rounded-xl border bg-card p-4">
        <div className="flex items-center gap-2">
          <Package className="size-4 text-muted-foreground" />
          <span className="text-base font-semibold">Library</span>
          <span className="text-xs text-muted-foreground">{templates.length} / 128</span>
          <Button
            size="sm"
            variant="ghost"
            className="ml-auto"
            aria-label="Refresh templates"
            disabled={loading}
            onClick={() => void refresh()}
          >
            {loading ? (
              <Loader2 className="size-3 animate-spin" />
            ) : (
              <RefreshCw className="size-3" />
            )}
          </Button>
        </div>
        <div className="scrollbar-thin min-h-0 flex-1 space-y-1 overflow-y-auto">
          {templates.length === 0 ? (
            <p className="p-1 text-sm text-muted-foreground">
              No templates yet. Save the current selection or import a Java .nbt file.
            </p>
          ) : (
            templates.map((entry) => (
              <button
                key={entry.id}
                type="button"
                onClick={() => setSelectedId(entry.id)}
                className={
                  entry.id === selectedId
                    ? "w-full rounded-lg border border-primary/60 bg-primary/10 px-3 py-2.5 text-left"
                    : "w-full rounded-lg border bg-card px-3 py-2.5 text-left hover:bg-accent"
                }
              >
                <div className="truncate text-sm">{entry.name}</div>
                <div className="truncate font-mono text-xs text-muted-foreground">
                  {entry.size[0]}×{entry.size[1]}×{entry.size[2]} ·{" "}
                  {entry.nonAirBlocks.toLocaleString()} blocks · {formatBytes(entry.bytes)}
                </div>
              </button>
            ))
          )}
        </div>
        <div className="flex gap-2">
          <Input
            value={name}
            onChange={(event) => setName(event.target.value)}
            aria-label="Template name"
          />
          <Button
            size="sm"
            variant="outline"
            disabled={busy || !selection}
            title={selection ? "Save the current selection" : "Select a box first"}
            onClick={() => void saveSelection()}
          >
            <Plus className="size-3" /> Save selection
          </Button>
        </div>
        <input
          ref={fileInput}
          type="file"
          accept=".nbt"
          className="hidden"
          onChange={(event) => void importFile(event.target.files?.[0])}
        />
        <Button
          size="sm"
          variant="outline"
          disabled={busy}
          onClick={() => fileInput.current?.click()}
        >
          <Upload className="size-3" /> Import .nbt
        </Button>
      </div>

      <div className="scrollbar-thin flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto rounded-xl border bg-card p-4">
        {selected ? (
          <>
            <div className="flex items-center gap-2">
              <Boxes className="size-4 text-muted-foreground" />
              <span className="text-base font-semibold">{selected.name}</span>
              <Badge variant="secondary" className="ml-auto font-mono">
                {selected.source}
              </Badge>
            </div>
            <div className="grid grid-cols-2 gap-x-4 gap-y-2 font-mono text-xs text-muted-foreground">
              <span>size: {selected.size.join(" × ")}</span>
              <span>non-air: {selected.nonAirBlocks.toLocaleString()}</span>
              <span>palette: {selected.paletteSize}</span>
              <span>bytes: {formatBytes(selected.bytes)}</span>
              <span className="col-span-2 truncate">sha256: {selected.sha256}</span>
            </div>
            <label className="flex items-center gap-2 text-sm">
              <Checkbox
                checked={includeAir}
                onCheckedChange={(checked) => setIncludeAir(checked === true)}
              />
              Include explicit air (clears the target volume)
            </label>
            <div className="flex flex-wrap gap-2">
              <Button
                size="sm"
                variant="outline"
                disabled={busy || !selection}
                onClick={() => void previewInsertion()}
              >
                Preview at selection min
              </Button>
              <Button
                size="sm"
                disabled={busy || !activePreview || !selection}
                title={activePreview ? "Apply this template" : "Preview first"}
                onClick={() => void applyInsertion()}
              >
                Insert template
              </Button>
              <Button
                size="sm"
                variant="outline"
                disabled={busy}
                onClick={() => void removeSelected()}
              >
                <Trash2 className="size-3" /> Delete
              </Button>
            </div>
            {activePreview ? (
              <div
                className="rounded border border-border bg-background/60 p-2 text-xs"
                data-testid="template-preview"
              >
                <div>
                  {activePreview.changedBlocks.toLocaleString()} block(s) would change out of{" "}
                  {activePreview.affectedBlocks.toLocaleString()} selected.
                </div>
                <div className="font-mono text-xs text-muted-foreground">
                  bounds ({activePreview.bounds.min.x},{activePreview.bounds.min.y},
                  {activePreview.bounds.min.z}) ..({activePreview.bounds.max.x},
                  {activePreview.bounds.max.y},{activePreview.bounds.max.z})
                </div>
                {activePreview.warnings.map((warning) => (
                  <div key={warning} className="mt-1 flex gap-2 text-status-warn">
                    <ShieldAlert className="mt-0.5 size-3 shrink-0" />
                    <span>{warning}</span>
                  </div>
                ))}
              </div>
            ) : (
              <p className="text-sm text-muted-foreground">
                Preview first; insertion starts at the selection minimum and is one reversible
                operation on the active layer.
              </p>
            )}
          </>
        ) : (
          <div className="flex h-full flex-col items-center justify-center gap-2 text-center text-sm text-muted-foreground">
            <Boxes className="size-5 text-muted-foreground/50" />
            <p>Select a template to preview or insert it.</p>
          </div>
        )}
        {error ? <p className="text-xs text-status-error">{error}</p> : null}
        {busy ? (
          <div className="flex items-center gap-1 text-xs text-muted-foreground">
            <Loader2 className="size-3 animate-spin" /> Working…
          </div>
        ) : null}
      </div>
    </div>
  );
}
