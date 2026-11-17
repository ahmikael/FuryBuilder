import { useEffect, useState } from "react";
import { invokeCommand } from "@/lib/ipc";
import type { AppInfo, BackendStatus } from "@/lib/contracts";

export interface BackendState {
  status: BackendStatus;
  info: AppInfo | null;
  error: string | null;
}

/**
 * Probes the Rust backend once per mount. A browser tab intentionally shows
 * "offline" instead of simulating world data.
 */
export function useBackendInfo(): BackendState {
  const [state, setState] = useState<BackendState>({
    status: "connecting",
    info: null,
    error: null,
  });

  useEffect(() => {
    let cancelled = false;
    async function probe(): Promise<void> {
      try {
        const info = await invokeCommand<AppInfo>("app_info");
        await invokeCommand("ping", { name: "ui" });
        if (!cancelled) setState({ status: "online", info, error: null });
      } catch (error) {
        if (!cancelled) {
          setState({
            status: "offline",
            info: null,
            error: error instanceof Error ? error.message : String(error),
          });
        }
      }
    }
    void probe();
    return () => {
      cancelled = true;
    };
  }, []);

  return state;
}
