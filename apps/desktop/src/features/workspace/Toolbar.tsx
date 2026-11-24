import {
  Bot,
  FolderOpen,
  Moon,
  PanelBottom,
  PanelLeft,
  PanelRight,
  Redo2,
  Save,
  Search,
  Settings,
  Sun,
  Undo2,
} from "lucide-react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useMemo, type MouseEvent } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Separator } from "@/components/ui/separator";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import type { BackendStatus } from "@/lib/contracts";
import { isTauriRuntime } from "@/lib/ipc";
import { redoWorld, undoWorld } from "@/editor/commands";
import { COMMAND_PALETTE_SHORTCUT, PANEL_SHORTCUTS } from "@/editor/layout";
import { useHistoryStore } from "@/state/history-store";
import { useEditorStore } from "@/state/editor-store";
import { projectIsDirty, useProjectStore } from "@/state/project-store";
import { useToolStore } from "@/state/tool-store";
import { useUiStore } from "@/state/ui-store";
import { useWorldStore } from "@/state/world-store";
import { formatRelativeTime } from "@/project/types";
import { cn } from "@/lib/utils";
import { ViewMenu } from "@/features/workspace/ViewMenu";
import { WindowControls } from "@/features/workspace/WindowControls";
import logoUrl from "@/assets/branding/furybuilder-logo.png";

interface ToolbarProps {
  backendStatus: BackendStatus;
  theme: "dark" | "light" | "system";
  onToggleTheme: () => void;
  onOpenSettings: () => void;
  onOpenMcpAccess: () => void;
}

