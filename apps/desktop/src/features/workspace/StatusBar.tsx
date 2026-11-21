import { Circle, Loader2 } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Separator } from "@/components/ui/separator";
import type { BackendState } from "@/hooks/use-backend";
import { TOOL_LABELS } from "@/editor/session";
import { projectIsDirty, useProjectStore } from "@/state/project-store";
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
  const loadRadius = useWorldStore((state) => state.loadRadius);
  const source = useWorldStore((state) => state.source);
  const metrics = useWorldStore((state) => state.metrics);
  const tool = useToolStore((state) => state.tool);
  const job = useToolStore((state) => state.job);
  const selection = useToolStore((state) => state.selection);
  const project = useProjectStore((state) => state.current);
  const dirty = projectIsDirty(project, revision);
  const autosaved = dirty && project?.autosavedRevision === revision;

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
      <span className="max-w-48 truncate">{project?.name ?? "No project"}</span>
      <BarSeparator />
      <span className="hidden truncate sm:inline">
        {project?.folder ? "Project layer" : source === "core" ? "Empty core" : "Preview scene"}
      </span>
      <BarSeparator />
      <span>{dimension.replace("minecraft:", "")}</span>
      <BarSeparator />
      <span className="inline-flex items-center gap-1.5">
        {job ? <Loader2 className="size-3 animate-spin text-primary" /> : null}
        {job ? job.label : project?.folder ? `tool: ${TOOL_LABELS[tool]}` : "no project open"}
      </span>
      {selection ? (
        <>
          <BarSeparator />
          <Badge variant="secondary">box selected</Badge>
        </>
      ) : null}

      <div className="ml-auto flex items-center gap-2">
        {!project?.folder ? (
          <span>no project</span>
        ) : dirty ? (
          <span className="text-status-warn">
            {autosaved ? "autosaved · unsaved" : "unsaved changes"}
          </span>
        ) : (
          <span className="text-status-ok">saved r{project.savedRevision}</span>
        )}
        <BarSeparator />
        <span className="font-mono">rev {revision}</span>
        <BarSeparator />
        <span className="hidden font-mono md:inline">{chunkCount} chunks</span>
        <BarSeparator />
        <span className="font-mono">{blockCount.toLocaleString()} blocks</span>
        <span className="hidden items-center gap-2 lg:flex">
          <BarSeparator />
          <span className="font-mono">radius {loadRadius}</span>
          <BarSeparator />
          <span className="font-mono">
            {metrics ? `${metrics.fps} fps · ${metrics.drawCalls} draws` : "— fps"}
          </span>
          <BarSeparator />
          <span className="font-mono">
            Minecraft {backend.info?.targetMinecraftVersion ?? "1.21.4"}
          </span>
          <BarSeparator />
          <span className="font-mono">DV {backend.info?.targetDataVersion ?? 4189}</span>
        </span>
      </div>
    </footer>
  );
}
