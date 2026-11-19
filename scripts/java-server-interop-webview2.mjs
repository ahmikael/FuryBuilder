/**
 * M11.5 release verification: import/export against the supported Java server.
 *
 * The supported world format is one exact target: Minecraft Java 1.21.4, classic Anvil,
 * DataVersion 4189. This harness proves both directions with the official server:
 *
 *   1. the official 1.21.4 server generates a fresh fixed-seed normal world;
 *   2. the real desktop app analyzes and imports that world into a project;
 *   3. the app applies a block edit through its normal preview/apply path and saves/reopens;
 *   4. the backend exports a complete new world folder from the immutable project source;
 *   5. the same official server force-loads the exported world and verifies the edited,
 *      untouched and terrain blocks with /execute if block;
 *   6. the app re-imports the server-saved exported world and reads the persisted blocks.
 *
 * The server JAR and Java runtime are user-provided and never copied into the repository.
 * The harness requires an explicit --accept-minecraft-eula flag and never accepts the
 * Minecraft EULA on the user's behalf.
 *
 * Usage (Vite dev server must already run at http://127.0.0.1:1420, and the debug desktop
 * app plus world-io examples must be built):
 *
 *   node scripts/java-server-interop-webview2.mjs <new-output-directory> \
 *     <java.exe> <server.jar> <expected-server-sha1> --accept-minecraft-eula
 *
 * Output:
 *   <output>/evidence.json
 *   <output>/work/... (generated and exported worlds, logs; not published to the repo)
 */
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import {
  mkdir,
  mkdtemp,
  readFile,
  readdir,
  rename,
  rm,
  stat,
  writeFile,
} from "node:fs/promises";
import { createWriteStream } from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, relative, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const origin = "http://127.0.0.1:1420";
const appPath = join(root, "target", "debug", "furybuilder-desktop.exe");
const exportProbePath = join(
  root,
  "target",
  "debug",
  "examples",
  "world_export_probe.exe",
);
const dimension = "minecraft:overworld";
const targetVersion = "1.21.4";
const targetDataVersion = 4189;
const fixtureSeed = "4189";
const editPosition = { x: 0, y: 300, z: 0 };
const footingPosition = { x: 0, y: -64, z: 0 };
const projectName = "Server Interop Import";
const reimportName = "Server Interop Reimport";
const browserArguments = [
  "--remote-debugging-port=0",
  "--disable-background-timer-throttling",
  "--disable-renderer-backgrounding",
  "--disable-backgrounding-occluded-windows",
  "--disable-features=CalculateNativeWinOcclusion",
];

const output = process.argv[2] ? resolve(process.argv[2]) : null;
const javaPath = process.argv[3] ? resolve(process.argv[3]) : null;
const serverJarPath = process.argv[4] ? resolve(process.argv[4]) : null;
const expectedSha1 = (process.argv[5] ?? "").toLowerCase();
const acceptEula = process.argv.includes("--accept-minecraft-eula");

if (process.platform !== "win32") {
  throw new Error("This harness is Windows-only.");
}
if (!output || !javaPath || !serverJarPath || !expectedSha1) {
  throw new Error(
    "Usage: node scripts/java-server-interop-webview2.mjs <new-output-directory> <java.exe> <server.jar> <expected-server-sha1> --accept-minecraft-eula",
  );
}
if (!acceptEula) {
  throw new Error(
    "This harness never accepts the Minecraft EULA for you. Read https://www.minecraft.net/en-us/eula and pass --accept-minecraft-eula only if you agree.",
  );
}
try {
  await stat(output);
  throw new Error(`Output directory already exists: ${output}`);
} catch (error) {
  if (error?.code !== "ENOENT") throw error;
}
await mkdir(output, { recursive: true });
const workRoot = join(output, "work");
await mkdir(workRoot, { recursive: true });

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

async function fileSha256(path) {
  return sha256(await readFile(path));
}

async function fileSha1(path) {
  return createHash("sha1")
    .update(await readFile(path))
    .digest("hex");
}

async function treeSummary(path) {
  const files = [];
  let bytes = 0;
  const pending = [path];
  while (pending.length > 0) {
    const directory = pending.pop();
    for (const entry of await readdir(directory, { withFileTypes: true })) {
      const full = join(directory, entry.name);
      if (entry.isDirectory()) {
        pending.push(full);
      } else if (entry.isFile()) {
        const contents = await readFile(full);
        files.push({
          relativePath: relative(path, full).replaceAll("\\", "/"),
          bytes: contents.length,
          sha256: sha256(contents),
        });
        bytes += contents.length;
      }
    }
  }
  files.sort((a, b) => a.relativePath.localeCompare(b.relativePath));
  return { files, fileCount: files.length, bytes };
}

