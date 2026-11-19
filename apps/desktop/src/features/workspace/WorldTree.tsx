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
import { Input } from "@/components/ui/input";
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
    <aside className="flex min-h-0 w-56 shrink-0 flex-col border-r border-border bg-sidebar text-sidebar-foreground">
      <div className="flex h-9 items-center gap-1.5 border-b border-border px-2.5">
        <Globe2 className="size-3.5 text-muted-foreground" />
        <span className="text-[11px] font-semibold tracking-wide uppercase">World</span>
        <button
          type="button"
          aria-label="Hide world panel"
          title="Hide world panel"
          className="ml-auto rounded p-1 text-muted-foreground hover:bg-accent hover:text-accent-foreground"
          onClick={() => setPanelOpen("left", false)}
        >
          <PanelLeftClose className="size-3.5" />
        </button>
      </div>

      <div className="border-b border-border p-1.5">
        <div className="relative">
          <Search className="absolute top-1/2 left-2 size-3 -translate-y-1/2 text-muted-foreground" />
          <Input
            className="h-6 pl-7"
            placeholder="Filter world tree…"
            value={search}
            onChange={(event) => setSearch(event.target.value)}
            aria-label="Filter world tree"
          />
        </div>
      </div>

      <div className="scrollbar-thin min-h-0 flex-1 overflow-y-auto p-1.5">
        <button
          type="button"
          onClick={() => setExpanded((value) => !value)}
          className="flex w-full items-center gap-1 rounded px-1 py-1 text-left hover:bg-accent/50"
        >
          {expanded ? <ChevronDown className="size-3" /> : <ChevronRight className="size-3" />}
          <Box className="size-3.5 text-primary" />
          <span className="truncate text-xs font-medium">Untitled Project</span>
          <span className="ml-auto rounded bg-muted px-1 py-0.5 text-[9px] text-muted-foreground">
            {source === "core" ? "Demo" : "Preview"}
          </span>
        </button>

        {expanded ? (
          <div className="mt-0.5 ml-2 border-l border-border pl-2">
            {visibleDimensions.map((dimension) => {
              const selected = selectedDimension === dimension.id;
              return (
                <button
                  key={dimension.id}
                  type="button"
                  onClick={() => setSelectedDimension(dimension.id)}
                  className={cn(
                    "flex w-full items-center gap-1.5 rounded px-1.5 py-1 text-left",
                    selected ? "bg-accent text-accent-foreground" : "hover:bg-accent/40",
                    !dimension.loaded && "opacity-60",
                  )}
                >
                  <Layers className="size-3" />
                  <span className="truncate text-[11px]">{dimension.label}</span>
                  <span className="ml-auto font-mono text-[9px] text-muted-foreground">
                    {dimension.id === "minecraft:overworld" && source
                      ? `${chunkCount} chunks`
                      : "—"}
                  </span>
                </button>
              );
            })}
            <div className="mt-1 px-1.5 py-1 font-mono text-[9px] text-muted-foreground">
              Y range {dimensions[0]?.yRange}
            </div>
          </div>
        ) : null}

        <div className="mt-4 px-1 text-[9px] font-semibold tracking-wider text-muted-foreground uppercase">
          Modes
        </div>
        <div className="mt-1 space-y-1 px-1">
          {visibleModes.map((mode) => (
            <div
              key={mode.label}
              className="flex items-center justify-between rounded border border-dashed border-border px-2 py-1.5 text-[11px] text-muted-foreground"
            >
              <span>{mode.label}</span>
              <span className="text-[9px]">{mode.milestone}</span>
            </div>
          ))}
          {visibleDimensions.length === 0 && visibleModes.length === 0 ? (
            <div className="rounded border border-dashed border-border px-2 py-1.5 text-[10px] text-muted-foreground">
              Nothing in the world tree matches “{search}”.
            </div>
          ) : null}
        </div>
      </div>

      <div
        className="truncate border-t border-border px-2.5 py-2 font-mono text-[9px] text-muted-foreground"
        title={`target 1.21.4 · DV 4189 · rev ${revision} · ${blockCount.toLocaleString()} blocks`}
      >
        1.21.4 · DV 4189 · rev {revision} · {blockCount.toLocaleString()} blocks
      </div>
    </aside>
  );
}
