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
import type { DimensionInfo } from "@/lib/contracts";
import { useEditorStore } from "@/state/editor-store";
import { useProjectStore } from "@/state/project-store";
import { useToolStore } from "@/state/tool-store";
import { useWorldStore } from "@/state/world-store";
import { reloadViewport } from "@/features/viewport/viewport-bus";
import { cn } from "@/lib/utils";

function dimensionLabel(dimension: DimensionInfo): string {
  switch (dimension.kind) {
    case "overworld":
      return "Overworld";
    case "nether":
      return "The Nether";
    case "end":
      return "The End";
    default: {
      const path = dimension.id.includes(":")
        ? dimension.id.split(":").slice(1).join(":")
        : dimension.id;
      const words = path.replaceAll("_", " ").trim();
      return words ? words.replace(/\b\w/g, (letter) => letter.toUpperCase()) : dimension.id;
    }
  }
}

function dimensionStatus(dimension: DimensionInfo): string {
  return dimension.chunkCount > 0
    ? `${dimension.chunkCount.toLocaleString()} ${dimension.chunkCount === 1 ? "chunk" : "chunks"}`
    : "Empty";
}

export function WorldTree() {
  const [expanded, setExpanded] = useState(true);
  const [search, setSearch] = useState("");
  const dimensions = useWorldStore((state) => state.dimensions);
  const selectedDimension = useEditorStore((state) => state.selectedDimension);
  const setSelectedDimension = useEditorStore((state) => state.setSelectedDimension);
  const setPanelOpen = useEditorStore((state) => state.setPanelOpen);
  const source = useWorldStore((state) => state.source);
  const project = useProjectStore((state) => state.current);
  const needle = search.trim().toLowerCase();
  const visibleDimensions = dimensions.filter(
    (dimension) =>
      !needle ||
      dimensionLabel(dimension).toLowerCase().includes(needle) ||
      dimension.id.toLowerCase().includes(needle),
  );
  const selected = dimensions.find((dimension) => dimension.id === selectedDimension) ?? null;

  const selectDimension = (dimension: DimensionInfo): void => {
    if (dimension.id === selectedDimension) return;
    setSelectedDimension(dimension.id);
    useWorldStore.getState().setDimension(dimension.id);
    useToolStore.getState().setSelection(null);
    reloadViewport();
  };

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
            <span className="truncate text-sm font-medium">
              {project?.name ?? (source === "core" ? "Empty world" : "Preview world")}
            </span>
          </button>

          {expanded ? (
            <div className="mt-1 ml-3 border-l pl-3">
              {visibleDimensions.map((dimension) => {
                const isSelected = selectedDimension === dimension.id;
                const status = dimensionStatus(dimension);
                return (
                  <button
                    key={dimension.id}
                    type="button"
                    onClick={() => selectDimension(dimension)}
                    title={`${dimension.id} · Y ${dimension.minY}…${dimension.maxY}`}
                    className={cn(
                      "flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm transition-colors",
                      isSelected
                        ? "bg-accent text-accent-foreground"
                        : "hover:bg-accent/60 hover:text-accent-foreground",
                      dimension.chunkCount === 0 && !isSelected && "opacity-60",
                    )}
                  >
                    <Layers className="size-3.5" />
                    <span className="truncate">{dimensionLabel(dimension)}</span>
                    <span className="ml-auto font-mono text-xs text-muted-foreground">
                      {status}
                    </span>
                  </button>
                );
              })}
              {selected ? (
                <div className="mt-2 px-2 font-mono text-xs text-muted-foreground">
                  Y {selected.minY} … {selected.maxY}
                </div>
              ) : null}
            </div>
          ) : null}

          {visibleDimensions.length === 0 ? (
            <Empty className="border-0 p-4">
              <EmptyHeader>
                <EmptyMedia variant="icon">
                  <Search />
                </EmptyMedia>
                <EmptyTitle>{dimensions.length === 0 ? "No dimensions" : "No matches"}</EmptyTitle>
                <EmptyDescription>
                  {dimensions.length === 0
                    ? "The project world has not reported its dimensions yet."
                    : `Nothing in the world tree matches “${search}”.`}
                </EmptyDescription>
              </EmptyHeader>
            </Empty>
          ) : null}
        </div>
      </ScrollArea>
    </aside>
  );
}