async function pathExists(path) {
  try {
    await stat(path);
    return true;
  } catch {
    return false;
  }
}

function treeChangedFiles(before, after) {
  const beforeMap = new Map(
    before.files.map((file) => [file.relativePath, file.sha256]),
  );
  const afterMap = new Map(
    after.files.map((file) => [file.relativePath, file.sha256]),
  );
  const changed = [];
  for (const [path, hash] of beforeMap) {
    if (afterMap.get(path) !== hash) changed.push(path);
  }
  for (const path of afterMap.keys()) {
    if (!beforeMap.has(path)) changed.push(path);
  }
  return changed.sort();
}

function run(command, args, options = {}) {
  const timeoutMs = options.timeoutMs ?? 600_000;
  return new Promise((resolvePromise, reject) => {
    const child = spawn(command, args, {
      cwd: options.cwd ?? root,
      env: { ...process.env, ...(options.env ?? {}) },
      stdio: ["ignore", "pipe", "pipe"],
      windowsHide: true,
    });
    let stdout = "";
    let stderr = "";
    const timeout = setTimeout(() => {
      child.kill();
      reject(new Error(`command timed out: ${command} ${args.join(" ")}`));
    }, timeoutMs);
    child.stdout?.on("data", (chunk) => {
      if (stdout.length < 4 * 1024 * 1024) stdout += chunk.toString();
    });
    child.stderr?.on("data", (chunk) => {
      if (stderr.length < 4 * 1024 * 1024) stderr += chunk.toString();
    });
    child.on("error", (error) => {
      clearTimeout(timeout);
      reject(error);
    });
    child.on("exit", (code) => {
      clearTimeout(timeout);
      resolvePromise({ code, stdout, stderr });
    });
  });
}

