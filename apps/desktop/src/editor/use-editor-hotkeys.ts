import { useEffect } from "react";
import {
  applyActiveTool,
  cancelInteraction,
  copySelection,
  deleteSelection,
  pasteClipboard,
  redoWorld,
  undoWorld,
} from "@/editor/commands";
import { editorShortcut } from "@/editor/session";
import { useProjectStore } from "@/state/project-store";
import { useToolStore } from "@/state/tool-store";

function dialogOpen(): boolean {
  return document.querySelector('[role="dialog"]') !== null;
}

/**
 * Global editor shortcuts. Text fields keep their native undo/copy/paste because
 * `editorShortcut` returns null for editable targets. Dialogs keep Escape/Enter.
 */
export function useEditorHotkeys(): void {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent): void => {
      const action = editorShortcut(event);
      if (!action) return;
      // Dialogs own Escape/Enter and must not trigger world edits from focus inside them.
      if (dialogOpen()) return;
      if (action.kind === "cancel") {
        event.preventDefault();
        cancelInteraction();
        return;
      }
      if (action.kind === "tool") {
        event.preventDefault();
        useToolStore.getState().setTool(action.tool);
        return;
      }
      if (event.repeat) return;
      switch (action.kind) {
        case "undo":
          event.preventDefault();
          void undoWorld();
          break;
        case "redo":
          event.preventDefault();
          void redoWorld();
          break;
        case "copy":
          event.preventDefault();
          void copySelection();
          break;
        case "paste":
          event.preventDefault();
          void pasteClipboard();
          break;
        case "delete-selection":
          event.preventDefault();
          void deleteSelection();
          break;
        case "apply-selection":
          event.preventDefault();
          void applyActiveTool();
          break;
        case "save":
          event.preventDefault();
          void useProjectStore.getState().save();
          break;
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);
}
