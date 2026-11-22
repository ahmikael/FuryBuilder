import { Circle } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Separator } from "@/components/ui/separator";
import type { BackendState } from "@/hooks/use-backend";
import { useToolStore } from "@/state/tool-store";
import { useWorldStore } from "@/state/world-store";
import { cn } from "@/lib/utils";

function BarSeparator() {
  return <Separator orientation="vertical" className="!h-4" />;
}

export function StatusBar({ backend }: { backend: BackendState }) {
  const revision = useWorldStore((state) => state.revision);
  const chunkCount = useWorldStore((state) => state.chunkCount);
  const blockCount = useWorldStore((state) => state.blockCount);
  const dimension = useWorldStore((state) => state.dimension);
  const selection = useToolStore((state) => state.selection);
  return (
    <footer className="flex h-8 shrink-0 items-center gap-2 border-t bg-background px-4 text-xs text-muted-foreground">
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
      <BarSeparator />
      <span>{dimension.replace("minecraft:", "")}</span>
      <BarSeparator />
      {selection ? (
        <>
          <BarSeparator />
          <Badge variant="secondary">box selected</Badge>
        </>
      ) : null}

      <div className="ml-auto flex items-center gap-2">
        <span className="font-mono">rev {revision}</span>
        <BarSeparator />
        <span className="hidden font-mono md:inline">{chunkCount} chunks</span>
        <BarSeparator />
        <span className="font-mono">{blockCount.toLocaleString()} blocks</span>
      </div>
    </footer>
  );
}