function sanitizeText(text) {
  return text
    .replaceAll(output, "<output>")
    .replace(/[A-Za-z]:\\[^\s"']+/g, "<path>")
    .slice(-200_000);
}

function freeLoopbackPort() {
  return new Promise((resolvePromise, reject) => {
    const probe = createServer();
    probe.once("error", reject);
    probe.listen(0, "127.0.0.1", () => {
      const address = probe.address();
      if (!address || typeof address === "string") {
        probe.close();
        reject(new Error("could not allocate a loopback port"));
        return;
      }
      const { port } = address;
      probe.close((error) => {
        if (error) reject(error);
        else resolvePromise(port);
      });
    });
  });
}

async function writeServerProperties(serverRoot) {
  const port = await freeLoopbackPort();
  const lines = [
    "allow-flight=false",
    "allow-nether=false",
    "difficulty=peaceful",
    "enable-command-block=false",
    "enable-query=false",
    "enable-rcon=false",
    "enforce-secure-profile=false",
    "force-gamemode=true",
    "gamemode=creative",
    "generate-structures=false",
    "level-name=world",
    `level-seed=${fixtureSeed}`,
    "level-type=minecraft:normal",
    "max-players=1",
    "motd=FuryBuilder release interop fixture",
    "online-mode=false",
    "pause-when-empty-seconds=0",
    "server-ip=127.0.0.1",
    `server-port=${port}`,
    "simulation-distance=2",
    "spawn-protection=0",
    "sync-chunk-writes=true",
    "view-distance=2",
  ];
  await writeFile(
    join(serverRoot, "server.properties"),
    `${lines.join("\n")}\n`,
    {
      encoding: "utf8",
      flag: "wx",
    },
  );
  await writeFile(join(serverRoot, "eula.txt"), "eula=true\n", {
    encoding: "utf8",
    flag: "wx",
  });
}

async function startServer(serverRoot, logPath) {
  await mkdir(serverRoot, { recursive: true });
  await writeServerProperties(serverRoot);
  const logStream = createWriteStream(logPath, { flags: "wx" });
  const child = spawn(
    javaPath,
    ["-Xms512M", "-Xmx1G", "-jar", serverJarPath, "nogui"],
    {
      cwd: serverRoot,
      env: { ...process.env },
      stdio: ["pipe", "pipe", "pipe"],
      windowsHide: true,
    },
  );
  child.stdout.pipe(logStream);
  child.stderr.pipe(logStream);
  let exit = null;
  child.on("exit", (code) => {
    exit = { code };
  });
  const gameLog = join(serverRoot, "logs", "latest.log");
  async function readLog() {
    try {
      return await readFile(gameLog, "utf8");
    } catch {
      return "";
    }
  }
  async function waitFor(needle, timeoutMs) {
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
      if ((await readLog()).includes(needle)) return;
      if (exit)
        throw new Error(`server exited before "${needle}": ${exit.code}`);
      await delay(250);
    }
    throw new Error(
      `server did not log "${needle}" in ${timeoutMs}ms\n${sanitizeText(await readLog()).slice(-4000)}`,
    );
  }
  function send(command) {
    child.stdin.write(`${command}\n`);
  }
  async function stop() {
    if (!exit) {
      send("stop");
      try {
        child.stdin.end();
      } catch {
        // The process may already be gone.
      }
    }
    const deadline = Date.now() + 180_000;
    while (!exit && Date.now() < deadline) await delay(250);
    if (!exit) {
      child.kill();
      throw new Error("server did not stop within 180 seconds");
    }
    await new Promise((resolvePromise) => logStream.end(resolvePromise));
    const text = await readLog().catch(() => "");
    return { exitCode: exit.code, log: text };
  }
  return { child, waitFor, send, stop, readLog };
}

function logLine(log, pattern) {
  return (
    log
      .split(/\r?\n/)
      .filter((line) => pattern.test(line))
      .map((line) => line.trim())
      .at(-1) ?? null
  );
}

function logIssues(log) {
  const patterns = [
    "Failed to load chunk",
    "Failed to save chunk",
    "Chunk file at",
    "Failed to load world",
    "Exception in server tick",
    "Unknown or incomplete command",
    "That position is not loaded",
  ];
  return log
    .split(/\r?\n/)
    .filter((line) => patterns.some((pattern) => line.includes(pattern)))
    .map((line) => line.trim());
}

function logErrors(log) {
  return log
    .split(/\r?\n/)
    .filter((line) => line.includes("ERROR]"))
    .map((line) => line.trim());
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
  if (!(await pathExists(javaPath)))
    throw new Error(`Java not found: ${javaPath}`);
  if (!(await pathExists(serverJarPath))) {
    throw new Error(`Server JAR not found: ${serverJarPath}`);
  }
  if (!(await pathExists(appPath))) {
    throw new Error(`Build the debug desktop app first: ${appPath}`);
  }
  const actualSha1 = (await fileSha1(serverJarPath)).toLowerCase();
  if (actualSha1 !== expectedSha1) {
    throw new Error(
      `server JAR SHA-1 mismatch: expected ${expectedSha1}, got ${actualSha1}`,
    );
  }

  const javaVersionRun = await run(javaPath, ["-version"], {
    timeoutMs: 30_000,
  });
  const javaVersion = `${javaVersionRun.stdout}${javaVersionRun.stderr}`.trim();

  await run(
    "cargo",
    ["build", "-q", "-p", "furybuilder-world-io", "--examples"],
    {
      cwd: root,
      timeoutMs: 600_000,
    },
  );
  if (!(await pathExists(exportProbePath))) {
    throw new Error(`world_export_probe was not built: ${exportProbePath}`);
  }
  const executableSha256 = await fileSha256(appPath);

  // 1. Official server generates the fixture world.
  const generatorRoot = join(workRoot, "generator");
  const generatorLog = join(workRoot, "generator.log");
  const generator = await startServer(generatorRoot, generatorLog);
  const generatorResult = await (async () => {
    try {
      await generator.waitFor("Done (", 300_000);
      generator.send("forceload add 0 0");
      await delay(3_000);
      generator.send(
        "execute if block 0 -64 0 minecraft:bedrock run say FURY_INTEROP_FIXTURE_BEDROCK_OK",
      );
      await generator.waitFor("FURY_INTEROP_FIXTURE_BEDROCK_OK", 30_000);
    } finally {
      return generator.stop();
    }
  })();
  if (generatorResult.exitCode !== 0) {
    throw new Error(`fixture server exited with ${generatorResult.exitCode}`);
  }
  const generatorVersionLine = logLine(
    generatorResult.log,
    /Starting minecraft server version/,
  );
  if (!generatorVersionLine?.includes(targetVersion)) {
    throw new Error(
      `fixture server is not ${targetVersion}: ${generatorVersionLine}`,
    );
  }
  if (logIssues(generatorResult.log).length > 0) {
    throw new Error(
      `fixture server issues: ${logIssues(generatorResult.log).join(" | ")}`,
    );
  }
  const fixtureWorld = join(generatorRoot, "world");
  if (!(await pathExists(fixtureWorld))) {
    throw new Error(`fixture world was not generated: ${fixtureWorld}`);
  }
  const fixtureBefore = await treeSummary(fixtureWorld);

  // 2. Real desktop app imports the server-generated world.
  const vite = await fetch(`${origin}/src/features/viewport/Viewport.tsx`, {
    signal: AbortSignal.timeout(5_000),
  }).catch(() => null);
  if (!vite?.ok)
    throw new Error(`Start this checkout's Vite server at ${origin}`);

  appData = await mkdtemp(join(tmpdir(), "fury-interop-data-"));
  profile = await mkdtemp(join(tmpdir(), "fury-interop-webview-"));
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
  if (!page) throw new Error("Tauri WebView2 did not expose a DevTools page");

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
      if (message.error)
        waiter.reject(new Error(JSON.stringify(message.error)));
      else waiter.resolve(message.result);
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
      });
    } else if (message.method === "Runtime.consoleAPICalled") {
      if (message.params?.type === "error") {
        consoleErrors.push({
          source: "console.error",
          text: (message.params?.args ?? [])
            .map((arg) => arg.value ?? arg.description ?? arg.type ?? "")
            .join(" "),
        });
      }
    }
  };

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

  async function evaluate(expression, timeoutMs = 300_000) {
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

  async function invoke(command, args = {}, timeoutMs = 300_000) {
    return evaluate(
      `window.__TAURI_INTERNALS__.invoke(${JSON.stringify(command)}, ${JSON.stringify(args)})`,
      timeoutMs,
    );
  }

  async function storeCall(modulePath, expression) {
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
  await waitFor(
    'typeof window.__TAURI_INTERNALS__ === "object" && document.readyState === "complete"',
    120_000,
    "Tauri internals",
  );

  const analysis = await storeCall(
    "/src/state/project-store.ts",
    `(async () => {
      const result = await store.useProjectStore.getState().analyzeWorld(${JSON.stringify(fixtureWorld)});
      return { result, error: store.useProjectStore.getState().error };
    })()`,
  );
  if (!analysis?.result?.supported) {
    throw new Error(
      `app rejected the fixture world: ${JSON.stringify(analysis)}`,
    );
  }
  const imported = await storeCall(
    "/src/state/project-store.ts",
    `(async () => {
      const world = await import("/src/state/world-store.ts");
      world.useWorldStore.getState().setLoadRadius(1);
      const ok = await store.useProjectStore.getState().importWorld(${JSON.stringify(projectName)}, ${JSON.stringify(fixtureWorld)});
      return {
        ok,
        error: store.useProjectStore.getState().error,
        current: store.useProjectStore.getState().current,
      };
    })()`,
  );
  if (!imported?.ok || !imported.current?.folder) {
    throw new Error(`app world import failed: ${JSON.stringify(imported)}`);
  }
  const projectFolder = imported.current.folder;
  await waitFor(
    `(async () => {
      const project = await import("/src/state/project-store.ts");
      const world = await import("/src/state/world-store.ts");
      const current = project.useProjectStore.getState().current;
      const state = world.useWorldStore.getState();
      return Boolean(current?.folder) &&
        current.folder === ${JSON.stringify(projectFolder)} &&
        state.status === "ready" &&
        state.revision >= 1;
    })()`,
    180_000,
    "imported project and world",
  );
  const fixtureAfterImport = await treeSummary(fixtureWorld);
  const importWorldInfo = await invoke("world_info");
  const blockBefore = await invoke("get_block", {
    dimension,
    pos: editPosition,
  });
  const appInfo = await invoke("app_info");
  const projectRoot = join(appData, "projects", projectFolder);
  const projectManifestAfterImport = JSON.parse(
    await readFile(join(projectRoot, "project.json"), "utf8"),
  );
  const sourceWorld = join(projectRoot, "source", "world");

  // 3. Real editor command path: preview, apply, save, close, reopen.
  const editResult = await evaluate(
    `(async () => {
      const commands = await import("/src/editor/commands.ts");
      const world = await import("/src/state/world-store.ts");
      const result = await commands.applyEditorOperation({
        type: "set_block",
        dimension: ${JSON.stringify(dimension)},
        pos: ${JSON.stringify(editPosition)},
        state: { name: "minecraft:gold_block", properties: {} },
      }, { silentSuccess: true });
      return {
        result,
        revision: world.useWorldStore.getState().revision,
        error: window.__furyProjectError ?? null,
      };
    })()`,
  );
  if (!editResult?.result || editResult.result.changedBlocks !== 1) {
    throw new Error(`app block edit failed: ${JSON.stringify(editResult)}`);
  }
  const blockAfter = await invoke("get_block", {
    dimension,
    pos: editPosition,
  });
  const saved = await storeCall(
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
    throw new Error(`app project save failed: ${JSON.stringify(saved)}`);
  }
  const closed = await storeCall(
    "/src/state/project-store.ts",
    `(async () => {
      const ok = await store.useProjectStore.getState().close(false);
      return { ok, error: store.useProjectStore.getState().error };
    })()`,
  );
  if (!closed?.ok)
    throw new Error(`app project close failed: ${JSON.stringify(closed)}`);
  await delay(1_000);
  const reopened = await storeCall(
    "/src/state/project-store.ts",
    `(async () => {
      const ok = await store.useProjectStore.getState().open(${JSON.stringify(projectFolder)});
      return { ok, error: store.useProjectStore.getState().error };
    })()`,
  );
  if (!reopened?.ok) {
    throw new Error(`app project reopen failed: ${JSON.stringify(reopened)}`);
  }
  await waitFor(
    `(async () => {
      const project = await import("/src/state/project-store.ts");
      const world = await import("/src/state/world-store.ts");
      return Boolean(project.useProjectStore.getState().current?.folder) &&
        world.useWorldStore.getState().status === "ready";
    })()`,
    180_000,
    "reopened project",
  );
  const reopenedWorldInfo = await invoke("world_info");
  const blockReopened = await invoke("get_block", {
    dimension,
    pos: editPosition,
  });
  const savedManifest = JSON.parse(
    await readFile(join(projectRoot, "project.json"), "utf8"),
  );
  if (blockReopened?.name !== "minecraft:gold_block") {
    throw new Error(
      `reopened block is not the saved edit: ${JSON.stringify(blockReopened)}`,
    );
  }
  await storeCall(
    "/src/state/project-store.ts",
    `store.useProjectStore.getState().close(false)`,
  );
  await delay(1_000);

  // 4. Backend export of a complete new world folder from the immutable source copy.
  const sourceBeforeExport = await treeSummary(sourceWorld);
  const exportedWorld = join(workRoot, "exported-world");
  const exportRun = await run(
    exportProbePath,
    [
      sourceWorld,
      exportedWorld,
      "--rebuild-derived",
      dimension,
      String(editPosition.x),
      String(editPosition.y),
      String(editPosition.z),
      "minecraft:gold_block",
    ],
    { cwd: root, timeoutMs: 600_000 },
  );
  if (exportRun.code !== 0) {
    throw new Error(
      `world export probe failed: ${exportRun.stderr.slice(-4000)}`,
    );
  }
  const exportProbe = JSON.parse(exportRun.stdout);
  if (!exportProbe.ok)
    throw new Error("world export probe did not report success");
  if (!exportProbe.report.sourceUnchanged) {
    throw new Error("world export changed its source tree");
  }
  if (!exportProbe.report.outputRevalidated) {
    throw new Error("world export did not revalidate its output");
  }
  if (!exportProbe.report.copiedFilesMatchSource) {
    throw new Error("world export copied files do not match the source");
  }
  if (exportProbe.report.replacedChunks !== 1) {
    throw new Error(
      `world export replaced ${exportProbe.report.replacedChunks} chunks, expected 1`,
    );
  }
  const sourceAfterExport = await treeSummary(sourceWorld);
  const sourceChanged = treeChangedFiles(sourceBeforeExport, sourceAfterExport);
  if (sourceChanged.length > 0) {
    throw new Error(
      `project source changed during export: ${sourceChanged.join(", ")}`,
    );
  }
  const exportedTree = await treeSummary(exportedWorld);

  // 5. Official server loads the exported world and verifies block states.
  const verifierRoot = join(workRoot, "verifier");
  await mkdir(verifierRoot, { recursive: true });
  const verifierWorld = join(verifierRoot, "world");
  await rename(exportedWorld, verifierWorld);
  const verifierBefore = await treeSummary(verifierWorld);
  const verifierLog = join(workRoot, "verifier.log");
  const verifier = await startServer(verifierRoot, verifierLog);
  const verifierResult = await (async () => {
    try {
      await verifier.waitFor("Done (", 300_000);
      verifier.send("forceload add 0 0");
      await delay(3_000);
      verifier.send(
        "execute if block 0 -64 0 minecraft:bedrock run say FURY_INTEROP_BEDROCK_OK",
      );
      verifier.send(
        "execute if block 0 300 0 minecraft:gold_block run say FURY_INTEROP_EDIT_OK",
      );
      verifier.send(
        "execute if block 0 300 1 minecraft:air run say FURY_INTEROP_NEIGHBOR_AIR_OK",
      );
      verifier.send(
        "execute if block 0 300 0 minecraft:air run say FURY_INTEROP_STALE_AIR",
      );
      await verifier.waitFor("FURY_INTEROP_BEDROCK_OK", 30_000);
      await verifier.waitFor("FURY_INTEROP_EDIT_OK", 30_000);
      await verifier.waitFor("FURY_INTEROP_NEIGHBOR_AIR_OK", 30_000);
      await delay(2_000);
    } finally {
      return verifier.stop();
    }
  })();
  if (verifierResult.exitCode !== 0) {
    throw new Error(`verifier server exited with ${verifierResult.exitCode}`);
  }
  const verifierVersionLine = logLine(
    verifierResult.log,
    /Starting minecraft server version/,
  );
  const verifierDoneLine = logLine(verifierResult.log, /Done \(/);
  const verifierIssues = logIssues(verifierResult.log);
  if (verifierIssues.length > 0) {
    throw new Error(`verifier server issues: ${verifierIssues.join(" | ")}`);
  }
  if (verifierResult.log.includes("FURY_INTEROP_STALE_AIR")) {
    throw new Error("verifier found air at the edited position");
  }
  const verifierAfter = await treeSummary(verifierWorld);
  const verifierChanged = treeChangedFiles(verifierBefore, verifierAfter);
  if (verifierChanged.length === 0) {
    throw new Error("verifier server did not rewrite any save file");
  }

  // 6. App re-imports the server-saved export and reads persisted blocks.
  const reimportAnalysis = await storeCall(
    "/src/state/project-store.ts",
    `(async () => {
      const result = await store.useProjectStore.getState().analyzeWorld(${JSON.stringify(verifierWorld)});
      return { result, error: store.useProjectStore.getState().error };
    })()`,
  );
  if (!reimportAnalysis?.result?.supported) {
    throw new Error(
      `app rejected the server-saved export: ${JSON.stringify(reimportAnalysis)}`,
    );
  }
  const reimported = await storeCall(
    "/src/state/project-store.ts",
    `(async () => {
      const world = await import("/src/state/world-store.ts");
      world.useWorldStore.getState().setLoadRadius(1);
      const ok = await store.useProjectStore.getState().importWorld(${JSON.stringify(reimportName)}, ${JSON.stringify(verifierWorld)});
      return {
        ok,
        error: store.useProjectStore.getState().error,
        current: store.useProjectStore.getState().current,
      };
    })()`,
  );
  if (!reimported?.ok || !reimported.current?.folder) {
    throw new Error(`app re-import failed: ${JSON.stringify(reimported)}`);
  }
  const reimportFolder = reimported.current.folder;
  await waitFor(
    `(async () => {
      const project = await import("/src/state/project-store.ts");
      const world = await import("/src/state/world-store.ts");
      const current = project.useProjectStore.getState().current;
      const state = world.useWorldStore.getState();
      return Boolean(current?.folder) &&
        current.folder === ${JSON.stringify(reimportFolder)} &&
        state.status === "ready";
    })()`,
    180_000,
    "re-imported exported world",
  );
  const reimportWorldInfo = await invoke("world_info");
  const reimportBlock = await invoke("get_block", {
    dimension,
    pos: editPosition,
  });
  const reimportFooting = await invoke("get_block", {
    dimension,
    pos: footingPosition,
  });
  const reimportManifest = JSON.parse(
    await readFile(
      join(appData, "projects", reimportFolder, "project.json"),
      "utf8",
    ),
  );
  await storeCall(
    "/src/state/project-store.ts",
    `store.useProjectStore.getState().close(false)`,
  );
  await delay(1_000);

  const fixtureAfter = await treeSummary(fixtureWorld);
  const fixtureChanged = treeChangedFiles(fixtureBefore, fixtureAfter);

  const checks = {
    officialServerGeneratedTargetVersion: Boolean(
      generatorVersionLine?.includes(targetVersion),
    ),
    appAcceptedGeneratedWorld: Boolean(analysis.result.supported),
    appImportedAsImportProject:
      projectManifestAfterImport.creationMode === "import" &&
      projectManifestAfterImport.targetMinecraftVersion === targetVersion &&
      projectManifestAfterImport.targetDataVersion === targetDataVersion,
    appReadFixtureBlock: blockBefore?.name === "minecraft:air",
    appEditAppliedThroughCommandPath:
      editResult.result.changedBlocks === 1 &&
      blockAfter?.name === "minecraft:gold_block",
    appSavedAndReopenedEdit: blockReopened?.name === "minecraft:gold_block",
    appSourceCopyIsReadOnly:
      fixtureChanged.length === 0 &&
      projectManifestAfterImport.source.immutable === true,
    exportSourceUnchanged: sourceChanged.length === 0,
    exportRevalidated: exportProbe.report.outputRevalidated === true,
    exportReplacedOneChunk: exportProbe.report.replacedChunks === 1,
    serverLoadedExportedWorld: Boolean(verifierDoneLine),
    serverFoundEditedBlock: verifierResult.log.includes("FURY_INTEROP_EDIT_OK"),
    serverFoundUntouchedNeighbor: verifierResult.log.includes(
      "FURY_INTEROP_NEIGHBOR_AIR_OK",
    ),
    serverFoundTerrainBlock: verifierResult.log.includes(
      "FURY_INTEROP_BEDROCK_OK",
    ),
    serverRejectedStaleAir: !verifierResult.log.includes(
      "FURY_INTEROP_STALE_AIR",
    ),
    serverSavedRewrittenFiles: verifierChanged.length > 0,
    serverLogHasNoChunkOrCommandIssues:
      verifierIssues.length === 0 &&
      logIssues(generatorResult.log).length === 0,
    appReimportedServerSavedWorld: Boolean(reimportAnalysis.result.supported),
    reimportedEditPersisted: reimportBlock?.name === "minecraft:gold_block",
    reimportedTerrainPersisted: reimportFooting?.name === "minecraft:bedrock",
    noConsoleErrors: consoleErrors.length === 0,
  };
  const failedChecks = Object.entries(checks)
    .filter(([, value]) => value !== true)
    .map(([name]) => name);
  if (failedChecks.length > 0) {
    throw new Error(`failed checks: ${failedChecks.join(", ")}`);
  }

  const evidence = {
    schemaVersion: 1,
    milestone: "M11",
    task: "java-server-import-export",
    recordedAt: new Date().toISOString(),
    pass: true,
    target: {
      minecraftVersion: targetVersion,
      dataVersion: targetDataVersion,
      layout: "classic_anvil",
      supportedVersions: [targetVersion],
    },
    environment: {
      platform: "Windows",
      appVersion: appInfo?.version ?? null,
      executable: "target/debug/furybuilder-desktop.exe",
      executableSha256,
      javaVersion: sanitizeText(javaVersion),
      serverJarSha1: expectedSha1,
      isolatedAppData: true,
    },
    fixture: {
      generator: "minecraft:normal",
      seed: fixtureSeed,
      generateStructures: false,
      serverVersionLine: generatorVersionLine,
      doneLine: logLine(generatorResult.log, /Done \(/),
      fileCount: fixtureBefore.fileCount,
      bytes: fixtureBefore.bytes,
      sourceUnchangedAfterImport: fixtureChanged.length === 0,
      generatedByOfficialServer: true,
    },
    appImport: {
      analysis: {
        supported: analysis.result.supported,
        versionName: analysis.result.versionName,
        dataVersion: analysis.result.dataVersion,
        layout: analysis.result.layout,
        chunks: analysis.result.chunks,
        regions: analysis.result.regions,
        blockEntities: analysis.result.blockEntities,
        entities: analysis.result.entities,
        warnings: analysis.result.warnings,
        blockers: analysis.result.blockers,
      },
      project: {
        folder: projectFolder,
        creationMode: projectManifestAfterImport.creationMode,
        targetMinecraftVersion:
          projectManifestAfterImport.targetMinecraftVersion,
        targetDataVersion: projectManifestAfterImport.targetDataVersion,
        worldRevision: projectManifestAfterImport.worldRevision,
        sourceImmutable: projectManifestAfterImport.source.immutable,
        sourceKind: projectManifestAfterImport.source.kind,
      },
      world: {
        revision: importWorldInfo.revision,
        chunks: importWorldInfo.chunkCount,
        sections: importWorldInfo.sectionCount,
        blocks: importWorldInfo.blockCount,
      },
      blockAtEditPositionBefore: blockBefore?.name ?? null,
      sourceCopy: {
        files: sourceBeforeExport.fileCount,
        bytes: sourceBeforeExport.bytes,
      },
    },
    appEdit: {
      operation: "set_block",
      dimension,
      position: editPosition,
      state: "minecraft:gold_block",
      changedBlocks: editResult.result.changedBlocks,
      revisionAfterApply: editResult.revision,
      blockAfterApply: blockAfter?.name ?? null,
      savedWorldRevision:
        saved.lastSave.project?.summary?.worldRevision ?? null,
      reopenedWorldRevision: reopenedWorldInfo.revision,
      blockAfterReopen: blockReopened?.name ?? null,
      manifestAfterSave: {
        worldRevision: savedManifest.worldRevision,
        lastSaveKind: savedManifest.lastSaveKind,
      },
    },
    export: {
      backend: "world_export_probe",
      derivedDataPolicy: exportProbe.derivedDataPolicy,
      report: {
        files: exportProbe.report.files,
        bytes: exportProbe.report.bytes,
        regions: exportProbe.report.regions,
        chunks: exportProbe.report.chunks,
        replacedRegions: exportProbe.report.replacedRegions,
        replacedChunks: exportProbe.report.replacedChunks,
        copiedFilesMatchSource: exportProbe.report.copiedFilesMatchSource,
        outputRevalidated: exportProbe.report.outputRevalidated,
        sourceUnchanged: exportProbe.report.sourceUnchanged,
        skippedFiles: exportProbe.report.skippedFiles,
        exportedRegions: exportProbe.report.exportedRegions.map((region) => ({
          dimension: region.dimension,
          regionX: region.regionX,
          regionZ: region.regionZ,
          bytes: region.bytes,
          replacedChunks: region.replacedChunks,
          sha256: region.sha256,
        })),
      },
      verifiedBlocks: exportProbe.verified,
      sourceTreeUnchanged: sourceChanged.length === 0,
      exportedTree: {
        fileCount: exportedTree.fileCount,
        bytes: exportedTree.bytes,
      },
    },
    serverVerification: {
      versionLine: verifierVersionLine,
      doneLine: verifierDoneLine,
      exitCode: verifierResult.exitCode,
      editMarker: verifierResult.log.includes("FURY_INTEROP_EDIT_OK"),
      staleAirMarkerAbsent: !verifierResult.log.includes(
        "FURY_INTEROP_STALE_AIR",
      ),
      neighborAirMarker: verifierResult.log.includes(
        "FURY_INTEROP_NEIGHBOR_AIR_OK",
      ),
      terrainMarker: verifierResult.log.includes("FURY_INTEROP_BEDROCK_OK"),
      issueLines: verifierIssues,
      errorLines: logErrors(verifierResult.log),
      filesChangedByServer: verifierChanged.length,
      changedFiles: verifierChanged,
    },
    reimport: {
      analysis: {
        supported: reimportAnalysis.result.supported,
        versionName: reimportAnalysis.result.versionName,
        dataVersion: reimportAnalysis.result.dataVersion,
        chunks: reimportAnalysis.result.chunks,
        regions: reimportAnalysis.result.regions,
      },
      project: {
        folder: reimportFolder,
        creationMode: reimportManifest.creationMode,
        worldRevision: reimportManifest.worldRevision,
      },
      world: {
        revision: reimportWorldInfo.revision,
        chunks: reimportWorldInfo.chunkCount,
        sections: reimportWorldInfo.sectionCount,
        blocks: reimportWorldInfo.blockCount,
      },
      editedBlock: reimportBlock?.name ?? null,
      terrainBlock: reimportFooting?.name ?? null,
    },
    checks,
    consoleErrors,
    limitations: [
      "Supported world format is only Minecraft Java 1.21.4 classic Anvil, DataVersion 4189; no other version is claimed.",
      "The official dedicated server is the engine check; a licensed Minecraft client was not launched in this environment.",
      "One fixed-seed normal world with structures disabled and one edited block are evidence, not a universal compatibility proof.",
      "The world has no intentional block-entity, entity, scheduled-tick or POI edits; those categories stay preserved in the source copy.",
      "The complete world-folder export is a backend path (KL-10); the beta UI exposes structure .nbt export, not a one-click world copy.",
      "The server JAR and Java runtime are user-provided and are not distributed with FuryBuilder.",
      "The generated/exported worlds and full server logs stay in the harness output; this evidence file contains counts, hashes, markers and sanitized summaries.",
    ],
  };
  await writeFile(
    join(output, "evidence.json"),
    `${JSON.stringify(evidence, null, 2)}\n`,
    "utf8",
  );
  console.log(
    JSON.stringify(
      {
        ok: true,
        evidence: join(output, "evidence.json"),
        projectFolder,
        reimportFolder,
        checks,
        serverVerification: {
          doneLine: verifierDoneLine,
          changedFiles: verifierChanged.length,
        },
      },
      null,
      2,
    ),
  );
} catch (error) {
  console.error(
    `JAVA_SERVER_INTEROP_FAILED: ${
      error instanceof Error
        ? `${error.message}\n${error.stack}`
        : String(error)
    }`,
  );
  if (appStderr) console.error(`APP_STDERR:\n${appStderr.slice(-4000)}`);
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
