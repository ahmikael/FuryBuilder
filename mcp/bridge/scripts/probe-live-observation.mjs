#!/usr/bin/env node
// Live MCP observation probe. It starts a real stdio bridge against the running
// FuryBuilder desktop app and checks that summary, block inspection and a bounded
// screenshot are structured and truthful about revision and loading.
//
// Usage:
//   1. Start the desktop app (pnpm --filter @furybuilder/desktop tauri dev).
//   2. node mcp/bridge/scripts/probe-live-observation.mjs
//
// The probe never writes to the world. It uses only read-only tools.

import { Client } from "@modelcontextprotocol/client";
import { StdioClientTransport } from "@modelcontextprotocol/client/stdio";
import { createHash } from "node:crypto";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const bridgeRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = resolve(bridgeRoot, "..", "..");
const runtimePath =
  process.env.FURYBUILDER_MCP_RUNTIME ||
  join(
    process.env.APPDATA ?? "",
    "com.furybuilder.desktop",
    "mcp",
    "runtime.json",
  );
const evidencePath =
  process.env.FURYBUILDER_OBSERVATION_EVIDENCE ||
  join(
    repoRoot,
    "docs",
    "testing",
    "evidence",
    "2026-11-18-mcp-observation.json",
  );

function sleep(ms) {
  return new Promise((done) => setTimeout(done, ms));
}

async function waitForRuntime(timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      return JSON.parse(await readFile(runtimePath, "utf8"));
    } catch {
      await sleep(250);
    }
  }
  throw new Error(`runtime file did not appear: ${runtimePath}`);
}

function textOf(result) {
  const block = result.content.find((entry) => entry.type === "text");
  if (!block || block.type !== "text") {
    throw new Error("tool returned no text content");
  }
  return block.text;
}

function jsonOf(result) {
  return JSON.parse(textOf(result));
}

function fail(message) {
  throw new Error(message);
}

