import type { ReactNode } from "react";
import {
  Box,
  Copy,
  Layers,
  Paintbrush,
  PanelRightClose,
  SlidersHorizontal,
  Sparkles,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from "@/components/ui/empty";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Separator } from "@/components/ui/separator";
import { copySelection, pasteClipboard } from "@/editor/commands";
import { adjacentPosition, boundsVolume } from "@/editor/session";
import { isTauriRuntime } from "@/lib/ipc";
import { creationModeLabel } from "@/project/types";
import { blockColor } from "@/renderer/assets";
import type { BlockState } from "@/renderer/types";
import { projectIsDirty, useProjectStore } from "@/state/project-store";
import { useEditorStore } from "@/state/editor-store";
import { useToolStore } from "@/state/tool-store";
import { useWorldStore } from "@/state/world-store";

function Field({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-center justify-between gap-3 py-1.5">
      <span className="shrink-0 text-sm text-muted-foreground">{label}</span>
      <span className="truncate font-mono text-xs" title={value}>
        {value}
      </span>
    </div>
  );
}

function SectionHeader({ icon, children }: { icon: ReactNode; children: ReactNode }) {
  return (
    <header className="mb-3 flex items-center gap-2 text-xs font-medium tracking-wide text-muted-foreground uppercase">
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
  const setPanelOpen = useEditorStore((state) => state.setPanelOpen);
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
    <aside className="flex min-h-0 w-80 shrink-0 flex-col border-l bg-sidebar text-sidebar-foreground">
      <div className="flex h-12 items-center gap-2 border-b px-4">
        <SlidersHorizontal className="size-4 text-muted-foreground" />
        <span className="text-sm font-medium">Inspector</span>
        <Button
          variant="ghost"
          size="icon-sm"
          className="ml-auto"
          aria-label="Hide inspector"
          title="Hide inspector"
          onClick={() => setPanelOpen("right", false)}
        >
          <PanelRightClose />
        </Button>
      </div>

      <ScrollArea className="min-h-0 flex-1">
        <div className="flex flex-col">
          <section className="border-b p-4">
            <SectionHeader icon={<Box className="size-4" />}>Block state</SectionHeader>
            <div className="flex items-center gap-3 rounded-lg border bg-card p-3">
              <div
                className="size-10 shrink-0 rounded-md border border-black/30 bg-[length:10px_10px]"
                style={{
                  backgroundColor: blockColor(state.name),
                  backgroundImage:
                    "repeating-conic-gradient(rgba(0,0,0,0.12) 0% 25%, transparent 0% 50%)",
                }}
              />
              <div className="min-w-0">
                <div className="truncate font-mono text-xs">{state.name}</div>
                <div className="truncate text-sm text-muted-foreground">
                  {selection ? "Selected block" : "No block selected"}
                </div>
              </div>
            </div>
            <div className="mt-3">
              {properties.length > 0 ? (
                properties.map(([key, value]) => <Field key={key} label={key} value={value} />)
              ) : (
                <Field label="Properties" value={propertiesLabel(state)} />
              )}
              <Field
                label="Position"
                value={
                  selection
                    ? `${selection.pos.x}, ${selection.pos.y}, ${selection.pos.z}`
                    : "—, —, —"
                }
              />
              <Field
                label="Chunk"
                value={selection ? chunkLabel(selection.pos.x, selection.pos.z) : "—"}
              />
              <Field label="Face" value={selection ? `[${selection.face.join(", ")}]` : "—"} />
            </div>
          </section>

          <section className="border-b p-4">
            <SectionHeader icon={<Layers className="size-4" />}>Selection</SectionHeader>
            {boxSelection ? (
              <div className="rounded-lg border bg-muted/40 p-3">
                <div className="font-mono text-xs">
                  {boxSelection.min.x},{boxSelection.min.y},{boxSelection.min.z} →{" "}
                  {boxSelection.max.x},{boxSelection.max.y},{boxSelection.max.z}
                </div>
                <div className="mt-2 flex items-center justify-between text-xs text-muted-foreground">
                  <span>{boundsVolume(boxSelection).toLocaleString()} blocks</span>
                  <Button variant="link" size="xs" onClick={() => setSelection(null)}>
                    Clear
                  </Button>
                </div>
              </div>
            ) : (
              <Empty className="border-0 p-4">
                <EmptyHeader>
                  <EmptyMedia variant="icon">
                    <Layers />
                  </EmptyMedia>
                  <EmptyTitle>Nothing selected</EmptyTitle>
                  <EmptyDescription>
                    Use the Box select tool (5) and click two corners.
                  </EmptyDescription>
                </EmptyHeader>
              </Empty>
            )}
            <div className="mt-3 flex flex-wrap gap-2">
              <Button
                variant="outline"
                size="sm"
                disabled={!boxSelection || !editing}
                onClick={() => void copySelection()}
              >
                <Copy data-icon="inline-start" />
                Copy
              </Button>
              <Button
                variant="outline"
                size="sm"
                disabled={!clipboard || !editing}
                onClick={() => void pasteClipboard()}
              >
                {pendingPaste && pastePreviewReady
                  ? `Confirm paste (${pendingPaste.blocks})`
                  : `Paste${clipboard ? ` (${clipboard.blocks.length})` : ""}`}
              </Button>
            </div>
            {clipboard ? (
              <div className="mt-3 rounded-lg border bg-muted/30 p-2.5 font-mono text-xs text-muted-foreground">
                Clipboard {clipboard.size[0]}×{clipboard.size[1]}×{clipboard.size[2]} ·{" "}
                {clipboard.blocks.length.toLocaleString()} blocks
                {pendingPaste
                  ? ` · preview ${pendingPaste.bounds.min.x},${pendingPaste.bounds.min.y},${pendingPaste.bounds.min.z} → ${pendingPaste.bounds.max.x},${pendingPaste.bounds.max.y},${pendingPaste.bounds.max.z}`
                  : ""}
              </div>
            ) : null}
            {pastePreviewReady ? (
              <div className="mt-2 text-xs text-status-warn">
                Large paste preview. Press Ctrl+V again or click Confirm paste to apply; Esc cancels
                without writing.
              </div>
            ) : null}
          </section>

          <section className="border-b p-4">
            <SectionHeader icon={<Paintbrush className="size-4" />}>Edit target</SectionHeader>
            <div className="flex items-center gap-2.5 rounded-lg border bg-card px-3 py-2.5">
              <span
                className="size-4 shrink-0 rounded-sm border border-black/30"
                style={{ backgroundColor: blockColor(selectedBlock.name) }}
              />
              <span className="min-w-0 flex-1 truncate font-mono text-xs">
                {selectedBlock.name.replace("minecraft:", "")}
              </span>
              <Badge variant="secondary">brush</Badge>
            </div>
            <div className="mt-3 flex flex-wrap gap-2">
              <Button
                variant="outline"
                size="sm"
                disabled={!selection}
                onClick={() => selection && setSelectedBlock(selection.state)}
              >
                Use picked as brush
              </Button>
              <Button
                variant="outline"
                size="sm"
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
          </section>

          <section className="border-b p-4">
            <SectionHeader icon={<Box className="size-4" />}>Project</SectionHeader>
            <Field label="Name" value={project?.name ?? "No project open"} />
            <Field label="Folder" value={project?.folder ?? "—"} />
            <Field
              label="Mode"
              value={projectSummary ? creationModeLabel(projectSummary.creationMode) : "—"}
            />
            <Field label="Minecraft" value={projectSummary?.targetMinecraftVersion ?? "1.21.4"} />
            <Field label="DataVersion" value={String(projectSummary?.targetDataVersion ?? 4189)} />
            <Field
              label="Schema"
              value={projectSummary ? `v${projectSummary.schemaVersion}` : "—"}
            />
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

          <section className="p-4">
            <SectionHeader icon={<Sparkles className="size-4" />}>AI actions</SectionHeader>
            <p className="text-sm leading-relaxed text-muted-foreground">
              The same Operation API will be exposed to MCP tools after the bridge milestone. Every
              edit is revisioned, audited and undoable.
            </p>
          </section>
        </div>
      </ScrollArea>

      <Separator />
      <div className="px-4 py-3 text-xs text-muted-foreground">
        Values shown here are read from the Rust world core and the versioned project manifest.
      </div>
    </aside>
  );
}