export function Toolbar({
  backendStatus,
  theme,
  onToggleTheme,
  onOpenSettings,
  onOpenMcpAccess,
}: ToolbarProps) {
  const canUndo = useHistoryStore((state) => state.canUndo);
  const canRedo = useHistoryStore((state) => state.canRedo);
  const job = useToolStore((state) => state.job);
  const current = useProjectStore((state) => state.current);
  const loading = useProjectStore((state) => state.loading);
  const lastSavedAtMs = useProjectStore((state) => state.lastSavedAtMs);
  const setManagerOpen = useProjectStore((state) => state.setManagerOpen);
  const leftPanelOpen = useEditorStore((state) => state.leftPanelOpen);
  const rightPanelOpen = useEditorStore((state) => state.rightPanelOpen);
  const bottomPanelOpen = useEditorStore((state) => state.bottomPanelOpen);
  const togglePanel = useEditorStore((state) => state.togglePanel);
  const setCommandPaletteOpen = useUiStore((state) => state.setCommandPaletteOpen);
  const worldRevision = useWorldStore((state) => state.revision);
  const tauri = isTauriRuntime();
  const desktopWindow = useMemo(() => (tauri ? getCurrentWindow() : null), [tauri]);
  const projectOpen = Boolean(current?.folder);
  const editing = backendStatus === "online" && tauri && projectOpen;
  const busy = Boolean(job) || loading;
  const dirty = projectIsDirty(current, worldRevision);
  const autosaved = dirty && current?.autosavedRevision === worldRevision;

  const startDragging = (event: MouseEvent<HTMLElement>): void => {
    if (!desktopWindow || event.button !== 0) return;
    if ((event.target as HTMLElement).closest("button")) return;
    void desktopWindow.startDragging().catch(() => undefined);
  };

  const saveState = !tauri
    ? "Preview scene"
    : !projectOpen
      ? "No project open"
      : dirty
        ? autosaved
          ? "Autosaved · unsaved"
          : "Unsaved changes"
        : lastSavedAtMs
          ? `Saved ${formatRelativeTime(lastSavedAtMs)}`
          : "Saved";

  return (
    <header
      className="flex h-14 shrink-0 select-none items-center gap-1 border-b bg-background/95 px-2 backdrop-blur supports-[backdrop-filter]:bg-background/80"
      data-testid="window-drag-region"
      onMouseDown={startDragging}
      onDoubleClick={(event) => {
        if (!desktopWindow || (event.target as HTMLElement).closest("button")) return;
        event.preventDefault();
        void desktopWindow.toggleMaximize().catch(() => undefined);
      }}
    >
      <div className="flex shrink-0 items-center gap-2 px-1.5">
        <img src={logoUrl} alt="" aria-hidden="true" draggable={false} className="size-5" />
        <span className="hidden text-sm font-semibold tracking-tight sm:inline">FuryBuilder</span>
      </div>

      <nav className="flex items-center gap-0.5" aria-label="Main menu">
        <Tooltip>
          <TooltipTrigger
            render={
              <Button
                variant="ghost"
                className={cn("text-muted-foreground", projectOpen && "text-foreground")}
                disabled={!projectOpen}
                title={
                  projectOpen
                    ? undefined
                    : "Open or create a project from the welcome screen first."
                }
                onClick={() => setManagerOpen(true)}
              />
            }
          >
            <FolderOpen data-icon="inline-start" />
            Project
          </TooltipTrigger>
          <TooltipContent>
            {projectOpen
              ? "Project manager — list, create, open, rename and checkpoints. Ctrl+S saves."
              : "Open or create a project from the welcome screen first."}
          </TooltipContent>
        </Tooltip>
        <ViewMenu />
        <Tooltip>
          <TooltipTrigger
            render={
              <Button
                variant="ghost"
                className="text-muted-foreground"
                aria-label="AI / MCP"
                onClick={onOpenMcpAccess}
              />
            }
          >
            <Bot data-icon="inline-start" />
            <span className="hidden xl:inline">AI / MCP</span>
          </TooltipTrigger>
          <TooltipContent>
            MCP access mode, write limits and confirmation. Read-only by default.
          </TooltipContent>
        </Tooltip>
      </nav>

      <div className="ml-1 flex items-center gap-0.5">
        <Tooltip>
          <TooltipTrigger
            render={
              <Button
                variant="ghost"
                size="icon"
                aria-label="Undo edit"
                disabled={!editing || busy || !canUndo}
                onClick={() => void undoWorld()}
              />
            }
          >
            <Undo2 />
          </TooltipTrigger>
          <TooltipContent>Undo · Ctrl+Z</TooltipContent>
        </Tooltip>
        <Tooltip>
          <TooltipTrigger
            render={
              <Button
                variant="ghost"
                size="icon"
                aria-label="Redo edit"
                disabled={!editing || busy || !canRedo}
                onClick={() => void redoWorld()}
              />
            }
          >
            <Redo2 />
          </TooltipTrigger>
          <TooltipContent>Redo · Ctrl+Shift+Z</TooltipContent>
        </Tooltip>
      </div>

      <div className="ml-auto flex items-center gap-1">
        <div className="flex items-center gap-0.5">
          <Tooltip>
            <TooltipTrigger
              render={
                <Button
                  variant="ghost"
                  size="icon"
                  aria-label="Toggle world panel"
                  aria-pressed={leftPanelOpen}
                  onClick={() => togglePanel("left")}
                  className={cn(!leftPanelOpen && "text-muted-foreground/60")}
                />
              }
            >
              <PanelLeft />
            </TooltipTrigger>
            <TooltipContent>World panel · {PANEL_SHORTCUTS.left}</TooltipContent>
          </Tooltip>
          <Tooltip>
            <TooltipTrigger
              render={
                <Button
                  variant="ghost"
                  size="icon"
                  aria-label="Toggle bottom panel"
                  aria-pressed={bottomPanelOpen}
                  onClick={() => togglePanel("bottom")}
                  className={cn(!bottomPanelOpen && "text-muted-foreground/60")}
                />
              }
            >
              <PanelBottom />
            </TooltipTrigger>
            <TooltipContent>Bottom panel · {PANEL_SHORTCUTS.bottom}</TooltipContent>
          </Tooltip>
          <Tooltip>
            <TooltipTrigger
              render={
                <Button
                  variant="ghost"
                  size="icon"
                  aria-label="Toggle inspector"
                  aria-pressed={rightPanelOpen}
                  onClick={() => togglePanel("right")}
                  className={cn(!rightPanelOpen && "text-muted-foreground/60")}
                />
              }
            >
              <PanelRight />
            </TooltipTrigger>
            <TooltipContent>Inspector · {PANEL_SHORTCUTS.right}</TooltipContent>
          </Tooltip>
        </div>

        <Separator orientation="vertical" className="mx-1 !h-6" />

        <Badge
          variant="outline"
          className="max-w-40 gap-2 px-2.5 py-1 font-normal text-muted-foreground xl:max-w-64"
          title={
            projectOpen
              ? `Revision ${worldRevision} · saved revision ${current?.savedRevision ?? 0}.`
              : "Create or open a project to start editing."
          }
        >
          <span
            className={cn(
              "size-2 shrink-0 rounded-full",
              !projectOpen ? "bg-muted-foreground/50" : dirty ? "bg-status-warn" : "bg-status-ok",
            )}
          />
          <span className="truncate">{current?.name ?? saveState}</span>
          {projectOpen ? <span className="truncate text-muted-foreground">{saveState}</span> : null}
        </Badge>

        <Tooltip>
          <TooltipTrigger
            render={
              <Button
                variant="ghost"
                size="icon"
                aria-label="Save project"
                disabled={!projectOpen || busy}
                onClick={() => void useProjectStore.getState().save()}
              />
            }
          >
            <Save />
          </TooltipTrigger>
          <TooltipContent>
            {projectOpen
              ? `Save project · Ctrl+S${dirty ? " · unsaved changes" : ""}`
              : "Open a project before saving."}
          </TooltipContent>
        </Tooltip>

        <Tooltip>
          <TooltipTrigger
            render={
              <Button
                variant="ghost"
                size="icon"
                aria-label="Open command palette"
                onClick={() => setCommandPaletteOpen(true)}
              />
            }
          >
            <Search />
          </TooltipTrigger>
          <TooltipContent>Command palette · {COMMAND_PALETTE_SHORTCUT}</TooltipContent>
        </Tooltip>

        <Tooltip>
          <TooltipTrigger
            render={
              <Button
                variant="ghost"
                size="icon"
                aria-label="Toggle theme"
                onClick={onToggleTheme}
              />
            }
          >
            {theme === "dark" ? <Sun /> : <Moon />}
          </TooltipTrigger>
          <TooltipContent>Toggle dark / light theme.</TooltipContent>
        </Tooltip>

        <Tooltip>
          <TooltipTrigger
            render={
              <Button variant="ghost" size="icon" aria-label="Settings" onClick={onOpenSettings} />
            }
          >
            <Settings />
          </TooltipTrigger>
          <TooltipContent>Settings.</TooltipContent>
        </Tooltip>
        <WindowControls />
      </div>
    </header>
  );
}
