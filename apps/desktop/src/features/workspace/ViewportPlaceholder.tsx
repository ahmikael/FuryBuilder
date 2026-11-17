import { useState } from "react";
import { Box, Grid3X3, Info, MousePointer2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";

function detectWebGl2(): boolean {
  try {
    const canvas = document.createElement("canvas");
    return canvas.getContext("webgl2") !== null;
  } catch {
    return false;
  }
}

export function ViewportPlaceholder() {
  // Lazy initializer: the capability probe runs once and never triggers an effect cascade.
  const [webgl2] = useState<boolean>(() => detectWebGl2());

  return (
    <main className="relative min-h-0 min-w-0 flex-1 overflow-hidden bg-[oklch(0.17_0.014_260)]">
      <div className="viewport-grid absolute inset-0" />

      <div className="pointer-events-none absolute inset-0 flex flex-col items-center justify-center gap-4 text-center">
        <div className="relative grid size-24 place-items-center rounded-xl border border-white/10 bg-white/[0.03] shadow-[0_0_80px_oklch(0.72_0.17_55_/_18%)]">
          <Box className="size-10 text-primary/80" strokeWidth={1.2} />
          <span className="absolute -right-2 -bottom-2 rounded bg-card px-1.5 py-0.5 font-mono text-[9px] text-muted-foreground">
            16³
          </span>
        </div>
        <div className="max-w-sm space-y-1">
          <h1 className="text-sm font-semibold tracking-tight text-white/90">3D viewport</h1>
          <p className="text-[11px] leading-relaxed text-white/45">
            The Three.js chunk renderer is introduced in M3. World data stays in the Rust core; this
            canvas is only a derived view.
          </p>
        </div>
        {webgl2 === false ? (
          <div className="flex items-center gap-1.5 rounded-md border border-status-warn/40 bg-status-warn/10 px-2.5 py-1.5 text-[10px] text-status-warn">
            <Info className="size-3" /> WebGL2 is unavailable in this WebView. Rendering will be
            disabled, editing data will still work.
          </div>
        ) : (
          <div className="rounded-md border border-white/10 bg-black/25 px-2.5 py-1 text-[10px] text-white/40">
            WebGL2 {webgl2 ? "available" : "unavailable"}
          </div>
        )}
      </div>

      <div className="pointer-events-none absolute top-2 left-2 flex items-center gap-2">
        <span className="rounded border border-white/10 bg-black/35 px-2 py-1 font-mono text-[10px] text-white/60">
          Overworld · 0, 64, 0
        </span>
      </div>

      <div className="absolute top-2 right-2 flex items-center gap-1">
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant="ghost"
              size="icon"
              className="text-white/50 hover:bg-white/10"
              disabled
            >
              <Grid3X3 />
            </Button>
          </TooltipTrigger>
          <TooltipContent>Chunk grid (M3).</TooltipContent>
        </Tooltip>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant="ghost"
              size="icon"
              className="text-white/50 hover:bg-white/10"
              disabled
            >
              <MousePointer2 />
            </Button>
          </TooltipTrigger>
          <TooltipContent>Selection tool (M4).</TooltipContent>
        </Tooltip>
      </div>

      <div className="pointer-events-none absolute right-2 bottom-2 flex items-center gap-3 rounded border border-white/10 bg-black/35 px-2.5 py-1 font-mono text-[9px] text-white/45">
        <span>WASD fly</span>
        <span>RMB look</span>
        <span>Shift boost</span>
        <span className={cn("text-white/25")}>Three.js pending</span>
      </div>
    </main>
  );
}
