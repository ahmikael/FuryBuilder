import type { ReactNode } from "react";
import { Box, ChevronDown, Layers, SlidersHorizontal, Sparkles } from "lucide-react";
import { Separator } from "@/components/ui/separator";
import { blockColor } from "@/renderer/assets";
import type { BlockState } from "@/renderer/types";
import { useWorldStore } from "@/state/world-store";

function Field({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-center justify-between gap-2 px-2.5 py-1">
      <span className="text-[11px] text-muted-foreground">{label}</span>
      <span className="truncate font-mono text-[10px]" title={value}>
        {value}
      </span>
    </div>
  );
}

function SectionHeader({ icon, children }: { icon: ReactNode; children: ReactNode }) {
  return (
    <header className="flex items-center gap-1 px-2 py-1 text-[10px] font-semibold tracking-wide text-muted-foreground uppercase">
      <ChevronDown className="size-3" />
      {icon}
      {children}
    </header>
  );
}

function propertiesLabel(state: BlockState): string {
  const entries = Object.entries(state.properties ?? {});
  if (entries.length === 0) return "—";
  return entries.map(([key, value]) => `${key}=${value}`).join(", ");
}

function chunkLabel(x: number, z: number): string {
  return `${Math.floor(x / 16)}, ${Math.floor(z / 16)}`;
}

export function Inspector() {
  const selection = useWorldStore((state) => state.selection);
  const revision = useWorldStore((state) => state.revision);
  const blockCount = useWorldStore((state) => state.blockCount);
  const chunkCount = useWorldStore((state) => state.chunkCount);
  const dimension = useWorldStore((state) => state.dimension);
  const source = useWorldStore((state) => state.source);
  const state = selection?.state ?? { name: "minecraft:air", properties: {} };
  const properties = Object.entries(state.properties ?? {});

  return (
    <aside className="flex min-h-0 w-64 shrink-0 flex-col border-l border-border bg-sidebar text-sidebar-foreground">
      <div className="flex h-9 items-center gap-1.5 border-b border-border px-2.5">
        <SlidersHorizontal className="size-3.5 text-muted-foreground" />
        <span className="text-[11px] font-semibold tracking-wide uppercase">Inspector</span>
      </div>

      <div className="scrollbar-thin min-h-0 flex-1 overflow-y-auto">
        <section className="border-b border-border py-1.5">
          <SectionHeader icon={<Box className="size-3" />}>Block state</SectionHeader>
          <div className="mx-2 flex items-center gap-2 rounded border border-border bg-card/50 p-2">
            <div
              className="size-8 shrink-0 rounded border border-black/30 bg-[length:10px_10px]"
              style={{
                backgroundColor: blockColor(state.name),
                backgroundImage:
                  "repeating-conic-gradient(rgba(0,0,0,0.12) 0% 25%, transparent 0% 50%)",
              }}
            />
            <div className="min-w-0">
              <div className="truncate font-mono text-[11px]">{state.name}</div>
              <div className="truncate text-[10px] text-muted-foreground">
                {selection ? "Selected block" : "No block selected"}
              </div>
            </div>
          </div>
          <div className="mt-1">
            {properties.length > 0 ? (
              properties.map(([key, value]) => <Field key={key} label={key} value={value} />)
            ) : (
              <Field label="Properties" value={propertiesLabel(state)} />
            )}
            <Field
              label="Position"
              value={
                selection ? `${selection.pos.x}, ${selection.pos.y}, ${selection.pos.z}` : "—, —, —"
              }
            />
            <Field
              label="Chunk"
              value={selection ? chunkLabel(selection.pos.x, selection.pos.z) : "—"}
            />
            <Field label="Face" value={selection ? `[${selection.face.join(", ")}]` : "—"} />
          </div>
        </section>

        <section className="border-b border-border py-1.5">
          <SectionHeader icon={<Layers className="size-3" />}>Selection</SectionHeader>
          <div className="px-2.5 py-1">
            {selection ? (
              <div className="rounded border border-primary/40 bg-accent/30 px-2 py-2">
                <div className="font-mono text-[10px]">
                  {selection.pos.x}, {selection.pos.y}, {selection.pos.z}
                </div>
                <div className="mt-1 truncate font-mono text-[9px] text-muted-foreground">
                  {selection.dimension}
                </div>
              </div>
            ) : (
              <div className="rounded border border-dashed border-border px-2 py-3 text-center text-[10px] text-muted-foreground">
                Click the viewport to pick a block
              </div>
            )}
            <div className="mt-1 flex justify-between font-mono text-[9px] text-muted-foreground">
              <span>{selection ? "1 block" : "0 blocks"}</span>
              <span>{selection ? "1 chunk" : "0 chunks"}</span>
            </div>
          </div>
        </section>

        <section className="border-b border-border py-1.5">
          <SectionHeader icon={<Box className="size-3" />}>Project</SectionHeader>
          <Field label="Mode" value="Empty Project (demo scene)" />
          <Field label="Minecraft" value="1.21.4" />
          <Field label="DataVersion" value="4189" />
          <Field label="Dimension" value={dimension} />
          <Field label="Source" value={source === "core" ? "Rust core" : "Preview data"} />
          <Field label="Revision" value={String(revision)} />
          <Field label="Chunks" value={String(chunkCount)} />
          <Field label="Blocks" value={blockCount.toLocaleString()} />
        </section>

        <section className="py-2">
          <SectionHeader icon={<Sparkles className="size-3" />}>AI actions</SectionHeader>
          <p className="px-2.5 text-[10px] leading-relaxed text-muted-foreground">
            The viewport already exposes revisioned render snapshots. MCP tools become available
            after the operation API and bridge milestones.
          </p>
        </section>
      </div>

      <Separator />
      <div className="px-2.5 py-2 text-[9px] text-muted-foreground">
        Values shown here are read from the Rust world core and revisioned render snapshots.
      </div>
    </aside>
  );
}
