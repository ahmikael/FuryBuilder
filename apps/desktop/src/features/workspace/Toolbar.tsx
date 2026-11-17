import { Flame, Save, Settings, Sun, Moon } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Separator } from "@/components/ui/separator";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import type { BackendStatus } from "@/lib/contracts";
import { cn } from "@/lib/utils";

const menus = ["Project", "Edit", "View", "Tools", "AI / MCP", "Export"] as const;

interface ToolbarProps {
  backendStatus: BackendStatus;
  theme: "dark" | "light" | "system";
  onToggleTheme: () => void;
  onOpenSettings: () => void;
}

export function Toolbar({ backendStatus, theme, onToggleTheme, onOpenSettings }: ToolbarProps) {
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
            <TooltipContent>{menu} commands arrive with the editor milestones.</TooltipContent>
          </Tooltip>
        ))}
      </nav>

      <div className="ml-auto flex items-center gap-1.5">
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
            <Button variant="ghost" size="icon" aria-label="Save project" disabled>
              <Save />
            </Button>
          </TooltipTrigger>
          <TooltipContent>Save project (available after M5).</TooltipContent>
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
