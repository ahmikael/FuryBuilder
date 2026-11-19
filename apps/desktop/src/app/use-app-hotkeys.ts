import { useEffect } from "react";

import { appShortcut } from "@/app/app-shortcuts";
import { useEditorStore } from "@/state/editor-store";
import { useUiStore } from "@/state/ui-store";

/**
 * Global shell shortcuts. Dialog inputs keep their own keys; when any dialog is open we
 * only allow Ctrl+K to close an already-open command palette. Text fields are not
 * excluded for Ctrl+K because a searchable palette should work while typing.
 */
export function useAppHotkeys(): void {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent): void => {
      const action = appShortcut(event);
      if (!action) return;
      const dialogOpen = document.querySelector('[role="dialog"]') !== null;
      if (action.kind === "command-palette") {
        event.preventDefault();
        if (event.repeat) return;
        if (dialogOpen && !useUiStore.getState().commandPaletteOpen) return;
        useUiStore.getState().toggleCommandPalette();
        return;
      }
      if (dialogOpen || event.repeat) return;
      event.preventDefault();
      useEditorStore.getState().togglePanel(action.panel);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);
}
