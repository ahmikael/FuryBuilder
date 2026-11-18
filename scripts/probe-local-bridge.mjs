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
    }, 20_000);
    socket.setEncoding("utf8");
    socket.once("connect", () => {
      socket.write(
        `${JSON.stringify({
          protocol: 1,
          id,
          token,
          clientId: "probe-local-bridge",
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
        rendererListening: ping.result.renderer?.listening ?? null,
        accessMode: ping.result.mcp?.mode ?? null,
        maxBlocks: ping.result.mcp?.maxBlocks ?? null,
        requireConfirmation: ping.result.mcp?.requireConfirmation ?? null,
      },
    },
    null,
    2,
  ),
);

async function waitForRenderer(timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  let last = null;
  while (Date.now() < deadline) {
    const probe = await request(
      runtime,
      `probe-renderer-${Date.now()}`,
      runtime.token,
      "ping",
    );
    if (probe.ok) {
      last = probe.result.renderer ?? null;
      if (last?.listening === true) return last;
    }
    await new Promise((resolve) => setTimeout(resolve, 1000));
  }
  throw new Error(
    `viewport listener did not become ready: ${JSON.stringify(last)}`,
  );
}

const renderer = await waitForRenderer(60_000);
console.log(JSON.stringify({ renderer }, null, 2));
if (ping.result.mcp?.mode !== "read_only") {
  throw new Error(
    `expected read-only MCP access by default, got ${JSON.stringify(ping.result.mcp)}`,
  );
}

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

const dimension = world.result.dimensions[0].id;
const worldRevision = world.result.revision;
const observations = {};
{
  const block = await request(runtime, "probe-block", runtime.token, "query", {
    projectId: ping.result.project.projectId,
    worldRevision,
    query: {
      type: "get_block",
      dimension,
      pos: { x: 0, y: 64, z: 0 },
    },
  });
  if (!block.ok) {
    throw new Error(
      `structured get_block failed: ${JSON.stringify(block.error)}`,
    );
  }
  if (block.result.worldRevision !== worldRevision) {
    throw new Error(
      `get_block reported revision ${block.result.worldRevision}, expected ${worldRevision}`,
    );
  }
  observations.block = {
    type: block.result.type,
    worldRevision: block.result.worldRevision,
    state: block.result.data.state.name,
    properties: Object.keys(block.result.data.state.properties ?? {}).length,
  };

  const summary = await request(
    runtime,
    "probe-region-summary",
    runtime.token,
    "query",
    {
      projectId: ping.result.project.projectId,
      worldRevision,
      query: {
        type: "region_summary",
        dimension,
        bounds: {
          min: { x: -1, y: 63, z: -1 },
          max: { x: 1, y: 65, z: 1 },
        },
      },
    },
  );
  if (!summary.ok) {
    throw new Error(
      `structured region_summary failed: ${JSON.stringify(summary.error)}`,
    );
  }
  const palette = summary.result.data.palette;
  const paletteTotal = palette.reduce((sum, entry) => sum + entry.count, 0);
  if (summary.result.worldRevision !== worldRevision) {
    throw new Error(
      `region_summary reported revision ${summary.result.worldRevision}, expected ${worldRevision}`,
    );
  }
  if (summary.result.data.blocks !== 27 || paletteTotal !== 27) {
    throw new Error(
      `region_summary palette does not cover the volume: blocks=${summary.result.data.blocks} palette=${paletteTotal}`,
    );
  }
  observations.summary = {
    type: summary.result.type,
    worldRevision: summary.result.worldRevision,
    blocks: summary.result.data.blocks,
    nonAirBlocks: summary.result.data.nonAirBlocks,
    paletteEntries: palette.length,
    paletteTotal,
  };

  const stale = await request(
    runtime,
    "probe-stale-query",
    runtime.token,
    "query",
    {
      projectId: ping.result.project.projectId,
      worldRevision: worldRevision + 1,
      query: {
        type: "get_block",
        dimension,
        pos: { x: 0, y: 64, z: 0 },
      },
    },
  );
  if (
    stale.ok ||
    stale.error?.code !== "revision_mismatch" ||
    stale.error?.details?.expectedRevision !== worldRevision + 1 ||
    stale.error?.details?.actualRevision !== worldRevision
  ) {
    throw new Error(
      `stale observation was not rejected with a structured conflict: ${JSON.stringify(stale)}`,
    );
  }
  observations.staleRejection = {
    code: stale.error.code,
    expectedRevision: stale.error.details.expectedRevision,
    actualRevision: stale.error.details.actualRevision,
  };
}
console.log(JSON.stringify({ observations }, null, 2));

const camera = await request(
  runtime,
  "probe-camera",
  runtime.token,
  "get_camera",
  {
    projectId: ping.result.project.projectId,
  },
);
if (!camera.ok) {
  throw new Error(
    `authorized get_camera failed: ${JSON.stringify(camera.error)}`,
  );
}
console.log(
  JSON.stringify(
    {
      camera: {
        dimension: camera.result.dimension,
        renderRevision: camera.result.verification.renderRevision,
        authoritativeRevision: camera.result.verification.authoritativeRevision,
        stale: camera.result.verification.stale,
        position: camera.result.pose.position,
      },
    },
    null,
    2,
  ),
);

const capture = await request(
  runtime,
  "probe-capture",
  runtime.token,
  "capture_viewport",
  {
    projectId: ping.result.project.projectId,
    dimension: camera.result.dimension,
  },
);
if (!capture.ok) {
  throw new Error(
    `authorized capture_viewport failed: ${JSON.stringify(capture.error)}`,
  );
}
const captureBytes = Buffer.from(
  capture.result.dataUrl.split(",")[1] ?? "",
  "base64",
);
console.log(
  JSON.stringify(
    {
      capture: {
        isPng: capture.result.dataUrl.startsWith("data:image/png;base64,"),
        pngSignature: captureBytes.subarray(0, 8).toString("hex"),
        bytes: captureBytes.length,
        partial: capture.result.verification.partial,
        stale: capture.result.verification.stale,
        renderRevision: capture.result.verification.renderRevision,
        authoritativeRevision:
          capture.result.verification.authoritativeRevision,
        missingChunks: capture.result.metadata.missingChunks.length,
        loadedChunks: capture.result.metadata.loadedChunks.length,
        legendEntries: capture.result.metadata.legend.length,
        legendTruncated: capture.result.metadata.legendTruncated,
      },
    },
    null,
    2,
  ),
);
if (captureBytes.subarray(0, 8).toString("hex") !== "89504e470d0a1a0a") {
  throw new Error("capture does not contain a PNG signature");
}
if (capture.result.verification.partial !== capture.result.metadata.partial) {
  throw new Error(
    "capture verification.partial disagrees with metadata.partial",
  );
}
if (
  capture.result.metadata.partial !==
  capture.result.metadata.missingChunks.length > 0
) {
  throw new Error(
    "capture metadata.partial disagrees with the missing chunk list",
  );
}
if (
  capture.result.verification.stale !==
  (capture.result.verification.renderRevision !==
    capture.result.verification.authoritativeRevision)
) {
  throw new Error(
    "capture verification.stale disagrees with the render and authoritative revisions",
  );
}

const previewOperation = {
  type: "set_block",
  dimension: camera.result.dimension,
  pos: { x: 0, y: 70, z: 0 },
  state: { name: "minecraft:stone", properties: {} },
};
const preview = await request(
  runtime,
  "probe-preview",
  runtime.token,
  "preview_operation",
  {
    projectId: ping.result.project.projectId,
    worldRevision: world.result.revision,
    operation: previewOperation,
  },
);
if (!preview.ok) {
  throw new Error(
    `authorized preview_operation failed: ${JSON.stringify(preview.error)}`,
  );
}
if (preview.result.access?.allowed !== false) {
  throw new Error(
    `preview should report denied write access in read-only mode: ${JSON.stringify(preview.result.access)}`,
  );
}
console.log(
  JSON.stringify(
    {
      preview: {
        affectedBlocks: preview.result.affectedBlocks,
        changedBlocks: preview.result.changedBlocks,
        worldRevision: preview.result.worldRevision,
        accessAllowed: preview.result.access?.allowed ?? null,
        accessCode: preview.result.access?.code ?? null,
      },
    },
    null,
    2,
  ),
);

const apply = await request(
  runtime,
  "probe-apply-denied",
  runtime.token,
  "apply_operation",
  {
    projectId: ping.result.project.projectId,
    worldRevision: preview.result.worldRevision,
    idempotencyKey: "probe-denied-write",
    operation: previewOperation,
  },
);
if (apply.ok) {
  throw new Error("MCP write unexpectedly succeeded without explicit access");
}
if (apply.error?.code !== "permission_denied") {
  throw new Error(
    `expected permission_denied, got ${JSON.stringify(apply.error)}`,
  );
}
if (apply.error.details?.mcpAccessMode !== "read_only") {
  throw new Error(
    `expected read-only policy details, got ${JSON.stringify(apply.error)}`,
  );
}
console.log(
  JSON.stringify(
    {
      writeGuard: {
        code: apply.error.code,
        accessMode: apply.error.details?.mcpAccessMode,
        operation: apply.error.details?.operation,
      },
    },
    null,
    2,
  ),
);

const status = await request(
  runtime,
  "probe-operation-status",
  runtime.token,
  "operation_status",
  { projectId: ping.result.project.projectId },
);
if (!status.ok) {
  throw new Error(
    `authorized operation_status failed: ${JSON.stringify(status.error)}`,
  );
}
console.log(
  JSON.stringify(
    {
      operationStatus: {
        currentRevision: status.result.currentRevision,
        canUndo: status.result.canUndo,
        jobAvailable: status.result.job.available,
        jobQueued: status.result.job.summary.queued,
        jobRetained: status.result.job.summary.retained,
      },
    },
    null,
    2,
  ),
);

const jobDenied = await request(
  runtime,
  "probe-job-denied",
  runtime.token,
  "start_job",
  {
    projectId: ping.result.project.projectId,
    kind: "apply_operation",
    parameters: {
      operation: {
        type: "set_block",
        dimension: "minecraft:overworld",
        pos: { x: 1, y: 64, z: 1 },
        state: { name: "minecraft:stone" },
      },
    },
    idempotencyKey: "probe-job-denied",
    worldRevision: world.result.revision,
  },
);
if (jobDenied.ok || jobDenied.error?.code !== "permission_denied") {
  throw new Error(
    `a read-only job start was not refused: ${JSON.stringify(jobDenied)}`,
  );
}
const jobs = await request(runtime, "probe-jobs", runtime.token, "list_jobs", {
  projectId: ping.result.project.projectId,
  limit: 10,
});
if (!jobs.ok || jobs.result.jobs.length !== 0) {
  throw new Error(
    `a denied job created a queue entry: ${JSON.stringify(jobs)}`,
  );
}
if (jobs.result.summary.available !== true) {
  throw new Error(
    `the queue did not report itself as available: ${JSON.stringify(jobs.result)}`,
  );
}
console.log(
  JSON.stringify(
    {
      jobGuard: {
        code: jobDenied.error.code,
        accessMode: jobDenied.error.details?.mcpAccessMode,
        queued: jobs.result.jobs.length,
        available: jobs.result.summary.available,
      },
    },
    null,
    2,
  ),
);

const activity = await request(
  runtime,
  "probe-activity",
  runtime.token,
  "mcp_activity",
  { projectId: ping.result.project.projectId, limit: 20 },
);
if (!activity.ok) {
  throw new Error(
    `authorized mcp_activity failed: ${JSON.stringify(activity.error)}`,
  );
}
const activityEntries = activity.result.entries;
if (!Array.isArray(activityEntries) || activityEntries.length < 5) {
  throw new Error(
    `expected authenticated calls in the activity journal, got ${JSON.stringify(activity.result)}`,
  );
}
const deniedEntry = activityEntries.find(
  (entry) => entry.requestId === "probe-apply-denied",
);
if (
  !deniedEntry ||
  deniedEntry.status !== "denied" ||
  deniedEntry.errorCode !== "permission_denied"
) {
  throw new Error(
    `denied write was not linked to the journal: ${JSON.stringify(deniedEntry)}`,
  );
}
const deniedJobEntry = activityEntries.find(
  (entry) => entry.requestId === "probe-job-denied",
);
if (
  !deniedJobEntry ||
  deniedJobEntry.method !== "start_job" ||
  deniedJobEntry.status !== "denied"
) {
  throw new Error(
    `denied job was not linked to the journal: ${JSON.stringify(deniedJobEntry)}`,
  );
}
if (
  activityEntries.some((entry) =>
    ["ping", "mcp_activity"].includes(entry.method),
  )
) {
  throw new Error("health checks must not flood the activity journal");
}
if (activityEntries.some((entry) => entry.clientId !== "probe-local-bridge")) {
  throw new Error("journal entries must keep the authenticated client label");
}
console.log(
  JSON.stringify(
    {
      activity: {
        stored: activity.result.stored,
        returned: activityEntries.length,
        bounded: activity.result.bounded,
        deniedRequest: {
          requestId: deniedEntry.requestId,
          method: deniedEntry.method,
          status: deniedEntry.status,
          errorCode: deniedEntry.errorCode,
          clientId: deniedEntry.clientId,
          projectId: deniedEntry.projectId,
          durationMs: deniedEntry.durationMs,
        },
        deniedJob: {
          requestId: deniedJobEntry.requestId,
          method: deniedJobEntry.method,
          status: deniedJobEntry.status,
          errorCode: deniedJobEntry.errorCode,
        },
      },
    },
    null,
    2,
  ),
);

const invalidActivity = await request(
  runtime,
  "probe-activity-invalid",
  runtime.token,
  "mcp_activity",
  { projectId: ping.result.project.projectId, limit: 0 },
);
if (invalidActivity.ok || invalidActivity.error?.code !== "invalid_argument") {
  throw new Error(
    `activity limit was not rejected: ${JSON.stringify(invalidActivity)}`,
  );
}

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
