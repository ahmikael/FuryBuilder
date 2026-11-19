import { Search } from "lucide-react";
import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
} from "react";

import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { redoWorld, undoWorld } from "@/editor/commands";
import {
  BOTTOM_PANEL_TABS,
  COMMAND_PALETTE_SHORTCUT,
  PANEL_LABELS,
  PANEL_SHORTCUTS,
  type BottomPanelTabId,
} from "@/editor/layout";
import { TOOL_LABELS, TOOL_SHORTCUTS } from "@/editor/session";
import type { EditorTool } from "@/editor/types";
import { focusViewportPosition } from "@/features/viewport/viewport-bus";
import { useEditorStore, type PanelId, type ThemeMode } from "@/state/editor-store";
import { useProjectStore } from "@/state/project-store";
import { useToolStore } from "@/state/tool-store";
import { useUiStore } from "@/state/ui-store";
import { cn } from "@/lib/utils";

export interface PaletteCommand {
  id: string;
  label: string;
  group: string;
  keywords: string;
  detail?: string | undefined;
  shortcut?: string | undefined;
  run: () => void;
}

const TOOLS: EditorTool[] = [
  "select",
  "place",
  "remove",
  "brush",
  "box",
  "fill",
  "line",
  "replace",
];

const THEMES: Array<{ value: ThemeMode; label: string; keywords: string }> = [
  { value: "dark", label: "Use dark theme", keywords: "appearance colors night" },
  { value: "light", label: "Use light theme", keywords: "appearance colors day" },
  { value: "system", label: "Use system theme", keywords: "appearance automatic os" },
];

function normalize(value: string): string {
  return value
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, " ")
    .trim();
}

function coordinateCommand(query: string): PaletteCommand | null {
  const match = query.trim().match(/^(-?\d{1,9})[\s,]+(-?\d{1,9})[\s,]+(-?\d{1,9})$/);
  if (!match) return null;
  const values = match.slice(1).map(Number);
  const x = values[0] ?? 0;
  const y = values[1] ?? 0;
  const z = values[2] ?? 0;
  if (![x, y, z].every((value) => Number.isSafeInteger(value))) return null;
  return {
    id: "go-to-coordinates",
    group: "World",
    label: `Go to ${x}, ${y}, ${z}`,
    keywords: "focus camera coordinates position navigate teleport",
    detail: "Move the camera",
    run: () => focusViewportPosition({ x, y, z }),
  };
}

function toolShortcut(tool: EditorTool): string | undefined {
  const code = Object.entries(TOOL_SHORTCUTS).find(([, value]) => value === tool)?.[0];
  return code?.replace("Digit", "");
}

export function buildPaletteCommands(context: {
  theme: ThemeMode;
  activeTool: EditorTool;
  bottomPanelTab: BottomPanelTabId;
  panels: Record<PanelId, boolean>;
  togglePanel: (panel: PanelId) => void;
  setTheme: (theme: ThemeMode) => void;
  setBottomPanelTab: (tab: BottomPanelTabId) => void;
  resetLayout: () => void;
  setManagerOpen: (open: boolean) => void;
  saveProject: () => void;
}): PaletteCommand[] {
  const commands: PaletteCommand[] = [];

  for (const panel of ["left", "right", "bottom"] as const) {
    const open = context.panels[panel];
    commands.push({
      id: `toggle-panel-${panel}`,
      group: "Layout",
      label: `${open ? "Hide" : "Show"} ${PANEL_LABELS[panel]}`,
      keywords: `panel layout toggle ${panel} sidebar`,
      shortcut: PANEL_SHORTCUTS[panel],
      detail: open ? "Visible" : "Hidden",
      run: () => context.togglePanel(panel),
    });
  }
  commands.push({
    id: "reset-layout",
    group: "Layout",
    label: "Reset panel layout",
    keywords: "layout panels restore default",
    detail: "Show all panels",
    run: context.resetLayout,
  });

  for (const theme of THEMES) {
    const active = context.theme === theme.value;
    commands.push({
      id: `theme-${theme.value}`,
      group: "Appearance",
      label: theme.label,
      keywords: theme.keywords,
      detail: active ? "Active" : undefined,
      run: () => context.setTheme(theme.value),
    });
  }

  for (const tab of BOTTOM_PANEL_TABS) {
    const active = context.bottomPanelTab === tab.id;
    commands.push({
      id: `open-tab-${tab.id}`,
      group: "Bottom panel",
      label: `Open ${tab.label}`,
      keywords: `tab panel ${tab.label}`,
      detail: active ? "Active" : undefined,
      run: () => context.setBottomPanelTab(tab.id),
    });
  }

  for (const tool of TOOLS) {
    const active = context.activeTool === tool;
    commands.push({
      id: `tool-${tool}`,
      group: "Tools",
      label: `Use ${TOOL_LABELS[tool]} tool`,
      keywords: `tool mode ${tool}`,
      detail: active ? "Active" : undefined,
      shortcut: toolShortcut(tool),
      run: () => useToolStore.getState().setTool(tool),
    });
  }

  commands.push(
    {
      id: "focus-origin",
      group: "World",
      label: "Focus world origin",
      keywords: "camera view navigate position spawn",
      run: () => focusViewportPosition({ x: 0, y: 64, z: 0 }),
    },
    {
      id: "project-manager",
      group: "Project",
      label: "Open project manager",
      keywords: "project open create rename checkpoint",
      run: () => context.setManagerOpen(true),
    },
    {
      id: "save-project",
      group: "Project",
      label: "Save project",
      keywords: "persist write save",
      shortcut: "Ctrl+S",
      run: context.saveProject,
    },
    {
      id: "undo",
      group: "Project",
      label: "Undo edit",
      keywords: "history revert undo",
      shortcut: "Ctrl+Z",
      run: () => void undoWorld(),
    },
    {
      id: "redo",
      group: "Project",
      label: "Redo edit",
      keywords: "history repeat redo",
      shortcut: "Ctrl+Shift+Z",
      run: () => void redoWorld(),
    },
    {
      id: "open-settings",
      group: "Project",
      label: "Open settings",
      keywords: "preferences appearance assets java",
      run: () => useUiStore.getState().setSettingsOpen(true),
    },
    {
      id: "open-mcp-access",
      group: "Project",
      label: "Open MCP access",
      keywords: "ai mcp permissions tools confirmation",
      run: () => useUiStore.getState().setMcpAccessOpen(true),
    },
  );

  return commands;
}

