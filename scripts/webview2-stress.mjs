/**
 * Windows-only large-map stress test for the actual Three.js viewport renderer.
 *
 * It starts its own Tauri process with an isolated WebView2 profile and a
 * loopback-only DevTools port, builds synthetic chunks with the project's real
 * SnapshotBuilder, and measures full-snapshot setup plus incremental
 * `updateChunks` streaming. It never attaches to an existing user process.
 *
 * Prerequisites: a built target/debug/furybuilder-desktop.exe and this
 * checkout's desktop Vite server on 127.0.0.1:1420.
 *
 * Usage: node scripts/webview2-stress.mjs <new-output-directory> [radii]
 * Example: node scripts/webview2-stress.mjs %TEMP%\fury-stress 8,12,16,20
 */
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdir, mkdtemp, readFile, writeFile } from "node:fs/promises";
import { cpus, release, tmpdir, totalmem } from "node:os";
import { dirname, join, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const appPath = join(root, "target", "debug", "furybuilder-desktop.exe");
const origin = "http://127.0.0.1:1420";
const defaultRadii = "8,12,16,20,24,28,32";

if (
  process.platform !== "win32" ||
  process.argv.length < 3 ||
  process.argv.length > 4
) {
  throw new Error(
    "Usage on Windows: node scripts/webview2-stress.mjs <new-output-directory> [radii]",
  );
}

const output = resolve(process.argv[2]);
const startedAt = Date.now();
const radii = (process.argv[3] ?? defaultRadii)
  .split(",")
  .map((value) => Number(value.trim()))
  .filter((value) => Number.isInteger(value) && value >= 1 && value <= 64);
if (radii.length === 0) throw new Error("No valid radii supplied");
radii.sort((left, right) => left - right);
await mkdir(output); // Fail rather than overwrite existing evidence.
const profile = await mkdtemp(join(tmpdir(), "fury-webview-stress-"));
const profileEnabled = process.env.FURY_PROFILE === "1";
const requestedLeakCycles = Number(process.env.FURY_LEAK_CYCLES ?? "4");
const leakCycles =
  Number.isInteger(requestedLeakCycles) &&
  requestedLeakCycles >= 0 &&
  requestedLeakCycles <= 12
    ? requestedLeakCycles
    : 4;

/**
 * Collapses a Chrome CPU profile into self-time rows. Time deltas are microseconds;
 * idle/program samples are reported separately so synthetic waits do not hide real work.
 */
function summarizeProfile(profile) {
  const nodeById = new Map(profile.nodes.map((node) => [node.id, node]));
  const selfMicros = new Map();
  let totalMicros = 0;
  const timeDeltas = profile.timeDeltas ?? [];
  for (let index = 0; index < profile.samples.length; index += 1) {
    const micros = timeDeltas[index] ?? 0;
    totalMicros += micros;
    const nodeId = profile.samples[index];
    selfMicros.set(nodeId, (selfMicros.get(nodeId) ?? 0) + micros);
  }
  const rows = [];
  for (const [nodeId, micros] of selfMicros) {
    const node = nodeById.get(nodeId);
    if (!node) continue;
    const frame = node.callFrame;
    rows.push({
      functionName: frame.functionName || "(anonymous)",
      url: frame.url,
      lineNumber: frame.lineNumber + 1,
      selfMs: Math.round(micros) / 1000,
    });
  }
  rows.sort((left, right) => right.selfMs - left.selfMs);
  const idleNames = new Set(["(idle)", "(program)", "(root)"]);
  const idleRows = rows.filter((row) => idleNames.has(row.functionName));
  const gcRows = rows.filter(
    (row) => row.functionName === "(garbage collector)",
  );
  const activeRows = rows.filter(
    (row) =>
      !idleNames.has(row.functionName) &&
      row.functionName !== "(garbage collector)",
  );
  const share = (micros) => (totalMicros > 0 ? micros / totalMicros : 0);
  return {
    sampleCount: profile.samples.length,
    durationMs: Math.round(totalMicros / 1000),
    idleMs: Math.round(idleRows.reduce((sum, row) => sum + row.selfMs, 0)),
    gcMs: Math.round(gcRows.reduce((sum, row) => sum + row.selfMs, 0)),
    idleShare: share(idleRows.reduce((sum, row) => sum + row.selfMs, 0) * 1000),
    gcShare: share(gcRows.reduce((sum, row) => sum + row.selfMs, 0) * 1000),
    topSelfTime: activeRows.slice(0, 40),
  };
}

// Do not accidentally test a different checkout already listening on the port.
for (const name of ["viewport-renderer", "builders", "mesher", "world"]) {
  const response = await fetch(`${origin}/src/renderer/${name}.ts`, {
    signal: AbortSignal.timeout(5000),
  });
  if (!response.ok)
    throw new Error(`Start this checkout's desktop Vite server at ${origin}`);
  const content = await response.text();
  const encodedMap = content.match(
    /sourceMappingURL=data:application\/json[^,]*;base64,(\S+)/,
  )?.[1];
  if (!encodedMap)
    throw new Error(`Vite did not provide a source map for ${name}`);
  const sourceMap = JSON.parse(
    Buffer.from(encodedMap, "base64").toString("utf8"),
  );
  const localSource = await readFile(
    join(root, "apps", "desktop", "src", "renderer", `${name}.ts`),
    "utf8",
  );
  if (!sourceMap.sourcesContent?.includes(localSource))
    throw new Error(
      `Vite serves a different ${name}.ts; start it from this checkout`,
    );
}

const executable = await readFile(appPath);
const executableSha256 = createHash("sha256").update(executable).digest("hex");
const app = spawn(appPath, [], {
  cwd: root,
  env: {
    ...process.env,
    WEBVIEW2_USER_DATA_FOLDER: profile,
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:
      "--remote-debugging-port=0 --js-flags=--expose-gc",
  },
  stdio: "ignore",
});

let ws;
let nextId = 0;
const pending = new Map();

function send(method, params = {}) {
  const id = ++nextId;
  return new Promise((resolvePromise, reject) => {
    const timeout = setTimeout(() => {
      pending.delete(id);
      reject(new Error(`DevTools request timed out: ${method}`));
    }, 600_000);
    pending.set(id, {
      resolve: (value) => {
        clearTimeout(timeout);
        resolvePromise(value);
      },
      reject: (error) => {
        clearTimeout(timeout);
        reject(error);
      },
    });
    ws.send(JSON.stringify({ id, method, params }));
  });
}

async function evaluate(expression) {
  const result = await send("Runtime.evaluate", {
    expression,
    awaitPromise: true,
    returnByValue: true,
  });
  if (result.exceptionDetails)
    throw new Error(
      result.exceptionDetails.exception?.description ??
        result.exceptionDetails.text,
    );
  return result.result.value;
}

// Executes inside the new Tauri WebView2, not in Node or a separate browser.
async function stressScene(config) {
  const [{ ViewportRenderer }, { SnapshotBuilder }] = await Promise.all([
    import("/src/renderer/viewport-renderer.ts"),
    import("/src/renderer/builders.ts"),
  ]);
  const host = document.createElement("div");
  Object.assign(host.style, {
    position: "fixed",
    left: "0",
    top: "0",
    width: `${config.width}px`,
    height: `${config.height}px`,
    zIndex: "2147483647",
  });
  document.body.appendChild(host);

  const nextFrame = () =>
    new Promise((resolve) => requestAnimationFrame(resolve));
  const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const memoryOf = () =>
    typeof performance.memory === "object" && performance.memory
      ? {
          jsHeapUsedBytes: performance.memory.usedJSHeapSize,
          jsHeapTotalBytes: performance.memory.totalJSHeapSize,
          jsHeapLimitBytes: performance.memory.jsHeapSizeLimit,
        }
      : null;
  const heap = () => memoryOf()?.jsHeapUsedBytes ?? null;
  const collectGarbage = async () => {
    if (typeof globalThis.gc !== "function") return false;
    globalThis.gc();
    await wait(40);
    globalThis.gc();
    await nextFrame();
    return true;
  };

  // Measures animation-frame gaps during the run. A synchronous batch or an
  // unbounded mesh flush shows up here as a spike even when average FPS recovers.
  let frameGap = { last: performance.now(), max: 0, samples: 0 };
  let frameProbeActive = true;
  const gcEvents = [];
  const longTasks = [];
  const observeEntries = (type, target, map) => {
    try {
      const observer = new PerformanceObserver((list) => {
        for (const entry of list.getEntries()) target.push(map(entry));
      });
      observer.observe({ entryTypes: [type] });
      return observer;
    } catch {
      return null;
    }
  };
  const gcObserver = observeEntries("gc", gcEvents, (entry) => ({
    startTime: Math.round(entry.startTime),
    durationMs: Math.round(entry.duration * 10) / 10,
    kind: entry.detail?.kind ?? null,
  }));
  const longTaskObserver = observeEntries("longtask", longTasks, (entry) => ({
    startTime: Math.round(entry.startTime),
    durationMs: Math.round(entry.duration * 10) / 10,
  }));
  const probeFrame = (now) => {
    if (!frameProbeActive) return;
    const gap = now - frameGap.last;
    frameGap.last = now;
    if (gap > frameGap.max) frameGap.max = gap;
    frameGap.samples += 1;
    requestAnimationFrame(probeFrame);
  };
  requestAnimationFrame(probeFrame);
  const resetFrameProbe = () => {
    frameGap = { last: performance.now(), max: 0, samples: 0 };
    gcEvents.length = 0;
    longTasks.length = 0;
  };
  const frameProbeResult = () => ({
    maxFrameGapMs: Math.round(frameGap.max * 10) / 10,
    frameGapSamples: frameGap.samples,
    gcEvents: [...gcEvents],
    longTasks: longTasks.filter((entry) => entry.durationMs >= 50),
  });

  let renderer;
  try {
    let metrics = null;
    let metricUpdates = 0;
    renderer = new ViewportRenderer(
      host,
      {
        onMetrics: (value) => {
          metrics = value;
          metricUpdates += 1;
        },
      },
      // The stress harness deliberately raises the residency cap so the 4,225-chunk
      // ceiling is measured without eviction. Production uses the default budget.
      { maxResidentChunks: 8192 },
    );
    renderer.setChunkBordersVisible(false);
    renderer.setGridVisible(false);
    const gl = renderer.renderer.getContext();
    if (!(gl instanceof WebGL2RenderingContext))
      throw new Error("WebGL2 context unavailable");
    const debugInfo = gl.getExtension("WEBGL_debug_renderer_info");
    const gpuTimer = gl.getExtension("EXT_disjoint_timer_query_webgl2");

    // Deterministic synthetic terrain. It is not a claim about Minecraft worldgen.
    const builder = new SnapshotBuilder();
    for (let cx = -config.maxRadius; cx <= config.maxRadius; cx += 1) {
      for (let cz = -config.maxRadius; cz <= config.maxRadius; cz += 1) {
        const x = cx * 16;
        const z = cz * 16;
        builder.setRegion([x, 57, z], [x + 15, 63, z + 15], "minecraft:stone");
        builder.setRegion(
          [x, 64, z],
          [x + 15, 64, z + 15],
          "minecraft:grass_block",
        );
        const height = 2 + ((cx * cx + cz * cz) % 8);
        builder.setRegion(
          [x + 6, 65, z + 6],
          [x + 9, 65 + height, z + 9],
          "minecraft:oak_planks",
        );
      }
    }
    const full = builder.build("minecraft:overworld", 1, config.maxRadius);
    const sourceChunks = full.chunks.filter((chunk) => chunk.loaded);
    const chunkMap = new Map(
      sourceChunks.map((chunk) => [`${chunk.chunk.x},${chunk.chunk.z}`, chunk]),
    );
    const nonAirOf = (chunks) =>
      chunks
        .flatMap((chunk) => chunk.sections)
        .reduce((sum, section) => sum + section.nonAir, 0);
    const allNonAir = nonAirOf(sourceChunks);
    const chunkKeys = [...chunkMap.keys()];

    const waitForMetrics = async (
      expectedChunks,
      timeoutMs,
      settleMs = 1800,
    ) => {
      const deadline = performance.now() + timeoutMs;
      const settled = () =>
        Boolean(metrics) &&
        metrics.loadedChunks === expectedChunks &&
        (metrics.pendingMeshJobs ?? 0) === 0;
      while (!settled() && performance.now() < deadline) {
        await nextFrame();
      }
      if (!settled())
        throw new Error(
          `metrics for ${expectedChunks} chunks did not settle (last ${metrics?.loadedChunks ?? "none"}, pending ${metrics?.pendingMeshJobs ?? "unknown"})`,
        );
      // Let the short-window FPS average cover frames after the mesh queue drained.
      await wait(settleMs);
      return metrics;
    };

    const snapshots = [];
    let lastCapture = null;
    for (const radius of config.radii) {
      const chunks = sourceChunks.filter(
        (chunk) =>
          Math.max(Math.abs(chunk.chunk.x), Math.abs(chunk.chunk.z)) <= radius,
      );
      if (chunks.length === 0) continue;
      const snapshot = {
        dimension: "minecraft:overworld",
        revision: 1,
        center: { x: 0, z: 0 },
        radius,
        chunks,
      };
      const record = {
        radius,
        chunks: chunks.length,
        nonAirBlocks: nonAirOf(chunks),
        setupMs: null,
        settleMs: null,
        metrics: null,
        overviewMetrics: null,
        heapUsedBytes: null,
        capture: null,
        failed: null,
      };
      try {
        renderer.setCameraPose([142, 160, 142], [0, 64, 0]);
        const started = performance.now();
        renderer.setSnapshot(snapshot);
        record.setupMs = Math.round((performance.now() - started) * 10) / 10;
        const settleStart = performance.now();
        metrics = null;
        record.metrics = await waitForMetrics(chunks.length, 60_000);
        record.settleMs =
          Math.round((performance.now() - settleStart) * 10) / 10;
        record.heapUsedBytes = heap();

        const capture = renderer.capture();
        record.capture = {
          resolution: capture.metadata.resolution,
          worldRevision: capture.metadata.worldRevision,
          loadedChunks: capture.metadata.loadedChunks.length,
          missingChunks: capture.metadata.missingChunks.length,
          partial: capture.metadata.partial,
          dataUrlLength: capture.dataUrl.length,
        };
        lastCapture = capture;

        // Worst-case visibility: look straight down at the whole tested square. The
        // normal camera only sees a frustum slice, so this isolates draw-call and
        // fill-rate cost from corner-only culling.
        renderer.setCameraPose(
          [0, Math.max(240, radius * 16 + 80), 0],
          [0, 0, 0],
        );
        metrics = null;
        await wait(1800);
        record.overviewMetrics = metrics;
      } catch (error) {
        record.failed = String(error?.message ?? error);
        snapshots.push(record);
        break;
      }
      snapshots.push(record);
    }

    // Streaming simulation: start with one chunk and add Chebyshev rings in
    // bounded batches through the same `updateChunks` path used after IPC edits.
    // Several batch sizes are measured because one large batch pauses the main
    // thread while many small batches add per-call overhead.
    const stream = {
      maxRadius: config.maxRadius,
      batchSizes: config.batchSizes,
      runs: [],
      failed: null,
    };
    try {
      const centre = chunkMap.get("0,0");
      if (!centre) throw new Error("centre chunk missing");
      for (const batchSize of config.batchSizes) {
        const run = {
          batchChunks: batchSize,
          rings: [],
          totalMs: null,
          updates: 0,
          metrics: null,
          heapUsedBytes: null,
          failed: null,
        };
        try {
          renderer.setSnapshot({
            dimension: "minecraft:overworld",
            revision: 1,
            center: { x: 0, z: 0 },
            radius: 0,
            chunks: [centre],
          });
          metrics = null;
          await waitForMetrics(1, 30_000);
          resetFrameProbe();
          const started = performance.now();
          for (let ring = 1; ring <= config.maxRadius; ring += 1) {
            const ringChunks = chunkKeys
              .map((key) => chunkMap.get(key))
              .filter(
                (chunk) =>
                  Math.max(Math.abs(chunk.chunk.x), Math.abs(chunk.chunk.z)) ===
                  ring,
              );
            const ringStarted = performance.now();
            let ringUpdates = 0;
            let ringMaxBatchMs = 0;
            for (
              let offset = 0;
              offset < ringChunks.length;
              offset += batchSize
            ) {
              const batch = ringChunks.slice(offset, offset + batchSize);
              const batchStarted = performance.now();
              renderer.updateChunks({
                dimension: "minecraft:overworld",
                revision: 1,
                chunks: batch,
              });
              const batchMs = performance.now() - batchStarted;
              ringMaxBatchMs = Math.max(ringMaxBatchMs, batchMs);
              ringUpdates += 1;
            }
            run.rings.push({
              ring,
              chunks: ringChunks.length,
              updates: ringUpdates,
              ms: Math.round((performance.now() - ringStarted) * 10) / 10,
              maxBatchMs: Math.round(ringMaxBatchMs * 10) / 10,
            });
            await wait(0);
          }
          run.totalMs = Math.round((performance.now() - started) * 10) / 10;
          run.updates = run.rings.reduce((sum, ring) => sum + ring.updates, 0);
          run.metrics = await waitForMetrics(sourceChunks.length, 60_000);
          run.heapUsedBytes = heap();
          Object.assign(run, frameProbeResult());
        } catch (error) {
          run.failed = String(error?.message ?? error);
        }
        stream.runs.push(run);
      }
    } catch (error) {
      stream.failed = String(error?.message ?? error);
    }

    // Capture the streaming end state before the cleanup probes below unload it.
    const finalRendererInfo = {
      calls: renderer.renderer.info.render.calls,
      triangles: renderer.renderer.info.render.triangles,
      geometries: renderer.renderer.info.memory.geometries,
      textures: renderer.renderer.info.memory.textures,
    };
    const finalMemory = memoryOf();

    // Border cost probe: the chunk border overlay is one merged line geometry, so
    // enabling it should add one draw call rather than one per loaded chunk.
    const overlays = {
      radius: null,
      chunks: null,
      withBorders: null,
      withoutBorders: null,
      failed: null,
    };
    try {
      const overlayRadius =
        config.radii.find((value) => value <= 8) ?? config.radii[0];
      const overlayChunks = sourceChunks.filter(
        (chunk) =>
          Math.max(Math.abs(chunk.chunk.x), Math.abs(chunk.chunk.z)) <=
          overlayRadius,
      );
      renderer.setCameraPose(
        [0, Math.max(240, overlayRadius * 16 + 80), 0],
        [0, 0, 0],
      );
      renderer.setSnapshot({
        dimension: "minecraft:overworld",
        revision: 1,
        center: { x: 0, z: 0 },
        radius: overlayRadius,
        chunks: overlayChunks,
      });
      await waitForMetrics(overlayChunks.length, 60_000);
      renderer.setChunkBordersVisible(true);
      metrics = null;
      await wait(1500);
      overlays.withBorders = metrics;
      renderer.setChunkBordersVisible(false);
      metrics = null;
      await wait(1500);
      overlays.withoutBorders = metrics;
      overlays.radius = overlayRadius;
      overlays.chunks = overlayChunks.length;
    } catch (error) {
      overlays.failed = String(error?.message ?? error);
    }

    // Memory probe: return to an empty renderer, force V8 collection, then repeat a
    // bounded load/unload cycle. Stable post-GC heap and geometry counts across cycles
    // indicate that replaced meshes and chunk data are actually released.
    const leak = {
      enabled: config.leakCycles > 0,
      leakRadius: Math.min(8, config.maxRadius),
      chunks: 0,
      gcAvailable: false,
      baseline: null,
      cycles: [],
      failed: null,
    };
    if (leak.enabled) {
      try {
        renderer.removeChunks(sourceChunks.map((chunk) => chunk.chunk));
        await waitForMetrics(0, 60_000, 250);
        await wait(200);
        leak.gcAvailable = await collectGarbage();
        leak.baseline = {
          heapUsedBytes: heap(),
          memory: memoryOf(),
          geometries: renderer.renderer.info.memory.geometries,
          textures: renderer.renderer.info.memory.textures,
        };
        if (!leak.gcAvailable) {
          leak.failed =
            "window.gc is not exposed; memory trend cannot be verified";
        } else {
          const leakChunks = sourceChunks.filter(
            (chunk) =>
              Math.max(Math.abs(chunk.chunk.x), Math.abs(chunk.chunk.z)) <=
              leak.leakRadius,
          );
          leak.chunks = leakChunks.length;
          const leakSnapshot = {
            dimension: "minecraft:overworld",
            revision: 1,
            center: { x: 0, z: 0 },
            radius: leak.leakRadius,
            chunks: leakChunks,
          };
          for (let cycle = 1; cycle <= config.leakCycles; cycle += 1) {
            renderer.setSnapshot(leakSnapshot);
            await waitForMetrics(leakChunks.length, 60_000, 250);
            renderer.removeChunks(leakChunks.map((chunk) => chunk.chunk));
            await waitForMetrics(0, 60_000, 250);
            await wait(150);
            await collectGarbage();
            await wait(150);
            leak.cycles.push({
              cycle,
              heapUsedBytes: heap(),
              memory: memoryOf(),
              geometries: renderer.renderer.info.memory.geometries,
              textures: renderer.renderer.info.memory.textures,
              meshCount: metrics?.meshCount ?? null,
              pendingMeshJobs: metrics?.pendingMeshJobs ?? null,
            });
          }
        }
      } catch (error) {
        leak.failed = String(error?.message ?? error);
      }
    }

    const captureDataUrl = lastCapture?.dataUrl ?? null;
    let captureSha256 = null;
    let capturePngBytes = null;
    if (captureDataUrl) {
      const binary = atob(
        captureDataUrl.slice("data:image/png;base64,".length),
      );
      const bytes = new Uint8Array(binary.length);
      for (let index = 0; index < binary.length; index += 1)
        bytes[index] = binary.charCodeAt(index);
      const digest = await crypto.subtle.digest("SHA-256", bytes);
      captureSha256 = [...new Uint8Array(digest)]
        .map((value) => value.toString(16).padStart(2, "0"))
        .join("");
      capturePngBytes = bytes.length;
    }

    return {
      environment: {
        userAgent: navigator.userAgent,
        webglVersion: gl.getParameter(gl.VERSION),
        shadingLanguage: gl.getParameter(gl.SHADING_LANGUAGE_VERSION),
        gpuVendor: debugInfo
          ? gl.getParameter(debugInfo.UNMASKED_VENDOR_WEBGL)
          : null,
        gpuRenderer: debugInfo
          ? gl.getParameter(debugInfo.UNMASKED_RENDERER_WEBGL)
          : null,
        maxTextureSize: gl.getParameter(gl.MAX_TEXTURE_SIZE),
        gpuTimerAvailable: Boolean(gpuTimer),
        glError: gl.getError(),
      },
      scenario: {
        width: config.width,
        height: config.height,
        maxRadius: config.maxRadius,
        totalChunks: sourceChunks.length,
        nonAirBlocks: allNonAir,
        radii: config.radii,
        batchSizes: config.batchSizes,
        worldModel:
          "synthetic 7-layer stone base, grass surface, bounded oak-plank towers",
      },
      snapshots,
      streaming: stream,
      overlays,
      leak,
      capture: {
        pngBytes: capturePngBytes,
        pngSha256: captureSha256,
        dataUrl: captureDataUrl,
      },
      rendererInfo: finalRendererInfo,
      memory: finalMemory,
      heapUsedBytes: heap(),
    };
  } finally {
    frameProbeActive = false;
    gcObserver?.disconnect();
    longTaskObserver?.disconnect();
    renderer?.dispose();
    host.remove();
  }
}

try {
  let port;
  let page;
  for (let attempt = 0; attempt < 120 && !page; attempt++) {
    if (app.exitCode !== null)
      throw new Error(`Tauri exited with ${app.exitCode}`);
    try {
      if (!port) {
        const activePort = await readFile(
          join(profile, "EBWebView", "DevToolsActivePort"),
          "utf8",
        );
        const candidate = Number(activePort.split(/\r?\n/, 1)[0]);
        if (!Number.isInteger(candidate) || candidate < 1 || candidate > 65535)
          throw new Error("invalid WebView2 DevTools port");
        port = candidate;
      }
      const response = await fetch(`http://127.0.0.1:${port}/json/list`, {
        signal: AbortSignal.timeout(1000),
      });
      if (response.ok)
        page = (await response.json()).find((item) => item.type === "page");
    } catch {
      // The WebView2 process may still be starting.
    }
    if (!page) await delay(250);
  }
  if (!page)
    throw new Error("new Tauri WebView2 did not expose a DevTools page");
  const debuggerUrl = new URL(page.webSocketDebuggerUrl);
  if (
    debuggerUrl.protocol !== "ws:" ||
    debuggerUrl.hostname !== "127.0.0.1" ||
    debuggerUrl.port !== String(port)
  )
    throw new Error("WebView2 returned a non-loopback DevTools URL");
  ws = new WebSocket(debuggerUrl);
  await new Promise((resolvePromise, reject) => {
    ws.onopen = resolvePromise;
    ws.onerror = reject;
  });
  ws.onmessage = (event) => {
    const result = JSON.parse(event.data);
    const waiter = pending.get(result.id);
    if (!waiter) return;
    pending.delete(result.id);
    if (result.error) waiter.reject(new Error(JSON.stringify(result.error)));
    else waiter.resolve(result.result);
  };

  await send("Page.enable");
  for (let attempt = 0; attempt < 80; attempt++) {
    try {
      if (await evaluate('document.readyState === "complete"')) break;
    } catch {
      // Initial navigation is still replacing the execution context.
    }
    await delay(250);
  }
  let ready = false;
  for (let attempt = 0; attempt < 80 && !ready; attempt++) {
    try {
      if (attempt % 8 === 0) await send("Page.navigate", { url: `${origin}/` });
      const state = await evaluate(
        "({ href: location.href, origin: location.origin, state: document.readyState })",
      );
      ready = state.origin === origin && state.state === "complete";
    } catch {
      // Execution context is replaced during navigation.
    }
    if (!ready) await delay(250);
  }
  if (!ready) throw new Error("WebView2 did not load the local Vite frontend");

  const config = {
    radii,
    maxRadius: Math.max(...radii),
    width: 1280,
    height: 720,
    batchSizes: [16, 32, 64],
    leakCycles,
  };
  if (profileEnabled) {
    await send("Profiler.enable");
    await send("Profiler.setSamplingInterval", { interval: 250 });
    await send("Profiler.start");
  }
  const result = await evaluate(
    `(${stressScene.toString()})(${JSON.stringify(config)})`,
  );
  if (profileEnabled) {
    const stopped = await send("Profiler.stop");
    const summary = summarizeProfile(stopped.profile);
    await writeFile(
      join(output, "cpu-profile-summary.json"),
      JSON.stringify(summary, null, 2) + "\n",
      { flag: "wx" },
    );
    result.profiling = {
      enabled: true,
      samplingIntervalMicros: 250,
      summaryFile: "cpu-profile-summary.json",
      note: "The profiled run has profiling overhead; use it for relative hot spots, not for the limit table.",
    };
  }
  if (result.environment.glError !== 0)
    throw new Error(`WebGL error: ${result.environment.glError}`);

  const base64 =
    result.capture.dataUrl?.slice("data:image/png;base64,".length) ?? null;
  delete result.capture.dataUrl;
  if (base64) {
    const png = Buffer.from(base64, "base64");
    if (!png.subarray(0, 8).equals(Buffer.from("89504e470d0a1a0a", "hex")))
      throw new Error("invalid PNG signature");
    result.capture.pngBytes = png.length;
    result.capture.pngSha256 = createHash("sha256").update(png).digest("hex");
    await writeFile(join(output, "viewport.png"), png, { flag: "wx" });
  }

  result.kind = "webview2-large-map-stress";
  result.schemaVersion = 2;
  result.generatedAt = new Date().toISOString();
  result.executableSha256 = executableSha256;
  result.node = process.version;
  result.os = { platform: process.platform, release: release() };
  result.cpu = { model: cpus()[0]?.model ?? null, logical: cpus().length };
  result.totalMemoryBytes = totalmem();
  result.durationMs = Date.now() - startedAt;
  await writeFile(
    join(output, "metrics.json"),
    JSON.stringify(result, null, 2) + "\n",
    { flag: "wx" },
  );
  console.log(JSON.stringify(result, null, 2));
} finally {
  ws?.close();
  if (app.exitCode === null) app.kill();
}
