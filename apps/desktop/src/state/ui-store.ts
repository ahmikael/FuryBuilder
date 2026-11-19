import { create } from "zustand";

interface UiOverlayState {
  settingsOpen: boolean;
  mcpAccessOpen: boolean;
  commandPaletteOpen: boolean;
  setSettingsOpen: (open: boolean) => void;
  setMcpAccessOpen: (open: boolean) => void;
  setCommandPaletteOpen: (open: boolean) => void;
  toggleCommandPalette: () => void;
}

/**
 * Ephemeral overlay state shared by the toolbar, global shortcuts and command
 * palette. It is deliberately not persisted: dialogs should not reopen after a
 * reload, while panel layout lives in the persisted editor store.
 */
export const useUiStore = create<UiOverlayState>()((set) => ({
  settingsOpen: false,
  mcpAccessOpen: false,
  commandPaletteOpen: false,
  setSettingsOpen: (settingsOpen) => set({ settingsOpen }),
  setMcpAccessOpen: (mcpAccessOpen) => set({ mcpAccessOpen }),
  setCommandPaletteOpen: (commandPaletteOpen) => set({ commandPaletteOpen }),
  toggleCommandPalette: () => set((state) => ({ commandPaletteOpen: !state.commandPaletteOpen })),
}));
