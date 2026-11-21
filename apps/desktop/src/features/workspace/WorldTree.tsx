import {
  Box,
  ChevronDown,
  ChevronRight,
  Globe2,
  Layers,
  PanelLeftClose,
  Search,
} from "lucide-react";
import { useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from "@/components/ui/empty";
import { Input } from "@/components/ui/input";
import { ScrollArea } from "@/components/ui/scroll-area";
import { useEditorStore } from "@/state/editor-store";
import { useWorldStore } from "@/state/world-store";
import { cn } from "@/lib/utils";

const dimensions = [
  {
    id: "minecraft:overworld",
    label: "Overworld",
    yRange: "-64 … 319",
    loaded: true,
  },
  { id: "minecraft:the_nether", label: "The Nether", yRange: "0 … 255", loaded: false },
  { id: "minecraft:the_end", label: "The End", yRange: "0 … 255", loaded: false },
];

const modes = [
  { label: "Vanilla World", milestone: "M9" },
  { label: "Import World", milestone: "M8" },
];

export function WorldTree() {
  const [expanded, setExpanded] = useState(true);
  const [search, setSearch] = useState("");
  const selectedDimension = useEditorStore((state) => state.selectedDimension);
  const setSelectedDimension = useEditorStore((state) => state.setSelectedDimension);
  const setPanelOpen = useEditorStore((state) => state.setPanelOpen);
  const chunkCount = useWorldStore((state) => state.chunkCount);
  const blockCount = useWorldStore((state) => state.blockCount);
  const revision = useWorldStore((state) => state.revision);
  const source = useWorldStore((state) => state.source);
  const needle = search.trim().toLowerCase();
  const visibleDimensions = dimensions.filter(
    (dimension) =>
      !needle ||
      dimension.label.toLowerCase().includes(needle) ||
      dimension.id.toLowerCase().includes(needle),
  );
  const visibleModes = modes.filter((mode) => !needle || mode.label.toLowerCase().includes(needle));

  return (
    <aside className="flex min-h-0 w-72 shrink-0 flex-col border-r bg-sidebar text-sidebar-foreground">
      <div className="flex h-12 items-center gap-2 border-b px-4">
        <Globe2 className="size-4 text-muted-foreground" />
        <span className="text-sm font-medium">World</span>
        <Button
          variant="ghost"
          size="icon-sm"
          className="ml-auto"
          aria-label="Hide world panel"
          title="Hide world panel"
          onClick={() => setPanelOpen("left", false)}
        >
          <PanelLeftClose />
        </Button>
      </div>

      <div className="p-3">
        <div className="relative">
          <Search className="absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
          <Input
            className="h-9 pl-9"
            placeholder="Filter world tree…"
            value={search}
            onChange={(event) => setSearch(event.target.value)}
            aria-label="Filter world tree"
          />
        </div>
      </div>

      <ScrollArea className="min-h-0 flex-1">
        <div className="px-3 pb-4">
          <button
            type="button"
            onClick={() => setExpanded((value) => !value)}
            className="flex w-full items-center gap-2 rounded-lg px-2 py-2 text-left transition-colors hover:bg-accent hover:text-accent-foreground"
          >
            {expanded ? (
              <ChevronDown className="size-4 text-muted-foreground" />
            ) : (
              <ChevronRight className="size-4 text-muted-foreground" />
            )}
            <Box className="size-4 text-primary" />
            <span className="truncate text-sm font-medium">Untitled Project</span>
            <Badge variant="secondary" className="ml-auto">
              {source === "core" ? "Demo" : "Preview"}
            </Badge>
          </button>

          {expanded ? (
            <div className="mt-1 ml-3 border-l pl-3">
              {visibleDimensions.map((dimension) => {
                const selected = selectedDimension === dimension.id;
                return (
                  <button
                    key={dimension.id}
                    type="button"
                    onClick={() => setSelectedDimension(dimension.id)}
                    className={cn(
                      "flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm transition-colors",
                      selected
                        ? "bg-accent text-accent-foreground"
                        : "hover:bg-accent/60 hover:text-accent-foreground",
                      !dimension.loaded && "opacity-60",
                    )}
                  >
                    <Layers className="size-3.5" />
                    <span className="truncate">{dimension.label}</span>
                    <span className="ml-auto font-mono text-xs text-muted-foreground">
                      {dimension.id === "minecraft:overworld" && source
                        ? `${chunkCount} chunks`
                        : "—"}
                    </span>
                  </button>
                );
              })}
              <div className="mt-2 px-2 font-mono text-xs text-muted-foreground">
                Y range {dimensions[0]?.yRange}
              </div>
            </div>
          ) : null}

          <div className="mt-6 px-2 text-xs font-medium tracking-wide text-muted-foreground uppercase">
            Modes
          </div>
          <div className="mt-2 flex flex-col gap-1.5">
            {visibleModes.map((mode) => (
              <div
                key={mode.label}
                className="flex items-center justify-between rounded-lg border border-dashed px-3 py-2 text-sm text-muted-foreground"
              >
                <span>{mode.label}</span>
                <Badge variant="outline">{mode.milestone}</Badge>
              </div>
            ))}
            {visibleDimensions.length === 0 && visibleModes.length === 0 ? (
              <Empty className="border-0 p-4">
                <EmptyHeader>
                  <EmptyMedia variant="icon">
                    <Search />
                  </EmptyMedia>
                  <EmptyTitle>No matches</EmptyTitle>
                  <EmptyDescription>Nothing in the world tree matches “{search}”.</EmptyDescription>
                </EmptyHeader>
              </Empty>
            ) : null}
          </div>
        </div>
      </ScrollArea>

      <div
        className="truncate border-t px-4 py-3 font-mono text-xs text-muted-foreground"
        title={`target 1.21.4 · DV 4189 · rev ${revision} · ${blockCount.toLocaleString()} blocks`}
      >
        1.21.4 · DV 4189 · rev {revision} · {blockCount.toLocaleString()} blocks
      </div>
    </aside>
  );
}
