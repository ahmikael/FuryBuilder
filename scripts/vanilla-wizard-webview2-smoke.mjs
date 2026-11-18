/**
 * M9.6-M9.8 desktop smoke: the Vanilla World wizard, World Core import and parity.
 *
 * Starts the real Tauri app with isolated app data, selects the provided verified Java 21
 * executable and official 1.21.4 server.jar, accepts the EULA through the wizard, confirms
 * the local run and generates a small real area. The completed run is imported as an
 * immutable Vanilla project, then the saved base is compared block-for-block with the
 * original generator's reference chunks through the backend and the project panel. A second
 * run with a validated data pack and `generateStructures=false` must be refused as a
 * reference for the first project because its seed/settings differ.
 *
 * Usage (Vite dev server must already run at 127.0.0.1:1420):
 *   node scripts/vanilla-wizard-webview2-smoke.mjs <java.exe> <server.jar> <pack.zip> <out.json> <out.png>
 */
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync } from "node:fs";
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
const expectedSha1 = "4707d00eb834b446575d89a61a11b5d548d8c001";

if (process.platform !== "win32" || process.argv.length !== 7) {
  throw new Error(
    "Usage on Windows: node scripts/vanilla-wizard-webview2-smoke.mjs <java.exe> <server.jar> <pack.zip> <out.json> <out.png>",
  );
}
const javaPath = resolve(process.argv[2]);
const serverJar = resolve(process.argv[3]);
const datapackPath = resolve(process.argv[4]);
const output = resolve(process.argv[5]);
const screenshotPath = resolve(process.argv[6]);
for (const path of [javaPath, serverJar, datapackPath]) {
  if (!existsSync(path)) throw new Error(`Required file not found: ${path}`);
}

await mkdir(dirname(output), { recursive: true });
await mkdir(dirname(screenshotPath), { recursive: true });
const profile = await mkdtemp(join(tmpdir(), "fury-vanilla-wizard-smoke-"));
const appData = join(profile, "appdata");
await mkdir(appData, { recursive: true });
const datapackSha256 = createHash("sha256")
  .update(await readFile(datapackPath))
  .digest("hex");

let response;
try {
  response = await fetch(
    `${origin}/src/features/project/VanillaWorldWizard.tsx`,
    {
      signal: AbortSignal.timeout(5000),
    },
  );
} catch {
  throw new Error(`Start this checkout's Vite server at ${origin}`);
}
if (!response.ok)
  throw new Error(`Vite did not serve the Vanilla wizard at ${origin}`);

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

