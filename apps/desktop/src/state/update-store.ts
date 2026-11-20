import { create } from "zustand";
import {
  canCheckForUpdates,
  checkForUpdate,
  relaunchApp,
  type AvailableUpdate,
  type UpdateProgress,
} from "@/lib/update-api";

export type UpdateStatus =
  | "idle"
  | "checking"
  | "up-to-date"
  | "available"
  | "downloading"
  | "installing"
  | "error";

export interface UpdateNotice {
  version: string;
  currentVersion: string;
  date: string | null;
  notes: string | null;
}

interface UpdateState {
  status: UpdateStatus;
  available: UpdateNotice | null;
  progress: UpdateProgress | null;
  error: string | null;
  lastCheckedAt: number | null;
  check: (options?: { silent?: boolean }) => Promise<void>;
  install: () => Promise<void>;
  dismiss: () => void;
}

let pendingUpdate: AvailableUpdate | null = null;

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function isBusy(status: UpdateStatus): boolean {
  return status === "checking" || status === "downloading" || status === "installing";
}

/**
 * Update state is intentionally not persisted: every launch checks the signed
 * release manifest again, and a dismissed notice returns on the next check.
 */
export const useUpdateStore = create<UpdateState>()((set, get) => ({
  status: "idle",
  available: null,
  progress: null,
  error: null,
  lastCheckedAt: null,

  check: async ({ silent = false } = {}) => {
    if (isBusy(get().status)) return;

    if (!canCheckForUpdates()) {
      if (!silent) {
        set({
          status: "error",
          error:
            "Updates are only available in an installed FuryBuilder desktop build, not in development or browser preview mode.",
          lastCheckedAt: Date.now(),
        });
      }
      return;
    }

    set({ status: "checking", error: null, progress: null });
    try {
      const update = await checkForUpdate();
      if (pendingUpdate && pendingUpdate !== update) {
        void pendingUpdate.close();
      }
      pendingUpdate = update;
      if (!update) {
        set({
          status: "up-to-date",
          available: null,
          progress: null,
          error: null,
          lastCheckedAt: Date.now(),
        });
        return;
      }
      set({
        status: "available",
        available: {
          version: update.version,
          currentVersion: update.currentVersion,
          date: update.date,
          notes: update.notes,
        },
        progress: null,
        error: null,
        lastCheckedAt: Date.now(),
      });
    } catch (error) {
      pendingUpdate = null;
      // A background check must not interrupt the editor just because the
      // network is unavailable. The manual check in Settings reports the error.
      set({
        status: silent ? "idle" : "error",
        available: null,
        progress: null,
        error: silent ? null : errorMessage(error),
        lastCheckedAt: Date.now(),
      });
    }
  },

  install: async () => {
    if (!pendingUpdate || !["available", "error"].includes(get().status)) return;
    const update = pendingUpdate;
    set({
      status: "downloading",
      progress: { downloadedBytes: 0, totalBytes: null, percent: null },
      error: null,
    });
    try {
      await update.install((progress) => {
        set((state) => ({
          progress,
          status: progress.percent === 100 ? "installing" : state.status,
        }));
      });
      // On Windows the installer exits the app before this resolves. Other
      // platforms need an explicit relaunch to start the new version.
      set({ status: "installing" });
      await relaunchApp();
    } catch (error) {
      set({
        status: "error",
        error: errorMessage(error),
        progress: null,
      });
    }
  },

  dismiss: () => {
    if (pendingUpdate) {
      void pendingUpdate.close();
      pendingUpdate = null;
    }
    set({ status: "idle", available: null, progress: null, error: null });
  },
}));
