/**
 * M9.2 desktop render smoke test.
 *
 * Starts the real Tauri app with an isolated APPDATA + WebView2 profile, selects the
 * supplied launcher root, loads the deterministic demo world, waits for the verified
 * blockstate/model/texture bundle to become active, and captures the actual WebGL
 * viewport. The screenshot is decoded and checked for a non-blank, varied scene.
 *
 * Usage (Vite dev server must already run at 127.0.0.1:1420):
 *   node scripts/assets-render-webview2-smoke.mjs <new-output-json> <new-output-png> <launcher-root>
 */
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdir, mkdtemp, readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath } from "node:url";
import { inflateSync } from "node:zlib";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const appPath = join(root, "target", "debug", "furybuilder-desktop.exe");
const origin = "http://127.0.0.1:1420";

if (process.platform !== "win32" || process.argv.length !== 5) {
  throw new Error(
    "Usage on Windows: node scripts/assets-render-webview2-smoke.mjs <new-output-json> <new-output-png> <launcher-root>",
  );
}
const output = resolve(process.argv[2]);
const screenshotPath = resolve(process.argv[3]);
const launcherRoot = resolve(process.argv[4]);
await mkdir(dirname(output), { recursive: true });
await mkdir(dirname(screenshotPath), { recursive: true });
const profile = await mkdtemp(join(tmpdir(), "fury-assets-render-"));
const appData = join(profile, "appdata");
await mkdir(appData, { recursive: true });

let response;
try {
  response = await fetch(`${origin}/src/features/viewport/Viewport.tsx`, {
    signal: AbortSignal.timeout(5000),
  });
} catch {
  throw new Error(`Start this checkout's Vite server at ${origin}`);
}
if (!response.ok)
  throw new Error(`Vite did not serve the viewport at ${origin}`);

const executable = await readFile(appPath);
const executableSha256 = createHash("sha256").update(executable).digest("hex");
const app = spawn(appPath, [], {
  cwd: root,
  env: {
    ...process.env,
    APPDATA: appData,
    WEBVIEW2_USER_DATA_FOLDER: profile,
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: "--remote-debugging-port=0",
  },
  stdio: "ignore",
});

let ws;
let nextId = 0;
const pending = new Map();
const consoleLogs = [];

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

async function waitFor(expression, timeoutMs = 60_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      if (await evaluate(expression)) return true;
    } catch {
      // The execution context may still be navigating.
    }
    await delay(250);
  }
  return false;
}

function decodePng(buffer) {
  const signature = "89504e470d0a1a0a";
  if (buffer.subarray(0, 8).toString("hex") !== signature)
    throw new Error("screenshot is not a PNG");
  let offset = 8;
  let width = 0;
  let height = 0;
  let bitDepth = 0;
  let colorType = 0;
  let interlace = 0;
  const idat = [];
  while (offset < buffer.length) {
    const length = buffer.readUInt32BE(offset);
    offset += 4;
    const type = buffer.toString("ascii", offset, offset + 4);
    offset += 4;
    const data = buffer.subarray(offset, offset + length);
    offset += length + 4;
    if (type === "IHDR") {
      width = data.readUInt32BE(0);
      height = data.readUInt32BE(4);
      bitDepth = data[8];
      colorType = data[9];
      interlace = data[12];
    } else if (type === "IDAT") {
      idat.push(data);
    } else if (type === "IEND") {
      break;
    }
  }
  if (
    bitDepth !== 8 ||
    interlace !== 0 ||
    (colorType !== 2 && colorType !== 6)
  ) {
    throw new Error(
      `unsupported screenshot PNG (depth ${bitDepth}, color ${colorType}, interlace ${interlace})`,
    );
  }
  const channels = colorType === 6 ? 4 : 3;
  const stride = width * channels;
  const raw = inflateSync(Buffer.concat(idat));
  const output = Buffer.alloc(stride * height);
  let position = 0;
  for (let y = 0; y < height; y += 1) {
    const filter = raw[position++];
    const rowStart = y * stride;
    for (let x = 0; x < stride; x += 1) {
      const byte = raw[position++];
      const left = x >= channels ? output[rowStart + x - channels] : 0;
      const up = y > 0 ? output[rowStart - stride + x] : 0;
      const upLeft =
        y > 0 && x >= channels ? output[rowStart - stride + x - channels] : 0;
      let value;
      switch (filter) {
        case 0:
          value = byte;
          break;
        case 1:
          value = byte + left;
          break;
        case 2:
          value = byte + up;
          break;
        case 3:
          value = byte + ((left + up) >> 1);
          break;
        case 4: {
          const p = left + up - upLeft;
          const pa = Math.abs(p - left);
          const pb = Math.abs(p - up);
          const pc = Math.abs(p - upLeft);
          const predictor =
            pa <= pb && pa <= pc ? left : pb <= pc ? up : upLeft;
          value = byte + predictor;
          break;
        }
        default:
          throw new Error(`unsupported PNG filter ${filter}`);
      }
      output[rowStart + x] = value & 0xff;
    }
  }
  return { width, height, channels, data: output };
}

