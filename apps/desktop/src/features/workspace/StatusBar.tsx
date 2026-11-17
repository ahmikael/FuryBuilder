import { Circle, Loader2 } from "lucide-react";
import type { BackendState } from "@/hooks/use-backend";
import { TOOL_LABELS } from "@/editor/session";
import { useToolStore } from "@/state/tool-store";
import { useWorldStore } from "@/state/world-store";
import { cn } from "@/lib/utils";

export function StatusBar({ backend }: { backend: BackendState }) {
  const revision = useWorldStore((state) => state.revision);
  const chunkCount = useWorldStore((state) => state.chunkCount);
  const blockCount = useWorldStore((state) => state.blockCount);
  const dimension = useWorldStore((state) => state.dimension);
  const loadRadius = useWorldStore((state) => state.loadRadius);
  const source = useWorldStore((state) => state.source);
  const metrics = useWorldStore((state) => state.metrics);
  const tool = useToolStore((state) => state.tool);
  const job = useToolStore((state) => state.job);
  const syncState = useToolStore((state) => state.syncState);
  const selection = useToolStore((state) => state.selection);

  return (
    <footer className="flex h-6 shrink-0 items-center gap-3 border-t border-border bg-card/80 px-2.5 text-[10px] text-muted-foreground">
      <span className="inline-flex items-center gap-1.5">
        <Circle
          className={cn(
            "size-2 fill-current",
            backend.status === "online" && "text-status-ok",
            backend.status === "connecting" && "text-status-warn",
            backend.status === "offline" && "text-status-error",
          )}
        />
        {backend.status === "online" ? "Core connected" : "Core offline"}
      </span>
      <span className="text-border">|</span>
      <span>Untitled Project</span>
      <span className="text-border">|</span>
      <span>{source === "core" ? "Empty Project · demo scene" : "Preview scene"}</span>
      <span className="text-border">|</span>
      <span>{dimension.replace("minecraft:", "")}</span>
      <span className="text-border">|</span>
      <span className="inline-flex items-center gap-1">
        {job ? <Loader2 className="size-2.5 animate-spin text-primary" /> : null}
        {job ? job.label : `tool: ${TOOL_LABELS[tool]}`}
      </span>
      {selection ? (
        <>
          <span className="text-border">|</span>
          <span className="text-sky-300">box selected</span>
        </>
      ) : null}

      <span className="ml-auto font-mono">
        {syncState === "modified" ? (
          <span className="text-status-warn">unsaved changes</span>
        ) : (
          "no edits"
        )}
      </span>
      <span className="text-border">|</span>
      <span className="font-mono">rev {revision}</span>
      <span className="text-border">|</span>
      <span className="font-mono">{chunkCount} chunks</span>
      <span className="text-border">|</span>
      <span className="font-mono">{blockCount.toLocaleString()} blocks</span>
      <span className="text-border">|</span>
      <span className="font-mono">radius {loadRadius}</span>
      <span className="text-border">|</span>
      <span className="font-mono">
        {metrics ? `${metrics.fps} fps · ${metrics.drawCalls} draws` : "— fps"}
      </span>
      <span className="text-border">|</span>
      <span className="font-mono">
        Minecraft {backend.info?.targetMinecraftVersion ?? "1.21.4"}
      </span>
      <span className="text-border">|</span>
      <span className="font-mono">DV {backend.info?.targetDataVersion ?? 4189}</span>
    </footer>
  );
}
