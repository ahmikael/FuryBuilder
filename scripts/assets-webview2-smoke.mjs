/**
 * M9.1 desktop smoke test: invoke the real Tauri asset commands and open the
 * Settings → Minecraft assets section in the actual WebView2 app.
 *
 * The app is started with an isolated WebView2 profile and a loopback-only
 * DevTools port. It never writes to the launcher root and never saves an asset
 * source; only read-only commands are invoked.
 *
 * Usage (Vite dev server must already run at 127.0.0.1:1420):
 *   node scripts/assets-webview2-smoke.mjs <new-output-json> <launcher-root>
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

if (process.platform !== "win32" || process.argv.length !== 4) {
  throw new Error(
    "Usage on Windows: node scripts/assets-webview2-smoke.mjs <new-output-json> <launcher-root>",
  );
}
const output = resolve(process.argv[2]);
const launcherRoot = resolve(process.argv[3]);
await mkdir(dirname(output), { recursive: true });
const profile = await mkdtemp(join(tmpdir(), "fury-assets-webview-"));

let response;
try {
  response = await fetch(
    `${origin}/src/features/settings/MinecraftAssetsSection.tsx`,
    {
      signal: AbortSignal.timeout(5000),
    },
  );
} catch {
  throw new Error(`Start this checkout's Vite server at ${origin}`);
}
if (!response.ok)
  throw new Error(`Vite did not serve the settings section at ${origin}`);

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
    const waiter = pending.get(result.id);
    if (!waiter) return;
    pending.delete(result.id);
    if (result.error) waiter.reject(new Error(JSON.stringify(result.error)));
    else waiter.resolve(result.result);
  };

  await send("Page.enable");
  await send("Runtime.enable");
  // The released Tauri window may boot from the bundled dist. Navigate it to this
  // checkout's Vite server after the initial startup navigation settles.
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

  const discovery = await evaluate(
    'window.__TAURI_INTERNALS__.invoke("assets_discover", {})',
  );
  const installation = await evaluate(
    `window.__TAURI_INTERNALS__.invoke("assets_inspect_path", { path: ${JSON.stringify(launcherRoot)} })`,
  );
  const savedBefore = await evaluate(
    'window.__TAURI_INTERNALS__.invoke("assets_saved_source", {})',
  );

  const opened = await evaluate(`(() => {
    const button = document.querySelector('button[aria-label="Settings"]');
    if (!button) return false;
    button.click();
    return true;
  })()`);
  if (!opened) throw new Error("Settings button was not found");
  const sectionVisible = await waitFor(
    'document.body.innerText.toLowerCase().includes("minecraft assets")',
    30_000,
  );
  if (!sectionVisible) {
    const diagnostics = await evaluate(`(() => ({
      href: location.href,
      bodyText: document.body.innerText.slice(0, 2000),
      dialogs: document.querySelectorAll('[role="dialog"]').length,
      settingsButtons: [...document.querySelectorAll("button")].filter((button) => (button.getAttribute("aria-label") ?? "").includes("ettings")).map((button) => button.getAttribute("aria-label")),
    }))()`);
    throw new Error(
      `Minecraft assets section did not render: ${JSON.stringify(diagnostics)}`,
    );
  }
  await waitFor(
    'document.body.innerText.includes("No Minecraft asset source is selected")',
    15_000,
  );
  const dom = await evaluate(`(() => {
    const text = document.body.innerText;
    return {
      settingsVisible: text.includes("Settings"),
      minecraftAssetsVisible: text.toLowerCase().includes("minecraft assets"),
      targetVisible: text.includes("Minecraft 1.21.4 · DV 4189 · pack format 46"),
      placeholderFallbackVisible: text.includes("Placeholder textures remain active"),
      manualRootButton: Boolean([...document.querySelectorAll("button")].find((button) => button.textContent.includes(".minecraft"))),
      clientJarButton: Boolean([...document.querySelectorAll("button")].find((button) => button.textContent.includes("client.jar"))),
      packZipButton: Boolean([...document.querySelectorAll("button")].find((button) => button.textContent.includes("Pack .zip"))),
      bodyText: text.slice(0, 4000),
    };
  })()`);

  const version = installation.version;
  const evidence = {
    tool: { name: "furybuilder-minecraft-assets-ui", version: 1 },
    generatedAt: new Date().toISOString(),
    target: "Minecraft Java 1.21.4",
    executableSha256,
    launcherRoot,
    discovery: {
      targetMinecraftVersion: discovery.targetMinecraftVersion,
      targetDataVersion: discovery.targetDataVersion,
      defaultRootPresent: discovery.defaultRootPresent,
      installations: discovery.installations.length,
      problems: discovery.problems,
    },
    installation: {
      kind: installation.kind,
      valid: installation.valid,
      targetMatch: installation.targetMatch,
      versions: installation.installation?.versions.length ?? 0,
      compatibleVersions: installation.installation?.compatibleVersions ?? [],
      version: version
        ? {
            id: version.id,
            status: version.status,
            checksumVerified: version.checksumVerified,
            worldVersion: version.worldVersion,
            resourcePackFormat: version.resourcePackFormat,
            jarBytes: version.jarBytes,
            assetIndexChecksumVerified: version.assetIndexChecksumVerified,
          }
        : null,
      errors: installation.errors,
    },
    savedSourceBefore: savedBefore.source,
    dom,
    conclusions: {
      realTauriCommandWorks: discovery.targetMinecraftVersion === "1.21.4",
      manualRootAccepted: installation.valid,
      targetVersionCompatible: version?.status === "compatible",
      clientJarSha1Verified: version?.checksumVerified === true,
      worldVersion4189: version?.worldVersion === 4189,
      resourcePackFormat46: version?.resourcePackFormat === 46,
      assetIndexVerified: version?.assetIndexChecksumVerified === true,
      settingsSectionRendered: dom.minecraftAssetsVisible,
      targetLabelRendered: dom.targetVisible,
      manualFallbackRendered:
        dom.manualRootButton && dom.clientJarButton && dom.packZipButton,
      noAssetSourceWasWritten: savedBefore.source === null,
    },
    limitations: [
      "the app was started with an isolated WebView2 profile and closed after the read-only probe",
      "no native file dialog was opened; manual selection is covered by UI tests and Rust tests",
      "the launcher root is read-only and is never modified",
    ],
  };
  const failed = Object.entries(evidence.conclusions).filter(
    ([, value]) => value !== true,
  );
  if (failed.length > 0) {
    throw new Error(`UI smoke conclusions failed: ${JSON.stringify(failed)}`);
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
}
