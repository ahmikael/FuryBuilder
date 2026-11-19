import { Check, Command, RotateCcw } from "lucide-react";
import { useEffect, useRef, useState, type ReactNode } from "react";

import { Button } from "@/components/ui/button";
import { COMMAND_PALETTE_SHORTCUT, PANEL_LABELS, PANEL_SHORTCUTS } from "@/editor/layout";
import { useEditorStore, type PanelId } from "@/state/editor-store";
import { useUiStore } from "@/state/ui-store";
import { cn } from "@/lib/utils";

function ViewItem({
  label,
  shortcut,
  checked,
  icon,
  onSelect,
}: {
  label: string;
  shortcut?: string;
  checked?: boolean;
  icon?: ReactNode;
  onSelect: () => void;
}) {
  return (
    <button
      type="button"
      role={checked === undefined ? "menuitem" : "menuitemcheckbox"}
      aria-checked={checked}
      onClick={onSelect}
      className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-[11px] outline-none hover:bg-accent focus-visible:bg-accent"
    >
      {checked === undefined ? (
        icon
      ) : (
        <span className="grid size-3.5 place-items-center">
          {checked ? <Check className="size-3.5 text-primary" /> : null}
        </span>
      )}
      <span className="truncate">{label}</span>
      {shortcut ? (
        <kbd className="ml-auto shrink-0 rounded border border-border bg-muted px-1 font-mono text-[9px] text-muted-foreground">
          {shortcut}
        </kbd>
      ) : null}
    </button>
  );
}

export function ViewMenu() {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement | null>(null);
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

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent): void => {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false);
    };
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.key === "Escape") setOpen(false);
    };
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [open]);

  return (
    <div className="relative" ref={rootRef}>
      <Button
        variant="ghost"
        size="sm"
        className={cn("text-muted-foreground", open && "bg-accent text-accent-foreground")}
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => setOpen((value) => !value)}
      >
        View
      </Button>
      {open ? (
        <div
          role="menu"
          aria-label="View menu"
          className="absolute top-full left-0 z-50 mt-1 w-60 rounded-md border border-border bg-card p-1 text-card-foreground shadow-xl"
        >
          {(["left", "right", "bottom"] as const).map((panel) => (
            <ViewItem
              key={panel}
              label={PANEL_LABELS[panel]}
              checked={visible[panel]}
              shortcut={PANEL_SHORTCUTS[panel]}
              onSelect={() => togglePanel(panel)}
            />
          ))}
          <div className="my-1 h-px bg-border" role="separator" />
          <ViewItem
            label="Reset panel layout"
            icon={<RotateCcw className="size-3.5 text-muted-foreground" />}
            onSelect={resetLayout}
          />
          <ViewItem
            label="Command palette"
            icon={<Command className="size-3.5 text-muted-foreground" />}
            shortcut={COMMAND_PALETTE_SHORTCUT}
            onSelect={() => {
              setOpen(false);
              setCommandPaletteOpen(true);
            }}
          />
        </div>
      ) : null}
    </div>
  );
}
