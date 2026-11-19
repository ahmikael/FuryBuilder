import { invoke } from "@tauri-apps/api/core";

function desktopRuntime(): boolean {
  return (
    typeof window !== "undefined" &&
    Object.prototype.hasOwnProperty.call(window, "__TAURI_INTERNALS__")
  );
}

function cleanMessage(message: string): string {
  let cleaned = "";
  for (const ch of message.slice(0, 4_000)) {
    const code = ch.charCodeAt(0);
    cleaned += code < 32 || code === 127 ? " " : ch;
  }
  return cleaned;
}

/**
 * Records a UI-level failure in the bounded local diagnostics log. This deliberately uses
 * the raw Tauri invoke: failure reporting must never recurse through `invokeCommand`.
 */
export function recordUiError(message: string, source: string): void {
  if (!desktopRuntime()) return;
  const safeMessage = cleanMessage(message);
  const safeSource = source.replace(/[^a-zA-Z0-9._:-]/g, "_").slice(0, 80) || "ui";
  try {
    void invoke("diagnostics_record_ui_error", {
      message: safeMessage,
      source: safeSource,
    }).catch(() => {
      // Diagnostics are best-effort; a logging failure must not affect editing.
    });
  } catch {
    // The runtime may be shutting down.
  }
}
