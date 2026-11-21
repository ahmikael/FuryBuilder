import { recordUiError } from "@/lib/diagnostics-client";
import { pushToast } from "@/state/toast-store";

let installed = false;
let lastToastAt = 0;

function report(message: string, source: string): void {
  recordUiError(message, source);
  const now = Date.now();
  if (now - lastToastAt < 2_000) return;
  lastToastAt = now;
  pushToast({
    kind: "error",
    title: "Unexpected interface error",
    description:
      "The message was saved to Settings → Diagnostics. If editing still works, save your project now.",
  });
}

/**
 * Browser-level safety net for errors React boundaries cannot see (event handlers, timers,
 * rejected promises). Repeated errors are logged but produce at most one toast every two
 * seconds so a failing render loop cannot flood the screen.
 */
export function installGlobalErrorReporting(): void {
  if (installed || typeof window === "undefined") return;
  installed = true;
  window.addEventListener("error", (event) => {
    const message =
      event.error instanceof Error ? event.error.message : (event.message ?? "unknown error");
    report(message, "window.error");
  });
  window.addEventListener("unhandledrejection", (event) => {
    const reason = event.reason;
    const message =
      reason instanceof Error
        ? reason.message
        : typeof reason === "string"
          ? reason
          : "unhandled promise rejection";
    report(message, "window.unhandledrejection");
  });
}
