import { getCurrentWindow } from "@tauri-apps/api/window";
import { Copy, Minus, Square, X } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { Button } from "@/components/ui/button";
import { isTauriRuntime } from "@/lib/ipc";

export function WindowControls() {
  const desktopWindow = useMemo(() => (isTauriRuntime() ? getCurrentWindow() : null), []);
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    if (!desktopWindow) return;

    let disposed = false;
    let stopListening: (() => void) | undefined;

    const syncMaximized = (): void => {
      void desktopWindow
        .isMaximized()
        .then((next) => {
          if (!disposed) setMaximized(next);
        })
        .catch(() => undefined);
    };

    syncMaximized();
    void desktopWindow
      .onResized(syncMaximized)
      .then((stop) => {
        if (disposed) stop();
        else stopListening = stop;
      })
      .catch(() => undefined);

    return () => {
      disposed = true;
      stopListening?.();
    };
  }, [desktopWindow]);

  const toggleMaximize = (): void => {
    if (!desktopWindow) return;
    void desktopWindow
      .toggleMaximize()
      .then(() => desktopWindow.isMaximized())
      .then(setMaximized)
      .catch(() => undefined);
  };

  if (!desktopWindow) return null;

  return (
    <div className="ml-1 flex shrink-0 items-center gap-0.5 border-l pl-1">
      <Button
        variant="ghost"
        size="icon-lg"
        className="rounded-md"
        aria-label="Minimize window"
        title="Minimize"
        onClick={() => void desktopWindow.minimize().catch(() => undefined)}
      >
        <Minus />
      </Button>
      <Button
        variant="ghost"
        size="icon-lg"
        className="rounded-md"
        aria-label={maximized ? "Restore window" : "Maximize window"}
        title={maximized ? "Restore" : "Maximize"}
        onClick={toggleMaximize}
      >
        {maximized ? <Copy /> : <Square />}
      </Button>
      <Button
        variant="ghost"
        size="icon-lg"
        className="rounded-md hover:bg-destructive hover:text-destructive-foreground"
        aria-label="Close window"
        title="Close"
        onClick={() => void desktopWindow.close().catch(() => undefined)}
      >
        <X />
      </Button>
    </div>
  );
}
