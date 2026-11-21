import { useMemo, useState } from "react";

import {
  Command,
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandShortcut,
} from "@/components/ui/command";
import { Kbd } from "@/components/ui/kbd";
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

  const groups = useMemo(() => {
    const grouped = new Map<string, PaletteCommand[]>();
    for (const command of filtered) {
      const entries = grouped.get(command.group) ?? [];
      entries.push(command);
      grouped.set(command.group, entries);
    }
    return [...grouped.entries()];
  }, [filtered]);

  const closePalette = (): void => {
    setOpen(false);
    setQuery("");
  };

  const execute = (command: PaletteCommand | undefined): void => {
    if (!command) return;
    closePalette();
    command.run();
  };

  return (
    <CommandDialog
      open={open}
      onOpenChange={(next) => {
        if (!next) closePalette();
        else setOpen(true);
      }}
      title="Command palette"
      description="Search editor commands, panels, themes, tools and coordinates. Use the arrow keys to choose a command and Enter to run it."
      className="top-[18%] max-w-xl"
      showCloseButton={false}
    >
      <Command shouldFilter={false} loop label="Search commands">
        <CommandInput
          autoFocus
          value={query}
          onValueChange={setQuery}
          placeholder="Search commands, panels, themes or “x y z”…"
          aria-label="Search commands"
        />
        <CommandList aria-label="Commands">
          {filtered.length === 0 ? (
            <CommandEmpty>
              No command matches “{query}”. Try “panel”, “theme”, “palette” or coordinates like “0
              64 0”.
            </CommandEmpty>
          ) : (
            groups.map(([group, entries]) => (
              <CommandGroup key={group} heading={group}>
                {entries.map((command) => (
                  <CommandItem
                    key={command.id}
                    value={command.id}
                    onSelect={() => execute(command)}
                  >
                    <span className="truncate">{command.label}</span>
                    {command.detail ? (
                      <span className="truncate text-xs text-muted-foreground">
                        {command.detail}
                      </span>
                    ) : null}
                    {command.shortcut ? (
                      <CommandShortcut>{command.shortcut}</CommandShortcut>
                    ) : null}
                  </CommandItem>
                ))}
              </CommandGroup>
            ))
          )}
        </CommandList>
        <div className="flex flex-wrap items-center gap-3 border-t p-3 text-xs text-muted-foreground">
          <span className="flex items-center gap-1.5">
            <Kbd>↑</Kbd>
            <Kbd>↓</Kbd>
            Navigate
          </span>
          <span className="flex items-center gap-1.5">
            <Kbd>Enter</Kbd>
            Run
          </span>
          <span className="flex items-center gap-1.5">
            <Kbd>Esc</Kbd>
            Close
          </span>
          <span className="ml-auto font-mono">{filtered.length} command(s)</span>
          <Kbd>{COMMAND_PALETTE_SHORTCUT}</Kbd>
        </div>
      </Command>
    </CommandDialog>
  );
}
