import { useEffect, useMemo, useState, type ReactNode } from "react";
import {
  AlertTriangle,
  Blocks,
  Boxes,
  Braces,
  Grid3X3,
  History,
  Layers,
  Leaf,
  Library,
  Loader2,
  Map,
  Mountain,
  PanelBottomClose,
  Redo2,
  RotateCcw,
  Route,
  Search,
  Shapes,
  ShieldCheck,
  Terminal,
  Undo2,
  X,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from "@/components/ui/empty";
import { Input } from "@/components/ui/input";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import type { BackendState } from "@/hooks/use-backend";
import type { BlockDefinition } from "@/editor/types";
import { BOTTOM_PANEL_TABS, isBottomPanelTabId, type BottomPanelTabId } from "@/editor/layout";
import { redoWorld, undoWorld } from "@/editor/commands";
import { TerrainPanel } from "@/features/workspace/TerrainPanel";
import { HeightmapPanel } from "@/features/workspace/HeightmapPanel";
import { BiomePanel } from "@/features/workspace/BiomePanel";
import { GeneratorsPanel } from "@/features/workspace/GeneratorsPanel";
import { WorkspacePanel } from "@/features/workspace/WorkspacePanel";
import { TemplateLibraryPanel } from "@/features/workspace/TemplateLibraryPanel";
import { ScenePanel } from "@/features/workspace/ScenePanel";
import { ParametricPanel } from "@/features/workspace/ParametricPanel";
import { BuildDslPanel } from "@/features/workspace/BuildDslPanel";
import { ObservationPanel } from "@/features/workspace/ObservationPanel";
import { ValidationPanel } from "@/features/workspace/ValidationPanel";
import { isTauriRuntime } from "@/lib/ipc";
import { fetchMcpActivity, type McpActivityEntry } from "@/lib/mcp-access";
import { fetchBlockCatalog } from "@/lib/world-api";
import { blockColor } from "@/renderer/assets";
import { useHistoryStore } from "@/state/history-store";
import { useEditorStore } from "@/state/editor-store";
import { useProjectStore } from "@/state/project-store";
import { useToolStore } from "@/state/tool-store";
import { useWorldStore } from "@/state/world-store";
import { cn } from "@/lib/utils";

function BlocksPanel() {
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
      <Empty className="h-full border-0">
        <EmptyHeader>
          <EmptyMedia variant="icon">
            <Blocks />
          </EmptyMedia>
          <EmptyTitle>Desktop core required</EmptyTitle>
          <EmptyDescription>The block catalog requires the desktop app.</EmptyDescription>
        </EmptyHeader>
      </Empty>
    );
  }

  if (error) {
    return (
      <Empty className="h-full border-0">
        <EmptyHeader>
          <EmptyMedia variant="icon">
            <AlertTriangle className="text-status-warn" />
          </EmptyMedia>
          <EmptyTitle>Block catalog unavailable</EmptyTitle>
          <EmptyDescription>{error}</EmptyDescription>
        </EmptyHeader>
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
      </Empty>
    );
  }

  return (
    <div className="flex min-h-0 flex-1 gap-4">
      <div className="flex min-h-0 w-80 flex-col gap-3">
        <div className="relative">
          <Search className="absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
          <Input
            className="h-9 pl-9"
            placeholder={catalog ? `Search ${catalog.length} blocks…` : "Loading catalog…"}
            value={search}
            disabled={!catalog}
            onChange={(event) => setSearch(event.target.value)}
            aria-label="Search blocks"
          />
        </div>
        <div className="scrollbar-thin flex min-h-0 flex-1 flex-wrap content-start gap-1.5 overflow-y-auto">
          {!catalog ? (
            <div className="flex items-center gap-1.5 p-1 text-xs text-muted-foreground">
              <Loader2 className="size-3 animate-spin" /> Reading block registry…
            </div>
          ) : filtered.length === 0 ? (
            <div className="p-1 text-xs text-muted-foreground">
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
                    "flex items-center gap-2 rounded-md border px-2 py-1.5 transition-colors",
                    active
                      ? "border-primary/60 bg-primary/15 text-foreground"
                      : "border-border bg-background text-muted-foreground hover:bg-accent hover:text-accent-foreground",
                  )}
                >
                  <span
                    className="size-4 rounded-sm border border-black/30"
                    style={{ backgroundColor: blockColor(entry.name) }}
                  />
                  <span className="font-mono text-xs">{entry.name.replace("minecraft:", "")}</span>
                </button>
              );
            })
          )}
        </div>
      </div>

      <Card className="scrollbar-thin flex min-h-0 w-80 shrink-0 flex-col gap-0 overflow-y-auto py-0">
        <CardContent className="space-y-4 p-4">
          <div className="flex items-center gap-3">
            <span
              className="size-9 rounded-lg border border-black/30"
              style={{ backgroundColor: blockColor(selectedBlock.name) }}
            />
            <div className="min-w-0">
              <div className="truncate font-mono text-sm">{selectedBlock.name}</div>
              <div className="text-xs text-muted-foreground">
                {definition ? "Curated state" : "Custom state"} · brush/paste target
              </div>
            </div>
          </div>

          <div className="space-y-2">
            {properties.length === 0 ? (
              <div className="text-xs text-muted-foreground">No state properties.</div>
            ) : (
              properties.map(([key, value]) => (
                <label key={key} className="grid grid-cols-[1fr_1.4fr] items-center gap-1.5">
                  <span className="truncate font-mono text-xs text-muted-foreground">{key}</span>
                  <Input
                    className="h-8 px-2 font-mono text-xs"
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

          <div className="flex flex-wrap gap-2">
            <Button
              size="sm"
              variant="outline"
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
              <RotateCcw data-icon="inline-start" />
              Defaults
            </Button>
            <Button size="sm" variant="outline" onClick={() => setReplaceFrom(selectedBlock)}>
              Use as replace source
            </Button>
          </div>

          <div className="rounded-xl border bg-background p-3 text-xs leading-relaxed text-muted-foreground">
            Replace source:{" "}
            <span className="font-mono text-foreground/80">
              {replaceFrom?.name.replace("minecraft:", "") ?? "not set"}
            </span>
            {replaceFrom ? (
              <Button
                variant="ghost"
                size="icon-xs"
                className="ml-1"
                aria-label="Clear replace source"
                onClick={() => setReplaceFrom(null)}
              >
                <X />
              </Button>
            ) : null}
            <div className="mt-1.5">
              Unknown property values are rejected by the Rust registry, never silently coerced.
            </div>
          </div>
        </CardContent>
      </Card>
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
  const project = useProjectStore((state) => state.current);
  const revision = useWorldStore((state) => state.revision);
  const editing = isTauriRuntime();
  const [mcpActivity, setMcpActivity] = useState<McpActivityEntry[]>([]);
  const [mcpActivityError, setMcpActivityError] = useState<string | null>(null);
  const [refreshNonce, setRefreshNonce] = useState(0);

  useEffect(() => {
    if (!editing || !project?.folder) return;
    let cancelled = false;
    const load = async () => {
      try {
        const response = await fetchMcpActivity(20);
        if (!cancelled) {
          setMcpActivity(response.entries);
          setMcpActivityError(null);
        }
      } catch (activityError) {
        if (!cancelled) {
          setMcpActivityError(
            activityError instanceof Error ? activityError.message : String(activityError),
          );
        }
      }
    };
    void load();
    const timer = window.setInterval(() => void load(), 5_000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [editing, project?.folder, revision, refreshNonce]);

  const refreshAll = () => {
    void useHistoryStore.getState().refresh();
    setRefreshNonce((current) => current + 1);
  };

  return (
    <div className="flex h-full flex-col gap-1.5">
      <div className="flex flex-wrap items-center gap-2">
        <Button
          size="sm"
          variant="outline"
          disabled={!editing || Boolean(job) || !canUndo}
          onClick={() => void undoWorld()}
        >
          <Undo2 data-icon="inline-start" />
          Undo
        </Button>
        <Button
          size="sm"
          variant="outline"
          disabled={!editing || Boolean(job) || !canRedo}
          onClick={() => void redoWorld()}
        >
          <Redo2 data-icon="inline-start" />
          Redo
        </Button>
        <Button size="sm" variant="ghost" onClick={refreshAll}>
          <RotateCcw data-icon="inline-start" />
          Refresh
        </Button>
        <Badge variant="secondary" className="ml-auto font-mono">
          {loading ? "loading…" : `${entries.length} transaction(s)`}
        </Badge>
      </div>

      {error ? (
        <p className="text-sm text-status-warn">History unavailable: {error}</p>
      ) : entries.length === 0 ? (
        <Empty className="min-h-24 border-0">
          <EmptyHeader>
            <EmptyMedia variant="icon">
              <History />
            </EmptyMedia>
            <EmptyTitle>No transactions yet</EmptyTitle>
            <EmptyDescription>Committed edits appear here.</EmptyDescription>
          </EmptyHeader>
        </Empty>
      ) : (
        <div className="scrollbar-thin min-h-0 flex-1 overflow-y-auto rounded-lg border">
          <Table>
            <TableHeader className="sticky top-0 bg-card">
              <TableRow>
                <TableHead>operation</TableHead>
                <TableHead>source</TableHead>
                <TableHead>blocks</TableHead>
                <TableHead>revision</TableHead>
                <TableHead>time</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {[...entries].reverse().map((entry) => (
                <TableRow key={entry.operationId} className="font-mono text-xs">
                  <TableCell>{entry.operationId.slice(0, 12)}</TableCell>
                  <TableCell>{entry.source}</TableCell>
                  <TableCell>{entry.changedBlocks.toLocaleString()}</TableCell>
                  <TableCell>
                    {entry.revisionBefore}→{entry.revisionAfter}
                  </TableCell>
                  <TableCell>{new Date(entry.timestampMs).toLocaleTimeString()}</TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
      )}

      {project?.folder ? (
        <div className="shrink-0 rounded-lg border bg-background">
          <div className="flex items-center gap-2 border-b px-3 py-2 text-xs text-muted-foreground">
            <Terminal className="size-3.5" />
            <span>MCP activity</span>
            <span className="font-mono">
              {mcpActivityError ? "unavailable" : `${mcpActivity.length} recent call(s)`}
            </span>
          </div>
          {mcpActivityError ? (
            <p className="px-3 py-2 text-sm text-status-warn">{mcpActivityError}</p>
          ) : mcpActivity.length === 0 ? (
            <p className="px-3 py-2 text-sm text-muted-foreground">
              No authenticated MCP calls for this project yet.
            </p>
          ) : (
            <div className="scrollbar-thin max-h-24 overflow-y-auto">
              <Table>
                <TableHeader className="sticky top-0 bg-card">
                  <TableRow>
                    <TableHead>time</TableHead>
                    <TableHead>call</TableHead>
                    <TableHead>status</TableHead>
                    <TableHead>operation</TableHead>
                    <TableHead>blocks</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {mcpActivity.map((entry) => (
                    <TableRow key={entry.sequence} className="font-mono text-xs">
                      <TableCell>{new Date(entry.timestampMs).toLocaleTimeString()}</TableCell>
                      <TableCell>
                        {entry.method.replace(/_operation$/, "")}
                        {entry.confirmation ? ` · ${entry.confirmation}` : ""}
                      </TableCell>
                      <TableCell
                        className={cn(
                          entry.status === "ok"
                            ? "text-status-ok"
                            : entry.status === "denied"
                              ? "text-status-warn"
                              : "text-status-error",
                        )}
                        title={entry.errorCode ?? undefined}
                      >
                        {entry.status}
                      </TableCell>
                      <TableCell>
                        {entry.operationId ? entry.operationId.slice(0, 12) : "—"}
                      </TableCell>
                      <TableCell>
                        {entry.changedBlocks !== null ? entry.changedBlocks.toLocaleString() : "—"}
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </div>
          )}
        </div>
      ) : null}
    </div>
  );
}

function ConsolePanel({ backend }: { backend: BackendState }) {
  const lastOperation = useToolStore((state) => state.lastOperation);
  const job = useToolStore((state) => state.job);
  const syncState = useToolStore((state) => state.syncState);
  const revision = useWorldStore((state) => state.revision);
  return (
    <Card className="scrollbar-thin h-full gap-0 overflow-y-auto py-0 font-mono text-xs leading-relaxed">
      <CardContent className="p-4">
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
            last op {lastOperation.operationId.slice(0, 12)} · {lastOperation.changedBlocks} blocks
            · rev {lastOperation.revision}
            {lastOperation.warnings.map((warning) => (
              <div key={warning} className="text-status-warn">
                warning: {warning}
              </div>
            ))}
          </div>
        ) : (
          <div className="text-muted-foreground">No operations in this session yet.</div>
        )}
      </CardContent>
    </Card>
  );
}

function AnalysisPanel() {
  return (
    <Tabs defaultValue="observation" className="min-h-0 flex-1 gap-2">
      <TabsList className="w-fit shrink-0">
        <TabsTrigger value="observation">
          <Map className="size-3" /> Observation
        </TabsTrigger>
        <TabsTrigger value="validation">
          <ShieldCheck className="size-3" /> Validation
        </TabsTrigger>
      </TabsList>
      <TabsContent value="observation" className="m-0 flex min-h-0 flex-1 flex-col">
        <ObservationPanel />
      </TabsContent>
      <TabsContent value="validation" className="m-0 flex min-h-0 flex-1 flex-col">
        <ValidationPanel />
      </TabsContent>
    </Tabs>
  );
}

function ActivityPanel({ backend }: { backend: BackendState }) {
  return (
    <Tabs defaultValue="console" className="min-h-0 flex-1 gap-2">
      <TabsList className="w-fit shrink-0">
        <TabsTrigger value="console">
          <Terminal className="size-3" /> Console
        </TabsTrigger>
        <TabsTrigger value="history">
          <History className="size-3" /> History
        </TabsTrigger>
      </TabsList>
      <TabsContent value="console" className="m-0 flex min-h-0 flex-1 flex-col">
        <ConsolePanel backend={backend} />
      </TabsContent>
      <TabsContent value="history" className="m-0 flex min-h-0 flex-1 flex-col">
        <HistoryPanel />
      </TabsContent>
    </Tabs>
  );
}

const TAB_ICONS: Record<BottomPanelTabId, ReactNode> = {
  blocks: <Blocks className="size-3" />,
  terrain: <Mountain className="size-3" />,
  heightmap: <Grid3X3 className="size-3" />,
  biomes: <Leaf className="size-3" />,
  generators: <Route className="size-3" />,
  layers: <Layers className="size-3" />,
  templates: <Library className="size-3" />,
  objects: <Shapes className="size-3" />,
  parametric: <Boxes className="size-3" />,
  dsl: <Braces className="size-3" />,
  analysis: <Map className="size-3" />,
  activity: <Terminal className="size-3" />,
};

export function BottomPanel({ backend }: { backend: BackendState }) {
  const bottomPanelTab = useEditorStore((state) => state.bottomPanelTab);
  const setBottomPanelTab = useEditorStore((state) => state.setBottomPanelTab);
  const setPanelOpen = useEditorStore((state) => state.setPanelOpen);
  return (
    <section className="flex h-64 shrink-0 flex-col border-t bg-background">
      <Tabs
        value={bottomPanelTab}
        onValueChange={(value) => {
          if (isBottomPanelTabId(value)) setBottomPanelTab(value);
        }}
        className="min-h-0 flex-1"
      >
        <div className="flex h-12 items-center gap-3 border-b px-3">
          <div className="min-w-0 overflow-x-auto">
            <TabsList>
              {BOTTOM_PANEL_TABS.map((tab) => (
                <TabsTrigger key={tab.id} value={tab.id}>
                  {TAB_ICONS[tab.id]} {tab.label}
                </TabsTrigger>
              ))}
            </TabsList>
          </div>
          <Button
            variant="ghost"
            size="icon-sm"
            className="shrink-0"
            aria-label="Hide bottom panel"
            title="Hide bottom panel"
            onClick={() => setPanelOpen("bottom", false)}
          >
            <PanelBottomClose />
          </Button>
        </div>

        <TabsContent value="blocks" className="m-0 flex min-h-0 flex-col p-4">
          <BlocksPanel />
        </TabsContent>

        <TabsContent value="terrain" className="m-0 p-4">
          <TerrainPanel />
        </TabsContent>

        <TabsContent value="heightmap" className="m-0 p-4">
          <HeightmapPanel />
        </TabsContent>

        <TabsContent value="biomes" className="m-0 p-4">
          <BiomePanel />
        </TabsContent>

        <TabsContent value="generators" className="m-0 p-4">
          <GeneratorsPanel />
        </TabsContent>

        <TabsContent value="layers" className="m-0 flex min-h-0 flex-col p-4">
          <WorkspacePanel />
        </TabsContent>

        <TabsContent value="templates" className="m-0 flex min-h-0 flex-col p-4">
          <TemplateLibraryPanel />
        </TabsContent>

        <TabsContent value="objects" className="m-0 flex min-h-0 flex-col p-4">
          <ScenePanel />
        </TabsContent>

        <TabsContent value="parametric" className="m-0 flex min-h-0 flex-col p-4">
          <ParametricPanel />
        </TabsContent>

        <TabsContent value="dsl" className="m-0 flex min-h-0 flex-col p-2">
          <BuildDslPanel />
        </TabsContent>

        <TabsContent value="analysis" className="m-0 flex min-h-0 flex-col p-2">
          <AnalysisPanel />
        </TabsContent>

        <TabsContent value="activity" className="m-0 flex min-h-0 flex-col p-2">
          <ActivityPanel backend={backend} />
        </TabsContent>
      </Tabs>
    </section>
  );
}
