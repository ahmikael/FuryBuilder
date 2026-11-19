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
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: "--remote-debugging-port=0",
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
  const stateOf = (name) => ({ name, properties: {} });

  let renderer;
  try {
    let metrics = null;
    let metricUpdates = 0;
    renderer = new ViewportRenderer(host, {
      onMetrics: (value) => {
        metrics = value;
        metricUpdates += 1;
      },
    });
    renderer.setChunkBordersVisible(false);
    renderer.setGridVisible(false);
    const gl = renderer.renderer.getContext();
    if (!(gl instanceof WebGL2RenderingContext))
      throw new Error("WebGL2 context unavailable");
    const debugInfo = gl.getExtension("WEBGL_debug_renderer_info");

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

    const waitForMetrics = async (expectedChunks, timeoutMs) => {
      const deadline = performance.now() + timeoutMs;
      while (
        (!metrics || metrics.loadedChunks !== expectedChunks) &&
        performance.now() < deadline
      ) {
        await nextFrame();
      }
      if (!metrics || metrics.loadedChunks !== expectedChunks)
        throw new Error(
          `metrics for ${expectedChunks} chunks did not settle (last ${metrics?.loadedChunks ?? "none"})`,
        );
      // Let the short-window FPS average cover frames after the synchronous setup.
      await wait(1800);
      return metrics;
    };

    const heap = () =>
      typeof performance.memory === "object" && performance.memory
        ? performance.memory.usedJSHeapSize
        : null;

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
        } catch (error) {
          run.failed = String(error?.message ?? error);
        }
        stream.runs.push(run);
      }
    } catch (error) {
      stream.failed = String(error?.message ?? error);
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
      capture: {
        pngBytes: capturePngBytes,
        pngSha256: captureSha256,
        dataUrl: captureDataUrl,
      },
      heapUsedBytes: heap(),
    };
  } finally {
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
  };
  const result = await evaluate(
    `(${stressScene.toString()})(${JSON.stringify(config)})`,
  );
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
  result.schemaVersion = 1;
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
