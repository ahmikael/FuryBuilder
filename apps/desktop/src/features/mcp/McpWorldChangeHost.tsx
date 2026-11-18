import { useEffect } from "react";

import { applyExternalWorldChange } from "@/editor/commands";
import { listenForMcpWorldChanges } from "@/lib/mcp-access";

/**
 * Keeps the mounted editor in sync with writes made by the MCP bridge. The Rust core
 * emits the event after a synchronous apply/undo or a completed job; the viewport would
 * otherwise keep rendering the revision it loaded last.
 */
export function McpWorldChangeHost() {
  useEffect(() => {
    let disposed = false;
    let stop: (() => void) | null = null;
    void listenForMcpWorldChanges((change) => {
      applyExternalWorldChange(change);
    }).then((unlisten) => {
      if (disposed) unlisten();
      else stop = unlisten;
    });
    return () => {
      disposed = true;
      stop?.();
    };
  }, []);
  return null;
}
