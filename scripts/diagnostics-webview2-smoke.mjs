/**
 * M10.6 desktop smoke for crash-recovery diagnostics.
 *
 * Starts the real Tauri app with an isolated APPDATA/WebView2 profile and local Vite
 * frontend, opens Settings → Diagnostics, verifies the bounded log is visible, triggers a
 * real unhandled-rejection path, checks the log for redacted paths, then clears only the
 * diagnostics log. It does not read or modify any real user project.
 *
 * Usage (Vite dev server must already run at 127.0.0.1:1420):
 *   node scripts/diagnostics-webview2-smoke.mjs <new-output-dir>
 */
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import {
  mkdir,
  mkdtemp,
  readFile,
  readdir,
  rm,
  stat,
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
    "Usage on Windows: node scripts/diagnostics-webview2-smoke.mjs <new-output-dir>",
  );
}
const outputDir = resolve(process.argv[2]);
await mkdir(outputDir, { recursive: true });
if ((await readdir(outputDir)).length > 0) {
  throw new Error("Output directory must be new or empty");
}

let response;
try {
  response = await fetch(
    `${origin}/src/features/settings/DiagnosticsSection.tsx`,
    {
      signal: AbortSignal.timeout(5000),
    },
  );
} catch {
  throw new Error(`Start this checkout's Vite server at ${origin}`);
}
if (!response.ok) {
  throw new Error(`Vite did not serve the diagnostics section at ${origin}`);
}

const executableSha256 = createHash("sha256")
  .update(await readFile(appPath))
  .digest("hex");
const profile = await mkdtemp(join(tmpdir(), "fury-diagnostics-smoke-"));
const appData = join(profile, "appdata");
await mkdir(appData, { recursive: true });
const logPath = join(appData, "logs", "furybuilder.log");

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

