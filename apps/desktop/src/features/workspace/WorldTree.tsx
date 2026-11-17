import { Box, ChevronDown, ChevronRight, Globe2, Layers, Search } from "lucide-react";
import { useState } from "react";
import { Input } from "@/components/ui/input";
import { useEditorStore } from "@/state/editor-store";
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

export function WorldTree() {
  const [expanded, setExpanded] = useState(true);
  const selectedDimension = useEditorStore((state) => state.selectedDimension);
  const setSelectedDimension = useEditorStore((state) => state.setSelectedDimension);

  return (
    <aside className="flex min-h-0 w-56 shrink-0 flex-col border-r border-border bg-sidebar text-sidebar-foreground">
      <div className="flex h-9 items-center gap-1.5 border-b border-border px-2.5">
        <Globe2 className="size-3.5 text-muted-foreground" />
        <span className="text-[11px] font-semibold tracking-wide uppercase">World</span>
      </div>

      <div className="border-b border-border p-1.5">
        <div className="relative">
          <Search className="absolute top-1/2 left-2 size-3 -translate-y-1/2 text-muted-foreground" />
          <Input
            className="h-6 pl-7"
            placeholder="Search world…"
            disabled
            aria-label="Search world"
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
            Empty
          </span>
        </button>

        {expanded ? (
          <div className="mt-0.5 ml-2 border-l border-border pl-2">
            {dimensions.map((dimension) => {
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
                    {dimension.loaded ? "0 chunks" : "—"}
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
          <div className="flex items-center justify-between rounded border border-dashed border-border px-2 py-1.5 text-[11px] text-muted-foreground">
            <span>Vanilla World</span>
            <span className="text-[9px]">M9</span>
          </div>
          <div className="flex items-center justify-between rounded border border-dashed border-border px-2 py-1.5 text-[11px] text-muted-foreground">
            <span>Import World</span>
            <span className="text-[9px]">M8</span>
          </div>
        </div>
      </div>

      <div className="border-t border-border px-2.5 py-2 font-mono text-[9px] text-muted-foreground">
        target 1.21.4 · DV 4189
      </div>
    </aside>
  );
}
