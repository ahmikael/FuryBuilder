import {
  FolderOpen,
  HardDrive,
  PanelBottom,
  PanelLeft,
  PanelRight,
  Redo2,
  Save,
  Search,
  Settings,
  Sun,
  Moon,
  Undo2,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Separator } from "@/components/ui/separator";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import type { BackendStatus } from "@/lib/contracts";
import { isTauriRuntime } from "@/lib/ipc";
import { redoWorld, undoWorld } from "@/editor/commands";
import { COMMAND_PALETTE_SHORTCUT, PANEL_SHORTCUTS } from "@/editor/layout";
import { blockColor } from "@/renderer/assets";
import { useHistoryStore } from "@/state/history-store";
import { useEditorStore } from "@/state/editor-store";
import { projectIsDirty, useProjectStore } from "@/state/project-store";
import { useToolStore } from "@/state/tool-store";
import { useUiStore } from "@/state/ui-store";
import { useWorldStore } from "@/state/world-store";
import { formatRelativeTime } from "@/project/types";
import { cn } from "@/lib/utils";
import { ViewMenu } from "@/features/workspace/ViewMenu";
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
  const selectedBlock = useToolStore((state) => state.selectedBlock);
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
  const projectOpen = Boolean(current?.folder);
  const editing = backendStatus === "online" && tauri && projectOpen;
  const busy = Boolean(job) || loading;
  const dirty = projectIsDirty(current, worldRevision);
  const autosaved = dirty && current?.autosavedRevision === worldRevision;

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
    <header className="flex h-10 shrink-0 items-center gap-1 border-b border-border bg-card/70 px-2">
      <div className="flex items-center gap-2 pr-2">
        <div className="grid size-6 place-items-center">
          <img
            src={logoUrl}
            alt=""
            aria-hidden="true"
            draggable={false}
            className="h-[22px] w-auto select-none"
          />
        </div>
        <div className="flex items-baseline gap-1.5">
          <span className="text-[13px] font-semibold tracking-tight">FuryBuilder</span>
          <span className="text-[9px] font-medium tracking-[0.14em] text-muted-foreground uppercase">
            alpha
          </span>
        </div>
      </div>

      <Separator orientation="vertical" className="mx-1 h-5" />

      <nav className="flex items-center" aria-label="Main menu">
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant="ghost"
              size="sm"
              className={cn("text-muted-foreground", projectOpen && "text-foreground")}
              onClick={() => setManagerOpen(true)}
            >
              <FolderOpen className="mr-1 size-3.5" />
              Project
            </Button>
          </TooltipTrigger>
          <TooltipContent>
            Project manager — list, create, open, rename and checkpoints. Ctrl+S saves.
          </TooltipContent>
        </Tooltip>
        <ViewMenu />
        <Tooltip>
          <TooltipTrigger asChild>
            <Button variant="ghost" size="sm" className="text-muted-foreground" disabled>
              Tools
            </Button>
          </TooltipTrigger>
          <TooltipContent>
            Tools menus arrive with later milestones. Editing actions live in the viewport toolbar
            and bottom panel.
          </TooltipContent>
        </Tooltip>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant="ghost"
              size="sm"
              className="text-muted-foreground"
              onClick={onOpenMcpAccess}
            >
              AI / MCP
            </Button>
          </TooltipTrigger>
          <TooltipContent>
            MCP access mode, write limits and confirmation. Read-only by default.
          </TooltipContent>
        </Tooltip>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button variant="ghost" size="sm" className="text-muted-foreground" disabled>
              Export
            </Button>
          </TooltipTrigger>
          <TooltipContent>
            Export controls live in the project manager. Supported formats and honest version limits
            are listed there.
          </TooltipContent>
        </Tooltip>
      </nav>

      <div className="ml-2 flex items-center gap-1">
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant="ghost"
              size="icon"
              aria-label="Undo edit"
              disabled={!editing || busy || !canUndo}
              onClick={() => void undoWorld()}
            >
              <Undo2 />
            </Button>
          </TooltipTrigger>
          <TooltipContent>Undo · Ctrl+Z</TooltipContent>
        </Tooltip>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant="ghost"
              size="icon"
              aria-label="Redo edit"
              disabled={!editing || busy || !canRedo}
              onClick={() => void redoWorld()}
            >
              <Redo2 />
            </Button>
          </TooltipTrigger>
          <TooltipContent>Redo · Ctrl+Shift+Z</TooltipContent>
        </Tooltip>
      </div>

      <div className="ml-auto flex items-center gap-1.5">
        <div className="flex items-center">
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                variant="ghost"
                size="icon"
                aria-label="Toggle world panel"
                aria-pressed={leftPanelOpen}
                onClick={() => togglePanel("left")}
                className={cn(!leftPanelOpen && "text-muted-foreground/60")}
              >
                <PanelLeft />
              </Button>
            </TooltipTrigger>
            <TooltipContent>World panel · {PANEL_SHORTCUTS.left}</TooltipContent>
          </Tooltip>
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                variant="ghost"
                size="icon"
                aria-label="Toggle bottom panel"
                aria-pressed={bottomPanelOpen}
                onClick={() => togglePanel("bottom")}
                className={cn(!bottomPanelOpen && "text-muted-foreground/60")}
              >
                <PanelBottom />
              </Button>
            </TooltipTrigger>
            <TooltipContent>Bottom panel · {PANEL_SHORTCUTS.bottom}</TooltipContent>
          </Tooltip>
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                variant="ghost"
                size="icon"
                aria-label="Toggle inspector"
                aria-pressed={rightPanelOpen}
                onClick={() => togglePanel("right")}
                className={cn(!rightPanelOpen && "text-muted-foreground/60")}
              >
                <PanelRight />
              </Button>
            </TooltipTrigger>
            <TooltipContent>Inspector · {PANEL_SHORTCUTS.right}</TooltipContent>
          </Tooltip>
        </div>

        <Separator orientation="vertical" className="mx-0.5 h-5" />

        <Tooltip>
          <TooltipTrigger asChild>
            <button
              type="button"
              onClick={() => setManagerOpen(true)}
              className={cn(
                "mr-1 inline-flex max-w-56 items-center gap-1.5 rounded-full border px-2 py-0.5 text-[10px]",
                !projectOpen
                  ? "border-border text-muted-foreground"
                  : dirty
                    ? "border-status-warn/40 text-status-warn"
                    : "border-status-ok/30 text-status-ok",
              )}
            >
              <span
                className={cn(
                  "size-1.5 shrink-0 rounded-full",
                  !projectOpen
                    ? "bg-muted-foreground/50"
                    : dirty
                      ? "bg-status-warn"
                      : "bg-status-ok",
                )}
              />
              <span className="truncate">{current?.name ?? saveState}</span>
              {projectOpen ? (
                <>
                  <span className="text-border">·</span>
                  <span className="truncate text-muted-foreground">{saveState}</span>
                </>
              ) : null}
            </button>
          </TooltipTrigger>
          <TooltipContent>
            {projectOpen
              ? `Revision ${worldRevision} · saved revision ${current?.savedRevision ?? 0}. Autosave runs every 30 s while edits are unsaved.`
              : "Create or open a project to start editing."}
          </TooltipContent>
        </Tooltip>

        <span
          className={cn(
            "mr-1 inline-flex items-center gap-1.5 rounded-full border border-border px-2 py-0.5 text-[10px] text-muted-foreground",
          )}
          title={
            backendStatus === "online"
              ? "Rust backend connected"
              : backendStatus === "connecting"
                ? "Connecting to Rust backend"
                : "Rust backend unavailable (browser preview)"
          }
        >
          <span
            className={cn(
              "size-1.5 rounded-full",
              backendStatus === "online" && "bg-status-ok",
              backendStatus === "connecting" && "bg-status-warn",
              backendStatus === "offline" && "bg-status-error",
            )}
          />
          {backendStatus === "online"
            ? "Core online"
            : backendStatus === "connecting"
              ? "Connecting"
              : "Preview mode"}
        </span>

        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant="ghost"
              size="sm"
              className="hidden gap-1.5 font-mono text-[10px] text-muted-foreground md:inline-flex"
              disabled
            >
              <span
                className="size-2.5 rounded-[2px] border border-black/30"
                style={{ backgroundColor: blockColor(selectedBlock.name) }}
              />
              {selectedBlock.name.replace("minecraft:", "")}
            </Button>
          </TooltipTrigger>
          <TooltipContent>Active palette block (choose it in the Palette panel).</TooltipContent>
        </Tooltip>

        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant="ghost"
              size="icon"
              aria-label="Save project"
              disabled={!projectOpen || busy}
              onClick={() => void useProjectStore.getState().save()}
            >
              <Save />
            </Button>
          </TooltipTrigger>
          <TooltipContent>
            {projectOpen
              ? `Save project · Ctrl+S${dirty ? " · unsaved changes" : ""}`
              : "Open a project before saving."}
          </TooltipContent>
        </Tooltip>

        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant="ghost"
              size="icon"
              aria-label="Project storage"
              onClick={() => setManagerOpen(true)}
            >
              <HardDrive />
            </Button>
          </TooltipTrigger>
          <TooltipContent>Project storage, checkpoints and recovery.</TooltipContent>
        </Tooltip>

        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant="ghost"
              size="icon"
              aria-label="Open command palette"
              onClick={() => setCommandPaletteOpen(true)}
            >
              <Search />
            </Button>
          </TooltipTrigger>
          <TooltipContent>Command palette · {COMMAND_PALETTE_SHORTCUT}</TooltipContent>
        </Tooltip>

        <Tooltip>
          <TooltipTrigger asChild>
            <Button variant="ghost" size="icon" aria-label="Toggle theme" onClick={onToggleTheme}>
              {theme === "dark" ? <Sun /> : <Moon />}
            </Button>
          </TooltipTrigger>
          <TooltipContent>Toggle dark / light theme.</TooltipContent>
        </Tooltip>

        <Tooltip>
          <TooltipTrigger asChild>
            <Button variant="ghost" size="icon" aria-label="Settings" onClick={onOpenSettings}>
              <Settings />
            </Button>
          </TooltipTrigger>
          <TooltipContent>Settings.</TooltipContent>
        </Tooltip>
      </div>
    </header>
  );
}