function analyzePixels(image) {
  let count = 0;
  let sum = 0;
  let sumSquares = 0;
  let green = 0;
  let blue = 0;
  let warm = 0;
  let opaque = 0;
  const colors = new Set();
  for (let index = 0; index < image.data.length; index += image.channels) {
    const r = image.data[index];
    const g = image.data[index + 1];
    const b = image.data[index + 2];
    const a = image.channels === 4 ? image.data[index + 3] : 255;
    if (a < 16) continue;
    opaque += 1;
    count += 1;
    const luminance = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    sum += luminance;
    sumSquares += luminance * luminance;
    if (g > 40 && g > r * 1.12 && g > b * 1.12) green += 1;
    if (b > 40 && b > r * 1.18 && b > g * 1.04) blue += 1;
    if (r > 60 && r > g * 1.12 && g > b * 1.08) warm += 1;
    colors.add(((r >> 3) << 10) | ((g >> 3) << 5) | (b >> 3));
  }
  const mean = count > 0 ? sum / count : 0;
  const variance =
    count > 0 ? Math.max(0, sumSquares / count - mean * mean) : 0;
  const total = image.width * image.height;
  return {
    width: image.width,
    height: image.height,
    opaqueRatio: total > 0 ? opaque / total : 0,
    meanLuminance: mean,
    standardDeviation: Math.sqrt(variance),
    uniqueColors: colors.size,
    greenRatio: count > 0 ? green / count : 0,
    blueRatio: count > 0 ? blue / count : 0,
    warmRatio: count > 0 ? warm / count : 0,
  };
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
      const list = await fetch(`http://127.0.0.1:${port}/json/list`, {
        signal: AbortSignal.timeout(1000),
      });
      if (list.ok)
        page = (await list.json()).find((item) => item.type === "page");
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
    if (result.method === "Runtime.consoleAPICalled") {
      consoleLogs.push({
        type: result.params.type,
        args: result.params.args.map(
          (argument) =>
            argument.value ??
            argument.preview ??
            argument.description ??
            argument.type,
        ),
      });
      return;
    }
    if (result.method === "Runtime.exceptionThrown") {
      consoleLogs.push({
        exception:
          result.params.exceptionDetails.exception?.description ??
          result.params.exceptionDetails.text,
      });
      return;
    }
    const waiter = pending.get(result.id);
    if (!waiter) return;
    pending.delete(result.id);
    if (result.error) waiter.reject(new Error(JSON.stringify(result.error)));
    else waiter.resolve(result.result);
  };

  await send("Page.enable");
  await send("Runtime.enable");
  let ready = false;
  for (let attempt = 0; attempt < 80 && !ready; attempt++) {
    try {
      if (attempt % 8 === 0) await send("Page.navigate", { url: `${origin}/` });
      const state = await evaluate(
        "({ origin: location.origin, state: document.readyState })",
      );
      ready = state.origin === origin && state.state === "complete";
    } catch {
      // The execution context is replaced during navigation.
    }
    if (!ready) await delay(250);
  }
  if (!ready) throw new Error("WebView2 did not load the local Vite frontend");
  const booted = await waitFor(
    'typeof window.__TAURI_INTERNALS__ === "object" && document.readyState === "complete"',
    120_000,
  );
  if (!booted) throw new Error("Tauri internals did not become available");

  const saved = await evaluate(
    `window.__TAURI_INTERNALS__.invoke("assets_save_source", { kind: "minecraft_root", path: ${JSON.stringify(launcherRoot)} })`,
  );
  const world = await evaluate(
    'window.__TAURI_INTERNALS__.invoke("load_demo_world", {})',
  );

  // Reload after loading the demo into the Rust core so the viewport starts from it.
  await send("Page.navigate", { url: `${origin}/` });
  const reloaded = await waitFor(
    'document.readyState === "complete" && typeof window.__TAURI_INTERNALS__ === "object"',
    120_000,
  );
  if (!reloaded) throw new Error("WebView2 did not reload the application");

  const assetsReady = await waitFor(
    'document.body.innerText.includes("models ·") && document.body.innerText.includes("tex")',
    30_000,
  );
  if (!assetsReady) {
    const diagnostics = await evaluate(`(async () => {
      const tauri = window.__TAURI_INTERNALS__;
      let saved = null;
      let savedError = null;
      try { saved = await tauri.invoke("assets_saved_source", {}); }
      catch (error) { savedError = String(error); }
      let bundle = null;
      let bundleError = null;
      try {
        bundle = await tauri.invoke("assets_render_bundle", {
          blockStates: [
            { name: "minecraft:stone", properties: {} },
            { name: "minecraft:grass_block", properties: { snowy: "false" } },
            { name: "minecraft:oak_stairs", properties: { facing: "north", half: "bottom", shape: "straight", waterlogged: "false" } }
          ]
        });
      } catch (error) { bundleError = String(error); }
      const assetsSpan = [...document.querySelectorAll("span")].find((span) =>
        span.textContent === "placeholder" || span.textContent.includes("models ·")
      );
      return {
        saved: saved ? { source: saved.source, reportValid: saved.report?.valid ?? null, packs: saved.packs?.length ?? null } : null,
        savedError,
        assetSpanText: assetsSpan?.textContent ?? null,
        assetSpanTitle: assetsSpan?.getAttribute("title") ?? null,
        tauriOwnProperty: Object.prototype.hasOwnProperty.call(window, "__TAURI_INTERNALS__"),
        bundleError,
        bundle: bundle ? {
          blocks: bundle.blocks?.length ?? null,
          missingModels: bundle.blocks?.filter((block) => block.missingModel).length ?? null,
          textures: bundle.textures?.length ?? null,
          atlas: [bundle.atlasWidth, bundle.atlasHeight],
          warnings: bundle.warnings?.slice(0, 5) ?? null,
        } : null,
        bodyText: document.body.innerText.slice(0, 1200),
      };
    })()`);
    diagnostics.consoleLogs = consoleLogs.slice(-40);
    throw new Error(
      `verified asset bundle did not become active: ${JSON.stringify(diagnostics)}`,
    );
  }
  const assetLabel = await evaluate(`(() => {
    const text = document.body.innerText;
    const match = text.match(/([0-9]+) models · ([0-9]+) tex/);
    return match ? match[0] : null;
  })()`);

  // The startup project overlay covers the scene. It is not part of the renderer;
  // hide it for the viewport screenshot so the reference blocks are visible.
  const overlayHidden = await evaluate(`(() => {
    const heading = [...document.querySelectorAll("h2")].find(
      (element) => element.textContent === "No project open",
    );
    const overlay = heading?.closest(".absolute.inset-0");
    if (!overlay) return false;
    overlay.style.display = "none";
    return true;
  })()`);
  if (!overlayHidden) throw new Error("startup project overlay was not found");
  await waitFor(
    '!document.body.innerText.includes("Create an Empty Project")',
    5_000,
  );

  // Give the renderer a moment to draw the rebuilt chunk meshes.
  await delay(1500);

  const rect = await evaluate(`(() => {
    const element = document.querySelector('[data-testid="viewport-canvas"]');
    if (!element) return null;
    const box = element.getBoundingClientRect();
    return { x: box.x, y: box.y, width: box.width, height: box.height };
  })()`);
  if (!rect || rect.width < 100 || rect.height < 100)
    throw new Error("viewport canvas has no usable bounds");
  const capture = await send("Page.captureScreenshot", {
    format: "png",
    captureBeyondViewport: false,
    fromSurface: true,
    clip: {
      x: Math.max(0, rect.x),
      y: Math.max(0, rect.y),
      width: Math.floor(rect.width),
      height: Math.floor(rect.height),
      scale: 1,
    },
  });
  const screenshot = Buffer.from(capture.data, "base64");
  await writeFile(screenshotPath, screenshot, { flag: "wx" });
  const pixels = analyzePixels(decodePng(screenshot));

  const diag = await evaluate(`(() => {
    const text = document.body.innerText;
    return {
      href: location.href,
      hasError: text.includes("Asset bundle") || text.includes("asset bundle failed"),
      performanceText: text.slice(text.indexOf("PERFORMANCE"), text.indexOf("PERFORMANCE") + 900),
    };
  })()`);

  const evidence = {
    tool: { name: "furybuilder-assets-render-ui", version: 1 },
    generatedAt: new Date().toISOString(),
    target: "Minecraft Java 1.21.4",
    executableSha256,
    launcherRoot,
    isolatedAppData: appData,
    savedSource: saved.source,
    world: {
      minecraftVersion:
        world.minecraftVersion ?? world.targetMinecraftVersion ?? null,
      dimensions: world.dimensions?.length ?? null,
      revision: world.revision ?? null,
      chunkCount: world.chunkCount ?? null,
      blockCount: world.blockCount ?? world.blockCount ?? null,
    },
    assetLabel,
    screenshot: {
      path: screenshotPath,
      bytes: screenshot.length,
      ...pixels,
    },
    diagnostics: diag,
    conclusions: {
      assetSourceSaved: saved.source?.kind === "minecraft_root",
      demoWorldLoaded: (world.chunkCount ?? 0) > 0,
      assetBundleActive: assetLabel !== null,
      screenshotCaptured: screenshot.length > 10_000,
      sceneNotBlank:
        pixels.standardDeviation > 12 &&
        pixels.uniqueColors > 150 &&
        pixels.opaqueRatio > 0.9,
      texturedReferenceBlocksVisible:
        pixels.greenRatio > 0.005 && pixels.warmRatio > 0.005,
      noAssetError: !diag.hasError,
      isolatedSettings: saved.source?.path === launcherRoot,
    },
    limitations: [
      "the demo scene is FuryBuilder demo content, not a Minecraft world",
      "the screenshot proves the WebGL scene and verified atlas are active; it is not a per-pixel vanilla parity test",
      "animation frames after the first frame are not played back",
      "vanilla fluids (water/lava) are rendered as a documented full-cube approximation with their real still/flow textures and a fixed tint",
      "block states without properties use a deterministic default variant with a warning",
      "the startup project overlay is hidden for the screenshot because it is not part of the renderer",
      "the launcher root and the user's real app settings are never modified",
    ],
  };
  const failed = Object.entries(evidence.conclusions).filter(
    ([, value]) => value !== true,
  );
  if (failed.length > 0) {
    throw new Error(
      `render smoke conclusions failed: ${JSON.stringify(failed)}`,
    );
  }
  await writeFile(output, JSON.stringify(evidence, null, 2) + "\n", {
    flag: "wx",
  });
  console.log(
    JSON.stringify(
      {
        output,
        screenshot: screenshotPath,
        assetLabel,
        screenshotStats: pixels,
        conclusions: evidence.conclusions,
      },
      null,
      2,
    ),
  );
} finally {
  ws?.close();
  if (app.exitCode === null) app.kill();
}