async function main() {
  const runtime = await waitForRuntime(120_000);
  const client = new Client(
    { name: "furybuilder-live-observation", version: "1.0.0" },
    {
      versionNegotiation: {
        mode: { pin: "2026-07-28" },
      },
    },
  );
  const transport = new StdioClientTransport({
    command: process.execPath,
    args: ["--import", "tsx", "src/main.ts"],
    cwd: bridgeRoot,
    env: { ...process.env, FURYBUILDER_MCP_RUNTIME: runtimePath },
    stderr: "pipe",
  });

  try {
    await client.connect(transport);
    const era = client.getProtocolEra();

    const info = jsonOf(
      await client.callTool({
        name: "bridge_info",
        arguments: { project_id: "local-project" },
      }),
    );
    const desktop = info.bridge?.desktop;
    if (!desktop?.connected) {
      fail(`desktop endpoint is not connected: ${JSON.stringify(desktop)}`);
    }
    const projectId = desktop.project?.projectId;
    if (typeof projectId !== "string" || projectId.length === 0) {
      fail("bridge_info did not report the active project id");
    }

    const world = jsonOf(
      await client.callTool({
        name: "world_info",
        arguments: { project_id: projectId },
      }),
    );
    const revision = world.revision;
    if (!Number.isInteger(revision) || revision < 0) {
      fail(`world_info returned no integer revision: ${JSON.stringify(world)}`);
    }
    const dimension = world.dimensions?.[0]?.id;
    if (typeof dimension !== "string") {
      fail("world_info returned no dimension id");
    }

    const blockResult = await client.callTool({
      name: "get_block",
      arguments: {
        project_id: projectId,
        world_revision: revision,
        dimension,
        pos: { x: 0, y: 64, z: 0 },
      },
    });
    if (blockResult.isError) {
      fail(`get_block failed: ${textOf(blockResult)}`);
    }
    const block = jsonOf(blockResult);
    if (block.worldRevision !== revision) {
      fail(
        `get_block reported revision ${block.worldRevision}, expected ${revision}`,
      );
    }
    if (typeof block.data?.state?.name !== "string") {
      fail("get_block returned no block state");
    }

    const regionResult = await client.callTool({
      name: "inspect_region",
      arguments: {
        project_id: projectId,
        world_revision: revision,
        dimension,
        bounds: {
          min: { x: -1, y: 63, z: -1 },
          max: { x: 1, y: 65, z: 1 },
        },
      },
    });
    if (regionResult.isError) {
      fail(`inspect_region failed: ${textOf(regionResult)}`);
    }
    const region = jsonOf(regionResult);
    const palette = Array.isArray(region.data?.palette)
      ? region.data.palette
      : [];
    const paletteTotal = palette.reduce((sum, entry) => sum + entry.count, 0);
    if (region.worldRevision !== revision) {
      fail(
        `inspect_region reported revision ${region.worldRevision}, expected ${revision}`,
      );
    }
    if (region.data?.blocks !== 27 || paletteTotal !== 27) {
      fail(
        `inspect_region palette does not cover the bounds: blocks=${region.data?.blocks}, palette=${paletteTotal}`,
      );
    }
    if (region.data?.nonAirBlocks > region.data?.blocks) {
      fail("inspect_region reported more non-air blocks than the volume");
    }

    const stale = await client.callTool({
      name: "get_block",
      arguments: {
        project_id: projectId,
        world_revision: revision + 1,
        dimension,
        pos: { x: 0, y: 64, z: 0 },
      },
    });
    const staleText = textOf(stale);
    if (!stale.isError || !staleText.startsWith("revision_mismatch: ")) {
      fail(
        `a stale observation was not rejected as revision_mismatch: ${staleText}`,
      );
    }

    // The viewport may still be mounting right after startup; retry only the
    // backend_unavailable case instead of fabricating a screenshot.
    let capture = null;
    const deadline = Date.now() + 60_000;
    while (Date.now() < deadline) {
      const attempt = await client.callTool({
        name: "capture_viewport",
        arguments: {
          project_id: projectId,
          world_revision: revision,
          dimension,
        },
      });
      if (!attempt.isError) {
        capture = attempt;
        break;
      }
      const text = textOf(attempt);
      if (!text.startsWith("backend_unavailable: ")) {
        fail(`capture_viewport failed: ${text}`);
      }
      await sleep(1000);
    }
    if (!capture) {
      fail("viewport did not become available for a screenshot");
    }
    const image = capture.content.find((entry) => entry.type === "image");
    if (!image || image.type !== "image") {
      fail("capture_viewport returned no image block");
    }
    const captureMeta = jsonOf(capture);
    const sha256 = createHash("sha256")
      .update(Buffer.from(image.data, "base64"))
      .digest("hex");
    if (captureMeta.image?.sha256 !== sha256) {
      fail("capture image digest disagrees with the published metadata");
    }
    if (captureMeta.image?.bytes !== Buffer.from(image.data, "base64").length) {
      fail("capture image size disagrees with the published metadata");
    }
    const verification = captureMeta.verification ?? {};
    const metadata = captureMeta.metadata ?? {};
    if (verification.partial !== metadata.partial) {
      fail("capture verification.partial disagrees with metadata.partial");
    }
    if (metadata.partial !== metadata.missingChunks?.length > 0) {
      fail("capture metadata.partial disagrees with its missing chunk list");
    }
    if (
      verification.stale !==
      (verification.renderRevision !== verification.authoritativeRevision)
    ) {
      fail("capture verification.stale disagrees with the revisions");
    }
    if (verification.authoritativeRevision !== revision) {
      fail(
        `capture authoritative revision ${verification.authoritativeRevision} differs from world revision ${revision}`,
      );
    }

    const evidence = {
      date: new Date().toISOString().slice(0, 10),
      result: "passed",
      protocolEra: era,
      projectId,
      worldRevision: revision,
      observations: {
        block: {
          type: block.type,
          worldRevision: block.worldRevision,
          state: block.data.state.name,
          propertyCount: Object.keys(block.data.state.properties ?? {}).length,
        },
        region: {
          type: region.type,
          worldRevision: region.worldRevision,
          dimension: region.data.dimension,
          bounds: region.data.bounds,
          blocks: region.data.blocks,
          nonAirBlocks: region.data.nonAirBlocks,
          paletteEntries: palette.length,
          paletteTotal,
        },
        staleRejection: {
          isError: stale.isError,
          code: "revision_mismatch",
          requestedRevision: revision + 1,
          actualRevision: revision,
        },
        capture: {
          mimeType: image.mimeType,
          bytes: captureMeta.image.bytes,
          sha256,
          imageMatchesMetadata: true,
          metadata: {
            dimension: metadata.dimension,
            worldRevision: metadata.worldRevision,
            partial: metadata.partial,
            missingChunks: metadata.missingChunks?.length ?? null,
            loadedChunks: metadata.loadedChunks?.length ?? null,
            legendEntries: metadata.legend?.length ?? null,
            legendTruncated: metadata.legendTruncated ?? null,
          },
          verification,
        },
      },
    };

    await mkdir(dirname(evidencePath), { recursive: true });
    await writeFile(evidencePath, `${JSON.stringify(evidence, null, 2)}\n`);
    console.log(JSON.stringify(evidence, null, 2));
    console.log(`MCP_OBSERVATION_PROBE_OK ${evidencePath}`);
  } finally {
    await client.close().catch(() => undefined);
  }
}

main().catch((error) => {
  console.error(
    `MCP_OBSERVATION_PROBE_FAILED: ${error instanceof Error ? error.message : String(error)}`,
  );
  process.exitCode = 1;
});
