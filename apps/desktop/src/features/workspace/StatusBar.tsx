import { Circle } from "lucide-react";
import type { BackendState } from "@/hooks/use-backend";
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

      <span className="ml-auto font-mono">rev {revision}</span>
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
