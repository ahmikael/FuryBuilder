import { useState } from "react";
import { TooltipProvider } from "@/components/ui/tooltip";
import { BottomPanel } from "@/features/workspace/BottomPanel";
import { Inspector } from "@/features/workspace/Inspector";
import { StatusBar } from "@/features/workspace/StatusBar";
import { Toolbar } from "@/features/workspace/Toolbar";
import { Viewport } from "@/features/viewport/Viewport";
import { WorldTree } from "@/features/workspace/WorldTree";
import { SettingsDialog } from "@/features/settings/SettingsDialog";
import { useBackendInfo } from "@/hooks/use-backend";
import { useAppliedScale, useAppliedTheme } from "@/app/theme";
import { useEditorStore } from "@/state/editor-store";

export function App() {
  const backend = useBackendInfo();
  const theme = useEditorStore((state) => state.theme);
  const setTheme = useEditorStore((state) => state.setTheme);
  const uiScale = useEditorStore((state) => state.uiScale);
  const [settingsOpen, setSettingsOpen] = useState(false);

  useAppliedTheme(theme);
  useAppliedScale(uiScale);

  const currentResolvedTheme =
    theme === "system"
      ? window.matchMedia?.("(prefers-color-scheme: dark)").matches
        ? "dark"
        : "light"
      : theme;

  return (
    <TooltipProvider>
      <div className="app-shell flex h-full flex-col bg-background text-foreground">
        <Toolbar
          backendStatus={backend.status}
          theme={currentResolvedTheme}
          onToggleTheme={() => setTheme(currentResolvedTheme === "dark" ? "light" : "dark")}
          onOpenSettings={() => setSettingsOpen(true)}
        />
        <div className="flex min-h-0 flex-1">
          <WorldTree />
          <Viewport />
          <Inspector />
        </div>
        <BottomPanel backend={backend} />
        <StatusBar backend={backend} />
      </div>
      <SettingsDialog open={settingsOpen} onOpenChange={setSettingsOpen} backend={backend} />
    </TooltipProvider>
  );
}
