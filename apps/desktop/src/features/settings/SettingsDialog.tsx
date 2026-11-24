import { Moon, Sun, SunMoon } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Slider } from "@/components/ui/slider";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { ComponentInventorySection } from "@/features/settings/ComponentInventorySection";
import { DiagnosticsSection } from "@/features/settings/DiagnosticsSection";
import { UpdatesSection } from "@/features/settings/UpdatesSection";
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

function isThemeMode(value: string | undefined): value is ThemeMode {
  return value === "dark" || value === "light" || value === "system";
}

export function SettingsDialog({ open, onOpenChange, backend }: SettingsDialogProps) {
  const theme = useEditorStore((state) => state.theme);
  const setTheme = useEditorStore((state) => state.setTheme);
  const uiScale = useEditorStore((state) => state.uiScale);
  const setUiScale = useEditorStore((state) => state.setUiScale);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[90vh] max-w-5xl gap-6 overflow-y-auto">
        <DialogHeader>
          <DialogTitle className="text-xl">Settings</DialogTitle>
          <DialogDescription>
            Preferences and shared local components. World data is never stored in the browser.
          </DialogDescription>
        </DialogHeader>

        <Tabs defaultValue="appearance" className="min-h-0">
          <TabsList className="grid w-full grid-cols-5">
            <TabsTrigger value="appearance">Appearance</TabsTrigger>
            <TabsTrigger value="components">Components</TabsTrigger>
            <TabsTrigger value="updates">Updates</TabsTrigger>
            <TabsTrigger value="diagnostics">Diagnostics</TabsTrigger>
            <TabsTrigger value="advanced">Advanced</TabsTrigger>
          </TabsList>

          <TabsContent value="appearance" className="m-0 space-y-4 pt-4">
            <h3 className="text-base font-semibold">Appearance</h3>
            <Card className="gap-0 py-0">
              <CardContent className="space-y-6 p-6">
                <div className="flex flex-wrap items-center gap-4">
                  <span className="w-24 text-sm text-muted-foreground">Theme</span>
                  <ToggleGroup
                    variant="outline"
                    spacing={0}
                    value={[theme]}
                    onValueChange={(value) => {
                      const next = value[0];
                      if (isThemeMode(next)) setTheme(next);
                    }}
                  >
                    {themes.map((option) => {
                      const Icon = option.icon;
                      return (
                        <ToggleGroupItem key={option.value} value={option.value}>
                          <Icon data-icon="inline-start" />
                          {option.label}
                        </ToggleGroupItem>
                      );
                    })}
                  </ToggleGroup>
                </div>
                <div className="flex flex-wrap items-center gap-4">
                  <span className="w-24 text-sm text-muted-foreground">UI scale</span>
                  <Slider
                    min={MIN_UI_SCALE}
                    max={MAX_UI_SCALE}
                    step={0.05}
                    value={uiScale}
                    onValueChange={(value) => {
                      const next = Array.isArray(value) ? value[0] : value;
                      if (typeof next === "number") setUiScale(next);
                    }}
                    aria-label="UI scale"
                    className="max-w-md flex-1"
                  />
                  <Badge variant="secondary" className="w-14 justify-center font-mono">
                    {Math.round(uiScale * 100)}%
                  </Badge>
                </div>
              </CardContent>
            </Card>
          </TabsContent>

          <TabsContent value="components" className="m-0 space-y-6 pt-4">
            <section className="space-y-1.5">
              <h3 className="text-base font-semibold">Components</h3>
              <p className="text-sm leading-relaxed text-muted-foreground">
                The shared local component cache. Verify, remove or open the folder of installed
                Java runtimes, client JARs and server JARs here. Downloads, manual paths and
                placeholder textures are chosen in the project wizard.
              </p>
            </section>
            <ComponentInventorySection />
          </TabsContent>

          <TabsContent value="updates" className="m-0 pt-4">
            <UpdatesSection currentVersion={backend.info?.version ?? null} />
          </TabsContent>

          <TabsContent value="diagnostics" className="m-0 pt-4">
            <DiagnosticsSection />
          </TabsContent>

          <TabsContent value="advanced" className="m-0 space-y-4 pt-4">
            <h3 className="text-base font-semibold">Backend</h3>
            <Card className="gap-0 py-0">
              <CardContent className="grid grid-cols-[160px_1fr] gap-x-4 gap-y-3 p-6 text-sm">
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
              </CardContent>
            </Card>
            {backend.error ? <p className="text-sm text-status-warn">{backend.error}</p> : null}
          </TabsContent>
        </Tabs>

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Close
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
