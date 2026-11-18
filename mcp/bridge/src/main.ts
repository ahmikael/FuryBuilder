#!/usr/bin/env node
import { serveStdio } from "@modelcontextprotocol/server/stdio";

import { createFuryBuilderServer } from "./server.js";

// stdout is the JSON-RPC channel; diagnostics must go to stderr only.
console.error("FuryBuilder MCP bridge listening on stdio");

const handle = serveStdio(() => createFuryBuilderServer(), {
  legacy: "serve",
  onerror: (error) => {
    console.error(`FuryBuilder MCP bridge error: ${error.message}`);
  },
});

async function shutdown(): Promise<void> {
  await handle.close();
  process.exit(0);
}

process.on("SIGINT", () => void shutdown());
process.on("SIGTERM", () => void shutdown());
