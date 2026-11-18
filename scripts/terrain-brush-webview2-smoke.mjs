/**
 * M9.9 desktop smoke: terrain brushes through the real WebView2 UI.
 *
 * Starts the Tauri app with isolated app data, creates a Flat base project through the
 * project dialog, selects a terrain area in the Three.js viewport, previews and applies a
 * raise brush through the Terrain panel, verifies the Rust world data, then undoes it.
 *
 * Usage (Vite dev server must already run at 127.0.0.1:1420):
 *   node scripts/terrain-brush-webview2-smoke.mjs <out.json> <out.png>
 */
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const appPath = join(root, "target", "debug", "furybuilder-desktop.exe");
const origin = "http://127.0.0.1:1420";
const output = process.argv[2] ? resolve(process.argv[2]) : null;
const screenshotPath = process.argv[3] ? resolve(process.argv[3]) : null;

if (process.platform !== "win32" || !output || !screenshotPath) {
  throw new Error(
    "Usage on Windows: node scripts/terrain-brush-webview2-smoke.mjs <out.json> <out.png>",
  );
}

try {
  const response = await fetch(
    `${origin}/src/features/workspace/TerrainPanel.tsx`,
    { signal: AbortSignal.timeout(5000) },
  );
  if (!response.ok) {
    throw new Error(`Start this checkout's Vite server at ${origin}`);
  }
} catch {
  throw new Error(`Start this checkout's Vite server at ${origin}`);
}

const appData = await mkdtemp(join(tmpdir(), "fury-terrain-brush-data-"));
const profile = await mkdtemp(join(tmpdir(), "fury-terrain-brush-webview-"));
const projectName = "Terrain Brush Smoke";
const executableSha256 = createHash("sha256")
  .update(await readFile(appPath))
  .digest("hex");

const app = spawn(appPath, [], {
  cwd: root,
  env: {
    ...process.env,
    APPDATA: appData,
    FURYBUILDER_APP_DATA_DIR: appData,
    WEBVIEW2_USER_DATA_FOLDER: profile,
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: "--remote-debugging-port=0",
  },
  stdio: "ignore",
});

let ws;
let nextId = 0;
const pending = new Map();
const consoleErrors = [];

