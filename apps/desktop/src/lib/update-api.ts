import type { DownloadEvent, Update } from "@tauri-apps/plugin-updater";
import { isTauriRuntime } from "@/lib/ipc";

export interface UpdateProgress {
  downloadedBytes: number;
  totalBytes: number | null;
  percent: number | null;
}

export interface AvailableUpdate {
  currentVersion: string;
  version: string;
  date: string | null;
  notes: string | null;
  install: (onProgress: (progress: UpdateProgress) => void) => Promise<void>;
  close: () => Promise<void>;
}

const UPDATE_TIMEOUT_MS = 30_000;

/**
 * Development builds must never replace themselves with the published release.
 * Browser preview mode has no updater plugin at all.
 */
export function canCheckForUpdates(): boolean {
  return isTauriRuntime() && import.meta.env.PROD;
}

export async function checkForUpdate(): Promise<AvailableUpdate | null> {
  if (!canCheckForUpdates()) {
    throw new Error(
      "Updates are only available in an installed FuryBuilder desktop build, not in development or browser preview mode.",
    );
  }

  const { check } = await import("@tauri-apps/plugin-updater");
  const update = await check({ timeout: UPDATE_TIMEOUT_MS });
  if (!update) return null;

  return {
    currentVersion: update.currentVersion,
    version: update.version,
    date: update.date ?? null,
    notes: update.body ?? null,
    install: (onProgress) => installUpdate(update, onProgress),
    close: () => update.close(),
  };
}

async function installUpdate(
  update: Update,
  onProgress: (progress: UpdateProgress) => void,
): Promise<void> {
  let totalBytes: number | null = null;
  let downloadedBytes = 0;

  await update.downloadAndInstall((event: DownloadEvent) => {
    switch (event.event) {
      case "Started":
        totalBytes = event.data.contentLength ?? null;
        onProgress({ downloadedBytes: 0, totalBytes, percent: 0 });
        break;
      case "Progress":
        downloadedBytes += event.data.chunkLength;
        onProgress({
          downloadedBytes,
          totalBytes,
          percent:
            totalBytes && totalBytes > 0
              ? Math.min(99, Math.round((downloadedBytes / totalBytes) * 100))
              : null,
        });
        break;
      case "Finished":
        onProgress({ downloadedBytes, totalBytes, percent: 100 });
        break;
    }
  });
}

/** Relaunches the app after an update on platforms where the installer does not do it. */
export async function relaunchApp(): Promise<void> {
  const { relaunch } = await import("@tauri-apps/plugin-process");
  await relaunch();
}
