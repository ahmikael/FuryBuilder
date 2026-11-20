import { useEffect } from "react";
import { isTauriRuntime } from "@/lib/ipc";
import { useUpdateStore } from "@/state/update-store";

const STARTUP_CHECK_DELAY_MS = 4_000;

/**
 * Checks once per launch after the editor has settled. Development and browser
 * preview builds are skipped so a local build can never replace itself.
 */
export function useAutoUpdate(): void {
  useEffect(() => {
    if (!import.meta.env.PROD || !isTauriRuntime()) return;
    const timer = window.setTimeout(() => {
      void useUpdateStore.getState().check({ silent: true });
    }, STARTUP_CHECK_DELAY_MS);
    return () => window.clearTimeout(timer);
  }, []);
}
