/**
 * Windows-only hardware smoke test of the actual Three.js viewport renderer.
 *
 * Prerequisites: a built target/debug/furybuilder-desktop.exe and the desktop
 * Vite dev server on 127.0.0.1:1420. This script starts its own Tauri process
 * with an isolated WebView2 profile and a loopback-only DevTools port. It
 * never attaches to, controls, or closes an existing user process.
 *
 * Usage: node scripts/webview2-smoke.mjs <new-output-directory>
 */
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdir, mkdtemp, readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const appPath = join(root, "target", "debug", "furybuilder-desktop.exe");
const origin = "http://127.0.0.1:1420";

if (process.platform !== "win32" || process.argv.length !== 3) {
  throw new Error(
    "Usage on Windows: node scripts/webview2-smoke.mjs <new-output-directory>",
  );
}

const output = resolve(process.argv[2]);
await mkdir(output); // Fail rather than overwrite existing evidence.
const profile = await mkdtemp(join(tmpdir(), "fury-webview-"));

// Do not accidentally test a different checkout already listening on the port.
// Vite embeds its original source in the dev-only inline source map.
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
  if (!sourceMap.sourcesContent?.includes(localSource)) {
    throw new Error(
      `Vite serves a different ${name}.ts; start it from this checkout`,
    );
  }
}
const executable = await readFile(appPath);
const executableSha256 = createHash("sha256").update(executable).digest("hex");
const app = spawn(appPath, [], {
  cwd: root,
  env: {
    ...process.env,
    WEBVIEW2_USER_DATA_FOLDER: profile,
    // Chromium chooses a free loopback port and writes it inside this private
    // profile. This avoids accidentally attaching to another process in a race.
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
    }, 180_000);
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
  if (result.exceptionDetails) {
    throw new Error(
      result.exceptionDetails.exception?.description ??
        result.exceptionDetails.text,
    );
  }
  return result.result.value;
}

