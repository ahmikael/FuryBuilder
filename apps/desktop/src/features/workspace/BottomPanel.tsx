import { useEffect, useState, type ReactNode } from "react";
import {
  Blocks,
  Boxes,
  Braces,
  Grid3X3,
  History,
  Layers,
  Leaf,
  Library,
  Map,
  Mountain,
  PanelBottomClose,
  Redo2,
  RotateCcw,
  Route,
  Shapes,
  ShieldCheck,
  Terminal,
  Undo2,
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
import { BOTTOM_PANEL_TABS, isBottomPanelTabId, type BottomPanelTabId } from "@/editor/layout";
import { redoWorld, undoWorld } from "@/editor/commands";
import { BlockPalette } from "@/features/workspace/BlockPalette";
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
import { useHistoryStore } from "@/state/history-store";
import { useEditorStore } from "@/state/editor-store";
import { useProjectStore } from "@/state/project-store";
import { useToolStore } from "@/state/tool-store";
import { useWorldStore } from "@/state/world-store";
import { cn } from "@/lib/utils";

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
  const projectFolder = useProjectStore((state) => state.current?.folder);
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
          <BlockPalette key={projectFolder} />
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
