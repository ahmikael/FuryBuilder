#!/usr/bin/env node
// Reproducible probe for the authenticated local endpoint exposed by the running
// FuryBuilder desktop app. It reads the per-launch discovery file, verifies that a wrong
// token is rejected, then reads the authoritative world info. No token is printed.
//
// Usage:
//   1. Start the desktop app (pnpm --filter @furybuilder/desktop tauri dev).
//   2. node scripts/probe-local-bridge.mjs

import { readFile } from "node:fs/promises";
import { createConnection } from "node:net";
import { join } from "node:path";

const appData = process.env.APPDATA;
if (!appData) {
  console.error("APPDATA is required on Windows");
  process.exit(2);
}
const runtimePath = join(
  appData,
  "com.furybuilder.desktop",
  "mcp",
  "runtime.json",
);

async function waitForRuntime(timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      return JSON.parse(await readFile(runtimePath, "utf8"));
    } catch {
      await new Promise((resolve) => setTimeout(resolve, 250));
    }
  }
  throw new Error(`runtime file did not appear: ${runtimePath}`);
}

function request(runtime, id, token, method, params = {}) {
  return new Promise((resolve, reject) => {
    const socket = createConnection(runtime.endpoint);
    let buffer = "";
    const timer = setTimeout(() => {
      socket.destroy();
      reject(new Error(`timeout for ${method}`));
    }, 5000);
    socket.setEncoding("utf8");
    socket.once("connect", () => {
      socket.write(
        `${JSON.stringify({
          protocol: 1,
          id,
          token,
          method,
          params,
        })}\n`,
      );
    });
    socket.on("data", (chunk) => {
      buffer += chunk;
      const newline = buffer.indexOf("\n");
      if (newline < 0) return;
      clearTimeout(timer);
      socket.destroy();
      resolve(JSON.parse(buffer.slice(0, newline)));
    });
    socket.once("error", (error) => {
      clearTimeout(timer);
      reject(error);
    });
  });
}

const runtime = await waitForRuntime(120_000);
console.log(
  JSON.stringify(
    {
      discovery: {
        schemaVersion: runtime.schemaVersion,
        transport: runtime.transport,
        protocolVersion: runtime.protocolVersion,
        endpointIsNamedPipe: runtime.endpoint.startsWith("\\\\.\\pipe\\"),
        tokenLength: runtime.token.length,
        pid: runtime.pid,
      },
    },
    null,
    2,
  ),
);

const ping = await request(runtime, "probe-ping", runtime.token, "ping");
if (!ping.ok) throw new Error("authorized ping failed");
console.log(
  JSON.stringify(
    {
      ping: {
        projectId: ping.result.project.projectId,
        worldRevision: ping.result.project.worldRevision,
        appVersion: ping.result.app.version,
      },
    },
    null,
    2,
  ),
);

const world = await request(
  runtime,
  "probe-world",
  runtime.token,
  "world_info",
  {
    projectId: ping.result.project.projectId,
  },
);
if (!world.ok) throw new Error("authorized world_info failed");
console.log(
  JSON.stringify(
    {
      worldInfo: {
        targetMinecraftVersion: world.result.targetMinecraftVersion,
        revision: world.result.revision,
        dimensions: world.result.dimensions.length,
      },
    },
    null,
    2,
  ),
);

const denied = await request(
  runtime,
  "probe-denied",
  "0".repeat(64),
  "world_info",
  {
    projectId: ping.result.project.projectId,
  },
);
if (denied.ok || denied.error?.code !== "unauthorized") {
  throw new Error("wrong token was not rejected as unauthorized");
}
console.log(
  JSON.stringify(
    {
      rejectedToken: {
        code: denied.error.code,
        responseContainsToken: JSON.stringify(denied).includes(runtime.token),
      },
    },
    null,
    2,
  ),
);

console.log("LOCAL_BRIDGE_PROBE_OK");