// This function executes inside the new Tauri WebView2, not in Node or a
// separate browser. It uses the project's real mesher and viewport renderer.
async function renderScene() {
  const [{ ViewportRenderer }, { SnapshotBuilder }] = await Promise.all([
    import("/src/renderer/viewport-renderer.ts"),
    import("/src/renderer/builders.ts"),
  ]);
  const host = document.createElement("div");
  Object.assign(host.style, {
    position: "fixed",
    left: "0",
    top: "0",
    width: "1280px",
    height: "720px",
    zIndex: "2147483647",
  });
  document.body.appendChild(host);
  let renderer;
  try {
    let metrics = null;
    renderer = new ViewportRenderer(host, {
      onMetrics: (value) => (metrics = value),
    });
    renderer.setChunkBordersVisible(false);
    renderer.setGridVisible(false);
    const gl = renderer.renderer.getContext();
    if (!(gl instanceof WebGL2RenderingContext))
      throw new Error("WebGL2 context unavailable");

    const builder = new SnapshotBuilder();
    for (let cx = -8; cx <= 8; cx++) {
      for (let cz = -8; cz <= 8; cz++) {
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
    const snapshot = builder.build("minecraft:overworld", 1, 8);
    const nonAirBlocks = snapshot.chunks
      .flatMap((chunk) => chunk.sections)
      .reduce((sum, section) => sum + section.nonAir, 0);
    if (nonAirBlocks !== 616_752)
      throw new Error(`unexpected scene size: ${nonAirBlocks}`);
    const started = performance.now();
    renderer.setSnapshot(snapshot);
    const setupMs = performance.now() - started;
    renderer.setCameraPose([142, 160, 142], [0, 64, 0]);

    // Wait for real animation frames and the renderer's metrics callback.
    const deadline = performance.now() + 15_000;
    while (
      (!metrics || metrics.loadedChunks !== 289) &&
      performance.now() < deadline
    ) {
      await new Promise((resolvePromise) =>
        requestAnimationFrame(resolvePromise),
      );
    }
    if (!metrics || metrics.loadedChunks !== 289)
      throw new Error("scene metrics did not settle");
    await new Promise((resolvePromise) => setTimeout(resolvePromise, 1200));

    const capture = renderer.capture();
    if (
      capture.metadata.partial ||
      capture.metadata.loadedChunks.length !== 289
    ) {
      throw new Error("capture did not include all 289 requested chunks");
    }
    if (!capture.dataUrl.startsWith("data:image/png;base64,"))
      throw new Error("capture is not PNG");

    // Decode the PNG and verify that it contains a scene, not a blank canvas.
    const image = new Image();
    image.src = capture.dataUrl;
    await image.decode();
    const sampleCanvas = document.createElement("canvas");
    sampleCanvas.width = 64;
    sampleCanvas.height = 36;
    const context = sampleCanvas.getContext("2d");
    context.drawImage(image, 0, 0, 64, 36);
    const pixels = context.getImageData(0, 0, 64, 36).data;
    const colors = new Set();
    for (let i = 0; i < pixels.length; i += 4) {
      colors.add(`${pixels[i]},${pixels[i + 1]},${pixels[i + 2]}`);
    }
    if (colors.size < 16)
      throw new Error(`capture has only ${colors.size} sampled colors`);
    const extension = gl.getExtension("WEBGL_debug_renderer_info");
    return {
      userAgent: navigator.userAgent,
      webglVersion: gl.getParameter(gl.VERSION),
      shadingLanguage: gl.getParameter(gl.SHADING_LANGUAGE_VERSION),
      gpuVendor: extension
        ? gl.getParameter(extension.UNMASKED_VENDOR_WEBGL)
        : null,
      gpuRenderer: extension
        ? gl.getParameter(extension.UNMASKED_RENDERER_WEBGL)
        : null,
      glError: gl.getError(),
      scene: {
        chunks: 289,
        nonAirBlocks,
        setupMs: Math.round(setupMs),
        metrics,
      },
      capture: {
        resolution: capture.metadata.resolution,
        worldRevision: capture.metadata.worldRevision,
        loadedChunks: capture.metadata.loadedChunks.length,
        missingChunks: capture.metadata.missingChunks.length,
        partial: capture.metadata.partial,
        sampledColors: colors.size,
        dataUrl: capture.dataUrl,
      },
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
        if (
          !Number.isInteger(candidate) ||
          candidate < 1 ||
          candidate > 65535
        ) {
          throw new Error("invalid WebView2 DevTools port");
        }
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
  ) {
    throw new Error("WebView2 returned a non-loopback DevTools URL");
  }
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
  // The WebView's initial Tauri navigation must settle before replacing it;
  // otherwise the application's startup navigation can overwrite this one.
  for (let attempt = 0; attempt < 80; attempt++) {
    try {
      if (await evaluate('document.readyState === "complete"')) break;
    } catch {
      // Initial navigation is still replacing the execution context.
    }
    await delay(250);
  }
  let ready = false;
  let lastPageState = null;
  let lastNavigation = null;
  for (let attempt = 0; attempt < 80 && !ready; attempt++) {
    try {
      // Tauri may finish its own initial navigation after DevTools connects.
      // Retry until the Vite page, rather than tauri.localhost, settles.
      if (attempt % 8 === 0)
        lastNavigation = await send("Page.navigate", { url: `${origin}/` });
      lastPageState = await evaluate(
        "({ href: location.href, origin: location.origin, state: document.readyState })",
      );
      ready =
        lastPageState.origin === origin && lastPageState.state === "complete";
    } catch {
      // Execution context is replaced during navigation.
    }
    if (!ready) await delay(250);
  }
  if (!ready)
    throw new Error(
      `WebView2 did not load the local Vite frontend: ${JSON.stringify({ lastPageState, lastNavigation })}`,
    );

  const result = await evaluate(`(${renderScene.toString()})()`);
  if (result.glError !== 0) throw new Error(`WebGL error: ${result.glError}`);
  const base64 = result.capture.dataUrl.slice("data:image/png;base64,".length);
  delete result.capture.dataUrl;
  const png = Buffer.from(base64, "base64");
  if (!png.subarray(0, 8).equals(Buffer.from("89504e470d0a1a0a", "hex"))) {
    throw new Error("invalid PNG signature");
  }
  result.capture.pngBytes = png.length;
  result.capture.pngSha256 = createHash("sha256").update(png).digest("hex");
  result.executableSha256 = executableSha256;
  result.capturedAt = new Date().toISOString();
  await writeFile(join(output, "viewport.png"), png, { flag: "wx" });
  await writeFile(
    join(output, "metrics.json"),
    JSON.stringify(result, null, 2) + "\n",
    {
      flag: "wx",
    },
  );
  console.log(JSON.stringify(result, null, 2));
} finally {
  ws?.close();
  if (app.exitCode === null) app.kill();
}
