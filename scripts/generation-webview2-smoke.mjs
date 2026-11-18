/**
 * M9.5 desktop smoke: explicit EULA confirmation and the isolated Vanilla generator.
 *
 * Starts the real Tauri app with an isolated app-data directory, selects the provided
 * verified Java 21 executable and server.jar, accepts the EULA through the Settings UI,
 * generates a small real 1.21.4 area, screenshots the result, starts a larger run and
 * cancels it, then clears the run directories through the UI.
 *
 * Usage (Vite dev server must already run at 127.0.0.1:1420):
 *   node scripts/generation-webview2-smoke.mjs <java.exe> <server.jar> <out.json> <out.png>
 *
 * The provided server.jar must be the official 1.21.4 file; the manual selection path
 * verifies its SHA-1, internal version.json and server entry point.
 */
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync } from "node:fs";
import {
  mkdir,
  mkdtemp,
  readFile,
  readdir,
  readFile as readText,
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
const expectedSha1 = "4707d00eb834b446575d89a61a11b5d548d8c001";

if (process.platform !== "win32" || process.argv.length !== 6) {
  throw new Error(
    "Usage on Windows: node scripts/generation-webview2-smoke.mjs <java.exe> <server.jar> <out.json> <out.png>",
  );
}
const javaPath = resolve(process.argv[2]);
const serverJar = resolve(process.argv[3]);
const output = resolve(process.argv[4]);
const screenshotPath = resolve(process.argv[5]);
if (!existsSync(javaPath))
  throw new Error(`Java executable not found: ${javaPath}`);
if (!existsSync(serverJar))
  throw new Error(`server.jar not found: ${serverJar}`);

await mkdir(dirname(output), { recursive: true });
await mkdir(dirname(screenshotPath), { recursive: true });
const profile = await mkdtemp(join(tmpdir(), "fury-generation-smoke-"));
const appData = join(profile, "appdata");
await mkdir(appData, { recursive: true });

let response;
try {
  response = await fetch(
    `${origin}/src/features/settings/ServerGenerationPanel.tsx`,
    {
      signal: AbortSignal.timeout(5000),
    },
  );
} catch {
  throw new Error(`Start this checkout's Vite server at ${origin}`);
}
if (!response.ok)
  throw new Error(`Vite did not serve the generation panel at ${origin}`);

const javaSha256Before = createHash("sha256")
  .update(await readFile(javaPath))
  .digest("hex");
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

async function invoke(command, args = {}) {
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

async function waitForInvoke(predicate, timeoutMs = 60_000) {
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

function plainPath(path) {
  return path.replace(/^\\\\\?\\/, "");
}

async function readJson(path) {
  return JSON.parse(await readFile(path, "utf8"));
}

async function runSummary(runDir) {
  const report = await readJson(join(runDir, "report.json"));
  const properties = await readText(join(runDir, "server.properties"), "utf8");
  const eula = (await readText(join(runDir, "eula.txt"), "utf8")).trim();
  const regionDir = join(runDir, "world", "region");
  const regionFiles = existsSync(regionDir)
    ? (await readdir(regionDir)).filter((name) => name.endsWith(".mca"))
    : [];
  return { report, properties, eula, regionFiles };
}

async function setInput(ariaLabel, value) {
  return evaluate(`(() => {
    const element = document.querySelector('input[aria-label="${ariaLabel}"]');
    if (!element) return false;
    const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set;
    setter.call(element, ${JSON.stringify(String(value))});
    element.dispatchEvent(new Event("input", { bubbles: true }));
    return true;
  })()`);
}

async function clickButton(ariaLabel) {
  return evaluate(`(() => {
    const button = document.querySelector('button[aria-label="${ariaLabel}"]');
    if (!button || button.disabled) return false;
    button.click();
    return true;
  })()`);
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

  const java = await invoke("runtime_select_java", { path: javaPath });
  const selected = await invoke("server_select_jar", { path: serverJar });
  if (!selected.ready || selected.sha1 !== expectedSha1) {
    throw new Error(`server JAR was not accepted: ${JSON.stringify(selected)}`);
  }
  const discoveryBefore = await invoke("server_discover");
  if (discoveryBefore.eulaMatchesSelected) {
    throw new Error(
      "isolated app data unexpectedly already had an accepted EULA",
    );
  }

  const opened = await evaluate(`(() => {
    const button = document.querySelector('button[aria-label="Settings"]');
    if (!button) return false;
    button.click();
    return true;
  })()`);
  if (!opened) throw new Error("Settings button was not found");
  const panelReady = await waitFor(
    'document.body.innerText.toLowerCase().includes("isolated vanilla generation")',
    30_000,
  );
  if (!panelReady) throw new Error("isolated generation panel did not render");
  const serverDiscoveryReady = await waitFor(
    'document.body.innerText.includes("Selected server JAR")',
    30_000,
  );
  if (!serverDiscoveryReady) {
    throw new Error("server discovery did not reach the Settings panel");
  }
  const eulaSectionReady = await waitFor(
    'Boolean(document.querySelector(\'input[aria-label="I have read and accept the Minecraft EULA"]\')) || document.body.innerText.includes("EULA accepted")',
    30_000,
  );
  if (!eulaSectionReady) throw new Error("EULA section did not become ready");

  const checkboxPresent = await evaluate(
    "Boolean(document.querySelector('input[aria-label=\"I have read and accept the Minecraft EULA\"]'))",
  );
  const acceptDisabledBefore = await evaluate(
    "Boolean(document.querySelector('button[aria-label=\"Accept EULA\"]')?.disabled)",
  );
  const checkboxClicked = await evaluate(`(() => {
    const checkbox = document.querySelector('input[aria-label="I have read and accept the Minecraft EULA"]');
    if (!checkbox) return false;
    checkbox.click();
    return true;
  })()`);
  if (!checkboxClicked) throw new Error("EULA checkbox was not found");
  const acceptEnabled = await waitFor(
    "!document.querySelector('button[aria-label=\"Accept EULA\"]')?.disabled",
    10_000,
  );
  if (!acceptEnabled)
    throw new Error("Accept EULA button did not enable after checking");
  const acceptClicked = await clickButton("Accept EULA");
  if (!acceptClicked)
    throw new Error("Accept EULA button could not be clicked");
  const eulaAccepted = await waitForInvoke(async () => {
    const report = await invoke("server_discover");
    return report.eulaMatchesSelected ? report : null;
  }, 30_000);
  if (!eulaAccepted)
    throw new Error("EULA acceptance was not recorded for the selected JAR");

  const settingsFilled =
    (await setInput("Generation seed", "FuryBuilderM95UI")) &&
    (await setInput("Generation center X", "2000")) &&
    (await setInput("Generation center Z", "2000")) &&
    (await setInput("Generation radius in chunks", "1")) &&
    (await setInput("Generation time limit seconds", "180")) &&
    (await setInput("Generation memory limit", "2048")) &&
    (await setInput("Generation view distance", "3")) &&
    (await setInput("Generation CPU threads", "2"));
  if (!settingsFilled)
    throw new Error("generation settings inputs were not found");
  const startEnabled = await waitFor(
    "!document.querySelector('button[aria-label=\"Start isolated generation\"]')?.disabled",
    30_000,
  );
  if (!startEnabled)
    throw new Error("Start isolated generation button did not enable");

  const started = await clickButton("Start isolated generation");
  if (!started)
    throw new Error("Start isolated generation button could not be clicked");
  const firstRun = await waitForInvoke(async () => {
    const status = await invoke("server_generation_status");
    return status.active ?? status.last;
  }, 30_000);
  if (!firstRun) throw new Error("generation did not start");
  const firstRunId = firstRun.runId;
  const completed = await waitForInvoke(async () => {
    const status = await invoke("server_generation_status");
    if (status.active) return null;
    return status.last?.snapshot.runId === firstRunId &&
      status.last.stopReason === "completed"
      ? status.last
      : null;
  }, 480_000);
  if (!completed) throw new Error("real isolated generation did not complete");

  const firstRunDir = completed.runDir;
  const first = await runSummary(firstRunDir);
  if (
    first.report.snapshot.generatedChunks < first.report.snapshot.expectedChunks
  ) {
    throw new Error("completed run does not contain the requested chunks");
  }
  if (!first.report.bindAddress?.startsWith("127.0.0.1:")) {
    throw new Error(
      `server did not bind to loopback: ${first.report.bindAddress}`,
    );
  }
  if (!first.report.processTreeGuarded) {
    throw new Error("Windows Job Object containment was not active");
  }
  if (first.regionFiles.length === 0)
    throw new Error("no region files were written");
  if (first.eula !== "eula=true")
    throw new Error("isolated eula.txt was not explicit");
  if (!/server-ip=127\.0\.0\.1/.test(first.properties)) {
    throw new Error("server.properties is not loopback-only");
  }

  await evaluate(`(() => {
    const dialog = document.querySelector('[role="dialog"]');
    if (dialog) dialog.scrollTop = dialog.scrollHeight;
    return true;
  })()`);
  await delay(500);
  const capture = await send("Page.captureScreenshot", {
    format: "png",
    captureBeyondViewport: false,
    fromSurface: true,
  });
  const screenshot = Buffer.from(capture.data, "base64");
  await writeFile(screenshotPath, screenshot, { flag: "wx" });

  const secondSettings =
    (await setInput("Generation center X", "6000")) &&
    (await setInput("Generation center Z", "6000")) &&
    (await setInput("Generation radius in chunks", "8")) &&
    (await setInput("Generation time limit seconds", "300"));
  if (!secondSettings)
    throw new Error("second generation settings were not found");
  const secondStartEnabled = await waitFor(
    "!document.querySelector('button[aria-label=\"Start isolated generation\"]')?.disabled",
    30_000,
  );
  if (!secondStartEnabled)
    throw new Error("second Start button did not enable");
  const secondStarted = await clickButton("Start isolated generation");
  if (!secondStarted) throw new Error("second generation could not start");
  const secondRun = await waitForInvoke(async () => {
    const status = await invoke("server_generation_status");
    return status.active?.running ? status.active : null;
  }, 30_000);
  if (!secondRun) throw new Error("second generation did not start");
  const generationReached = await waitForInvoke(async () => {
    const status = await invoke("server_generation_status");
    return status.active?.phase === "generating" ? status.active : null;
  }, 120_000);
  if (!generationReached)
    throw new Error("second generation never reached the generating phase");
  const cancelClicked = await clickButton("Cancel generation");
  if (!cancelClicked)
    throw new Error("Cancel generation button could not be clicked");
  const cancelled = await waitForInvoke(async () => {
    const status = await invoke("server_generation_status");
    if (status.active) return null;
    return status.last?.snapshot.runId === secondRun.runId &&
      status.last.stopReason === "cancelled"
      ? status.last
      : null;
  }, 120_000);
  if (!cancelled) throw new Error("cancelled run did not report cancellation");
  let pidAlive = false;
  if (cancelled.serverPid) {
    try {
      process.kill(cancelled.serverPid, 0);
      pidAlive = true;
    } catch {
      pidAlive = false;
    }
  }

  const runsBeforeClear = await invoke("server_generation_runs");
  const clearEnabled = await waitFor(
    "!document.querySelector('button[aria-label=\"Clear generation runs\"]')?.disabled",
    30_000,
  );
  if (!clearEnabled)
    throw new Error("Clear generation runs button did not enable");
  const clearClicked = await clickButton("Clear generation runs");
  if (!clearClicked)
    throw new Error("Clear generation runs button could not be clicked");
  const runsAfterClear = await waitForInvoke(async () => {
    const report = await invoke("server_generation_runs");
    return report.runs.length === 0 ? report : null;
  }, 30_000);
  if (!runsAfterClear)
    throw new Error("generation runs were not cleared through the UI");
  const runDirsAfterClear = existsSync(join(appData, "server-runs"))
    ? (
        await readdir(join(appData, "server-runs"), { withFileTypes: true })
      ).filter((entry) => entry.isDirectory()).length
    : 0;

  const javaSha256After = createHash("sha256")
    .update(await readFile(javaPath))
    .digest("hex");
  const settings = await readJson(join(appData, "server.json"));
  const evidence = {
    tool: { name: "furybuilder-generation-ui", version: 1 },
    generatedAt: new Date().toISOString(),
    target: "Minecraft Java 1.21.4 / isolated Vanilla generation",
    executableSha256,
    isolatedAppData: appData,
    java: {
      path: plainPath(java.path),
      version: java.version,
      major: java.major,
      status: java.status,
      sha256Before: javaSha256Before,
      sha256After: javaSha256After,
    },
    server: {
      path: plainPath(selected.path),
      sha1: selected.sha1,
      size: selected.size,
      dataVersion: selected.dataVersion,
      matchesOfficial: selected.matchesOfficial,
    },
    eulaUi: {
      checkboxPresent,
      acceptDisabledBefore,
      eulaMatchesSelectedAfter: Boolean(eulaAccepted.eulaMatchesSelected),
      eulaRecord: settings.eula ?? null,
    },
    firstRun: {
      report: first.report,
      regionFiles: first.regionFiles,
      eulaFile: first.eula,
      properties: first.properties,
    },
    cancelledRun: {
      report: cancelled,
      pidAliveAfterCancel: pidAlive,
    },
    cleanup: {
      runsBeforeClear: runsBeforeClear.runs.length,
      runsAfterClear: runsAfterClear.runs.length,
      runDirectoriesAfterClear: runDirsAfterClear,
    },
    screenshot: { path: screenshotPath, bytes: screenshot.length },
    consoleErrors,
    conclusions: {
      javaProbedAndSelected: java.status === "compatible" && java.major === 21,
      serverJarVerified:
        selected.ready === true &&
        selected.sha1 === expectedSha1 &&
        selected.size === 56_880_250 &&
        selected.dataVersion === 4189,
      eulaRequiredInUi: checkboxPresent && acceptDisabledBefore === true,
      eulaAcceptedExplicitly: Boolean(eulaAccepted.eulaMatchesSelected),
      firstRunCompleted:
        completed.stopReason === "completed" &&
        completed.worldComplete === true &&
        completed.exitCode === 0,
      realChunksAndRegions:
        first.report.snapshot.generatedChunks >=
          first.report.snapshot.expectedChunks && first.regionFiles.length > 0,
      loopbackOnly:
        first.report.bindAddress?.startsWith("127.0.0.1:") === true &&
        first.properties.includes("server-ip=127.0.0.1") &&
        first.properties.includes("enable-rcon=false") &&
        first.properties.includes("enable-query=false"),
      processTreeGuarded: first.report.processTreeGuarded === true,
      cancelledAndTerminated:
        cancelled.stopReason === "cancelled" && pidAlive === false,
      generationRunsCleared:
        runsAfterClear.runs.length === 0 && runDirsAfterClear === 0,
      externalJavaUnchanged: javaSha256Before === javaSha256After,
      screenshotCaptured: screenshot.length > 10_000,
      noConsoleErrors: consoleErrors.length === 0,
    },
    limitations: [
      "the JAR is the previously verified official 1.21.4 server.jar selected manually; this smoke does not re-download it",
      "generation is a small verification area, not a full world",
      "the isolated world is deleted by Clear generation runs after the evidence is captured",
      "no external player ever connects; the server listens on an ephemeral loopback port only",
    ],
  };
  const failed = Object.entries(evidence.conclusions)
    .filter(([, value]) => value !== true)
    .map(([key]) => key);
  if (failed.length > 0) {
    throw new Error(
      `generation smoke conclusions failed: ${JSON.stringify(failed)}; consoleErrors=${JSON.stringify(consoleErrors)}`,
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
        firstRun: {
          chunks: first.report.snapshot.generatedChunks,
          expected: first.report.snapshot.expectedChunks,
          bind: first.report.bindAddress,
          guarded: first.report.processTreeGuarded,
        },
        cancelledRun: {
          elapsedMs: cancelled.snapshot.elapsedMs,
          pidAlive,
        },
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
