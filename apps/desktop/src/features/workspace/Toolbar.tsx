import { Flame, Redo2, Save, Settings, Sun, Moon, Undo2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Separator } from "@/components/ui/separator";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import type { BackendStatus } from "@/lib/contracts";
import { redoWorld, undoWorld } from "@/editor/commands";
import { blockColor } from "@/renderer/assets";
import { useHistoryStore } from "@/state/history-store";
import { useToolStore } from "@/state/tool-store";
import { cn } from "@/lib/utils";

const menus = ["Project", "View", "Tools", "AI / MCP", "Export"] as const;

interface ToolbarProps {
  backendStatus: BackendStatus;
  theme: "dark" | "light" | "system";
  onToggleTheme: () => void;
  onOpenSettings: () => void;
}

export function Toolbar({ backendStatus, theme, onToggleTheme, onOpenSettings }: ToolbarProps) {
  const canUndo = useHistoryStore((state) => state.canUndo);
  const canRedo = useHistoryStore((state) => state.canRedo);
  const job = useToolStore((state) => state.job);
  const syncState = useToolStore((state) => state.syncState);
  const selectedBlock = useToolStore((state) => state.selectedBlock);
  const editing = backendStatus === "online";
  const busy = Boolean(job);

  return (
    <header className="flex h-10 shrink-0 items-center gap-1 border-b border-border bg-card/70 px-2">
      <div className="flex items-center gap-2 pr-2">
        <div className="grid size-6 place-items-center rounded-md bg-primary text-primary-foreground shadow-sm">
          <Flame className="size-3.5" />
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
        {menus.map((menu) => (
          <Tooltip key={menu}>
            <TooltipTrigger asChild>
              <Button variant="ghost" size="sm" className="text-muted-foreground" disabled>
                {menu}
              </Button>
            </TooltipTrigger>
            <TooltipContent>
              {menu} menus arrive with later milestones. Editing actions live in the viewport
              toolbar and context menu.
            </TooltipContent>
          </Tooltip>
        ))}
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
        <Tooltip>
          <TooltipTrigger asChild>
            <span
              className={cn(
                "mr-1 inline-flex items-center gap-1.5 rounded-full border px-2 py-0.5 text-[10px]",
                syncState === "modified"
                  ? "border-status-warn/30 text-status-warn"
                  : "border-border text-muted-foreground",
              )}
            >
              <span
                className={cn(
                  "size-1.5 rounded-full",
                  syncState === "modified" ? "bg-status-warn" : "bg-status-ok",
                )}
              />
              {syncState === "modified"
                ? "Unsaved changes"
                : editing
                  ? "No unsaved edits"
                  : "Preview scene"}
            </span>
          </TooltipTrigger>
          <TooltipContent>
            Project persistence arrives in M5. Edits are kept in the running Rust session and export
            remembers the revision.
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
            <Button variant="ghost" size="icon" aria-label="Save project" disabled>
              <Save />
            </Button>
          </TooltipTrigger>
          <TooltipContent>
            Save project — persistence lands in M5. Current edits live in memory only.
          </TooltipContent>
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
