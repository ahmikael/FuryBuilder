import { useEffect } from "react";
import { TooltipProvider } from "@/components/ui/tooltip";
import { BottomPanel } from "@/features/workspace/BottomPanel";
import { Inspector } from "@/features/workspace/Inspector";
import { StatusBar } from "@/features/workspace/StatusBar";
import { Toolbar } from "@/features/workspace/Toolbar";
import { Viewport } from "@/features/viewport/Viewport";
import { WelcomeScreen } from "@/features/welcome/WelcomeScreen";
import { WorldTree } from "@/features/workspace/WorldTree";
import { CommandPalette } from "@/features/workspace/CommandPalette";
import { SettingsDialog } from "@/features/settings/SettingsDialog";
import { McpAccessDialog } from "@/features/mcp/McpAccessDialog";
import { McpConfirmationHost } from "@/features/mcp/McpConfirmationHost";
import { McpWorldChangeHost } from "@/features/mcp/McpWorldChangeHost";
import { ProjectManager, useProjectAutosave } from "@/features/project/ProjectManager";
import { RecoveryBanner } from "@/features/project/RecoveryBanner";
import { UpdateBanner } from "@/features/updates/UpdateBanner";
import { useAutoUpdate } from "@/features/updates/use-auto-update";
import { Toaster } from "@/components/ui/toast";
import { useBackendInfo } from "@/hooks/use-backend";
import { isTauriRuntime } from "@/lib/ipc";
import { useAppliedScale, useAppliedTheme } from "@/app/theme";
import { useAppHotkeys } from "@/app/use-app-hotkeys";
import { useEditorHotkeys } from "@/editor/use-editor-hotkeys";
import { useEditorStore } from "@/state/editor-store";
import { useProjectStore } from "@/state/project-store";
import { useUiStore } from "@/state/ui-store";

export function App() {
  const backend = useBackendInfo();
  const theme = useEditorStore((state) => state.theme);
  const setTheme = useEditorStore((state) => state.setTheme);
  const uiScale = useEditorStore((state) => state.uiScale);
  const leftPanelOpen = useEditorStore((state) => state.leftPanelOpen);
  const rightPanelOpen = useEditorStore((state) => state.rightPanelOpen);
  const bottomPanelOpen = useEditorStore((state) => state.bottomPanelOpen);
  const settingsOpen = useUiStore((state) => state.settingsOpen);
  const setSettingsOpen = useUiStore((state) => state.setSettingsOpen);
  const mcpAccessOpen = useUiStore((state) => state.mcpAccessOpen);
  const setMcpAccessOpen = useUiStore((state) => state.setMcpAccessOpen);
  const phase = useProjectStore((state) => state.phase);
  const current = useProjectStore((state) => state.current);
  const bootstrap = useProjectStore((state) => state.bootstrap);

  useAppliedTheme(theme);
  useAppliedScale(uiScale);
  useEditorHotkeys();
  useAppHotkeys();
  useProjectAutosave();
  useAutoUpdate();

  useEffect(() => {
    if (!isTauriRuntime()) return;
    void (async () => {
      await bootstrap();
    })();
  }, [bootstrap]);

  const currentResolvedTheme =
    theme === "system"
      ? window.matchMedia?.("(prefers-color-scheme: dark)").matches
        ? "dark"
        : "light"
      : theme;
  const editorVisible = Boolean(current?.folder) && phase !== "closed" && phase !== "preparing";

  return (
    <Toaster limit={4}>
      <TooltipProvider>
        <div className="app-shell flex h-full flex-col bg-background text-foreground">
          <Toolbar
            backendStatus={backend.status}
            theme={currentResolvedTheme}
            onToggleTheme={() => setTheme(currentResolvedTheme === "dark" ? "light" : "dark")}
            onOpenSettings={() => setSettingsOpen(true)}
            onOpenMcpAccess={() => setMcpAccessOpen(true)}
          />
          <UpdateBanner />
          {editorVisible ? (
            <>
              <RecoveryBanner />
              <div className="flex min-h-0 flex-1">
                {leftPanelOpen ? <WorldTree /> : null}
                <Viewport />
                {rightPanelOpen ? <Inspector /> : null}
              </div>
              {bottomPanelOpen ? <BottomPanel backend={backend} /> : null}
            </>
          ) : (
            <WelcomeScreen backend={backend} />
          )}
          <StatusBar backend={backend} />
        </div>
        <ProjectManager />
        <CommandPalette />
        <SettingsDialog open={settingsOpen} onOpenChange={setSettingsOpen} backend={backend} />
        <McpAccessDialog open={mcpAccessOpen} onOpenChange={setMcpAccessOpen} />
        <McpConfirmationHost />
        <McpWorldChangeHost />
      </TooltipProvider>
    </Toaster>
  );
}
