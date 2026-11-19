/**
 * M11.4 demo-project and screenshot harness for the real Windows desktop app.
 *
 * It starts the debug Tauri build with an isolated app-data directory and WebView2
 * profile, creates a project through the real frontend store, loads the deterministic
 * built-in demo scene, saves it through the real project store, reloads the app, and
 * captures the actual WebView2 window, viewport, MCP access dialog and a deterministic
 * observation map.
 *
 * It also runs the real stdio MCP observation probe against the live endpoint. Nothing
 * in this harness fabricates a screenshot, a project snapshot or an MCP response.
 *
 * Usage (the Vite dev server must already run at http://127.0.0.1:1420):
 *   node scripts/demo-webview2.mjs <new-output-directory> [<minecraft-launcher-root>|placeholder]
 *
 * Output:
 *   evidence.json
 *   mcp-probe.json
 *   project/furybuilder-demo.furybuilder/   (clean project, no cache/session lock)
 *   screenshots/editor.png
 *   screenshots/viewport.png
 *   screenshots/mcp-access.png
 *   screenshots/observation-top-down.png
 *
 * Mojang game files are never copied into the output. Screenshots are rendered with the
 * locally installed Minecraft assets supplied by the caller; no Mojang JAR, texture or
 * asset file is written to the repository by this script.
 */
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import {
  copyFile,
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
const dimension = "minecraft:overworld";
const projectName = "FuryBuilder Demo";
const projectFolderName = "furybuilder-demo.furybuilder";
const browserArguments = [
  "--remote-debugging-port=0",
  "--disable-background-timer-throttling",
  "--disable-renderer-backgrounding",
  "--disable-backgrounding-occluded-windows",
  "--disable-features=CalculateNativeWinOcclusion",
];

const output = process.argv[2] ? resolve(process.argv[2]) : null;
const launcherRoot =
  process.argv[3] && process.argv[3] !== "placeholder"
    ? resolve(process.argv[3])
    : null;

if (process.platform !== "win32") {
  throw new Error("This harness is Windows-only.");
}
if (!output) {
  throw new Error(
    "Usage: node scripts/demo-webview2.mjs <new-output-directory> [<minecraft-launcher-root>|placeholder]",
  );
}
try {
  await stat(output);
  throw new Error(`Output directory already exists: ${output}`);
} catch (error) {
  if (error?.code !== "ENOENT") throw error;
}
await mkdir(output, { recursive: true });

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

function pngSize(bytes) {
  if (!bytes.subarray(0, 8).equals(Buffer.from("89504e470d0a1a0a", "hex"))) {
    throw new Error("not a PNG");
  }
  if (bytes.toString("ascii", 12, 16) !== "IHDR") {
    throw new Error("PNG has no IHDR");
  }
  return { width: bytes.readUInt32BE(16), height: bytes.readUInt32BE(20) };
}

async function writePng(path, bytes) {
  const size = pngSize(bytes);
  await mkdir(dirname(path), { recursive: true });
  await writeFile(path, bytes, { flag: "wx" });
  return {
    file: path.slice(output.length + 1).replaceAll("\\", "/"),
    width: size.width,
    height: size.height,
    bytes: bytes.length,
    sha256: sha256(bytes),
  };
}

async function pathExists(path) {
  try {
    await stat(path);
    return true;
  } catch {
    return false;
  }
}

let app = null;
let ws = null;
let appData = null;
let profile = null;
let appStderr = "";
let nextId = 0;
const pending = new Map();
const consoleErrors = [];

try {
  const vite = await fetch(`${origin}/src/features/viewport/Viewport.tsx`, {
    signal: AbortSignal.timeout(5_000),
  }).catch(() => null);
  if (!vite?.ok) {
    throw new Error(`Start this checkout's Vite server at ${origin}`);
  }
  if (!(await pathExists(appPath))) {
    throw new Error(`Build the debug desktop app first: ${appPath}`);
  }
  if (launcherRoot && !(await pathExists(launcherRoot))) {
    throw new Error(`Minecraft launcher root not found: ${launcherRoot}`);
  }

  const executable = await readFile(appPath);
  const executableSha256 = sha256(executable);
  appData = await mkdtemp(join(tmpdir(), "fury-demo-data-"));
  profile = await mkdtemp(join(tmpdir(), "fury-demo-webview-"));
  app = spawn(appPath, [], {
    cwd: root,
    env: {
      ...process.env,
      APPDATA: appData,
      FURYBUILDER_APP_DATA_DIR: appData,
      WEBVIEW2_USER_DATA_FOLDER: profile,
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: browserArguments.join(" "),
    },
    stdio: ["ignore", "ignore", "pipe"],
    windowsHide: false,
  });
  app.stderr?.on("data", (chunk) => {
    if (appStderr.length < 64 * 1024) appStderr += chunk.toString();
  });

  let port = null;
  let page = null;
  for (let attempt = 0; attempt < 240 && !page; attempt += 1) {
    if (app.exitCode !== null) {
      throw new Error(
        `Tauri exited with ${app.exitCode}: ${appStderr.slice(-2000)}`,
      );
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
      const response = await fetch(`http://127.0.0.1:${port}/json/list`, {
        signal: AbortSignal.timeout(1_000),
      });
      if (response.ok) {
        page = (await response.json()).find((item) => item.type === "page");
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
    const message = JSON.parse(event.data);
    if (message.id) {
      const waiter = pending.get(message.id);
      if (!waiter) return;
      pending.delete(message.id);
      if (message.error) {
        waiter.reject(new Error(JSON.stringify(message.error)));
      } else {
        waiter.resolve(message.result);
      }
      return;
    }
    if (message.method === "Runtime.exceptionThrown") {
      const details = message.params?.exceptionDetails ?? {};
      consoleErrors.push({
        source: "exception",
        text:
          details.exception?.description ??
          details.text ??
          "uncaught exception",
        url: details.url ?? null,
      });
    } else if (message.method === "Runtime.consoleAPICalled") {
      if (message.params?.type === "error") {
        consoleErrors.push({
          source: "console.error",
          text: (message.params?.args ?? [])
            .map((arg) => arg.value ?? arg.description ?? arg.type ?? "")
            .join(" "),
          url: null,
        });
      }
    }
  };

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

  async function evaluate(expression, timeoutMs = 180_000) {
    const result = await send(
      "Runtime.evaluate",
      { expression, awaitPromise: true, returnByValue: true },
      timeoutMs,
    );
    if (result.exceptionDetails) {
      throw new Error(
        result.exceptionDetails.exception?.description ??
          result.exceptionDetails.text,
      );
    }
    return result.result.value;
  }

  async function waitFor(expression, timeoutMs, label) {
    const deadline = Date.now() + timeoutMs;
    let lastError = null;
    while (Date.now() < deadline) {
      try {
        if (await evaluate(expression)) return;
      } catch (error) {
        lastError = error;
      }
      await delay(250);
    }
    throw new Error(
      `Timed out waiting for ${label}${lastError ? `: ${lastError.message}` : ""}`,
    );
  }

  async function invoke(command, args = {}, timeoutMs = 180_000) {
    return evaluate(
      `window.__TAURI_INTERNALS__.invoke(${JSON.stringify(command)}, ${JSON.stringify(args)})`,
      timeoutMs,
    );
  }

  async function importStore(modulePath, expression) {
    return evaluate(
      `(async () => {
        const store = await import(${JSON.stringify(modulePath)});
        return ${expression};
      })()`,
    );
  }

  await send("Page.enable");
  await send("Runtime.enable");
  let ready = false;
  for (let attempt = 0; attempt < 80 && !ready; attempt += 1) {
    try {
      if (attempt % 8 === 0) {
        await send("Page.navigate", { url: `${origin}/` });
      }
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
  await waitFor(
    'typeof window.__TAURI_INTERNALS__ === "object" && document.readyState === "complete"',
    120_000,
    "Tauri internals",
  );

  let assetLabel = null;
  if (launcherRoot) {
    await invoke("assets_save_source", {
      kind: "minecraft_root",
      path: launcherRoot,
    });
  }

  const created = await importStore(
    "/src/state/project-store.ts",
    `(async () => {
      const world = await import("/src/state/world-store.ts");
      world.useWorldStore.getState().setLoadRadius(1);
      const ok = await store.useProjectStore.getState().create(${JSON.stringify(projectName)}, "empty");
      return {
        ok,
        error: store.useProjectStore.getState().error,
        current: store.useProjectStore.getState().current,
      };
    })()`,
  );
  if (!created?.ok || !created.current?.folder) {
    throw new Error(`project creation failed: ${JSON.stringify(created)}`);
  }

  const demo = await invoke("load_demo_world", {});
  if (!demo || demo.revision < 1) {
    throw new Error(
      `load_demo_world returned no world: ${JSON.stringify(demo)}`,
    );
  }
  const saved = await importStore(
    "/src/state/project-store.ts",
    `(async () => {
      const ok = await store.useProjectStore.getState().save();
      return {
        ok,
        error: store.useProjectStore.getState().error,
        lastSave: store.useProjectStore.getState().lastSave,
      };
    })()`,
  );
  if (!saved?.ok || !saved.lastSave) {
    throw new Error(`demo project save failed: ${JSON.stringify(saved)}`);
  }
  const savedRevision = saved.lastSave.project?.summary?.worldRevision ?? null;

  // Reload from the published snapshot, so the screenshots and MCP probe use the same
  // saved project a user will open, not the transient in-memory setup state.
  await send("Page.navigate", { url: `${origin}/` });
  await waitFor(
    'document.readyState === "complete" && typeof window.__TAURI_INTERNALS__ === "object"',
    120_000,
    "app reload",
  );
  await waitFor("!!window.__furyViewport", 120_000, "viewport mount");
  await waitFor(
    '!document.body.innerText.includes("No project open")',
    60_000,
    "project resume",
  );
  await waitFor(
    `(async () => {
      const world = await import("/src/state/world-store.ts");
      const state = world.useWorldStore.getState();
      return state.revision === ${Number(demo.revision)} && state.status === "ready";
    })()`,
    60_000,
    "demo world revision in the frontend",
  );
  await waitFor(
    `(() => {
      const renderer = window.__furyViewport;
      return Boolean(
        renderer &&
        renderer.world?.revision === ${Number(demo.revision)} &&
        (renderer.world.loadedChunkCount?.() ?? 0) >= 9
      );
    })()`,
    60_000,
    "demo chunks in the renderer",
  );

  if (launcherRoot) {
    await waitFor(
      'document.body.innerText.includes("models ·") && document.body.innerText.includes("tex")',
      60_000,
      "verified Minecraft asset bundle",
    );
    assetLabel = await evaluate(`(() => {
      const match = document.body.innerText.match(/([0-9]+) models · ([0-9]+) tex/);
      return match ? match[0] : null;
    })()`);
  }

  // Wait for success toasts from create/save to disappear; they are UI feedback, not
  // part of the demo scene.
  await delay(6_000);

  await evaluate(`(() => {
    const renderer = window.__furyViewport;
    renderer.setCameraPose([30, 86, 30], [3, 68, -3]);
    return true;
  })()`);
  await delay(1_500);

  const viewport = await evaluate(
    `(() => {
      const renderer = window.__furyViewport;
      const capture = renderer.capture();
      return { dataUrl: capture.dataUrl, metadata: capture.metadata };
    })()`,
    60_000,
  );
  const pngPrefix = "data:image/png;base64,";
  if (!viewport?.dataUrl?.startsWith(pngPrefix)) {
    throw new Error("viewport capture did not return a PNG data URL");
  }
  const viewportPng = Buffer.from(
    viewport.dataUrl.slice(pngPrefix.length),
    "base64",
  );
  const viewportRecord = await writePng(
    join(output, "screenshots", "viewport.png"),
    viewportPng,
  );

  const appShot = await send("Page.captureScreenshot", {
    format: "png",
    captureBeyondViewport: false,
    fromSurface: true,
  });
  const editorRecord = await writePng(
    join(output, "screenshots", "editor.png"),
    Buffer.from(appShot.data, "base64"),
  );

  await importStore(
    "/src/state/ui-store.ts",
    `(() => {
      store.useUiStore.getState().setMcpAccessOpen(true);
      return true;
    })()`,
  );
  await waitFor(
    'document.body.innerText.includes("AI / MCP access") && document.body.innerText.includes("Read-only")',
    15_000,
    "MCP access dialog",
  );
  await delay(750);
  const dialogShot = await send("Page.captureScreenshot", {
    format: "png",
    captureBeyondViewport: false,
    fromSurface: true,
  });
  const mcpDialogRecord = await writePng(
    join(output, "screenshots", "mcp-access.png"),
    Buffer.from(dialogShot.data, "base64"),
  );
  await importStore(
    "/src/state/ui-store.ts",
    `(() => {
      store.useUiStore.getState().setMcpAccessOpen(false);
      return true;
    })()`,
  );
  await delay(500);

  const observation = await invoke("observe", {
    request: {
      dimension,
      bounds: {
        min: { x: -16, y: 60, z: -16 },
        max: { x: 31, y: 72, z: 31 },
      },
      mode: "top_down",
    },
  });
  if (!observation?.image?.startsWith(pngPrefix)) {
    throw new Error("observe did not return a PNG data URL");
  }
  const observationPng = Buffer.from(
    observation.image.slice(pngPrefix.length),
    "base64",
  );
  const observationRecord = await writePng(
    join(output, "screenshots", "observation-top-down.png"),
    observationPng,
  );

  const worldInfo = await invoke("world_info");
  const appInfo = await invoke("app_info");
  const assetsSource = await invoke("assets_saved_source").catch(() => null);

  // The real stdio MCP client probe. It reads bridge_info, world_info, a block, a region,
  // rejects a stale revision and captures the live viewport. It writes no world data.
  const runtimePath = join(appData, "mcp", "runtime.json");
  const probePath = join(output, "mcp-probe.json");
  const probe = spawn(
    process.execPath,
    [join(root, "mcp", "bridge", "scripts", "probe-live-observation.mjs")],
    {
      cwd: root,
      env: {
        ...process.env,
        APPDATA: appData,
        FURYBUILDER_MCP_RUNTIME: runtimePath,
        FURYBUILDER_OBSERVATION_EVIDENCE: probePath,
      },
      stdio: ["ignore", "pipe", "pipe"],
      windowsHide: true,
    },
  );
  let probeStdout = "";
  let probeStderr = "";
  probe.stdout?.on("data", (chunk) => {
    if (probeStdout.length < 64 * 1024) probeStdout += chunk.toString();
  });
  probe.stderr?.on("data", (chunk) => {
    if (probeStderr.length < 64 * 1024) probeStderr += chunk.toString();
  });
  const probeExit = await new Promise((resolvePromise) => {
    const timeout = setTimeout(() => {
      probe.kill();
      resolvePromise({ code: null, timeout: true });
    }, 180_000);
    probe.once("exit", (code) => {
      clearTimeout(timeout);
      resolvePromise({ code, timeout: false });
    });
  });
  if (probeExit.code !== 0 || !(await pathExists(probePath))) {
    throw new Error(
      `MCP observation probe failed (exit ${probeExit.code}, timeout ${probeExit.timeout}): ${probeStderr.slice(-3000)}`,
    );
  }
  const mcpProbe = JSON.parse(await readFile(probePath, "utf8"));

  await importStore(
    "/src/state/project-store.ts",
    `(async () => {
      await store.useProjectStore.getState().close(false);
      return true;
    })()`,
  );
  await delay(1_000);

  const projectsRoot = join(appData, "projects");
  const projectSource = join(projectsRoot, created.current.folder);
  const projectDestination = join(output, "project", projectFolderName);
  await mkdir(join(projectDestination, "data"), { recursive: true });
  await mkdir(join(projectDestination, "source"), { recursive: true });
  await copyFile(
    join(projectSource, "project.json"),
    join(projectDestination, "project.json"),
  );
  const dataFiles = await readdir(join(projectSource, "data"));
  const worldFiles = dataFiles.filter(
    (name) => name.startsWith("world-") && name.endsWith(".fwl"),
  );
  if (worldFiles.length !== 1) {
    throw new Error(
      `expected exactly one world snapshot, found ${JSON.stringify(worldFiles)}`,
    );
  }
  await copyFile(
    join(projectSource, "data", worldFiles[0]),
    join(projectDestination, "data", worldFiles[0]),
  );
  if (await pathExists(join(projectSource, "source", "source.json"))) {
    await copyFile(
      join(projectSource, "source", "source.json"),
      join(projectDestination, "source", "source.json"),
    );
  }

  const manifestBytes = await readFile(
    join(projectDestination, "project.json"),
  );
  const manifest = JSON.parse(manifestBytes.toString("utf8"));
  const worldBytes = await readFile(
    join(projectDestination, manifest.worldFile),
  );
  const worldChecksum = sha256(worldBytes);
  if (manifest.worldChecksum !== worldChecksum) {
    throw new Error("copied demo world checksum does not match its manifest");
  }
  if (manifest.worldBytes !== worldBytes.length) {
    throw new Error("copied demo world byte count does not match its manifest");
  }

  const screenshotMetadata = viewport.metadata ?? null;
  const evidence = {
    schemaVersion: 1,
    milestone: "M11",
    task: "demo-project-screenshots-mcp-setup",
    recordedAt: new Date().toISOString(),
    pass: true,
    environment: {
      platform: "Windows",
      origin,
      isolatedAppData: true,
      executable: "target/debug/furybuilder-desktop.exe",
      executableSha256,
      appVersion: appInfo?.version ?? null,
      webview2: appInfo?.webview2 ?? null,
    },
    assets: {
      mode: launcherRoot ? "local_minecraft_installation" : "placeholder",
      label: assetLabel,
      savedSourceKind: assetsSource?.source?.kind ?? null,
      savedSourceValid: assetsSource?.report?.valid ?? null,
      mojangAssetsBundled: false,
      note: "Screenshots use the caller's legally installed local assets. No Mojang JAR, texture or asset file is copied into this repository or the demo project.",
    },
    project: {
      name: manifest.name,
      folder: projectFolderName,
      projectId: manifest.projectId,
      creationMode: manifest.creationMode,
      targetMinecraftVersion: manifest.targetMinecraftVersion,
      targetDataVersion: manifest.targetDataVersion,
      worldRevision: manifest.worldRevision,
      savedRevision,
      chunks: worldInfo?.chunkCount ?? null,
      sections: worldInfo?.sectionCount ?? null,
      blockCount: worldInfo?.blockCount ?? null,
      dimensions: worldInfo?.dimensions?.length ?? null,
      worldFile: manifest.worldFile,
      worldBytes: worldBytes.length,
      worldChecksum,
      manifestBytes: manifestBytes.length,
      manifestSha256: sha256(manifestBytes),
      demoContent:
        "Seeded by the built-in load_demo_world scene; not Minecraft worldgen.",
    },
    screenshots: {
      editor: {
        ...editorRecord,
        kind: "desktop_app",
        worldRevision: demo.revision,
      },
      viewport: {
        ...viewportRecord,
        kind: "webgl_viewport",
        metadata: screenshotMetadata,
      },
      mcpAccess: { ...mcpDialogRecord, kind: "mcp_access_dialog" },
      observationTopDown: {
        ...observationRecord,
        kind: "deterministic_observation",
        mode: observation.mode,
        worldRevision: observation.worldRevision,
        bounds: observation.bounds,
        legendEntries: observation.legend?.length ?? null,
        partial: observation.partial,
        warnings: observation.warnings ?? [],
      },
    },
    mcp: {
      probeFile: "mcp-probe.json",
      protocolEra: mcpProbe.protocolEra,
      worldRevision: mcpProbe.worldRevision,
      block: mcpProbe.observations?.block ?? null,
      region: mcpProbe.observations?.region ?? null,
      staleRejection: mcpProbe.observations?.staleRejection ?? null,
      capture: mcpProbe.observations?.capture ?? null,
    },
    consoleErrors,
    notes: [
      "The demo project contains only FuryBuilder block-state data, a manifest and an empty source manifest.",
      "The project folder is renamed from the app-generated folder to furybuilder-demo.furybuilder; the display name stays FuryBuilder Demo.",
      "Screenshots and the MCP capture are real WebView2/WebGL output from this run, not mockups.",
      "The demo scene is deterministic demo content, not a Minecraft world and not evidence of vanilla worldgen parity.",
      "No code-signing or Minecraft redistribution claim is made by this evidence.",
    ],
  };
  await writeFile(
    join(output, "evidence.json"),
    `${JSON.stringify(evidence, null, 2)}\n`,
  );

  console.log(
    JSON.stringify(
      {
        ok: true,
        output,
        project: projectDestination,
        screenshots: [
          editorRecord,
          viewportRecord,
          mcpDialogRecord,
          observationRecord,
        ],
        mcp: {
          probeFile: probePath,
          protocolEra: mcpProbe.protocolEra,
          worldRevision: mcpProbe.worldRevision,
        },
        consoleErrors,
      },
      null,
      2,
    ),
  );
} catch (error) {
  console.error(
    `DEMO_HARNESS_FAILED: ${error instanceof Error ? `${error.message}\n${error.stack}` : String(error)}`,
  );
  if (appStderr) {
    console.error(`APP_STDERR:\n${appStderr.slice(-4000)}`);
  }
  process.exitCode = 1;
} finally {
  ws?.close();
  if (app && app.exitCode === null) app.kill();
  await delay(1_500);
  if (appData) {
    await rm(appData, { recursive: true, force: true, maxRetries: 3 }).catch(
      () => {},
    );
  }
  if (profile) {
    await rm(profile, { recursive: true, force: true, maxRetries: 3 }).catch(
      () => {},
    );
  }
}