function send(method, params = {}, timeoutMs = 900_000) {
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

function plainPath(path) {
  return path.replace(/^\\\\\?\\/, "");
}

async function readJson(path) {
  return JSON.parse(await readFile(path, "utf8"));
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
    const element = Array.from(document.querySelectorAll('button')).find(
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

async function hashTree(rootPath) {
  const files = [];
  async function walk(current, prefix) {
    const entries = await readdir(current, { withFileTypes: true });
    for (const entry of entries) {
      if (entry.name === "session.lock") continue;
      const relativePath = prefix ? `${prefix}/${entry.name}` : entry.name;
      const full = join(current, entry.name);
      if (entry.isDirectory()) {
        await walk(full, relativePath);
      } else if (entry.isFile()) {
        files.push({
          relativePath,
          sha256: createHash("sha256")
            .update(await readFile(full))
            .digest("hex"),
        });
      } else {
        throw new Error(`unexpected file type in generated world: ${full}`);
      }
    }
  }
  await walk(rootPath, "");
  files.sort((left, right) =>
    left.relativePath.localeCompare(right.relativePath),
  );
  return files;
}

function treeDigest(files) {
  return createHash("sha256").update(JSON.stringify(files)).digest("hex");
}

async function runSummary(runDir) {
  const report = await readJson(join(runDir, "report.json"));
  const properties = await readFile(join(runDir, "server.properties"), "utf8");
  const eula = (await readFile(join(runDir, "eula.txt"), "utf8")).trim();
  const regionDir = join(runDir, "world", "region");
  const regionFiles = existsSync(regionDir)
    ? (await readdir(regionDir)).filter((name) => name.endsWith(".mca"))
    : [];
  const packDir = join(runDir, "world", "datapacks");
  const packs = existsSync(packDir)
    ? await Promise.all(
        (await readdir(packDir)).map(async (name) => ({
          name,
          sha256: createHash("sha256")
            .update(await readFile(join(packDir, name)))
            .digest("hex"),
        })),
      )
    : [];
  return { report, properties, eula, regionFiles, packs };
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
  const selectedJar = await invoke("server_select_jar", { path: serverJar });
  if (!selectedJar.ready || selectedJar.sha1 !== expectedSha1) {
    throw new Error(
      `server JAR was not accepted: ${JSON.stringify(selectedJar)}`,
    );
  }
  const setupBefore = await invoke("vanilla_setup_check");
  if (setupBefore.eulaMatchesSelected) {
    throw new Error(
      "isolated app data unexpectedly already had an accepted EULA",
    );
  }
  if (setupBefore.ready) {
    throw new Error("setup check reported ready before the EULA was accepted");
  }

  const projectsOpened = await clickByText("Project");
  if (!projectsOpened) throw new Error("Project manager button was not found");
  const projectTabs = await waitFor(
    "document.querySelectorAll('[role=\"tab\"]').length > 0",
    30_000,
  );
  if (!projectTabs) throw new Error("Project manager tabs did not render");
  const newProjectTab = await clickTabByText("New project");
  if (!newProjectTab) throw new Error("New project tab was not found");
  const vanillaMode = await waitFor(
    'Array.from(document.querySelectorAll("button")).some((button) => button.textContent.includes("Vanilla World"))',
    30_000,
  );
  if (!vanillaMode) {
    const debug = await evaluate(
      'Array.from(document.querySelectorAll("button")).map((button) => button.textContent.trim()).filter(Boolean).slice(0, 80)',
    );
    throw new Error(
      `Vanilla World creation mode was not found; buttons=${JSON.stringify(debug)}`,
    );
  }
  await evaluate(`(() => {
    const button = Array.from(document.querySelectorAll("button")).find(
      (candidate) => candidate.textContent.includes("Vanilla World"),
    );
    if (!button || button.disabled) return false;
    button.click();
    return true;
  })()`);
  const wizardVisible = await waitFor(
    'document.body.innerText.toLowerCase().includes("vanilla world setup")',
    30_000,
  );
  if (!wizardVisible) throw new Error("Vanilla World wizard did not render");

  const eulaCheckboxPresent = await waitFor(
    "Boolean(document.querySelector('input[aria-label=\"I have read and accept the Minecraft EULA\"]'))",
    90_000,
  );
  if (!eulaCheckboxPresent) {
    throw new Error(
      "EULA checkbox did not appear; setup may not have finished loading",
    );
  }
  const acceptDisabledBefore = await evaluate(
    "Boolean(document.querySelector('button[aria-label=\"Accept EULA\"]')?.disabled)",
  );
  const startDisabledBeforeEula = await evaluate(
    "Boolean(document.querySelector('button[aria-label=\"Start Vanilla World generation\"]')?.disabled)",
  );
  const eulaChecked = await evaluate(`(() => {
    const checkbox = document.querySelector('input[aria-label="I have read and accept the Minecraft EULA"]');
    if (!checkbox) return false;
    checkbox.click();
    return true;
  })()`);
  if (!eulaChecked) throw new Error("EULA checkbox could not be clicked");
  const acceptEnabled = await waitFor(
    "!document.querySelector('button[aria-label=\"Accept EULA\"]')?.disabled",
    15_000,
  );
  if (!acceptEnabled)
    throw new Error("Accept EULA button did not enable after checking");
  const accepted = await clickByAriaLabel("Accept EULA");
  if (!accepted) throw new Error("Accept EULA button could not be clicked");
  const setupReady = await waitForInvoke(async () => {
    const report = await invoke("vanilla_setup_check");
    return report.eulaMatchesSelected && report.ready ? report : null;
  }, 45_000);
  if (!setupReady)
    throw new Error(
      "the wizard setup never became ready after EULA acceptance",
    );
  const readyText = await waitFor(
    'document.body.innerText.includes("Ready to generate")',
    30_000,
  );
  if (!readyText) throw new Error("wizard did not show the ready state");

  const settingsFilled =
    (await setInput("Vanilla generation seed", "FuryBuilderM96UI")) &&
    (await setInput("Vanilla generation center X", "4500")) &&
    (await setInput("Vanilla generation center Z", "4500")) &&
    (await setInput("Vanilla generation radius in chunks", "1")) &&
    (await setInput("Vanilla generation time limit seconds", "180")) &&
    (await setInput("Vanilla generation memory limit", "2048")) &&
    (await setInput("Vanilla generation view distance", "3")) &&
    (await setInput("Vanilla generation CPU threads", "2"));
  if (!settingsFilled)
    throw new Error("Vanilla wizard settings inputs were not found");
  await waitFor(
    "!document.querySelector('button[aria-label=\"Refresh Vanilla World setup\"]')?.disabled",
    30_000,
  );

  const startDisabledBeforeConsent = await evaluate(
    "Boolean(document.querySelector('button[aria-label=\"Start Vanilla World generation\"]')?.disabled)",
  );
  const consentChecked = await evaluate(`(() => {
    const checkbox = document.querySelector('input[aria-label="Confirm local official server run"]');
    if (!checkbox) return false;
    checkbox.click();
    return true;
  })()`);
  if (!consentChecked) throw new Error("run consent checkbox was not found");
  const startEnabled = await waitFor(
    "!document.querySelector('button[aria-label=\"Start Vanilla World generation\"]')?.disabled",
    15_000,
  );
  if (!startEnabled)
    throw new Error("Start generation button did not enable after consent");
  const started = await clickByAriaLabel("Start Vanilla World generation");
  if (!started) throw new Error("Start generation button could not be clicked");
  const firstRun = await waitForInvoke(async () => {
    const status = await invoke("server_generation_status");
    return status.active ?? status.last;
  }, 45_000);
  if (!firstRun) throw new Error("wizard generation did not start");
  const firstRunId = firstRun.runId;
  const firstCompleted = await waitForInvoke(async () => {
    const status = await invoke("server_generation_status");
    if (status.active) return null;
    return status.last?.snapshot.runId === firstRunId &&
      status.last.stopReason === "completed"
      ? status.last
      : null;
  }, 600_000);
  if (!firstCompleted)
    throw new Error("the wizard's real generation did not complete");
  const first = await runSummary(firstCompleted.runDir);
  if (
    first.report.snapshot.generatedChunks <
      first.report.snapshot.expectedChunks ||
    !first.report.bindAddress?.startsWith("127.0.0.1:") ||
    !first.report.processTreeGuarded
  ) {
    throw new Error(
      "the wizard run did not satisfy chunks/loopback/job-object guarantees",
    );
  }
  if (!first.report.generateStructures || first.report.consentAt === 0) {
    throw new Error("the wizard run did not record structures/consent");
  }
  if (first.report.datapacks.length !== 0) {
    throw new Error("the wizard run unexpectedly used a data pack");
  }
  const finalReportVisible = await waitFor(
    'document.body.innerText.includes("Last isolated generation")',
    30_000,
  );
  if (!finalReportVisible) {
    await clickByAriaLabel("Refresh Vanilla World setup");
    await waitFor(
      'document.body.innerText.includes("Last isolated generation")',
      30_000,
    );
  }
  await delay(750);

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

  const generatedTreeBefore = await hashTree(first.report.worldDir);
  const openEnabled = await waitFor(
    "!document.querySelector('button[aria-label=\"Open generated world as a project\"]')?.disabled",
    30_000,
  );
  if (!openEnabled) {
    throw new Error("the generated world could not be opened as a project");
  }
  const openClicked = await clickByAriaLabel(
    "Open generated world as a project",
  );
  if (!openClicked) {
    throw new Error("Open as project button could not be clicked");
  }
  const importedList = await waitForInvoke(async () => {
    const list = await invoke("project_list");
    return list.projects.length === 1 ? list : null;
  }, 120_000);
  if (!importedList) throw new Error("the generated world was not imported");
  const projectSummary = importedList.projects[0];
  const projectDir = join(appData, "projects", projectSummary.folder);
  const projectManifest = await readJson(join(projectDir, "project.json"));
  const importedInfo = await invoke("world_info");
  const session = await invoke("project_status");
  const sourceTree = await hashTree(join(projectDir, "source", "world"));
  const generatedTreeAfter = await hashTree(first.report.worldDir);
  const generatedTreePreserved =
    JSON.stringify(sourceTree) === JSON.stringify(generatedTreeBefore);
  const generatedWorldUnchanged =
    JSON.stringify(generatedTreeAfter) === JSON.stringify(generatedTreeBefore);
  const provenance = projectManifest.vanilla;
  const provenanceStored =
    Boolean(provenance) &&
    provenance.seed === first.report.seed &&
    provenance.preset === first.report.preset &&
    provenance.centerX === first.report.centerX &&
    provenance.centerZ === first.report.centerZ &&
    provenance.radiusChunks === first.report.radiusChunks &&
    provenance.generateStructures === first.report.generateStructures &&
    provenance.datapacks.length === 0 &&
    provenance.minecraftVersion === "1.21.4" &&
    provenance.dataVersion === 4189 &&
    provenance.serverJarSha1 === first.report.serverSha1 &&
    provenance.runId === firstRunId &&
    provenance.chunks === first.report.snapshot.generatedChunks &&
    provenance.regionFiles === first.report.snapshot.regionFiles;
  const importedAsVanilla =
    projectManifest.creationMode === "vanilla" &&
    projectManifest.source?.kind === "generated" &&
    projectManifest.source?.immutable === true &&
    projectManifest.source?.reference === "world" &&
    session.folder === projectSummary.folder &&
    session.name === projectSummary.name;
  if (!provenanceStored) {
    throw new Error(
      `vanilla provenance was not stored: ${JSON.stringify(provenance)}`,
    );
  }
  if (!generatedTreePreserved) {
    throw new Error(
      "the project source copy does not match the generated world",
    );
  }
  if (!generatedWorldUnchanged) {
    throw new Error("importing changed the generated run world");
  }
  if (
    !importedAsVanilla ||
    importedInfo.chunkCount === 0 ||
    importedInfo.blockCount === 0
  ) {
    throw new Error(
      `the imported project is not an editable vanilla world: ${JSON.stringify({ projectManifest, importedInfo, session })}`,
    );
  }

  const parity = await invoke("project_verify_vanilla_parity", {
    folder: projectSummary.folder,
    runId: null,
  });
  if (
    parity.status !== "matched" ||
    parity.comparison?.status !== "matched" ||
    parity.comparison?.mismatchCount !== 0 ||
    parity.comparison?.checkedChunks !== 9 ||
    parity.comparison?.checkedBlocks === 0
  ) {
    throw new Error(
      `reference-chunk parity did not match: ${JSON.stringify(parity)}`,
    );
  }
  if (
    parity.referenceKind !== "generatedRun" ||
    parity.comparison?.requestedChunks !== 9 ||
    parity.comparison?.sampledChunks !== 9 ||
    parity.comparison?.limitations.length === 0
  ) {
    throw new Error(
      `reference-chunk parity reported an unexpected scope: ${JSON.stringify(parity)}`,
    );
  }

  const managerReopened = await clickByText("Project");
  if (!managerReopened) {
    throw new Error("the project manager could not be reopened");
  }
  const projectsTabSelected = await clickTabByText("Projects");
  if (!projectsTabSelected) {
    throw new Error("the Projects tab could not be selected");
  }
  const parityButtonPresent = await waitFor(
    "Boolean(document.querySelector('button[aria-label=\"Verify generated base\"]'))",
    30_000,
  );
  if (!parityButtonPresent) {
    throw new Error("the generated base verification panel did not render");
  }
  const parityUiClicked = await clickByAriaLabel("Verify generated base");
  if (!parityUiClicked) {
    throw new Error("the generated base verification button was not clickable");
  }
  const parityUiMatched = await waitFor(
    'document.body.innerText.toLowerCase().includes("block parity matched")',
    120_000,
  );
  if (!parityUiMatched) {
    throw new Error("the parity panel did not show a matching result");
  }
  const parityUiCounts = await evaluate(
    'document.querySelector(\'[data-testid="vanilla-parity-counts"]\')?.textContent ?? ""',
  );
  if (!parityUiCounts.includes("checked 9 of 9")) {
    throw new Error(
      `the parity panel did not show the checked chunk count: ${parityUiCounts}`,
    );
  }
  const parityCapture = await send("Page.captureScreenshot", {
    format: "png",
    captureBeyondViewport: false,
    fromSurface: true,
  });
  const parityScreenshot = Buffer.from(parityCapture.data, "base64");
  const parityScreenshotPath = screenshotPath.replace(/\.png$/i, "-parity.png");
  await writeFile(parityScreenshotPath, parityScreenshot, { flag: "wx" });

  const inspected = await invoke("server_inspect_datapacks", {
    paths: [datapackPath],
  });
  if (
    inspected.length !== 1 ||
    !inspected[0].valid ||
    inspected[0].packFormat !== 61 ||
    inspected[0].sha256 !== datapackSha256
  ) {
    throw new Error(
      `data pack inspection failed: ${JSON.stringify(inspected)}`,
    );
  }

  const secondSnapshot = await invoke("server_start_generation", {
    request: {
      seed: "FuryBuilderM96Pack",
      preset: "normal",
      centerX: 5000,
      centerZ: 5000,
      radiusChunks: 1,
      timeLimitSeconds: 180,
      memoryMb: 2048,
      viewDistance: 3,
      cpuThreads: 2,
      generateStructures: false,
      consent: true,
      datapacks: [{ path: datapackPath }],
    },
  });
  const secondRunId = secondSnapshot.runId;
  const secondCompleted = await waitForInvoke(async () => {
    const status = await invoke("server_generation_status");
    if (status.active) return null;
    return status.last?.snapshot.runId === secondRunId &&
      status.last.stopReason === "completed"
      ? status.last
      : null;
  }, 600_000);
  if (!secondCompleted)
    throw new Error("the data pack generation did not complete");
  const second = await runSummary(secondCompleted.runDir);
  if (second.report.datapacks.length !== 1) {
    throw new Error("the data pack was not recorded in the run report");
  }
  if (
    second.report.datapacks[0].sha256 !== datapackSha256 ||
    second.report.datapacks[0].packFormat !== 61
  ) {
    throw new Error(
      "the run report data pack metadata does not match the inspected file",
    );
  }
  const copiedPack = second.packs.find(
    (pack) => pack.name === second.report.datapacks[0].name,
  );
  if (!copiedPack || copiedPack.sha256 !== datapackSha256) {
    throw new Error(
      "the data pack was not copied into the isolated world with the same hash",
    );
  }
  if (second.report.generateStructures) {
    throw new Error("generateStructures=false was not honored");
  }
  if (!second.properties.includes("generate-structures=false")) {
    throw new Error(
      "server.properties does not contain generate-structures=false",
    );
  }
  if (!second.report.bindAddress?.startsWith("127.0.0.1:")) {
    throw new Error("the data pack run did not bind to loopback");
  }

  const parityWrongReference = await invoke("project_verify_vanilla_parity", {
    folder: projectSummary.folder,
    runId: secondRunId,
  });
  if (
    parityWrongReference.status !== "refused" ||
    parityWrongReference.comparison !== null ||
    parityWrongReference.refusals.length === 0 ||
    !parityWrongReference.refusals.some((reason) => reason.includes("seed"))
  ) {
    throw new Error(
      `a different generator configuration was not refused: ${JSON.stringify(parityWrongReference)}`,
    );
  }

  const runs = await invoke("server_generation_runs");
  const javaSha256After = createHash("sha256")
    .update(await readFile(javaPath))
    .digest("hex");
  const settings = await readJson(join(appData, "server.json"));
  const evidence = {
    tool: { name: "furybuilder-vanilla-wizard-ui", version: 3 },
    generatedAt: new Date().toISOString(),
    target:
      "Minecraft Java 1.21.4 / Vanilla World wizard, World Core import and reference-chunk parity",
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
      path: plainPath(selectedJar.path),
      sha1: selectedJar.sha1,
      size: selectedJar.size,
      dataVersion: selectedJar.dataVersion,
    },
    setup: {
      beforeEulaReady: setupBefore.ready,
      beforeEulaEulaMatches: setupBefore.eulaMatchesSelected,
      afterEulaReady: setupReady.ready,
      afterEulaEulaMatches: setupReady.eulaMatchesSelected,
      javaReady: setupReady.javaReady,
      serverReady: setupReady.serverReady,
      targetMinecraftVersion: setupReady.targetMinecraftVersion,
      targetDataVersion: setupReady.targetDataVersion,
      targetJavaMajor: setupReady.targetJavaMajor,
      serverDatapackFormat: setupReady.serverDatapackFormat,
      blockersBeforeEula: setupBefore.blockers,
      eulaRecord: settings.eula ?? null,
    },
    wizardUi: {
      eulaCheckboxPresent,
      acceptDisabledBefore,
      startDisabledBeforeEula,
      startDisabledBeforeConsent,
      eulaAcceptedThroughWizard: Boolean(setupReady.eulaMatchesSelected),
      consentRequiredForStart: startDisabledBeforeConsent === true,
    },
    firstRun: {
      report: first.report,
      regionFiles: first.regionFiles,
      eulaFile: first.eula,
      properties: first.properties,
    },
    importedProject: {
      summary: projectSummary,
      creationMode: projectManifest.creationMode,
      source: projectManifest.source,
      vanilla: provenance ?? null,
      worldFile: projectManifest.worldFile,
      worldChecksum: projectManifest.worldChecksum,
      worldBytes: projectManifest.worldBytes,
      worldInfo: importedInfo,
      session,
      sourceFiles: sourceTree.length,
      sourceTreeDigest: treeDigest(sourceTree),
      generatedTreeDigest: treeDigest(generatedTreeBefore),
      generatedTreeAfterDigest: treeDigest(generatedTreeAfter),
      generatedTreePreserved,
      generatedWorldUnchanged,
    },
    datapackRun: {
      inspection: inspected[0],
      report: second.report,
      copiedPacks: second.packs,
      eulaFile: second.eula,
      properties: second.properties,
    },
    parity: {
      matched: parity,
      wrongReference: parityWrongReference,
      uiCounts: parityUiCounts,
      uiScreenshot: {
        path: parityScreenshotPath,
        bytes: parityScreenshot.length,
      },
    },
    cleanup: { runsReported: runs.runs.length },
    screenshot: { path: screenshotPath, bytes: screenshot.length },
    consoleErrors,
    conclusions: {
      javaProbedAndSelected: java.status === "compatible" && java.major === 21,
      serverJarVerified:
        selectedJar.ready === true &&
        selectedJar.sha1 === expectedSha1 &&
        selectedJar.dataVersion === 4189,
      eulaRequiredBeforeAcceptance:
        setupBefore.ready === false &&
        setupBefore.eulaMatchesSelected === false &&
        acceptDisabledBefore === true,
      eulaAcceptedThroughWizard: setupReady.eulaMatchesSelected === true,
      consentGateEnforced:
        startDisabledBeforeConsent === true && startEnabled === true,
      wizardRunCompleted:
        firstCompleted.stopReason === "completed" &&
        firstCompleted.worldComplete === true &&
        firstCompleted.exitCode === 0 &&
        first.report.snapshot.generatedChunks >=
          first.report.snapshot.expectedChunks,
      realRegionsAndChunks:
        first.report.snapshot.generatedChunks >=
          first.report.snapshot.expectedChunks && first.regionFiles.length > 0,
      worldgenSettingsHonored:
        first.report.generateStructures === true &&
        second.report.generateStructures === false &&
        second.properties.includes("generate-structures=false"),
      datapackValidated:
        inspected[0].valid === true &&
        inspected[0].packFormat === 61 &&
        inspected[0].sha256 === datapackSha256,
      datapackCopiedAndHashed:
        copiedPack?.sha256 === datapackSha256 &&
        second.report.datapacks[0].sha256 === datapackSha256,
      eulaAndConsentRecorded:
        first.report.consentAt > 0 &&
        first.report.eulaAcceptedAt > 0 &&
        second.report.consentAt > 0 &&
        second.report.eulaAcceptedAt > 0,
      loopbackOnly:
        first.report.bindAddress?.startsWith("127.0.0.1:") === true &&
        second.report.bindAddress?.startsWith("127.0.0.1:") === true &&
        first.properties.includes("server-ip=127.0.0.1") &&
        first.properties.includes("enable-rcon=false") &&
        first.properties.includes("enable-query=false"),
      processTreeGuarded:
        first.report.processTreeGuarded === true &&
        second.report.processTreeGuarded === true,
      worldImportedAsVanillaProject: importedAsVanilla,
      vanillaProvenanceStored: provenanceStored,
      generatedWorldSourcePreserved: generatedTreePreserved,
      generatedWorldUnchanged: generatedWorldUnchanged,
      importedWorldNonEmpty:
        importedInfo.chunkCount > 0 && importedInfo.blockCount > 0,
      referenceChunkParityMatched:
        parity.status === "matched" &&
        parity.comparison?.mismatchCount === 0 &&
        parity.comparison?.checkedChunks === 9 &&
        parity.comparison?.checkedBlocks > 0,
      paritySettingsMismatchRefused:
        parityWrongReference.status === "refused" &&
        parityWrongReference.comparison === null &&
        parityWrongReference.refusals.some((reason) => reason.includes("seed")),
      parityUiRendered:
        parityButtonPresent === true &&
        parityUiMatched === true &&
        parityUiCounts.includes("checked 9 of 9"),
      parityScreenshotCaptured: parityScreenshot.length > 10_000,
      externalJavaUnchanged: javaSha256Before === javaSha256After,
      screenshotCaptured: screenshot.length > 10_000,
      noConsoleErrors: consoleErrors.length === 0,
    },
    limitations: [
      "the official 1.21.4 server.jar is the previously verified file; this smoke does not re-download it",
      "the data pack is a small local validation fixture, not a third-party pack",
      "only a completed run can be imported; incomplete runs stay in the run inventory until a new run is generated",
      "the imported project keeps a byte-for-byte immutable source copy; edits are stored in the project overlay and are not exported by this smoke",
      "reference-chunk parity compares block states only; biomes remain in the immutable source copy and are not modelled by the editable world",
      "parity covers the requested 9 overworld chunks and this exact seed/preset/data pack configuration, not general vanilla parity",
      "generation is a small verification area, not a full world",
    ],
  };
  const failed = Object.entries(evidence.conclusions)
    .filter(([, value]) => value !== true)
    .map(([key]) => key);
  if (failed.length > 0) {
    throw new Error(
      `Vanilla wizard smoke conclusions failed: ${JSON.stringify(failed)}; consoleErrors=${JSON.stringify(consoleErrors)}`,
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
        parityScreenshot: parityScreenshotPath,
        parity: {
          status: parity.status,
          checkedChunks: parity.comparison?.checkedChunks,
          checkedSections: parity.comparison?.checkedSections,
          checkedBlocks: parity.comparison?.checkedBlocks,
          mismatchCount: parity.comparison?.mismatchCount,
          wrongReference: parityWrongReference.status,
        },
        setup: {
          ready: setupReady.ready,
          java: setupReady.javaReady,
          server: setupReady.serverReady,
          eula: setupReady.eulaMatchesSelected,
        },
        firstRun: {
          chunks: first.report.snapshot.generatedChunks,
          expected: first.report.snapshot.expectedChunks,
          bind: first.report.bindAddress,
          guarded: first.report.processTreeGuarded,
        },
        datapackRun: {
          chunks: second.report.snapshot.generatedChunks,
          expected: second.report.snapshot.expectedChunks,
          packFormat: second.report.datapacks[0].packFormat,
          structures: second.report.generateStructures,
        },
        importedProject: {
          mode: projectManifest.creationMode,
          source: projectManifest.source,
          seed: provenance?.seed,
          chunks: importedInfo.chunkCount,
          blocks: importedInfo.blockCount,
          generatedTreePreserved,
          generatedWorldUnchanged,
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
