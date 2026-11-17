import { useEffect, useMemo, useState } from "react";
import {
  AlertTriangle,
  Blocks,
  History,
  Loader2,
  Redo2,
  RotateCcw,
  Search,
  Terminal,
  Undo2,
  Wrench,
  X,
} from "lucide-react";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import type { BackendState } from "@/hooks/use-backend";
import type { BlockDefinition } from "@/editor/types";
import { TOOL_LABELS, TOOL_SHORTCUTS } from "@/editor/session";
import { applyActiveTool, deleteSelection, redoWorld, undoWorld } from "@/editor/commands";
import { isTauriRuntime } from "@/lib/ipc";
import { fetchBlockCatalog } from "@/lib/world-api";
import { blockColor } from "@/renderer/assets";
import type { EditorTool } from "@/editor/types";
import { useHistoryStore } from "@/state/history-store";
import { useToolStore } from "@/state/tool-store";
import { useWorldStore } from "@/state/world-store";
import { cn } from "@/lib/utils";

const TOOLS: EditorTool[] = [
  "select",
  "place",
  "remove",
  "brush",
  "box",
  "fill",
  "line",
  "replace",
];

function PalettePanel() {
  const [catalog, setCatalog] = useState<BlockDefinition[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [search, setSearch] = useState("");
  const selectedBlock = useToolStore((state) => state.selectedBlock);
  const setSelectedBlock = useToolStore((state) => state.setSelectedBlock);
  const setReplaceFrom = useToolStore((state) => state.setReplaceFrom);
  const replaceFrom = useToolStore((state) => state.replaceFrom);

  useEffect(() => {
    if (!isTauriRuntime()) return;
    let cancelled = false;
    fetchBlockCatalog()
      .then((definitions) => {
        if (!cancelled) setCatalog(definitions);
      })
      .catch((reason: unknown) => {
        if (!cancelled) setError(reason instanceof Error ? reason.message : String(reason));
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const filtered = useMemo(() => {
    if (!catalog) return [];
    const needle = search.trim().toLowerCase();
    if (!needle) return catalog;
    return catalog.filter((entry) => entry.name.toLowerCase().includes(needle));
  }, [catalog, search]);

  const definition = catalog?.find((entry) => entry.name === selectedBlock.name) ?? null;
  const properties = Object.entries(selectedBlock.properties ?? {});

  if (!isTauriRuntime()) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-1.5 text-center text-[11px] text-muted-foreground">
        <Blocks className="size-5 text-muted-foreground/60" />
        <p className="max-w-md leading-relaxed">
          The block catalog is served by the Rust core (curated 1.21.4 states). Browser preview mode
          can inspect the demo scene, but palette selection and world edits need the desktop app.
        </p>
      </div>
    );
  }

  if (error) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-2 text-[11px] text-muted-foreground">
        <AlertTriangle className="size-5 text-status-warn" />
        <p>Block catalog unavailable: {error}</p>
        <Button
          size="sm"
          variant="outline"
          onClick={() => {
            setError(null);
            fetchBlockCatalog()
              .then(setCatalog)
              .catch((reason: unknown) =>
                setError(reason instanceof Error ? reason.message : String(reason)),
              );
          }}
        >
          Retry
        </Button>
      </div>
    );
  }

  return (
    <div className="flex min-h-0 flex-1 gap-2">
      <div className="flex min-h-0 w-72 flex-col gap-1.5">
        <div className="relative">
          <Search className="absolute top-1/2 left-2 size-3 -translate-y-1/2 text-muted-foreground" />
          <Input
            className="h-6 pl-7"
            placeholder={catalog ? `Search ${catalog.length} blocks…` : "Loading catalog…"}
            value={search}
            disabled={!catalog}
            onChange={(event) => setSearch(event.target.value)}
            aria-label="Search blocks"
          />
        </div>
        <div className="scrollbar-thin flex min-h-0 flex-1 flex-wrap content-start gap-1 overflow-y-auto">
          {!catalog ? (
            <div className="flex items-center gap-1.5 p-1 text-[10px] text-muted-foreground">
              <Loader2 className="size-3 animate-spin" /> Reading block registry…
            </div>
          ) : filtered.length === 0 ? (
            <div className="p-1 text-[10px] text-muted-foreground">
              No curated block matches “{search}”.
            </div>
          ) : (
            filtered.map((entry) => {
              const active = entry.name === selectedBlock.name;
              return (
                <button
                  key={entry.name}
                  type="button"
                  title={entry.name}
                  aria-pressed={active}
                  onClick={() =>
                    setSelectedBlock({
                      name: entry.name,
                      properties: { ...entry.properties },
                    })
                  }
                  className={cn(
                    "flex items-center gap-1.5 rounded border px-1.5 py-1",
                    active
                      ? "border-primary/60 bg-primary/15 text-foreground"
                      : "border-border bg-background/40 text-muted-foreground hover:border-border hover:bg-accent/40",
                  )}
                >
                  <span
                    className="size-3.5 rounded-[2px] border border-black/30"
                    style={{ backgroundColor: blockColor(entry.name) }}
                  />
                  <span className="font-mono text-[9px]">
                    {entry.name.replace("minecraft:", "")}
                  </span>
                </button>
              );
            })
          )}
        </div>
      </div>

      <div className="scrollbar-thin flex min-h-0 w-64 shrink-0 flex-col overflow-y-auto rounded border border-border bg-background/40 p-2">
        <div className="flex items-center gap-2">
          <span
            className="size-6 rounded border border-black/30"
            style={{ backgroundColor: blockColor(selectedBlock.name) }}
          />
          <div className="min-w-0">
            <div className="truncate font-mono text-[10px]">{selectedBlock.name}</div>
            <div className="text-[9px] text-muted-foreground">
              {definition ? "Curated state" : "Custom state"} · brush/paste target
            </div>
          </div>
        </div>

        <div className="mt-2 space-y-1">
          {properties.length === 0 ? (
            <div className="text-[10px] text-muted-foreground">No state properties.</div>
          ) : (
            properties.map(([key, value]) => (
              <label key={key} className="grid grid-cols-[1fr_1.4fr] items-center gap-1.5">
                <span className="truncate font-mono text-[9px] text-muted-foreground">{key}</span>
                <Input
                  className="h-5 px-1 font-mono text-[9px]"
                  value={value}
                  aria-label={`Property ${key}`}
                  onChange={(event) =>
                    setSelectedBlock({
                      ...selectedBlock,
                      properties: {
                        ...selectedBlock.properties,
                        [key]: event.target.value,
                      },
                    })
                  }
                />
              </label>
            ))
          )}
        </div>

        <div className="mt-2 flex flex-wrap gap-1">
          <Button
            size="sm"
            variant="outline"
            className="h-6 px-2 text-[10px]"
            disabled={!definition}
            onClick={() => {
              if (definition) {
                setSelectedBlock({
                  name: definition.name,
                  properties: { ...definition.properties },
                });
              }
            }}
          >
            <RotateCcw className="mr-1 size-3" /> Defaults
          </Button>
          <Button
            size="sm"
            variant="outline"
            className="h-6 px-2 text-[10px]"
            onClick={() => setReplaceFrom(selectedBlock)}
          >
            Use as replace source
          </Button>
        </div>

        <div className="mt-2 rounded border border-border bg-card/40 p-1.5 text-[9px] leading-relaxed text-muted-foreground">
          Replace source:{" "}
          <span className="font-mono text-foreground/80">
            {replaceFrom?.name.replace("minecraft:", "") ?? "not set"}
          </span>
          {replaceFrom ? (
            <button
              type="button"
              aria-label="Clear replace source"
              className="ml-1 rounded p-0.5 hover:bg-accent"
              onClick={() => setReplaceFrom(null)}
            >
              <X className="inline size-2.5" />
            </button>
          ) : null}
          <div className="mt-1">
            Unknown property values are rejected by the Rust registry, never silently coerced.
          </div>
        </div>
      </div>
    </div>
  );
}

function ToolsPanel() {
  const tool = useToolStore((state) => state.tool);
  const setTool = useToolStore((state) => state.setTool);
  const brushRadius = useToolStore((state) => state.brushRadius);
  const setBrushRadius = useToolStore((state) => state.setBrushRadius);
  const selection = useToolStore((state) => state.selection);
  const setSelection = useToolStore((state) => state.setSelection);
  const lastOperation = useToolStore((state) => state.lastOperation);
  const job = useToolStore((state) => state.job);
  const editing = isTauriRuntime();

  return (
    <div className="flex gap-3">
      <div className="flex max-w-sm flex-wrap content-start gap-1">
        {TOOLS.map((entry) => (
          <Button
            key={entry}
            size="sm"
            variant={tool === entry ? "default" : "outline"}
            className="h-6 px-2 text-[10px]"
            disabled={!editing}
            onClick={() => setTool(entry)}
          >
            {TOOL_LABELS[entry]}
            <span className="ml-1 font-mono text-[8px] opacity-60">
              {Object.entries(TOOL_SHORTCUTS)
                .find(([, value]) => value === entry)?.[0]
                ?.replace("Digit", "")}
            </span>
          </Button>
        ))}
      </div>

      <div className="w-64 space-y-1 rounded border border-border bg-background/40 p-2 text-[10px]">
        <label className="flex items-center justify-between gap-2">
          <span className="text-muted-foreground">Brush radius</span>
          <input
            type="range"
            min={1}
            max={4}
            step={1}
            value={brushRadius}
            disabled={!editing}
            aria-label="Brush radius slider"
            onChange={(event) => setBrushRadius(Number(event.target.value))}
          />
          <span className="font-mono">{brushRadius}</span>
        </label>
        <div className="flex items-center justify-between gap-2">
          <span className="text-muted-foreground">Selection</span>
          <span className="font-mono">
            {selection
              ? `${selection.max.x - selection.min.x + 1}×${selection.max.y - selection.min.y + 1}×${selection.max.z - selection.min.z + 1}`
              : "—"}
          </span>
        </div>
        <div className="flex flex-wrap gap-1 pt-1">
          <Button
            size="sm"
            className="h-6 px-2 text-[10px]"
            disabled={!editing || !selection || Boolean(job)}
            onClick={() => void applyActiveTool("fill")}
          >
            Fill
          </Button>
          <Button
            size="sm"
            variant="outline"
            className="h-6 px-2 text-[10px]"
            disabled={!editing || !selection || Boolean(job)}
            onClick={() => void applyActiveTool("replace")}
          >
            Replace
          </Button>
          <Button
            size="sm"
            variant="outline"
            className="h-6 px-2 text-[10px]"
            disabled={!editing || !selection || Boolean(job)}
            onClick={() => void deleteSelection()}
          >
            Delete
          </Button>
          <Button
            size="sm"
            variant="ghost"
            className="h-6 px-2 text-[10px]"
            disabled={!selection}
            onClick={() => setSelection(null)}
          >
            Clear selection
          </Button>
        </div>
        <div className="truncate pt-1 text-[9px] text-muted-foreground">
          Last operation:{" "}
          {lastOperation
            ? `${lastOperation.changedBlocks} blocks · rev ${lastOperation.revision}`
            : "none"}
        </div>
      </div>
    </div>
  );
}

function HistoryPanel() {
  const canUndo = useHistoryStore((state) => state.canUndo);
  const canRedo = useHistoryStore((state) => state.canRedo);
  const entries = useHistoryStore((state) => state.entries);
  const loading = useHistoryStore((state) => state.loading);
  const error = useHistoryStore((state) => state.error);
  const job = useToolStore((state) => state.job);
  const editing = isTauriRuntime();

  return (
    <div className="flex h-full flex-col gap-1.5">
      <div className="flex items-center gap-1.5">
        <Button
          size="sm"
          variant="outline"
          className="h-6 px-2 text-[10px]"
          disabled={!editing || Boolean(job) || !canUndo}
          onClick={() => void undoWorld()}
        >
          <Undo2 className="mr-1 size-3" /> Undo
        </Button>
        <Button
          size="sm"
          variant="outline"
          className="h-6 px-2 text-[10px]"
          disabled={!editing || Boolean(job) || !canRedo}
          onClick={() => void redoWorld()}
        >
          <Redo2 className="mr-1 size-3" /> Redo
        </Button>
        <Button
          size="sm"
          variant="ghost"
          className="h-6 px-2 text-[10px]"
          onClick={() => void useHistoryStore.getState().refresh()}
        >
          <RotateCcw className="mr-1 size-3" /> Refresh
        </Button>
        <span className="ml-auto font-mono text-[9px] text-muted-foreground">
          {loading ? "loading…" : `${entries.length} transaction(s)`}
        </span>
      </div>

      {error ? (
        <div className="text-[10px] text-status-warn">History unavailable: {error}</div>
      ) : entries.length === 0 ? (
        <div className="flex flex-1 items-center gap-2 text-[11px] text-muted-foreground">
          <History className="size-3.5" /> No transactions yet. Committed edits appear here.
        </div>
      ) : (
        <div className="scrollbar-thin min-h-0 flex-1 overflow-y-auto rounded border border-border">
          <table className="w-full font-mono text-[9px]">
            <thead className="sticky top-0 bg-card text-left text-muted-foreground">
              <tr>
                <th className="px-2 py-1 font-normal">operation</th>
                <th className="px-2 py-1 font-normal">source</th>
                <th className="px-2 py-1 font-normal">blocks</th>
                <th className="px-2 py-1 font-normal">revision</th>
                <th className="px-2 py-1 font-normal">time</th>
              </tr>
            </thead>
            <tbody>
              {[...entries].reverse().map((entry) => (
                <tr key={entry.operationId} className="border-t border-border/60">
                  <td className="px-2 py-0.5">{entry.operationId.slice(0, 12)}</td>
                  <td className="px-2 py-0.5">{entry.source}</td>
                  <td className="px-2 py-0.5">{entry.changedBlocks.toLocaleString()}</td>
                  <td className="px-2 py-0.5">
                    {entry.revisionBefore}→{entry.revisionAfter}
                  </td>
                  <td className="px-2 py-0.5">
                    {new Date(entry.timestampMs).toLocaleTimeString()}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

function ConsolePanel({ backend }: { backend: BackendState }) {
  const lastOperation = useToolStore((state) => state.lastOperation);
  const job = useToolStore((state) => state.job);
  const syncState = useToolStore((state) => state.syncState);
  const revision = useWorldStore((state) => state.revision);
  return (
    <div className="scrollbar-thin h-full overflow-y-auto rounded border border-border bg-background/60 p-2 font-mono text-[10px] leading-relaxed">
      <div className="text-muted-foreground">FuryBuilder desktop · alpha</div>
      <div>
        <span className="text-status-ok">core</span>{" "}
        {backend.status === "online"
          ? `connected · v${backend.info?.version ?? "?"}`
          : backend.status === "connecting"
            ? "connecting…"
            : "unavailable (browser preview)"}
      </div>
      {backend.error ? <div className="text-status-warn">{backend.error}</div> : null}
      <div>world revision {revision}</div>
      <div>save state {syncState}</div>
      {job ? <div className="text-primary">job: {job.label}</div> : null}
      {lastOperation ? (
        <div className="text-muted-foreground">
          last op {lastOperation.operationId.slice(0, 12)} · {lastOperation.changedBlocks} blocks ·
          rev {lastOperation.revision}
          {lastOperation.warnings.map((warning) => (
            <div key={warning} className="text-status-warn">
              warning: {warning}
            </div>
          ))}
        </div>
      ) : (
        <div className="text-muted-foreground">No operations in this session yet.</div>
      )}
    </div>
  );
}

function WarningsPanel() {
  const lastOperation = useToolStore((state) => state.lastOperation);
  const warnings = lastOperation?.warnings ?? [];
  return (
    <div className="space-y-1.5 text-[11px]">
      <div className="flex items-center gap-2 text-muted-foreground">
        <AlertTriangle className="size-3.5" />
        {warnings.length === 0 ? "No operation warnings." : `${warnings.length} warning(s)`}
      </div>
      {warnings.map((warning) => (
        <div
          key={warning}
          className="rounded border border-status-warn/30 bg-status-warn/5 px-2 py-1 text-[10px]"
        >
          {warning}
        </div>
      ))}
      <ul className="list-disc space-y-0.5 pl-5 text-[10px] text-muted-foreground">
        <li>Textures are original placeholders, not Minecraft assets.</li>
        <li>Slabs, stairs, snow and torches use approximate shapes.</li>
        <li>Project persistence arrives in M5; edits live in the Rust session.</li>
      </ul>
    </div>
  );
}

export function BottomPanel({ backend }: { backend: BackendState }) {
  const job = useToolStore((state) => state.job);
  const tool = useToolStore((state) => state.tool);
  return (
    <section className="flex h-44 shrink-0 flex-col border-t border-border bg-card/50">
      <Tabs defaultValue="palette" className="min-h-0 flex-1">
        <div className="flex h-8 items-center gap-2 border-b border-border px-2">
          <TabsList>
            <TabsTrigger value="palette">
              <Blocks className="size-3" /> Palette
            </TabsTrigger>
            <TabsTrigger value="tools">
              <Wrench className="size-3" /> Tools
            </TabsTrigger>
            <TabsTrigger value="console">
              <Terminal className="size-3" /> Console
            </TabsTrigger>
            <TabsTrigger value="history">
              <History className="size-3" /> History
            </TabsTrigger>
            <TabsTrigger value="warnings">
              <AlertTriangle className="size-3" /> Warnings
            </TabsTrigger>
          </TabsList>
          <span className="ml-auto font-mono text-[9px] text-muted-foreground">
            active tool · {TOOL_LABELS[tool]}
          </span>
        </div>

        <TabsContent value="palette" className="m-0 flex min-h-0 flex-col p-2">
          <PalettePanel />
        </TabsContent>

        <TabsContent value="tools" className="m-0 p-2">
          <ToolsPanel />
        </TabsContent>

        <TabsContent value="console" className="m-0 p-2">
          <ConsolePanel backend={backend} />
        </TabsContent>

        <TabsContent value="history" className="m-0 p-2">
          <HistoryPanel />
        </TabsContent>

        <TabsContent value="warnings" className="m-0 p-2">
          <WarningsPanel />
        </TabsContent>
      </Tabs>
      <div className="flex h-6 shrink-0 items-center gap-2 border-t border-border px-2 text-[10px] text-muted-foreground">
        {job ? (
          <>
            <Loader2 className="size-3 animate-spin text-primary" />
            <span>{job.label}</span>
            {job.cancellable && !job.cancelRequested ? (
              <Button
                size="sm"
                variant="ghost"
                className="h-5 px-1.5 text-[9px]"
                onClick={() => useToolStore.getState().requestCancel()}
              >
                Cancel
              </Button>
            ) : null}
          </>
        ) : (
          <>
            <Terminal className="size-3 text-primary" />
            <span>Jobs queue idle</span>
          </>
        )}
        <span className="ml-auto font-mono">
          {job ? "1 job" : "0 jobs"} · {TOOL_LABELS[tool]} ·{" "}
          {backend.status === "online" ? "core" : "preview"}
        </span>
      </div>
    </section>
  );
}
