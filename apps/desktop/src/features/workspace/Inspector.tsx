import type { ReactNode } from "react";
import {
  Box,
  ChevronDown,
  Copy,
  Layers,
  Paintbrush,
  SlidersHorizontal,
  Sparkles,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Separator } from "@/components/ui/separator";
import { copySelection, pasteClipboard } from "@/editor/commands";
import { adjacentPosition, boundsVolume } from "@/editor/session";
import { isTauriRuntime } from "@/lib/ipc";
import { creationModeLabel } from "@/project/types";
import { blockColor } from "@/renderer/assets";
import type { BlockState } from "@/renderer/types";
import { projectIsDirty, useProjectStore } from "@/state/project-store";
import { useToolStore } from "@/state/tool-store";
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
  const selectedBlock = useToolStore((state) => state.selectedBlock);
  const replaceFrom = useToolStore((state) => state.replaceFrom);
  const boxSelection = useToolStore((state) => state.selection);
  const clipboard = useToolStore((state) => state.clipboard);
  const pendingPaste = useToolStore((state) => state.pendingPaste);
  const hover = useToolStore((state) => state.hover);
  const setSelectedBlock = useToolStore((state) => state.setSelectedBlock);
  const setReplaceFrom = useToolStore((state) => state.setReplaceFrom);
  const setSelection = useToolStore((state) => state.setSelection);
  const project = useProjectStore((state) => state.current);
  const projectSummary = useProjectStore((state) =>
    state.projects.find((entry) => entry.folder === state.current?.folder),
  );
  const projectDirty = projectIsDirty(project, revision);
  const autosaved = projectDirty && project?.autosavedRevision === revision;
  const state = selection?.state ?? { name: "minecraft:air", properties: {} };
  const properties = Object.entries(state.properties ?? {});
  const editing = isTauriRuntime();
  const pasteTarget = boxSelection?.min ?? (hover ? adjacentPosition(hover.pos, hover.face) : null);
  const pastePreviewReady =
    pendingPaste !== null &&
    pasteTarget !== null &&
    pendingPaste.target.x === pasteTarget.x &&
    pendingPaste.target.y === pasteTarget.y &&
    pendingPaste.target.z === pasteTarget.z;

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
            {boxSelection ? (
              <div className="rounded border border-sky-400/40 bg-sky-400/5 px-2 py-2">
                <div className="font-mono text-[10px]">
                  {boxSelection.min.x},{boxSelection.min.y},{boxSelection.min.z} →{" "}
                  {boxSelection.max.x},{boxSelection.max.y},{boxSelection.max.z}
                </div>
                <div className="mt-1 flex justify-between font-mono text-[9px] text-muted-foreground">
                  <span>{boundsVolume(boxSelection).toLocaleString()} blocks</span>
                  <button
                    type="button"
                    className="underline-offset-2 hover:underline"
                    onClick={() => setSelection(null)}
                  >
                    clear
                  </button>
                </div>
              </div>
            ) : (
              <div className="rounded border border-dashed border-border px-2 py-3 text-center text-[10px] text-muted-foreground">
                Use the Box select tool (5) and click two corners
              </div>
            )}
            <div className="mt-1 flex flex-wrap gap-1">
              <Button
                size="sm"
                variant="outline"
                className="h-6 px-2 text-[10px]"
                disabled={!boxSelection || !editing}
                onClick={() => void copySelection()}
              >
                <Copy className="mr-1 size-3" /> Copy
              </Button>
              <Button
                size="sm"
                variant="outline"
                className="h-6 px-2 text-[10px]"
                disabled={!clipboard || !editing}
                onClick={() => void pasteClipboard()}
              >
                {pendingPaste && pastePreviewReady
                  ? `Confirm paste (${pendingPaste.blocks})`
                  : `Paste${clipboard ? ` (${clipboard.blocks.length})` : ""}`}
              </Button>
            </div>
            {clipboard ? (
              <div className="mt-1 rounded border border-border px-2 py-1 font-mono text-[9px] text-muted-foreground">
                Clipboard {clipboard.size[0]}×{clipboard.size[1]}×{clipboard.size[2]} ·{" "}
                {clipboard.blocks.length.toLocaleString()} blocks
                {pendingPaste
                  ? ` · preview ${pendingPaste.bounds.min.x},${pendingPaste.bounds.min.y},${pendingPaste.bounds.min.z} → ${pendingPaste.bounds.max.x},${pendingPaste.bounds.max.y},${pendingPaste.bounds.max.z}`
                  : ""}
              </div>
            ) : null}
            {pastePreviewReady ? (
              <div className="mt-1 text-[9px] text-status-warn">
                Large paste preview. Press Ctrl+V again or click Confirm paste to apply; Esc cancels
                without writing.
              </div>
            ) : null}
          </div>
        </section>

        <section className="border-b border-border py-1.5">
          <SectionHeader icon={<Paintbrush className="size-3" />}>Edit target</SectionHeader>
          <div className="px-2.5 py-1">
            <div className="flex items-center gap-2 rounded border border-border bg-card/50 px-2 py-1.5">
              <span
                className="size-4 shrink-0 rounded-[2px] border border-black/30"
                style={{ backgroundColor: blockColor(selectedBlock.name) }}
              />
              <span className="min-w-0 flex-1 truncate font-mono text-[10px]">
                {selectedBlock.name.replace("minecraft:", "")}
              </span>
              <span className="font-mono text-[8px] text-muted-foreground">brush</span>
            </div>
            <div className="mt-1 flex flex-wrap gap-1">
              <Button
                size="sm"
                variant="outline"
                className="h-6 px-2 text-[10px]"
                disabled={!selection}
                onClick={() => selection && setSelectedBlock(selection.state)}
              >
                Use picked as brush
              </Button>
              <Button
                size="sm"
                variant="outline"
                className="h-6 px-2 text-[10px]"
                disabled={!selection}
                onClick={() => selection && setReplaceFrom(selection.state)}
              >
                Use picked as source
              </Button>
            </div>
            <Field
              label="Replace source"
              value={replaceFrom?.name.replace("minecraft:", "") ?? "not set"}
            />
          </div>
        </section>

        <section className="border-b border-border py-1.5">
          <SectionHeader icon={<Box className="size-3" />}>Project</SectionHeader>
          <Field label="Name" value={project?.name ?? "No project open"} />
          <Field label="Folder" value={project?.folder ?? "—"} />
          <Field
            label="Mode"
            value={projectSummary ? creationModeLabel(projectSummary.creationMode) : "—"}
          />
          <Field label="Minecraft" value={projectSummary?.targetMinecraftVersion ?? "1.21.4"} />
          <Field label="DataVersion" value={String(projectSummary?.targetDataVersion ?? 4189)} />
          <Field label="Schema" value={projectSummary ? `v${projectSummary.schemaVersion}` : "—"} />
          <Field label="Dimension" value={dimension} />
          <Field label="Source" value={project ? "Project layer" : "Runtime core only"} />
          <Field label="Revision" value={String(revision)} />
          <Field label="Saved revision" value={project ? String(project.savedRevision) : "—"} />
          <Field
            label="Autosave"
            value={project?.autosavedRevision ? `r${project.autosavedRevision}` : "—"}
          />
          <Field
            label="State"
            value={
              !project?.folder
                ? "no project"
                : projectDirty
                  ? autosaved
                    ? "autosaved, not saved"
                    : "unsaved changes"
                  : "saved"
            }
          />
          <Field label="Chunks" value={String(chunkCount)} />
          <Field label="Blocks" value={blockCount.toLocaleString()} />
        </section>

        <section className="py-2">
          <SectionHeader icon={<Sparkles className="size-3" />}>AI actions</SectionHeader>
          <p className="px-2.5 text-[10px] leading-relaxed text-muted-foreground">
            The same Operation API will be exposed to MCP tools after the bridge milestone. Every
            edit is revisioned, audited and undoable.
          </p>
        </section>
      </div>

      <Separator />
      <div className="px-2.5 py-2 text-[9px] text-muted-foreground">
        Values shown here are read from the Rust world core and the versioned project manifest.
      </div>
    </aside>
  );
}
