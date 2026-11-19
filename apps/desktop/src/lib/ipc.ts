import { invoke } from "@tauri-apps/api/core";
import { recordUiError } from "@/lib/diagnostics-client";
import type { AppErrorShape } from "@/lib/contracts";

export class BackendError extends Error {
  readonly code: string;
  readonly details: unknown;

  constructor(code: string, message: string, details?: unknown) {
    super(message);
    this.name = "BackendError";
    this.code = code;
    this.details = details;
  }
}

/** True when running inside the Tauri WebView (as opposed to a plain browser dev tab). */
export function isTauriRuntime(): boolean {
  if (typeof window === "undefined") return false;
  return Object.prototype.hasOwnProperty.call(window, "__TAURI_INTERNALS__");
}

export function normalizeError(error: unknown): BackendError {
  if (error instanceof BackendError) return error;
  if (typeof error === "object" && error !== null) {
    const candidate = error as Partial<AppErrorShape>;
    if (typeof candidate.code === "string" && typeof candidate.message === "string") {
      return new BackendError(candidate.code, candidate.message, candidate.details);
    }
  }
  if (error instanceof Error) return new BackendError("unknown", error.message);
  return new BackendError("unknown", String(error));
}

/**
 * Typed invoke wrapper. The browser build intentionally fails instead of faking a
 * backend: the world source of truth must stay in Rust.
 */
export async function invokeCommand<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  if (!isTauriRuntime()) {
    throw new BackendError(
      "backend_unavailable",
      "The Rust backend is not available in this window. Run `pnpm tauri dev` to use the desktop app.",
    );
  }
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    const normalized = normalizeError(error);
    recordUiError(normalized.message, `invoke:${command}`);
    throw normalized;
  }
}