export function CommandPalette() {
  const open = useUiStore((state) => state.commandPaletteOpen);
  const setOpen = useUiStore((state) => state.setCommandPaletteOpen);
  const theme = useEditorStore((state) => state.theme);
  const setTheme = useEditorStore((state) => state.setTheme);
  const leftPanelOpen = useEditorStore((state) => state.leftPanelOpen);
  const rightPanelOpen = useEditorStore((state) => state.rightPanelOpen);
  const bottomPanelOpen = useEditorStore((state) => state.bottomPanelOpen);
  const bottomPanelTab = useEditorStore((state) => state.bottomPanelTab);
  const togglePanel = useEditorStore((state) => state.togglePanel);
  const setBottomPanelTab = useEditorStore((state) => state.setBottomPanelTab);
  const resetLayout = useEditorStore((state) => state.resetLayout);
  const activeTool = useToolStore((state) => state.tool);
  const setManagerOpen = useProjectStore((state) => state.setManagerOpen);
  const saveProject = useProjectStore((state) => state.save);
  const [query, setQuery] = useState("");
  const [activeIndex, setActiveIndex] = useState(0);
  const listRef = useRef<HTMLUListElement | null>(null);

  const commands = useMemo(
    () =>
      buildPaletteCommands({
        theme,
        activeTool,
        bottomPanelTab,
        panels: { left: leftPanelOpen, right: rightPanelOpen, bottom: bottomPanelOpen },
        togglePanel,
        setTheme,
        setBottomPanelTab,
        resetLayout,
        setManagerOpen,
        saveProject: () => void saveProject(),
      }),
    [
      theme,
      activeTool,
      bottomPanelTab,
      leftPanelOpen,
      rightPanelOpen,
      bottomPanelOpen,
      togglePanel,
      setTheme,
      setBottomPanelTab,
      resetLayout,
      setManagerOpen,
      saveProject,
    ],
  );

  const filtered = useMemo(() => {
    const tokens = normalize(query).split(" ").filter(Boolean);
    const matches = commands.filter((command) => {
      if (tokens.length === 0) return true;
      const haystack = normalize(
        `${command.group} ${command.label} ${command.keywords} ${command.detail ?? ""}`,
      );
      return tokens.every((token) => haystack.includes(token));
    });
    const coordinate = coordinateCommand(query);
    return coordinate ? [coordinate, ...matches] : matches;
  }, [commands, query]);

  const active = filtered.length === 0 ? -1 : Math.min(activeIndex, filtered.length - 1);

  useEffect(() => {
    if (active < 0) return;
    const node = listRef.current?.querySelector('[data-active="true"]');
    node?.scrollIntoView?.({ block: "nearest" });
  }, [active]);

  const closePalette = (): void => {
    setOpen(false);
    setQuery("");
    setActiveIndex(0);
  };

  const execute = (command: PaletteCommand | undefined): void => {
    if (!command) return;
    closePalette();
    command.run();
  };

  const onInputKeyDown = (event: ReactKeyboardEvent<HTMLInputElement>): void => {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      if (filtered.length > 0) setActiveIndex((active + 1) % filtered.length);
      return;
    }
    if (event.key === "ArrowUp") {
      event.preventDefault();
      if (filtered.length > 0) {
        setActiveIndex((active - 1 + filtered.length) % filtered.length);
      }
      return;
    }
    if (event.key === "Home") {
      event.preventDefault();
      setActiveIndex(0);
      return;
    }
    if (event.key === "End") {
      event.preventDefault();
      setActiveIndex(Math.max(0, filtered.length - 1));
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      execute(filtered[active]);
    }
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) closePalette();
        else setOpen(true);
      }}
    >
      <DialogContent
        showCloseButton={false}
        className="top-[18%] max-w-xl -translate-y-0 gap-2 p-2"
      >
        <DialogTitle className="sr-only">Command palette</DialogTitle>
        <DialogDescription className="sr-only">
          Search editor commands, panels, themes, tools and coordinates. Use the arrow keys to
          choose a command and Enter to run it.
        </DialogDescription>

        <div className="relative">
          <Search className="absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-muted-foreground" />
          <Input
            autoFocus
            value={query}
            onChange={(event) => {
              setQuery(event.target.value);
              setActiveIndex(0);
            }}
            onKeyDown={onInputKeyDown}
            placeholder="Search commands, panels, themes or “x y z”…"
            aria-label="Search commands"
            role="combobox"
            aria-expanded="true"
            aria-controls="command-palette-list"
            aria-autocomplete="list"
            aria-activedescendant={
              active >= 0 ? `command-palette-option-${filtered[active]?.id}` : undefined
            }
            className="h-8 pl-8"
          />
          <kbd className="absolute top-1/2 right-2 -translate-y-1/2 rounded border border-border bg-muted px-1 font-mono text-[9px] text-muted-foreground">
            {COMMAND_PALETTE_SHORTCUT}
          </kbd>
        </div>

        <div className="scrollbar-thin max-h-80 min-h-12 overflow-y-auto rounded border border-border bg-background/40">
          {filtered.length === 0 ? (
            <div className="p-3 text-[11px] text-muted-foreground">
              No command matches “{query}”. Try “panel”, “theme”, “palette” or coordinates like “0
              64 0”.
            </div>
          ) : (
            <ul
              id="command-palette-list"
              ref={listRef}
              role="listbox"
              aria-label="Commands"
              className="py-1"
            >
              {filtered.map((command, index) => {
                const showGroup = index === 0 || filtered[index - 1]?.group !== command.group;
                return (
                  <li key={command.id} role="presentation">
                    {showGroup ? (
                      <div
                        role="presentation"
                        className="px-2 pt-2 pb-0.5 text-[9px] font-semibold tracking-wider text-muted-foreground uppercase"
                      >
                        {command.group}
                      </div>
                    ) : null}
                    <button
                      type="button"
                      id={`command-palette-option-${command.id}`}
                      role="option"
                      aria-selected={index === active}
                      data-active={index === active}
                      onMouseEnter={() => setActiveIndex(index)}
                      onClick={() => execute(command)}
                      className={cn(
                        "flex w-full items-center gap-2 px-2 py-1.5 text-left text-[11px]",
                        index === active
                          ? "bg-accent text-accent-foreground"
                          : "text-foreground/80 hover:bg-accent/40",
                      )}
                    >
                      <span className="truncate">{command.label}</span>
                      {command.detail ? (
                        <span className="truncate text-[9px] text-muted-foreground">
                          {command.detail}
                        </span>
                      ) : null}
                      {command.shortcut ? (
                        <kbd className="ml-auto shrink-0 rounded border border-border bg-muted px-1 font-mono text-[9px] text-muted-foreground">
                          {command.shortcut}
                        </kbd>
                      ) : null}
                    </button>
                  </li>
                );
              })}
            </ul>
          )}
        </div>

        <div className="flex items-center gap-3 border-t border-border pt-1 text-[9px] text-muted-foreground">
          <span>↑↓ Navigate</span>
          <span>Enter Run</span>
          <span>Esc Close</span>
          <span className="ml-auto font-mono">{filtered.length} command(s)</span>
        </div>
      </DialogContent>
    </Dialog>
  );
}