async function tauri(command, args = {}) {
  return evaluate(
    `window.__TAURI_INTERNALS__.invoke(${JSON.stringify(command)}, ${JSON.stringify(args)})`,
  );
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

async function waitForLog(predicate, timeoutMs = 15_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const text = await readFile(logPath, "utf8");
      if (predicate(text)) return text;
    } catch {
      // The file may not exist yet.
    }
    await delay(250);
  }
  return null;
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
    'document.querySelector(\'[role="dialog"]\')?.textContent.includes("Diagnostics") ?? false',
    30_000,
  );
  if (!dialogOpen)
    throw new Error("Settings dialog did not render the Diagnostics section");

  const loaded = await waitFor(
    `(() => {
      const text = document.querySelector('[role="dialog"]')?.textContent ?? "";
      return text.includes("furybuilder.log") && text.includes("App version");
    })()`,
    30_000,
  );
  if (!loaded) throw new Error("Diagnostics information did not load");

  const before = await tauri("diagnostics_info");
  const fileBefore = await stat(logPath).catch(() => null);

  await evaluate(`setTimeout(() => {
    Promise.reject(new Error("smoke diagnostics failure C:\\\\Users\\\\Private\\\\Secret\\\\world"));
  }, 0); true`);
  const logAfterError = await waitForLog(
    (text) =>
      text.includes("window.unhandledrejection") && text.includes("<path>"),
  );
  if (!logAfterError)
    throw new Error("The UI error did not reach the diagnostics log");

  await evaluate(`(() => {
    const button = [...document.querySelectorAll("button")].find(
      (candidate) => candidate.getAttribute("aria-label") === "Refresh diagnostics",
    );
    button?.click();
  })()`);
  await delay(500);

  const after = await tauri("diagnostics_info");
  const pathRedacted =
    !logAfterError.includes("Private") && logAfterError.includes("<path>");
  const errorRecorded = logAfterError.includes('"event":"ui.error"');

  await evaluate(`(() => {
    const heading = [...document.querySelectorAll("h3")].find(
      (candidate) => candidate.textContent.trim() === "Diagnostics",
    );
    heading?.scrollIntoView({ block: "start" });
  })()`);
  await delay(400);

  const beforeClearState = await evaluate(`(() => {
    const text = document.querySelector('[role="dialog"]')?.textContent ?? "";
    const diagnostics = document.querySelector('[aria-labelledby="diagnostics-heading"]');
    return {
      dialogTitle: document.querySelector('[role="dialog"] h2')?.textContent ?? null,
      diagnosticsText: diagnostics?.textContent?.slice(0, 1200) ?? null,
      logPathShown: text.includes("furybuilder.log"),
      appVersionShown: text.includes("0.1.0"),
      rotationShown: text.includes("per file") && text.includes("files maximum"),
    };
  })()`);

  const screenshot = await send("Page.captureScreenshot", { format: "png" });
  const png = Buffer.from(screenshot.data, "base64");
  const size = pngSize(png);
  await writeFile(join(outputDir, "diagnostics-settings.png"), png);

  const cleared = await evaluate(`(() => {
    const button = [...document.querySelectorAll("button")].find(
      (candidate) => candidate.getAttribute("aria-label") === "Clear diagnostic logs",
    );
    if (!button) return false;
    button.click();
    return true;
  })()`);
  if (!cleared) throw new Error("Clear diagnostic logs button was not found");
  const clearSettled = await waitFor(
    'document.querySelector(\'[role="dialog"]\')?.textContent.includes("Diagnostic logs cleared") ?? false',
    30_000,
  );
  if (!clearSettled) throw new Error("Clear logs did not report success");
  const afterClear = await tauri("diagnostics_info");
  const fileAfterClear = await stat(logPath).catch(() => null);

  const state = await evaluate(`(() => {
    const text = document.querySelector('[role="dialog"]')?.textContent ?? "";
    return {
      clearNotice: text.includes("Diagnostic logs cleared"),
      emptyState: text.includes("No log entries yet."),
    };
  })()`);

  const relevantConsoleErrors = consoleErrors.filter(
    (entry) => !entry.includes("smoke diagnostics failure"),
  );
  const evidence = {
    schema: 1,
    generatedAt: new Date().toISOString(),
    app: {
      executable: "target/debug/furybuilder-desktop.exe",
      sha256: executableSha256,
    },
    origin,
    isolatedAppData: true,
    log: {
      path: "appdata/logs/furybuilder.log",
      beforeBytes: fileBefore?.size ?? 0,
      beforeFiles: before?.files?.length ?? 0,
      afterBytes: after?.totalBytes ?? 0,
      afterFiles: after?.files?.length ?? 0,
      containsUiError: errorRecorded,
      containsPathPlaceholder: logAfterError.includes("<path>"),
      containsPrivatePath: logAfterError.includes("Private"),
      afterClearBytes: fileAfterClear?.size ?? 0,
      afterClearFiles: afterClear?.files?.length ?? 0,
    },
    state: {
      ...beforeClearState,
      afterClear: state,
    },
    checks: {
      settingsOpened: beforeClearState.dialogTitle === "Settings",
      diagnosticsLoaded:
        beforeClearState.logPathShown &&
        beforeClearState.appVersionShown &&
        beforeClearState.rotationShown,
      startupLogCreated: (fileBefore?.size ?? 0) > 0,
      uiErrorRecorded: errorRecorded,
      pathRedacted,
      refreshObserved: (after?.totalBytes ?? 0) >= (before?.totalBytes ?? 0),
      clearWorked:
        state.clearNotice &&
        state.emptyState &&
        (afterClear?.files?.length ?? 0) === 0 &&
        (fileAfterClear?.size ?? 0) === 0,
      consoleClean: relevantConsoleErrors.length === 0,
    },
    intentionalErrorConsoleNoise:
      consoleErrors.length - relevantConsoleErrors.length,
    consoleErrors: relevantConsoleErrors,
    screenshot: {
      file: "diagnostics-settings.png",
      width: size.width,
      height: size.height,
      bytes: png.length,
      sha256: createHash("sha256").update(png).digest("hex"),
    },
  };
  await writeFile(
    join(outputDir, "diagnostics.json"),
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
