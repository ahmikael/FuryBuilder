import { ChevronDown, SlidersHorizontal, Sparkles } from "lucide-react";
import { Separator } from "@/components/ui/separator";

function Field({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-center justify-between gap-2 px-2.5 py-1">
      <span className="text-[11px] text-muted-foreground">{label}</span>
      <span className="truncate font-mono text-[10px]">{value}</span>
    </div>
  );
}

export function Inspector() {
  return (
    <aside className="flex min-h-0 w-64 shrink-0 flex-col border-l border-border bg-sidebar text-sidebar-foreground">
      <div className="flex h-9 items-center gap-1.5 border-b border-border px-2.5">
        <SlidersHorizontal className="size-3.5 text-muted-foreground" />
        <span className="text-[11px] font-semibold tracking-wide uppercase">Inspector</span>
      </div>

      <div className="scrollbar-thin min-h-0 flex-1 overflow-y-auto">
        <section className="border-b border-border py-1.5">
          <header className="flex items-center gap-1 px-2 py-1 text-[10px] font-semibold tracking-wide text-muted-foreground uppercase">
            <ChevronDown className="size-3" /> Block state
          </header>
          <div className="mx-2 flex items-center gap-2 rounded border border-border bg-card/50 p-2">
            <div className="size-8 rounded border border-border bg-[repeating-conic-gradient(oklch(0.4_0_0)_0%_25%,oklch(0.3_0_0)_0%_50%)] bg-[length:10px_10px]" />
            <div className="min-w-0">
              <div className="truncate font-mono text-[11px]">minecraft:air</div>
              <div className="text-[10px] text-muted-foreground">No block selected</div>
            </div>
          </div>
          <div className="mt-1">
            <Field label="Properties" value="—" />
            <Field label="Position" value="—, —, —" />
            <Field label="Chunk" value="—" />
          </div>
        </section>

        <section className="border-b border-border py-1.5">
          <header className="flex items-center gap-1 px-2 py-1 text-[10px] font-semibold tracking-wide text-muted-foreground uppercase">
            <ChevronDown className="size-3" /> Selection
          </header>
          <div className="px-2.5 py-1">
            <div className="rounded border border-dashed border-border px-2 py-3 text-center text-[10px] text-muted-foreground">
              Nothing selected
            </div>
            <div className="mt-1 flex justify-between font-mono text-[9px] text-muted-foreground">
              <span>0 blocks</span>
              <span>0 chunks</span>
            </div>
          </div>
        </section>

        <section className="border-b border-border py-1.5">
          <header className="flex items-center gap-1 px-2 py-1 text-[10px] font-semibold tracking-wide text-muted-foreground uppercase">
            <ChevronDown className="size-3" /> Project
          </header>
          <Field label="Mode" value="empty" />
          <Field label="Minecraft" value="1.21.4" />
          <Field label="DataVersion" value="4189" />
          <Field label="Revision" value="0" />
        </section>

        <section className="py-2">
          <header className="flex items-center gap-1 px-2 py-1 text-[10px] font-semibold tracking-wide text-muted-foreground uppercase">
            <Sparkles className="size-3" /> AI actions
          </header>
          <p className="px-2.5 text-[10px] leading-relaxed text-muted-foreground">
            MCP tools become available after the operation API and bridge milestones.
          </p>
        </section>
      </div>

      <Separator />
      <div className="px-2.5 py-2 text-[9px] text-muted-foreground">
        Values shown here will always be read from the Rust world core.
      </div>
    </aside>
  );
}