function send(method, params = {}, timeoutMs = 180_000) {
  const id = ++nextId;
  return new Promise((resolvePromise, reject) => {
    const timeout = setTimeout(() => {
      pending.delete(id);
      reject(new Error(`DevTools request timed out: ${method}`));
    }, timeoutMs);
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

async function invoke(command, args = {}) {
  return evaluate(
    `window.__TAURI_INTERNALS__.invoke(${JSON.stringify(command)}, ${JSON.stringify(args)})`,
  );
}

async function waitFor(expression, timeoutMs = 90_000) {
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

async function waitForInvoke(predicate, timeoutMs = 90_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const value = await predicate();
      if (value) return value;
    } catch {
      // The command may not be registered yet.
    }
    await delay(500);
  }
  return null;
}

async function setInputById(id, value) {
  return evaluate(`(() => {
    const element = document.querySelector('input[id="${id}"]');
    if (!element) return false;
    const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set;
    setter.call(element, ${JSON.stringify(String(value))});
    element.dispatchEvent(new Event("input", { bubbles: true }));
    return true;
  })()`);
}

async function setInputByAriaLabel(ariaLabel, value) {
  return evaluate(`(() => {
    const element = document.querySelector('input[aria-label="${ariaLabel}"]');
    if (!element) return false;
    const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set;
    setter.call(element, ${JSON.stringify(String(value))});
    element.dispatchEvent(new Event("input", { bubbles: true }));
    return true;
  })()`);
}

async function clickByAriaLabel(ariaLabel) {
  return evaluate(`(() => {
    const button = document.querySelector('button[aria-label="${ariaLabel}"]');
    if (!button || button.disabled) return false;
    button.click();
    return true;
  })()`);
}

async function clickByText(text) {
  return evaluate(`(() => {
    const element = Array.from(document.querySelectorAll("button")).find(
      (candidate) => candidate.textContent.trim() === ${JSON.stringify(text)},
    );
    if (!element || element.disabled) return false;
    element.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, cancelable: true, view: window }));
    element.dispatchEvent(new MouseEvent("mouseup", { bubbles: true, cancelable: true, view: window }));
    element.click();
    return true;
  })()`);
}

async function clickTabByText(text) {
  return evaluate(`(() => {
    const tab = Array.from(document.querySelectorAll('[role="tab"]')).find(
      (candidate) => candidate.textContent.includes(${JSON.stringify(text)}),
    );
    if (!tab || tab.disabled) return false;
    tab.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, cancelable: true, view: window }));
    tab.dispatchEvent(new MouseEvent("mouseup", { bubbles: true, cancelable: true, view: window }));
    tab.click();
    return true;
  })()`);
}

async function clickViewport(x, y) {
  return evaluate(`(() => {
    const element = document.querySelector('[data-testid="viewport-canvas"]');
    if (!element) return false;
    const base = {
      bubbles: true,
      cancelable: true,
      composed: true,
      clientX: ${x},
      clientY: ${y},
      button: 0,
      buttons: 1,
      pointerId: 1,
      pointerType: "mouse",
      isPrimary: true,
      view: window,
    };
    element.dispatchEvent(new PointerEvent("pointerdown", base));
    element.dispatchEvent(new PointerEvent("pointerup", { ...base, buttons: 0 }));
    return true;
  })()`);
}

async function screenshot() {
  const capture = await send("Page.captureScreenshot", {
    format: "png",
    captureBeyondViewport: false,
    fromSurface: true,
  });
  return Buffer.from(capture.data, "base64");
}

function countState(region, name, y) {
  return region.blocks.filter(
    (block) => block.state.name === name && block.pos.y === y,
  ).length;
}

try {
  let port;
  let page;
  for (let attempt = 0; attempt < 120 && !page; attempt++) {
    if (app.exitCode !== null) {
      throw new Error(`Tauri exited with ${app.exitCode}`);
    }
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
      if (list.ok) {
        page = (await list.json()).find((item) => item.type === "page");
      }
    } catch {
      // WebView2 may still be starting.
    }
    if (!page) await delay(250);
  }
  if (!page) {
    throw new Error("new Tauri WebView2 did not expose a DevTools page");
  }
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
    if (
      result.method === "Runtime.consoleAPICalled" &&
      result.params.type === "error"
    ) {
      consoleErrors.push(
        result.params.args
          .map((argument) => argument.value ?? argument.description)
          .join(" "),
      );
      return;
    }
    if (result.method === "Runtime.exceptionThrown") {
      consoleErrors.push(
        result.params.exceptionDetails.exception?.description ??
          result.params.exceptionDetails.text,
      );
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

  const opened = await clickByText("Project");
  if (!opened) throw new Error("Project manager button was not found");
  if (
    !(await waitFor(
      "document.querySelectorAll('[role=\"tab\"]').length > 0",
      30_000,
    ))
  ) {
    throw new Error("Project manager tabs did not render");
  }
  if (!(await clickTabByText("New project"))) {
    throw new Error("New project tab was not found");
  }
  if (
    !(await waitFor(
      'Array.from(document.querySelectorAll("button")).some((button) => button.textContent.includes("Flat base"))',
      30_000,
    ))
  ) {
    throw new Error("Flat base creation mode was not found");
  }
  await evaluate(`(() => {
    const button = Array.from(document.querySelectorAll("button")).find(
      (candidate) => candidate.textContent.includes("Flat base"),
    );
    if (!button || button.disabled) return false;
    button.click();
    return true;
  })()`);
  if (!(await setInputById("project-name", projectName))) {
    throw new Error("Project name input was not found");
  }
  if (!(await clickByText("Create project"))) {
    throw new Error("Create project button was not clickable");
  }
  const projectList = await waitForInvoke(async () => {
    const list = await invoke("project_list");
    return list.projects.length === 1 ? list : null;
  }, 120_000);
  if (!projectList) throw new Error("the flat project was not created");
  const project = projectList.projects[0];
  if (
    !(await waitFor("!document.querySelector('[role=\"dialog\"]')", 30_000))
  ) {
    throw new Error("the project dialog did not close");
  }
  if (
    !(await waitFor(
      "Boolean(document.querySelector('[data-testid=\"viewport-canvas\"] canvas'))",
      60_000,
    ))
  ) {
    throw new Error("the viewport canvas did not render");
  }
  await waitFor("document.body.innerText.includes('Rust core')", 60_000);
  await delay(1500);

  const before = await invoke("world_info");
  if (before.chunk_count === 0 || before.block_count === 0) {
    throw new Error("the flat project did not load editable blocks");
  }
  const beforeRegion = await invoke("read_region", {
    dimension: "minecraft:overworld",
    bounds: {
      min: { x: -16, y: 63, z: -16 },
      max: { x: 16, y: 66, z: 16 },
    },
  });
  if (countState(beforeRegion, "minecraft:stone_bricks", 65) !== 0) {
    throw new Error(
      "the flat base unexpectedly already contains brush material",
    );
  }

  if (!(await clickByAriaLabel("Box select"))) {
    throw new Error("Box select tool was not found");
  }
  if (
    !(await waitFor(
      'Boolean(window.__furyViewport && typeof window.__furyViewport.pickAt === "function" && typeof window.__furyViewport.setCameraPose === "function")',
      30_000,
    ))
  ) {
    throw new Error("the viewport renderer debug hook was not available");
  }
  // Scripts may place the real camera explicitly. The same screen points are then verified
  // with the renderer picker before real pointer events are dispatched, so chunk loading and
  // camera drift cannot silently turn this into a coordinate lottery.
  await evaluate(
    "window.__furyViewport.setCameraPose([0, 84, 22], [0, 64, 0])",
  );
  const rect = await evaluate(`(() => {
    const element = document.querySelector('[data-testid="viewport-canvas"]');
    const box = element.getBoundingClientRect();
    return { left: box.left, top: box.top, width: box.width, height: box.height };
  })()`);
  const pair = await waitForInvoke(async () => {
    const result = await evaluate(`(() => {
      const view = window.__furyViewport;
      const element = document.querySelector('[data-testid="viewport-canvas"]');
      const box = element.getBoundingClientRect();
      const candidates = [
        [[-48, 0], [48, 0]],
        [[-64, 20], [64, 20]],
        [[-40, -20], [40, 20]],
        [[-80, 0], [0, 40]],
      ];
      for (const candidate of candidates) {
        const points = candidate.map(([dx, dy]) => {
          const x = box.left + box.width / 2 + dx;
          const y = box.top + box.height / 2 + dy;
          const hit = view.pickAt(x, y);
          return hit ? { x, y, pos: hit.pos, face: hit.face } : null;
        });
        if (!points[0] || !points[1]) continue;
        const dx = Math.abs(points[0].pos.x - points[1].pos.x);
        const dz = Math.abs(points[0].pos.z - points[1].pos.z);
        if (dx + dz < 2) continue;
        return { points };
      }
      return { points: null };
    })()`);
    return result.points;
  }, 30_000);
  if (!pair) {
    throw new Error(
      "the viewport raycast did not produce two separate terrain hits",
    );
  }
  const first = pair[0];
  const second = pair[1];
  await clickViewport(first.x, first.y);
  if (
    !(await waitFor(
      'document.body.innerText.includes("Box corner set")',
      10_000,
    ))
  ) {
    throw new Error("the first viewport click did not select a block corner");
  }
  await clickViewport(second.x, second.y);
  if (
    !(await waitFor('document.body.innerText.includes("Selected ")', 10_000))
  ) {
    throw new Error(
      "the second viewport click did not finish the box selection",
    );
  }

  if (!(await clickTabByText("Terrain"))) {
    throw new Error("the Terrain tab was not found");
  }
  if (
    !(await waitFor(
      "document.body.innerText.includes('Terrain brush')",
      30_000,
    ))
  ) {
    throw new Error("the terrain brush panel did not render");
  }
  if (
    !(await waitFor(
      '!document.body.innerText.includes("Use the Box select tool")',
      10_000,
    ))
  ) {
    throw new Error("the terrain panel did not receive the box selection");
  }
  if (!(await setInputByAriaLabel("Terrain brush amount", "2"))) {
    throw new Error("the terrain amount input was not found");
  }
  if (!(await clickByAriaLabel("Preview terrain brush"))) {
    throw new Error("the terrain preview button was not clickable");
  }
  if (
    !(await waitFor(
      "Boolean(document.querySelector('[data-testid=\"terrain-preview\"]'))",
      60_000,
    ))
  ) {
    throw new Error("the terrain preview did not produce a result");
  }
  const previewText = await evaluate(
    "document.querySelector('[data-testid=\"terrain-preview\"]').innerText",
  );
  if (!/would change/.test(previewText)) {
    throw new Error(
      `the terrain preview is not a real estimate: ${previewText}`,
    );
  }
  const panelScreenshot = await screenshot();
  await writeFile(screenshotPath, panelScreenshot, { flag: "wx" });

  if (!(await clickByAriaLabel("Apply terrain brush"))) {
    throw new Error("the terrain apply button was not clickable");
  }
  const after = await waitForInvoke(async () => {
    const info = await invoke("world_info");
    return info.revision > before.revision ? info : null;
  }, 120_000);
  if (!after) throw new Error("the terrain brush did not commit a revision");
  const afterRegion = await invoke("read_region", {
    dimension: "minecraft:overworld",
    bounds: {
      min: { x: -16, y: 63, z: -16 },
      max: { x: 16, y: 66, z: 16 },
    },
  });
  const raisedBlocks = countState(afterRegion, "minecraft:stone_bricks", 65);
  if (raisedBlocks === 0) {
    throw new Error(
      "the applied raise brush did not add blocks above the surface",
    );
  }
  const appliedScreenshot = await screenshot();
  const appliedScreenshotPath = screenshotPath.replace(
    /\.png$/i,
    "-applied.png",
  );
  await writeFile(appliedScreenshotPath, appliedScreenshot, { flag: "wx" });

  const historyAfter = await invoke("history_state");
  if (!historyAfter.canUndo || historyAfter.entries.length === 0) {
    throw new Error("the terrain brush is missing from undo history");
  }
  if (!(await clickByAriaLabel("Undo edit"))) {
    throw new Error("the Undo edit button was not clickable");
  }
  const undone = await waitForInvoke(async () => {
    const info = await invoke("world_info");
    return info.revision > after.revision ? info : null;
  }, 120_000);
  if (!undone) throw new Error("undo did not commit a new revision");
  const undoRegion = await invoke("read_region", {
    dimension: "minecraft:overworld",
    bounds: {
      min: { x: -16, y: 63, z: -16 },
      max: { x: 16, y: 66, z: 16 },
    },
  });
  const raisedAfterUndo = countState(undoRegion, "minecraft:stone_bricks", 65);
  if (raisedAfterUndo !== 0) {
    throw new Error("undo left terrain brush blocks behind");
  }

  const evidence = {
    tool: { name: "furybuilder-terrain-brush-ui", version: 1 },
    generatedAt: new Date().toISOString(),
    target: "Minecraft Java 1.21.4 / M9.9 deterministic terrain brushes",
    executableSha256,
    project: {
      name: project.name,
      folder: project.folder,
      creationMode: project.creationMode,
    },
    viewport: {
      selectionClicks: "real viewport pointer events",
      canvas: rect,
    },
    panel: {
      kind: "raise",
      amount: 2,
      previewText,
      previewScreenshot: {
        path: screenshotPath,
        bytes: panelScreenshot.length,
      },
      appliedScreenshot: {
        path: appliedScreenshotPath,
        bytes: appliedScreenshot.length,
      },
    },
    world: {
      revisionBefore: before.revision,
      revisionAfter: after.revision,
      revisionAfterUndo: undone.revision,
      chunks: before.chunk_count,
      blocks: before.block_count,
      stoneBricksAtY65Before: countState(
        beforeRegion,
        "minecraft:stone_bricks",
        65,
      ),
      stoneBricksAtY65After: raisedBlocks,
      stoneBricksAtY65AfterUndo: raisedAfterUndo,
      canUndoAfterApply: historyAfter.canUndo,
      historyEntriesAfterApply: historyAfter.entries.length,
    },
    conclusions: {
      realFlatProjectOpened: project.creationMode === "flat",
      realSelectionProduced: true,
      previewWasNonEmpty: /would change/.test(previewText),
      brushCommittedRevision: after.revision > before.revision,
      brushRaisedTerrain: raisedBlocks > 0,
      undoRestoredTerrain: raisedAfterUndo === 0,
      undoRecordedInHistory: historyAfter.canUndo,
      noConsoleErrors: consoleErrors.length === 0,
    },
    consoleErrors,
    limitations: [
      "the viewport selection used synthetic pointer events on the real viewport element; all subsequent operations used the real Tauri backend",
      "the flat base is deliberate manual geometry, not Minecraft world generation",
      "the brush raises the top surface inside the selected footprint; biomes and derived data are outside this smoke",
    ],
  };
  const failed = Object.entries(evidence.conclusions).filter(
    ([, value]) => value !== true,
  );
  if (failed.length > 0) {
    throw new Error(
      `terrain brush conclusions failed: ${JSON.stringify(failed)}`,
    );
  }
  await writeFile(output, JSON.stringify(evidence, null, 2) + "\n", {
    flag: "wx",
  });
  console.log(
    JSON.stringify({ output, conclusions: evidence.conclusions }, null, 2),
  );
} finally {
  ws?.close();
  if (app.exitCode === null) app.kill();
  if (app.exitCode === null) {
    await new Promise((resolvePromise) => {
      const timer = setTimeout(resolvePromise, 10_000);
      app.once("exit", () => {
        clearTimeout(timer);
        resolvePromise();
      });
    });
  }
  // WebView2 releases its profile lock asynchronously; retry instead of masking the
  // real test result with an EBUSY cleanup error.
  for (let attempt = 0; attempt < 20; attempt++) {
    try {
      await rm(appData, { recursive: true, force: true });
      await rm(profile, { recursive: true, force: true });
      break;
    } catch {
      await delay(500);
    }
  }
}
