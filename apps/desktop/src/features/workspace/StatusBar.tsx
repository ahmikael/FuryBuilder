import { Circle } from "lucide-react";
import type { BackendState } from "@/hooks/use-backend";
import { cn } from "@/lib/utils";

export function StatusBar({ backend }: { backend: BackendState }) {
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
      <span>Empty Project</span>
      <span className="text-border">|</span>
      <span>No unsaved changes</span>

      <span className="ml-auto font-mono">rev 0</span>
      <span className="text-border">|</span>
      <span className="font-mono">0 chunks</span>
      <span className="text-border">|</span>
      <span className="font-mono">
        Minecraft {backend.info?.targetMinecraftVersion ?? "1.21.4"}
      </span>
      <span className="text-border">|</span>
      <span className="font-mono">DV {backend.info?.targetDataVersion ?? 4189}</span>
      <span className="text-border">|</span>
      <span className="font-mono">60 fps target</span>
    </footer>
  );
}
