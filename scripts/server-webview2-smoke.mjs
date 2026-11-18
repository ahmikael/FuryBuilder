/**
 * M9.4 desktop smoke test for the official Minecraft server JAR.
 *
 * Starts the real Tauri app with an isolated app-data directory, resolves the official
 * Mojang metadata for Minecraft 1.21.4, downloads the announced server.jar, verifies its
 * SHA-1/size and internal version.json, renders Settings, and clears the managed cache.
 *
 * No JAR is committed, no EULA is accepted and no server is started.
 *
 * Usage (Vite dev server must already run at 127.0.0.1:1420):
 *   node scripts/server-webview2-smoke.mjs <new-output-json> <new-output-png>
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

if (process.platform !== "win32" || process.argv.length !== 4) {
  throw new Error(
    "Usage on Windows: node scripts/server-webview2-smoke.mjs <new-output-json> <new-output-png>",
  );
}
const output = resolve(process.argv[2]);
const screenshotPath = resolve(process.argv[3]);
await mkdir(dirname(output), { recursive: true });
await mkdir(dirname(screenshotPath), { recursive: true });
const profile = await mkdtemp(join(tmpdir(), "fury-server-smoke-"));
const appData = join(profile, "appdata");
await mkdir(appData, { recursive: true });

let response;
try {
  response = await fetch(
    `${origin}/src/features/settings/MinecraftServerSection.tsx`,
    {
      signal: AbortSignal.timeout(5000),
    },
  );
} catch {
  throw new Error(`Start this checkout's Vite server at ${origin}`);
}
if (!response.ok)
  throw new Error(`Vite did not serve the server section at ${origin}`);

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

function send(method, params = {}, timeoutMs = 600_000) {
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

async function readServerSettings() {
  const candidates = [join(appData, "server.json")];
  for (const entry of await readdir(appData, { withFileTypes: true })) {
    if (entry.isDirectory())
      candidates.push(join(appData, entry.name, "server.json"));
  }
  for (const candidate of candidates) {
    try {
      return JSON.parse(await readFile(candidate, "utf8"));
    } catch {
      // Try the next app-data layout.
    }
  }
  throw new Error("isolated server.json was not found");
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

  const release = await evaluate(
    'window.__TAURI_INTERNALS__.invoke("server_release", {})',
  );
  const installed = await evaluate(
    `window.__TAURI_INTERNALS__.invoke("server_install", { version: ${JSON.stringify(release.minecraftVersion)} })`,
  );
  const afterInstall = await evaluate(
    'window.__TAURI_INTERNALS__.invoke("server_discover", {})',
  );
  const settingsAfterInstall = await readServerSettings();

  const opened = await evaluate(`(() => {
    const button = document.querySelector('button[aria-label="Settings"]');
    if (!button) return false;
    button.click();
    return true;
  })()`);
  if (!opened) throw new Error("Settings button was not found");
  const uiReady = await waitFor(
    'document.body.innerText.toLowerCase().includes("minecraft server jar") && document.body.innerText.includes("Selected server JAR")',
    30_000,
  );
  if (!uiReady)
    throw new Error("Minecraft server settings section did not render");
  const sectionText = await evaluate(`(() => {
    const text = document.body.innerText;
    const start = text.toLowerCase().indexOf("minecraft server jar");
    return start >= 0 ? text.slice(start, start + 1600) : "";
  })()`);
  const scrolled = await evaluate(`(() => {
    const dialog = document.querySelector('[role="dialog"]');
    if (!dialog) return false;
    dialog.scrollTop = dialog.scrollHeight;
    return dialog.scrollTop > 0;
  })()`);
  if (!scrolled)
    throw new Error(
      "Settings dialog could not be scrolled to the server section",
    );
  await delay(500);
  const capture = await send("Page.captureScreenshot", {
    format: "png",
    captureBeyondViewport: false,
    fromSurface: true,
  });
  const screenshot = Buffer.from(capture.data, "base64");
  await writeFile(screenshotPath, screenshot, { flag: "wx" });

  const cleared = await evaluate(
    'window.__TAURI_INTERNALS__.invoke("server_clear_managed", {})',
  );
  const afterClear = await evaluate(
    'window.__TAURI_INTERNALS__.invoke("server_discover", {})',
  );
  const settingsAfterClear = await readServerSettings();

  const managed = installed.server;
  const expectedJar = plainPath(managed.jarPath);
  const evidence = {
    tool: { name: "furybuilder-server-jar-ui", version: 1 },
    generatedAt: new Date().toISOString(),
    target: "Minecraft Java 1.21.4 / server.jar",
    executableSha256,
    isolatedAppData: appData,
    mojang: release,
    installed: {
      server: managed,
      report: installed.report,
      selected: installed.selected,
      discoveredManaged: afterInstall.managed.length,
      selectedAfterInstall: afterInstall.selected?.path ?? null,
      settingsSelectedPath: settingsAfterInstall.selected?.path ?? null,
    },
    settingsUi: {
      rendered: uiReady,
      text: sectionText,
    },
    cleanup: {
      clearedReport: cleared,
      discoveredManagedAfterClear: afterClear.managed.length,
      selectedAfterClear: afterClear.selected?.path ?? null,
      settingsSelectedAfterClear: settingsAfterClear.selected?.path ?? null,
    },
    screenshot: { path: screenshotPath, bytes: screenshot.length },
    consoleErrors,
    conclusions: {
      mojangMetadataIsExact:
        release.minecraftVersion === "1.21.4" &&
        release.versionType === "release" &&
        release.sha1 === "4707d00eb834b446575d89a61a11b5d548d8c001" &&
        release.size === 56_880_250 &&
        release.javaMajor === 21 &&
        release.expectedDataVersion === 4189 &&
        release.source === "Mojang version_manifest_v2",
      installedFromAnnouncedRelease:
        managed.sha1 === release.sha1 &&
        managed.size === release.size &&
        managed.dataVersion === 4189 &&
        managed.javaMajor === 21 &&
        installed.report.ready === true &&
        installed.report.matchesOfficial === true,
      internalVersionVerified:
        installed.report.version === "1.21.4" &&
        installed.report.protocolVersion === 769 &&
        installed.report.resourcePackFormat === 46,
      managedJarLivesInIsolatedAppData: expectedJar.startsWith(appData),
      installedJarSelected:
        afterInstall.selected?.path === managed.jarPath &&
        settingsAfterInstall.selected?.path === managed.jarPath,
      settingsUiShowsServerJar:
        uiReady &&
        sectionText.includes("Selected server JAR") &&
        sectionText.includes("1.21.4") &&
        sectionText.includes(release.sha1),
      managedCacheCleared:
        afterClear.managed.length === 0 &&
        afterClear.selected === null &&
        settingsAfterClear.selected === null,
      screenshotCaptured: screenshot.length > 10_000,
      noConsoleErrors: consoleErrors.length === 0,
    },
    limitations: [
      "the official server JAR is downloaded only after this smoke explicitly invokes server_install",
      "the JAR is verified but not executed; EULA acceptance and the isolated server process belong to M9.5",
      "the managed JAR is removed at the end; no Mojang binary is stored in the repository",
      "Empty Project and Import World are unaffected by this test",
    ],
  };
  const failed = Object.entries(evidence.conclusions)
    .filter(([, value]) => value !== true)
    .map(([key]) => key);
  if (failed.length > 0) {
    throw new Error(
      `server smoke conclusions failed: ${JSON.stringify(failed)}; consoleErrors=${JSON.stringify(consoleErrors)}`,
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
        version: managed.version,
        sha1: managed.sha1,
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
