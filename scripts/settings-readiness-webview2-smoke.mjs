/**
 * M10.5 desktop smoke for the dependency settings UI.
 *
 * Starts the real Tauri app with an isolated APPDATA/WebView2 profile, opens Settings,
 * waits for the read-only Vanilla World readiness roll-up, checks the grouped asset/Java/
 * server sections, and exercises the derived-asset-cache clear command. It never touches
 * the user's launcher, Java selection or settings.
 *
 * Usage (Vite dev server must already run at 127.0.0.1:1420):
 *   node scripts/settings-readiness-webview2-smoke.mjs <new-output-dir>
 */
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import {
  mkdir,
  mkdtemp,
  readFile,
  readdir,
  rm,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const appPath = join(root, "target", "debug", "furybuilder-desktop.exe");
const origin = "http://127.0.0.1:1420";

if (process.platform !== "win32" || process.argv.length !== 3) {
  throw new Error(
    "Usage on Windows: node scripts/settings-readiness-webview2-smoke.mjs <new-output-dir>",
  );
}
const outputDir = resolve(process.argv[2]);
await mkdir(outputDir, { recursive: true });
if ((await readdir(outputDir)).length > 0) {
  throw new Error("Output directory must be new or empty");
}

let response;
try {
  response = await fetch(`${origin}/src/features/settings/SettingsDialog.tsx`, {
    signal: AbortSignal.timeout(5000),
  });
} catch {
  throw new Error(`Start this checkout's Vite server at ${origin}`);
}
if (!response.ok) {
  throw new Error(`Vite did not serve the settings dialog at ${origin}`);
}

const executableSha256 = createHash("sha256")
  .update(await readFile(appPath))
  .digest("hex");
const profile = await mkdtemp(join(tmpdir(), "fury-settings-smoke-"));
const appData = join(profile, "appdata");
await mkdir(appData, { recursive: true });

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

function send(method, params = {}, timeoutMs = 300_000) {
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

function pngSize(buffer) {
  if (buffer.length < 24 || buffer.toString("ascii", 1, 4) !== "PNG") {
    throw new Error("DevTools did not return a PNG");
  }
  return { width: buffer.readUInt32BE(16), height: buffer.readUInt32BE(20) };
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
      // WebView2 may still be starting.
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
    'typeof window.__TAURI_INTERNALS__ === "object" && document.querySelector(\'button[aria-label="Settings"]\') !== null',
    120_000,
  );
  if (!booted)
    throw new Error(
      "Tauri internals or the Settings button did not become available",
    );

  await evaluate(
    "document.querySelector('button[aria-label=\"Settings\"]').click()",
  );
  const dialogOpen = await waitFor(
    'document.querySelector(\'[role="dialog"]\')?.textContent.includes("Minecraft assets") ?? false',
    30_000,
  );
  if (!dialogOpen) throw new Error("Settings dialog did not render");

  const settled = await waitFor(
    `(() => {
      const text = document.querySelector('[role="dialog"]')?.textContent ?? "";
      return (
        !text.includes("Scanning launcher folders") &&
        (text.includes("Vanilla World is ready") ||
          text.includes("Setup required") ||
          text.includes("Readiness check unavailable"))
      );
    })()`,
    180_000,
  );
  if (!settled) throw new Error("Dependency sections did not settle");

  const cacheClicked = await evaluate(`(() => {
    const button = [...document.querySelectorAll("button")].find(
      (candidate) => candidate.getAttribute("aria-label") === "Clear derived asset cache",
    );
    if (!button) return false;
    button.click();
    return true;
  })()`);
  if (!cacheClicked) throw new Error("Clear asset cache button was not found");
  const cacheCleared = await waitFor(
    'document.querySelector(\'[role="dialog"]\')?.textContent.includes("Derived asset cache cleared") ?? false',
    30_000,
  );
  if (!cacheCleared)
    throw new Error("Clear asset cache did not report success");

  await evaluate(`(() => {
    const heading = [...document.querySelectorAll('[role="dialog"] h3')].find(
      (candidate) => candidate.textContent.trim() === "Java & World Generation",
    );
    heading?.scrollIntoView({ block: "start" });
  })()`);
  await delay(400);

  const state = await evaluate(`(() => {
    const dialog = document.querySelector('[role="dialog"]');
    const text = dialog?.textContent ?? "";
    const readiness = document.querySelector('[aria-labelledby="vanilla-readiness-heading"]');
    const buttons = [...document.querySelectorAll("button")];
    return {
      dialogTitle: dialog?.querySelector("h2")?.textContent ?? null,
      groupHeading: [...(dialog?.querySelectorAll("h3") ?? [])].some(
        (heading) => heading.textContent.trim() === "Java & World Generation",
      ),
      optionalLabel: text.includes("Optional for Empty Project"),
      readinessState: text.includes("Vanilla World is ready")
        ? "ready"
        : text.includes("Setup required")
          ? "blocked"
          : text.includes("Readiness check unavailable")
            ? "unavailable"
            : "missing",
      readinessText: readiness?.textContent?.slice(0, 900) ?? null,
      assetEmptyState:
        text.includes("No Minecraft asset source is selected") ||
        text.includes("Automatic detection found no Java Edition"),
      assetCacheButton: buttons.some(
        (button) => button.getAttribute("aria-label") === "Clear derived asset cache",
      ),
      assetCacheNotice: text.includes("Derived asset cache cleared"),
      javaTarget: text.includes("Java 21"),
      serverTarget: text.includes("MC 1.21.4"),
      javaClearButton: buttons.some((button) => button.textContent.includes("Clear managed Java")),
      serverClearButton: buttons.some((button) =>
        button.textContent.includes("Clear managed server JARs"),
      ),
      manualHints:
        text.includes("Choose java.exe") &&
        text.includes("Choose server.jar"),
      offlineFallback: text.includes("Empty Project"),
      dialogScrollTop: dialog?.scrollTop ?? null,
    };
  })()`);

  const screenshot = await send("Page.captureScreenshot", { format: "png" });
  const png = Buffer.from(screenshot.data, "base64");
  const size = pngSize(png);
  await writeFile(join(outputDir, "settings-dependencies.png"), png);

  const evidence = {
    schema: 1,
    generatedAt: new Date().toISOString(),
    app: {
      executable: "target/debug/furybuilder-desktop.exe",
      sha256: executableSha256,
    },
    origin,
    isolatedAppData: true,
    state,
    checks: {
      groupedSections: state.groupHeading && state.optionalLabel,
      readinessReported: state.readinessState !== "missing",
      assetEmptyStateExplained: state.assetEmptyState,
      cacheClearWorked: state.assetCacheButton && state.assetCacheNotice,
      versionsShown: state.javaTarget && state.serverTarget,
      manualPathsOffered: state.manualHints,
      cacheControlsVisible: state.javaClearButton && state.serverClearButton,
      offlineFallbackExplained: state.offlineFallback,
      consoleClean: consoleErrors.length === 0,
    },
    consoleErrors,
    screenshot: {
      file: "settings-dependencies.png",
      width: size.width,
      height: size.height,
      bytes: png.length,
      sha256: createHash("sha256").update(png).digest("hex"),
    },
  };
  await writeFile(
    join(outputDir, "settings-readiness.json"),
    `${JSON.stringify(evidence, null, 2)}\n`,
  );
  console.log(JSON.stringify(evidence, null, 2));
} finally {
  try {
    ws?.close();
  } catch {
    // Ignore close failures.
  }
  app.kill();
  await delay(500);
  await rm(profile, { recursive: true, force: true }).catch(() => {});
}
