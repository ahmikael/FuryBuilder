/**
 * M9.3 desktop smoke test for Java runtime management.
 *
 * Starts the real Tauri app with an isolated APPDATA/WebView2 profile and exercises the
 * actual Rust commands: manual java.exe probe, discovery, Adoptium metadata, opt-in Temurin
 * download with checksum verification, Settings rendering, and managed-cache cleanup.
 *
 * The user's selected Java executable is only executed and read; its bytes are hashed
 * before and after to prove the app never modifies it.
 *
 * Usage (Vite dev server must already run at 127.0.0.1:1420):
 *   node scripts/runtime-webview2-smoke.mjs <new-output-json> <new-output-png> <java.exe>
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

if (process.platform !== "win32" || process.argv.length !== 5) {
  throw new Error(
    "Usage on Windows: node scripts/runtime-webview2-smoke.mjs <new-output-json> <new-output-png> <java.exe>",
  );
}
const output = resolve(process.argv[2]);
const screenshotPath = resolve(process.argv[3]);
const javaPath = resolve(process.argv[4]);
await mkdir(dirname(output), { recursive: true });
await mkdir(dirname(screenshotPath), { recursive: true });
const profile = await mkdtemp(join(tmpdir(), "fury-runtime-smoke-"));
const appData = join(profile, "appdata");
await mkdir(appData, { recursive: true });

let response;
try {
  response = await fetch(
    `${origin}/src/features/settings/JavaRuntimeSection.tsx`,
    {
      signal: AbortSignal.timeout(5000),
    },
  );
} catch {
  throw new Error(`Start this checkout's Vite server at ${origin}`);
}
if (!response.ok)
  throw new Error(`Vite did not serve the Java runtime section at ${origin}`);

const javaBytes = await readFile(javaPath);
const javaSha256Before = createHash("sha256").update(javaBytes).digest("hex");
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

function plainPath(path) {
  return path.replace(/^\\\\\?\\/, "");
}

/** Tauri stores app data under APPDATA/<bundle-id>; find runtime.json without hardcoding it. */
async function readRuntimeSettings() {
  const candidates = [join(appData, "runtime.json")];
  for (const entry of await readdir(appData, { withFileTypes: true })) {
    if (entry.isDirectory())
      candidates.push(join(appData, entry.name, "runtime.json"));
  }
  for (const candidate of candidates) {
    try {
      return JSON.parse(await readFile(candidate, "utf8"));
    } catch {
      // Try the next app-data layout.
    }
  }
  throw new Error("isolated runtime.json was not found");
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
    'typeof window.__TAURI_INTERNALS__ === "object" && document.readyState === "complete"',
    120_000,
  );
  if (!booted) throw new Error("Tauri internals did not become available");

  const selected = await evaluate(
    `window.__TAURI_INTERNALS__.invoke("runtime_select_java", { path: ${JSON.stringify(javaPath)} })`,
  );
  const discovery = await evaluate(
    'window.__TAURI_INTERNALS__.invoke("runtime_discover", {})',
  );
  const release = await evaluate(
    'window.__TAURI_INTERNALS__.invoke("runtime_temurin_latest", {})',
  );
  const installed = await evaluate(
    `window.__TAURI_INTERNALS__.invoke("runtime_install_temurin", { version: ${JSON.stringify(release.version)} })`,
  );
  const afterInstall = await evaluate(
    'window.__TAURI_INTERNALS__.invoke("runtime_discover", {})',
  );
  const settingsAfterInstall = await readRuntimeSettings();

  // Open the real Settings dialog and verify the section is fed by the commands above.
  const opened = await evaluate(`(() => {
    const button = document.querySelector('button[aria-label="Settings"]');
    if (!button) return false;
    button.click();
    return true;
  })()`);
  if (!opened) throw new Error("Settings button was not found");
  const uiReady = await waitFor(
    'document.body.innerText.toLowerCase().includes("java & world generation") && document.body.innerText.includes("Selected runtime")',
    30_000,
  );
  if (!uiReady) throw new Error("Java runtime settings section did not render");
  const sectionText = await evaluate(`(() => {
    const text = document.body.innerText;
    const start = text.toLowerCase().indexOf("java & world generation");
    return start >= 0 ? text.slice(start, start + 1400) : "";
  })()`);
  const scrolledToJava = await evaluate(`(() => {
    const dialog = document.querySelector('[role="dialog"]');
    if (!dialog) return false;
    dialog.scrollTop = dialog.scrollHeight;
    return dialog.scrollTop > 0;
  })()`);
  if (!scrolledToJava)
    throw new Error("Settings dialog could not be scrolled to Java");
  await delay(500);
  await delay(750);
  const capture = await send("Page.captureScreenshot", {
    format: "png",
    captureBeyondViewport: false,
    fromSurface: true,
  });
  const screenshot = Buffer.from(capture.data, "base64");
  await writeFile(screenshotPath, screenshot, { flag: "wx" });

  const cleared = await evaluate(
    'window.__TAURI_INTERNALS__.invoke("runtime_clear_managed", {})',
  );
  const afterClear = await evaluate(
    'window.__TAURI_INTERNALS__.invoke("runtime_discover", {})',
  );
  const settingsAfterClear = await readRuntimeSettings();

  const javaSha256After = createHash("sha256")
    .update(await readFile(javaPath))
    .digest("hex");
  const managed = installed.runtime;
  const expectedJava = plainPath(managed.javaPath);

  const evidence = {
    tool: { name: "furybuilder-runtime-ui", version: 1 },
    generatedAt: new Date().toISOString(),
    target: "Minecraft Java 1.21.4 / Java 21",
    executableSha256,
    isolatedAppData: appData,
    externalJava: {
      path: javaPath,
      sha256Before: javaSha256Before,
      sha256After: javaSha256After,
    },
    manualSelection: selected,
    discovery: {
      targetJavaMajor: discovery.targetJavaMajor,
      targetMinecraftVersion: discovery.targetMinecraftVersion,
      candidates: discovery.candidates.length,
      savedStatus: discovery.saved?.status ?? null,
      savedPath: discovery.saved?.path ?? null,
    },
    adoptium: release,
    installed: {
      runtime: managed,
      report: installed.report,
      selected: installed.selected,
      discoveredManaged: afterInstall.managed.length,
      selectedAfterInstall: afterInstall.saved?.path ?? null,
      settingsSelectedJava: settingsAfterInstall.selectedJava,
    },
    settingsUi: {
      rendered: uiReady,
      text: sectionText,
    },
    cleanup: {
      clearedReport: cleared,
      discoveredManagedAfterClear: afterClear.managed.length,
      selectedAfterClear: afterClear.saved?.path ?? null,
      settingsSelectedJavaAfterClear: settingsAfterClear.selectedJava,
    },
    screenshot: { path: screenshotPath, bytes: screenshot.length },
    consoleErrors,
    conclusions: {
      manualJavaProbedAndAccepted:
        selected.status === "compatible" &&
        selected.major === 21 &&
        selected.vendor === "Eclipse Adoptium",
      discoveryTargetsJava21:
        discovery.targetJavaMajor === 21 &&
        discovery.targetMinecraftVersion === "1.21.4" &&
        discovery.saved?.path === selected.path,
      adoptiumMetadataIsHttpsAndChecksummed:
        release.source === "Adoptium API v3" &&
        /^[0-9a-f]{64}$/.test(release.sha256) &&
        release.size > 10_000_000 &&
        release.downloadUrl.startsWith("https://github.com/"),
      temurinInstalledFromAnnouncedRelease:
        managed.sha256 === release.sha256 &&
        managed.version === release.version &&
        managed.bytes !== 0 &&
        installed.report.status === "compatible" &&
        installed.report.managed === true,
      managedRuntimeLivesInIsolatedAppData: expectedJava.startsWith(appData),
      installedRuntimeSelected:
        afterInstall.saved?.path === managed.javaPath &&
        settingsAfterInstall.selectedJava === managed.javaPath,
      settingsUiShowsManagedJava:
        uiReady &&
        sectionText.includes("Selected runtime") &&
        sectionText.toLowerCase().includes("managed") &&
        sectionText.includes(release.version),
      managedCacheCleared:
        afterClear.managed.length === 0 &&
        afterClear.saved === null &&
        settingsAfterClear.selectedJava === null,
      userJavaFileUnchanged: javaSha256Before === javaSha256After,
      screenshotCaptured: screenshot.length > 10_000,
      noConsoleErrors: consoleErrors.length === 0,
    },
    limitations: [
      "Temurin is downloaded from the official Adoptium API/GitHub release only after the smoke explicitly invokes the install command",
      "the managed runtime is removed at the end; the user's external Java installation and real app settings are never modified",
      "this task covers Java discovery/provisioning only; the official server.jar, EULA flow and isolated worldgen belong to M9.4-M9.5",
      "the Settings dialog is shown with an isolated APPDATA profile; no project or world data is created",
    ],
  };
  const failed = Object.entries(evidence.conclusions)
    .filter(([, value]) => value !== true)
    .map(([key]) => key);
  if (failed.length > 0) {
    throw new Error(
      `runtime smoke conclusions failed: ${JSON.stringify(failed)}; consoleErrors=${JSON.stringify(consoleErrors)}`,
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
        java: selected.version,
        temurin: managed.version,
        sha256: managed.sha256,
        size: managed.size,
        managedAfterInstall: afterInstall.managed.length,
        managedAfterClear: afterClear.managed.length,
        conclusions: evidence.conclusions,
      },
      null,
      2,
    ),
  );
} finally {
  ws?.close();
  if (app.exitCode === null) app.kill();
  await delay(750);
  await rm(profile, { recursive: true, force: true }).catch(() => {});
}
