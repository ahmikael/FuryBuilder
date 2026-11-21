import { CircleAlert, CircleCheck, CircleX, Info, X } from "lucide-react";
import { useToastStore } from "@/state/tool-store";
import { cn } from "@/lib/utils";

const ICONS = {
  info: Info,
  success: CircleCheck,
  warning: CircleAlert,
  error: CircleX,
} as const;

export function Toaster() {
  const toasts = useToastStore((state) => state.toasts);
  const dismiss = useToastStore((state) => state.dismiss);
  if (toasts.length === 0) return null;

  return (
    <div
      className="pointer-events-none fixed right-3 bottom-9 z-50 flex w-80 flex-col gap-1.5"
      aria-live="polite"
    >
      {toasts.map((toast) => {
        const Icon = ICONS[toast.kind];
        return (
          <div
            key={toast.id}
            role={toast.kind === "error" ? "alert" : "status"}
            className={cn(
              "pointer-events-auto flex items-start gap-2 rounded-md border bg-card/95 px-2.5 py-2 text-xs shadow-xl backdrop-blur",
              toast.kind === "info" && "border-border",
              toast.kind === "success" && "border-status-ok/40",
              toast.kind === "warning" && "border-status-warn/40",
              toast.kind === "error" && "border-status-error/40",
            )}
          >
            <Icon
              className={cn(
                "mt-0.5 size-3.5 shrink-0",
                toast.kind === "info" && "text-muted-foreground",
                toast.kind === "success" && "text-status-ok",
                toast.kind === "warning" && "text-status-warn",
                toast.kind === "error" && "text-status-error",
              )}
            />
            <div className="min-w-0 flex-1">
              <div className="font-medium">{toast.title}</div>
              {toast.description ? (
                <div className="mt-0.5 text-xs leading-relaxed text-muted-foreground">
                  {toast.description}
                </div>
              ) : null}
            </div>
            <button
              type="button"
              aria-label={`Dismiss ${toast.title}`}
              className="rounded p-0.5 text-muted-foreground hover:bg-accent hover:text-foreground"
              onClick={() => dismiss(toast.id)}
            >
              <X className="size-3" />
            </button>
          </div>
        );
      })}
    </div>
  );
}
