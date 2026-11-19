import type { PanelId } from "@/state/editor-store";

export type AppShortcut = { kind: "command-palette" } | { kind: "toggle-panel"; panel: PanelId };

/**
 * Shell-level shortcuts. They are separate from `editorShortcut` because the command
 * palette must open even while the user is typing in a field, and because panel
 * visibility is a UI concern even when a world edit is not possible.
 */
export function appShortcut(event: {
  code: string;
  ctrlKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
}): AppShortcut | null {
  const mod = event.ctrlKey || event.metaKey;
  if (!mod) return null;
  if (event.altKey) {
    return event.code === "KeyB" ? { kind: "toggle-panel", panel: "right" } : null;
  }
  if (event.shiftKey) return null;
  if (event.code === "KeyK") return { kind: "command-palette" };
  if (event.code === "KeyB") return { kind: "toggle-panel", panel: "left" };
  if (event.code === "KeyJ") return { kind: "toggle-panel", panel: "bottom" };
  return null;
}
