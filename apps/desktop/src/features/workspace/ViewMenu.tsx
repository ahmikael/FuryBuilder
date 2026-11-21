import { Command, RotateCcw } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuShortcut,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { COMMAND_PALETTE_SHORTCUT, PANEL_LABELS, PANEL_SHORTCUTS } from "@/editor/layout";
import { useEditorStore, type PanelId } from "@/state/editor-store";
import { useUiStore } from "@/state/ui-store";

export function ViewMenu() {
  const leftPanelOpen = useEditorStore((state) => state.leftPanelOpen);
  const rightPanelOpen = useEditorStore((state) => state.rightPanelOpen);
  const bottomPanelOpen = useEditorStore((state) => state.bottomPanelOpen);
  const togglePanel = useEditorStore((state) => state.togglePanel);
  const resetLayout = useEditorStore((state) => state.resetLayout);
  const setCommandPaletteOpen = useUiStore((state) => state.setCommandPaletteOpen);

  const visible: Record<PanelId, boolean> = {
    left: leftPanelOpen,
    right: rightPanelOpen,
    bottom: bottomPanelOpen,
  };

  return (
    <DropdownMenu>
      <DropdownMenuTrigger render={<Button variant="ghost" className="text-muted-foreground" />}>
        View
      </DropdownMenuTrigger>
      <DropdownMenuContent className="w-60" aria-label="View menu">
        {(["left", "right", "bottom"] as const).map((panel) => (
          <DropdownMenuCheckboxItem
            key={panel}
            checked={visible[panel]}
            onCheckedChange={() => togglePanel(panel)}
          >
            {PANEL_LABELS[panel]}
            <DropdownMenuShortcut>{PANEL_SHORTCUTS[panel]}</DropdownMenuShortcut>
          </DropdownMenuCheckboxItem>
        ))}
        <DropdownMenuSeparator />
        <DropdownMenuItem onClick={resetLayout}>
          <RotateCcw />
          Reset panel layout
        </DropdownMenuItem>
        <DropdownMenuItem onClick={() => setCommandPaletteOpen(true)}>
          <Command />
          Command palette
          <DropdownMenuShortcut>{COMMAND_PALETTE_SHORTCUT}</DropdownMenuShortcut>
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
