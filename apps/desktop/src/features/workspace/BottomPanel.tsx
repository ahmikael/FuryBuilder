import { AlertTriangle, Blocks, History, Play, Search, Terminal, Wrench } from "lucide-react";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import type { BackendState } from "@/hooks/use-backend";

const palette = [
  { name: "minecraft:stone_bricks", color: "#7d7d7d" },
  { name: "minecraft:oak_planks", color: "#b28a54" },
  { name: "minecraft:glass", color: "#a8d4e0" },
  { name: "minecraft:dark_oak_stairs", color: "#4b3421" },
  { name: "minecraft:spruce_log", color: "#5a4128" },
  { name: "minecraft:stone", color: "#8a8a8a" },
  { name: "minecraft:grass_block", color: "#6e9442" },
  { name: "minecraft:water", color: "#3d6fd1" },
  { name: "minecraft:sand", color: "#dbc58b" },
  { name: "minecraft:deepslate", color: "#4c4c50" },
  { name: "minecraft:gold_block", color: "#e8c447" },
  { name: "minecraft:red_wool", color: "#a43131" },
];

const tools = ["Place", "Brush", "Box select", "Fill", "Replace", "Line", "Pick"];

export function BottomPanel({ backend }: { backend: BackendState }) {
  return (
    <section className="flex h-44 shrink-0 flex-col border-t border-border bg-card/50">
      <Tabs defaultValue="palette" className="min-h-0 flex-1">
        <div className="flex h-8 items-center gap-2 border-b border-border px-2">
          <TabsList>
            <TabsTrigger value="palette">
              <Blocks className="size-3" /> Palette
            </TabsTrigger>
            <TabsTrigger value="tools">
              <Wrench className="size-3" /> Tools
            </TabsTrigger>
            <TabsTrigger value="console">
              <Terminal className="size-3" /> Console
            </TabsTrigger>
            <TabsTrigger value="history">
              <History className="size-3" /> History
            </TabsTrigger>
            <TabsTrigger value="warnings">
              <AlertTriangle className="size-3" /> Warnings
            </TabsTrigger>
          </TabsList>
          <span className="ml-auto font-mono text-[9px] text-muted-foreground">
            Viewport live · editing tools arrive in M4
          </span>
        </div>

        <TabsContent value="palette" className="m-0 flex min-h-0 flex-col p-2">
          <div className="mb-2 flex items-center gap-2">
            <div className="relative w-64">
              <Search className="absolute top-1/2 left-2 size-3 -translate-y-1/2 text-muted-foreground" />
              <Input className="h-6 pl-7" placeholder="Search blocks…" disabled />
            </div>
            <span className="text-[10px] text-muted-foreground">1.21.4 catalog · placeholder</span>
          </div>
          <div className="scrollbar-thin flex min-h-0 flex-wrap gap-1 overflow-y-auto">
            {palette.map((block) => (
              <div
                key={block.name}
                className="flex items-center gap-1.5 rounded border border-border bg-background/40 px-1.5 py-1"
                title={block.name}
              >
                <span
                  className="size-3.5 rounded-[2px] border border-black/30"
                  style={{ backgroundColor: block.color }}
                />
                <span className="font-mono text-[9px] text-muted-foreground">
                  {block.name.replace("minecraft:", "")}
                </span>
              </div>
            ))}
          </div>
        </TabsContent>

        <TabsContent value="tools" className="m-0 p-2">
          <div className="flex flex-wrap gap-1.5">
            {tools.map((tool) => (
              <Button key={tool} variant="outline" size="sm" disabled>
                {tool}
              </Button>
            ))}
          </div>
          <p className="mt-2 text-[10px] text-muted-foreground">
            Tools are enabled once the operation API and renderer are connected.
          </p>
        </TabsContent>

        <TabsContent value="console" className="m-0 p-2">
          <div className="scrollbar-thin h-full overflow-y-auto rounded border border-border bg-background/60 p-2 font-mono text-[10px] leading-relaxed">
            <div className="text-muted-foreground">FuryBuilder desktop · alpha</div>
            <div>
              <span className="text-status-ok">core</span>{" "}
              {backend.status === "online"
                ? `connected — v${backend.info?.version ?? "?"}`
                : backend.status === "connecting"
                  ? "connecting…"
                  : "unavailable (browser preview)"}
            </div>
            {backend.error ? <div className="text-status-warn">{backend.error}</div> : null}
            <div className="text-muted-foreground">Waiting for the world core…</div>
          </div>
        </TabsContent>

        <TabsContent value="history" className="m-0 p-2">
          <div className="flex items-center gap-2 text-[11px] text-muted-foreground">
            <History className="size-3.5" /> No transactions yet.
          </div>
        </TabsContent>

        <TabsContent value="warnings" className="m-0 p-2">
          <div className="flex items-center gap-2 text-[11px] text-muted-foreground">
            <AlertTriangle className="size-3.5" /> No warnings.
          </div>
        </TabsContent>
      </Tabs>
      <div className="flex h-6 shrink-0 items-center gap-2 border-t border-border px-2 text-[10px] text-muted-foreground">
        <Play className="size-3 text-primary" />
        <span>Jobs queue idle</span>
        <span className="ml-auto font-mono">0 jobs · 0 warnings</span>
      </div>
    </section>
  );
}
