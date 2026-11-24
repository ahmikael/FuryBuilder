import { normalizeError } from "@/lib/ipc";

export interface ErrorPresentation {
  code: string;
  message: string;
  hint: string | null;
}

/**
 * Actionable hints for stable backend error codes. The original message is preserved: the
 * hint explains what the user can do without hiding what actually failed.
 */
const HINTS: Record<string, string> = {
  backend_unavailable:
    "The Rust backend is not reachable. Start the desktop build with `pnpm tauri dev` instead of a plain browser tab.",
  out_of_bounds:
    "The target is outside the editable world height or the allowed area. Move the selection or adjust the world height.",
  unsupported_version:
    "This data was produced by a Minecraft version that this FuryBuilder build does not support yet. Open the project's version details to see the supported targets.",
  unsupported_format:
    "The file or archive is not a supported format, or its structure is damaged. Use a file from a supported Java version.",
  permission_denied:
    "The action is blocked by the current policy, protected region or connection scope. Check the MCP access mode and workspace locks.",
  conflict:
    "The project changed since the action started. Refresh the preview and retry so nothing is overwritten blindly.",
  limit_exceeded:
    "The operation exceeds a documented size or count limit. Reduce the region, file size or number of items.",
  disk_space:
    "The destination volume does not have enough free space for this download and its temporary files. Free some space or choose another cache location.",
  checksum_mismatch:
    "The file does not match the official checksum. Remove it and download again from the official source; FuryBuilder never substitutes a different file.",
  busy: "Another component operation is already running for this kind. Wait for it to finish or cancel it, then retry.",
  cancelled:
    "The operation was cancelled and its partial file was removed. Start it again when you are ready.",
  not_ready:
    "The selected component is not verified. Check its exact version, architecture and checksum, then retry.",
  offline:
    "Network access is disabled or unavailable. Cached metadata can still be used; reconnect or turn off offline mode, then retry.",
  not_found:
    "The requested project, revision, job or file no longer exists. Refresh the list and choose it again.",
  invalid_argument:
    "The input is invalid. Check coordinates, names, selected files and version-specific options.",
  no_history:
    "There is no matching undo or redo step. The history may have been cleared by reopening the project.",
  internal:
    "The action failed inside FuryBuilder. Retry once; if it repeats, open Settings → Diagnostics and keep the log file.",
  unknown:
    "The action failed unexpectedly. Retry once; diagnostics record the message without world data or credentials.",
};

export function presentError(error: unknown): ErrorPresentation {
  const normalized = normalizeError(error);
  return {
    code: normalized.code,
    message: normalized.message,
    hint: HINTS[normalized.code] ?? HINTS.unknown,
  };
}

/** One-line description used by toasts and error banners. */
export function describeError(error: unknown): string {
  const presented = presentError(error);
  return `${presented.message}${presented.hint ? ` ${presented.hint}` : ""}`;
}
