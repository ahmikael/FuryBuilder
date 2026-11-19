import { Moon, Sun, SunMoon } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Separator } from "@/components/ui/separator";
import { MinecraftAssetsSection } from "@/features/settings/MinecraftAssetsSection";
import { JavaRuntimeSection } from "@/features/settings/JavaRuntimeSection";
import { MinecraftServerSection } from "@/features/settings/MinecraftServerSection";
import { DependencyReadiness } from "@/features/settings/DependencyReadiness";
import { DiagnosticsSection } from "@/features/settings/DiagnosticsSection";
import type { BackendState } from "@/hooks/use-backend";
import { MAX_UI_SCALE, MIN_UI_SCALE, type ThemeMode, useEditorStore } from "@/state/editor-store";
import { cn } from "@/lib/utils";

interface SettingsDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  backend: BackendState;
}

const themes: Array<{ value: ThemeMode; label: string; icon: typeof Sun }> = [
  { value: "dark", label: "Dark", icon: Moon },
  { value: "light", label: "Light", icon: Sun },
  { value: "system", label: "System", icon: SunMoon },
];

export function SettingsDialog({ open, onOpenChange, backend }: SettingsDialogProps) {
  const theme = useEditorStore((state) => state.theme);
  const setTheme = useEditorStore((state) => state.setTheme);
  const uiScale = useEditorStore((state) => state.uiScale);
  const setUiScale = useEditorStore((state) => state.setUiScale);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[88vh] max-w-xl overflow-x-hidden overflow-y-auto">
        <DialogHeader>
          <DialogTitle>Settings</DialogTitle>
          <DialogDescription>
            UI preferences are stored locally. World data is never stored in the browser.
          </DialogDescription>
        </DialogHeader>

        <section className="space-y-2">
          <h3 className="text-[11px] font-semibold tracking-wide uppercase">Appearance</h3>
          <div className="flex items-center gap-1">
            {themes.map((option) => {
              const Icon = option.icon;
              return (
                <Button
                  key={option.value}
                  variant={theme === option.value ? "default" : "outline"}
                  size="sm"
                  onClick={() => setTheme(option.value)}
                >
                  <Icon /> {option.label}
                </Button>
              );
            })}
          </div>
          <div className="flex items-center gap-3 pt-1">
            <span className="w-24 text-[11px] text-muted-foreground">UI scale</span>
            <input
              type="range"
              min={MIN_UI_SCALE}
              max={MAX_UI_SCALE}
              step={0.05}
              value={uiScale}
              onChange={(event) => setUiScale(Number(event.target.value))}
              className="h-1.5 flex-1 accent-[var(--primary)]"
              aria-label="UI scale"
            />
            <span className="w-10 text-right font-mono text-[11px]">
              {Math.round(uiScale * 100)}%
            </span>
          </div>
        </section>

        <Separator />

        <section className="space-y-1.5">
          <h3 className="text-[11px] font-semibold tracking-wide uppercase">Backend</h3>
          <div className="grid grid-cols-[120px_1fr] gap-x-3 gap-y-1 rounded border border-border bg-background/40 p-2 text-[11px]">
            <span className="text-muted-foreground">Status</span>
            <span
              className={cn(
                "font-mono",
                backend.status === "online" ? "text-status-ok" : "text-status-warn",
              )}
            >
              {backend.status}
            </span>
            <span className="text-muted-foreground">Application</span>
            <span className="font-mono">{backend.info?.name ?? "FuryBuilder"}</span>
            <span className="text-muted-foreground">Version</span>
            <span className="font-mono">{backend.info?.version ?? "0.1.0"}</span>
            <span className="text-muted-foreground">Minecraft target</span>
            <span className="font-mono">
              {backend.info?.targetMinecraftVersion ?? "1.21.4"} · DV{" "}
              {backend.info?.targetDataVersion ?? 4189}
            </span>
          </div>
          {backend.error ? <p className="text-[10px] text-status-warn">{backend.error}</p> : null}
        </section>

        <Separator />

        <MinecraftAssetsSection />

        <Separator />

        <div className="min-w-0 space-y-3">
          <div className="space-y-1">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <h3 className="text-[11px] font-semibold tracking-wide uppercase">
                Java &amp; World Generation
              </h3>
              <span className="rounded bg-muted px-1.5 py-0.5 text-[9px] text-muted-foreground">
                Optional for Empty Project
              </span>
            </div>
            <p className="text-[10px] text-muted-foreground">
              These components are only needed to generate a real Vanilla World. Discovery is local;
              downloads start only from the explicit buttons below and are verified before use.
            </p>
          </div>

          <DependencyReadiness />

          <JavaRuntimeSection />

          <MinecraftServerSection />
        </div>

        <Separator />

        <DiagnosticsSection />

        <DialogFooter>
          <Button variant="outline" size="sm" onClick={() => onOpenChange(false)}>
            Close
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
