/**
 * M11.7 clean-user-setup verification.
 *
 * The supported release target is Windows. This harness runs the real desktop app in an
 * isolated per-user environment that has no Minecraft launcher, no Java on PATH, no
 * JAVA_HOME and no FuryBuilder-managed dependencies:
 *
 *   1. Empty Project creation, save, close and reopen must work without Java or Minecraft.
 *   2. Asset and Java discovery must report the missing dependencies instead of guessing.
 *   3. Settings must say Java/server are optional for Empty Project, and opening the
 *      Vanilla World wizard must not download anything automatically.
 *   4. The backend must reject a server run without the explicit consent flag before it
 *      reads any dependency, and no server process or run directory may appear.
 *   5. Offline metadata requests must fail without leaving downloads/staging directories.
 *   6. A second session seeds conventional launcher layouts (default root plus two
 *      launcher-profile game directories) and corrupted/stale Java and server caches.
 *      Discovery must find the launcher roots read-only and reject every broken cache.
 *
 * This is not a fresh Windows installation and does not emulate different launcher brands.
 * It uses isolated environment variables, synthetic `.minecraft`-style folders and an
 * optional real third-party root for manual selection. The remaining external gap stays in
 * known limitations (KL-43).
 *
 * Usage (Vite dev server must already run at http://127.0.0.1:1420):
 *
 *   node scripts/clean-setup-webview2.mjs <new-output-directory> [<third-party-root>|placeholder] <java21.exe>
 *
 * The Java executable is not used by the clean session. It is selected explicitly only in
 * the final consent-gate check, after the clean no-Java scenario has already been recorded.
 * Output:
 *   <output>/evidence.json
 *   <output>/work/... (isolated app data and synthetic launchers; not published)
 */
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdir, mkdtemp, readFile, readdir, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath } from "node:url";
import { crc32 } from "node:zlib";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const origin = "http://127.0.0.1:1420";
const appPath = join(root, "target", "debug", "furybuilder-desktop.exe");
const browserArguments = [
  "--remote-debugging-port=0",
  "--disable-background-timer-throttling",
  "--disable-renderer-backgrounding",
  "--disable-backgrounding-occluded-windows",
  "--disable-features=CalculateNativeWinOcclusion",
];
const projectName = "Clean Setup Empty";
const targetVersion = "1.21.4";
const targetDataVersion = 4189;

const output = process.argv[2] ? resolve(process.argv[2]) : null;
const thirdPartyArg = process.argv[3] ?? null;
const javaArg = process.argv[4] ?? null;
const thirdPartyRoot =
  thirdPartyArg && thirdPartyArg !== "placeholder" ? resolve(thirdPartyArg) : null;
const javaPath = javaArg && javaArg !== "placeholder" ? resolve(javaArg) : null;

if (process.platform !== "win32") {
  throw new Error("This harness is Windows-only.");
}
if (!output || !javaPath) {
  throw new Error(
    "Usage: node scripts/clean-setup-webview2.mjs <new-output-directory> [<third-party-root>|placeholder] <java21.exe>",
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

const sandbox = join(workRoot, "sandbox");
const appData = join(sandbox, "app-data");
const home = join(sandbox, "user");
const roaming = join(home, "AppData", "Roaming");
const localAppData = join(home, "AppData", "Local");
const programFiles = join(sandbox, "program-files");
const programFilesX86 = join(sandbox, "program-files-x86");
for (const directory of [appData, home, roaming, localAppData, programFiles, programFilesX86]) {
  await mkdir(directory, { recursive: true });
}

const cleanEnv = { ...process.env };
for (const variable of [
  "JAVA_HOME",
  "JDK_HOME",
  "JRE_HOME",
  "GRAALVM_HOME",
  "_JAVA_OPTIONS",
  "CLASSPATH",
]) {
  delete cleanEnv[variable];
}
cleanEnv.APPDATA = roaming;
cleanEnv.LOCALAPPDATA = localAppData;
cleanEnv.USERPROFILE = home;
cleanEnv.HOMEDRIVE = home.slice(0, 2);
cleanEnv.HOMEPATH = home.slice(2);
cleanEnv.ProgramFiles = programFiles;
cleanEnv["ProgramFiles(x86)"] = programFilesX86;
cleanEnv.FURYBUILDER_APP_DATA_DIR = appData;
cleanEnv.PATH = `${process.env.SystemRoot ?? "C:\\Windows"}\\System32;${process.env.SystemRoot ?? "C:\\Windows"}`;
// Deterministic offline path: every managed-dependency request goes through a closed
// loopback proxy. The host itself may still have network access; the app must not use it.
cleanEnv.HTTPS_PROXY = "http://127.0.0.1:9";
cleanEnv.https_proxy = cleanEnv.HTTPS_PROXY;
cleanEnv.HTTP_PROXY = cleanEnv.HTTPS_PROXY;
cleanEnv.http_proxy = cleanEnv.HTTPS_PROXY;
cleanEnv.ALL_PROXY = cleanEnv.HTTPS_PROXY;
cleanEnv.all_proxy = cleanEnv.HTTPS_PROXY;
cleanEnv.NO_PROXY = "127.0.0.1,localhost,::1";
cleanEnv.no_proxy = cleanEnv.NO_PROXY;

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

async function fileSha256(path) {
  return sha256(await readFile(path));
}

async function pathExists(path) {
  try {
    await stat(path);
    return true;
  } catch {
    return false;
  }
}

async function pathKind(path) {
  try {
    const metadata = await stat(path);
    return metadata.isDirectory() ? "directory" : metadata.isFile() ? "file" : "other";
  } catch (error) {
    return error?.code === "ENOENT" ? "missing" : "unreadable";
  }
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
          relativePath: full.slice(path.length + 1).replaceAll("\\", "/"),
          bytes: contents.length,
          sha256: sha256(contents),
        });
        bytes += contents.length;
      }
    }
  }
  files.sort((left, right) => left.relativePath.localeCompare(right.relativePath));
  return { files, fileCount: files.length, bytes };
}

function treeChangedFiles(before, after) {
  const beforeMap = new Map(before.files.map((file) => [file.relativePath, file.sha256]));
  const afterMap = new Map(after.files.map((file) => [file.relativePath, file.sha256]));
  const changed = [];
  for (const [path, hash] of beforeMap) {
    if (afterMap.get(path) !== hash) changed.push(path);
  }
  for (const path of afterMap.keys()) {
    if (!beforeMap.has(path)) changed.push(path);
  }
  return changed.sort();
}

async function listFiles(path) {
  if (!(await pathExists(path))) return [];
  const files = [];
  const pending = [path];
  while (pending.length > 0) {
    const directory = pending.pop();
    for (const entry of await readdir(directory, { withFileTypes: true })) {
      const full = join(directory, entry.name);
      if (entry.isDirectory()) pending.push(full);
      else if (entry.isFile()) files.push(full.slice(path.length + 1).replaceAll("\\", "/"));
    }
  }
  return files.sort();
}

function sanitizeText(text) {
  return String(text)
    .replaceAll(output, "<output>")
    .replaceAll(sandbox, "<sandbox>")
    .replace(/[A-Za-z]:\\[^\s"']+/g, "<path>")
    .slice(-4000);
}

// Minimal STORE-method ZIP writer. Used only for synthetic client JAR version metadata;
// no Mojang bytes are created, copied or redistributed.
function zipStore(entries) {
  const localParts = [];
  const centralParts = [];
  let offset = 0;
  for (const entry of entries) {
    const name = Buffer.from(entry.name, "utf8");
    const data = Buffer.isBuffer(entry.data) ? entry.data : Buffer.from(entry.data);
    const crc = crc32(data) >>> 0;
    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(20, 4);
    local.writeUInt16LE(0, 6);
    local.writeUInt16LE(0, 8);
    local.writeUInt16LE(0, 10);
    local.writeUInt16LE(0, 12);
    local.writeUInt32LE(crc, 14);
    local.writeUInt32LE(data.length, 18);
    local.writeUInt32LE(data.length, 22);
    local.writeUInt16LE(name.length, 26);
    local.writeUInt16LE(0, 28);
    localParts.push(local, name, data);

    const central = Buffer.alloc(46);
    central.writeUInt32LE(0x02014b50, 0);
    central.writeUInt16LE(20, 4);
    central.writeUInt16LE(20, 6);
    central.writeUInt16LE(0, 8);
    central.writeUInt16LE(0, 10);
    central.writeUInt16LE(0, 12);
    central.writeUInt16LE(0, 14);
    central.writeUInt32LE(crc, 16);
    central.writeUInt32LE(data.length, 20);
    central.writeUInt32LE(data.length, 24);
    central.writeUInt16LE(name.length, 28);
    central.writeUInt16LE(0, 30);
    central.writeUInt16LE(0, 32);
    central.writeUInt16LE(0, 34);
    central.writeUInt16LE(0, 36);
    central.writeUInt32LE(0, 38);
    central.writeUInt32LE(offset, 42);
    centralParts.push(central, name);
    offset += local.length + name.length + data.length;
  }
  const centralSize = centralParts.reduce((sum, part) => sum + part.length, 0);
  const eocd = Buffer.alloc(22);
  eocd.writeUInt32LE(0x06054b50, 0);
  eocd.writeUInt16LE(0, 4);
  eocd.writeUInt16LE(0, 6);
  eocd.writeUInt16LE(entries.length, 8);
  eocd.writeUInt16LE(entries.length, 10);
  eocd.writeUInt32LE(centralSize, 12);
  eocd.writeUInt32LE(offset, 16);
  eocd.writeUInt16LE(0, 20);
  return Buffer.concat([...localParts, ...centralParts, eocd]);
}

async function writeSyntheticClient(rootPath) {
  const versionDir = join(rootPath, "versions", targetVersion);
  await mkdir(versionDir, { recursive: true });
  const versionJson = Buffer.from(
    `${JSON.stringify(
      {
        id: targetVersion,
        name: targetVersion,
        world_version: targetDataVersion,
        java_version: 21,
        pack_version: { resource: 46, data: 61 },
      },
      null,
      2,
    )}\n`,
    "utf8",
  );
  const jar = zipStore([{ name: "version.json", data: versionJson }]);
  const jarPath = join(versionDir, `${targetVersion}.jar`);
  await writeFile(jarPath, jar);
  const metadata = {
    id: targetVersion,
    type: "release",
    releaseTime: "2024-12-03T10:12:57+00:00",
    javaVersion: { component: "java-runtime-delta", majorVersion: 21 },
    downloads: { client: { sha1: createHash("sha1").update(jar).digest("hex"), size: jar.length } },
  };
  await writeFile(
    join(versionDir, `${targetVersion}.json`),
    `${JSON.stringify(metadata, null, 2)}\n`,
    "utf8",
  );
  return jarPath;
}

async function seedLauncherLayouts() {
  const defaultRoot = join(roaming, ".minecraft");
  const instanceA = join(sandbox, "launchers", "instance-a");
  const instanceB = join(sandbox, "launchers", "instance-b");
  await writeSyntheticClient(defaultRoot);
  await writeSyntheticClient(instanceA);
  await writeSyntheticClient(instanceB);
  await writeFile(
    join(defaultRoot, "launcher_profiles.json"),
    `${JSON.stringify(
      {
        profiles: {
          "third-party-instance-a": { gameDir: instanceA },
          "third-party-instance-b": { gameDir: instanceB },
        },
      },
      null,
      2,
    )}\n`,
    "utf8",
  );
  return { defaultRoot, instanceA, instanceB };
}

async function seedCorruptedCaches() {
  const brokenJava = join(appData, "runtimes", "java", "broken-java");
  await mkdir(join(brokenJava, "bin"), { recursive: true });
  await writeFile(join(brokenJava, "bin", "java.exe"), "not a Java executable\n", "utf8");
  await writeFile(
    join(brokenJava, ".furybuilder-runtime.json"),
    `${JSON.stringify(
      {
        schemaVersion: 1,
        id: "broken-java",
        version: "21.0.0",
        releaseName: "Corrupted test runtime",
        sha256: "0".repeat(64),
        sourceUrl: "https://example.invalid/broken-java.zip",
        vendor: "test",
        os: "windows",
        arch: "x64",
        installedAt: 0,
        javaRelativePath: "bin/java.exe",
      },
      null,
      2,
    )}\n`,
    "utf8",
  );

  const managedServer = join(appData, "servers", `minecraft-${targetVersion}`);
  await mkdir(managedServer, { recursive: true });
  await writeFile(join(managedServer, "server.jar"), "corrupted server jar\n", "utf8");
  await writeFile(
    join(managedServer, ".furybuilder-server.json"),
    `${JSON.stringify(
      {
        schemaVersion: 1,
        version: targetVersion,
        dataVersion: targetDataVersion,
        javaMajor: 21,
        sha1: "0".repeat(40),
        size: 123456,
        releaseTime: "2024-12-03T10:12:57+00:00",
        sourceUrl: "https://example.invalid/server.jar",
        installedAt: 0,
      },
      null,
      2,
    )}\n`,
    "utf8",
  );

  const missingJava = join(appData, "missing-java", "bin", "java.exe");
  await writeFile(
    join(appData, "runtime.json"),
    `${JSON.stringify({ schemaVersion: 1, selectedJava: missingJava }, null, 2)}\n`,
    "utf8",
  );
  const missingServer = join(appData, "missing-server", "server.jar");
  await writeFile(
    join(appData, "server.json"),
    `${JSON.stringify(
      {
        schemaVersion: 1,
        selected: {
          path: missingServer,
          sha1: "0".repeat(40),
          size: 123456,
          dataVersion: targetDataVersion,
        },
        eula: null,
      },
      null,
      2,
    )}\n`,
    "utf8",
  );

  return { brokenJava, managedServer, missingJava, missingServer };
}
function waitForExit(child, timeoutMs) {
  return new Promise((resolvePromise) => {
    if (child.exitCode !== null) {
      resolvePromise(true);
      return;
    }
    const timer = setTimeout(() => resolvePromise(false), timeoutMs);
    child.once("exit", () => {
      clearTimeout(timer);
      resolvePromise(true);
    });
  });
}

async function openSession({ label, profileRoot }) {
  const profile = profileRoot;
  await mkdir(profile, { recursive: true });
  const app = spawn(appPath, [], {
    cwd: root,
    env: {
      ...cleanEnv,
      WEBVIEW2_USER_DATA_FOLDER: profile,
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: browserArguments.join(" "),
    },
    stdio: ["ignore", "ignore", "pipe"],
    windowsHide: false,
  });
  let stderr = "";
  app.stderr.on("data", (chunk) => {
    if (stderr.length < 64 * 1024) stderr += chunk.toString();
  });

  let ws = null;
  let nextId = 0;
  const pending = new Map();
  const consoleErrors = [];
  try {
    let port = null;
    let page = null;
    for (let attempt = 0; attempt < 240 && !page; attempt += 1) {
      if (app.exitCode !== null) {
        throw new Error(
          `${label}: Tauri exited with ${app.exitCode}: ${sanitizeText(stderr.slice(-2000))}`,
        );
      }
      try {
        if (!port) {
          const activePort = await readFile(
            join(profile, "EBWebView", "DevToolsActivePort"),
            "utf8",
          );
          const candidate = Number(activePort.split(/\r?\n/, 1)[0]);
          if (!Number.isInteger(candidate) || candidate < 1 || candidate > 65535) {
            throw new Error("invalid WebView2 DevTools port");
          }
          port = candidate;
        }
        const response = await fetch(`http://127.0.0.1:${port}/json/list`, {
          signal: AbortSignal.timeout(1000),
        });
        if (response.ok) {
          page = (await response.json()).find((item) => item.type === "page");
        }
      } catch {
        // WebView2 may still be starting.
      }
      if (!page) await delay(250);
    }
    if (!page) throw new Error(`${label}: WebView2 did not expose a DevTools page`);

    const debuggerUrl = new URL(page.webSocketDebuggerUrl);
    if (
      debuggerUrl.protocol !== "ws:" ||
      debuggerUrl.hostname !== "127.0.0.1" ||
      debuggerUrl.port !== String(port)
    ) {
      throw new Error(`${label}: WebView2 returned a non-loopback DevTools URL`);
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
        if (message.error) waiter.reject(new Error(JSON.stringify(message.error)));
        else waiter.resolve(message.result);
        return;
      }
      if (message.method === "Runtime.exceptionThrown") {
        const details = message.params?.exceptionDetails ?? {};
        consoleErrors.push({
          source: "exception",
          text: details.exception?.description ?? details.text ?? "uncaught exception",
        });
      } else if (message.method === "Runtime.consoleAPICalled") {
        if (message.params?.type === "error") {
          consoleErrors.push({
            source: "console.error",
            text: (message.params?.args ?? [])
              .map((argument) => argument.value ?? argument.description ?? argument.type ?? "")
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
          reject(new Error(`${label}: DevTools request timed out: ${method}`));
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
          result.exceptionDetails.exception?.description ?? result.exceptionDetails.text,
        );
      }
      return result.result.value;
    }

    async function waitFor(expression, timeoutMs, description) {
      const deadline = Date.now() + timeoutMs;
      let lastError = null;
      while (Date.now() < deadline) {
        try {
          if (await evaluate(expression)) return true;
        } catch (error) {
          lastError = error;
        }
        await delay(250);
      }
      throw new Error(
        `${label}: timed out waiting for ${description}${lastError ? `: ${lastError.message}` : ""}`,
      );
    }

    async function invoke(command, args = {}, timeoutMs = 300_000) {
      return evaluate(
        `window.__TAURI_INTERNALS__.invoke(${JSON.stringify(command)}, ${JSON.stringify(args)})`,
        timeoutMs,
      );
    }

    async function storeCall(modulePath, expression, timeoutMs = 300_000) {
      return evaluate(
        `(async () => {
          const store = await import(${JSON.stringify(modulePath)});
          return ${expression};
        })()`,
        timeoutMs,
      );
    }

    /** Clicks a visible element by selector and text with a real CDP mouse press. */
    async function click(selector, text) {
      const point = await evaluate(`(() => {
        const element = [...document.querySelectorAll(${JSON.stringify(selector)})].find(
          (candidate) => (candidate.textContent ?? "").includes(${JSON.stringify(text)}),
        );
        if (!element) return null;
        element.scrollIntoView({ block: "center", inline: "center" });
        const rect = element.getBoundingClientRect();
        if (rect.width <= 0 || rect.height <= 0) return null;
        return { x: rect.x + rect.width / 2, y: rect.y + rect.height / 2 };
      })()`);
      if (!point) return false;
      await send("Input.dispatchMouseEvent", {
        type: "mouseMoved",
        x: point.x,
        y: point.y,
      });
      await send("Input.dispatchMouseEvent", {
        type: "mousePressed",
        x: point.x,
        y: point.y,
        button: "left",
        clickCount: 1,
      });
      await send("Input.dispatchMouseEvent", {
        type: "mouseReleased",
        x: point.x,
        y: point.y,
        button: "left",
        clickCount: 1,
      });
      return true;
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
    if (!ready) throw new Error(`${label}: WebView2 did not load the local Vite frontend`);
    await waitFor(
      'typeof window.__TAURI_INTERNALS__ === "object" && document.readyState === "complete"',
      120_000,
      "Tauri internals",
    );

    async function close() {
      try {
        ws?.close();
      } catch {
        // Ignore an already-closed socket.
      }
      if (app.exitCode === null) app.kill();
      await waitForExit(app, 10_000);
      return { stderr, consoleErrors };
    }

    return { app, invoke, evaluate, waitFor, storeCall, click, close };
  } catch (error) {
    try {
      ws?.close();
    } catch {
      // Ignore.
    }
    if (app.exitCode === null) app.kill();
    await waitForExit(app, 10_000);
    throw error;
  }
}

function countByStatus(candidates) {
  const counts = {};
  for (const candidate of candidates) {
    const status = candidate.status ?? "unknown";
    counts[status] = (counts[status] ?? 0) + 1;
  }
  return counts;
}

function compatibleCount(candidates) {
  return candidates.filter((candidate) => candidate.status === "compatible").length;
}

async function dependencyDirectoryState() {
  const javaRoot = join(appData, "runtimes", "java");
  const serverRoot = join(appData, "servers");
  const runsRoot = join(appData, "server-runs");
  return {
    javaRootKind: await pathKind(javaRoot),
    serverRootKind: await pathKind(serverRoot),
    runsRootKind: await pathKind(runsRoot),
    javaFiles: await listFiles(javaRoot),
    serverFiles: await listFiles(serverRoot),
    runFiles: await listFiles(runsRoot),
  };
}

function hasDownloadArtifacts(files) {
  return files.some(
    (file) =>
      file.includes("/.download-") ||
      file.startsWith(".download-") ||
      file.includes("/.staging-") ||
      file.startsWith(".staging-"),
  );
}

async function networkProbe() {
  try {
    const response = await fetch(
      "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json",
      { signal: AbortSignal.timeout(8_000) },
    );
    return { reachable: response.ok, status: response.status };
  } catch (error) {
    return { reachable: false, error: sanitizeText(error?.message ?? error) };
  }
}

let firstSession = null;
let secondSession = null;
try {
  if (!(await pathExists(appPath))) {
    throw new Error(`Build the debug desktop app first: ${appPath}`);
  }
  if (!(await pathExists(javaPath))) {
    throw new Error(`Java 21 executable not found: ${javaPath}`);
  }
  const vite = await fetch(`${origin}/src/features/project/VanillaWorldWizard.tsx`, {
    signal: AbortSignal.timeout(5_000),
  }).catch(() => null);
  if (!vite?.ok) throw new Error(`Start this checkout's Vite server at ${origin}`);

  const executableSha256 = await fileSha256(appPath);
  const network = await networkProbe();

  // ---- Session 1: clean per-user environment, no launcher, no Java, no cache. ----
  const profileOne = join(workRoot, "webview-profile-1");
  firstSession = await openSession({ label: "clean session", profileRoot: profileOne });
  const first = firstSession;

  const cleanRuntime = await first.invoke("runtime_discover");
  const cleanAssets = await first.invoke("assets_discover");
  const cleanServers = await first.invoke("server_discover");
  const cleanJavaCache = await first.invoke("runtime_managed_list");
  const cleanServerCache = await first.invoke("server_managed_list");
  const cleanSetup = await first.invoke("vanilla_setup_check");
  const cleanDependenciesBefore = await dependencyDirectoryState();

  const appInfo = await first.invoke("app_info");

  // Settings: readiness must say Vanilla World is optional and blocked, without downloading.
  const settingsButton = await first.evaluate(
    `document.querySelector('button[aria-label="Settings"]') !== null`,
  );
  if (!settingsButton) throw new Error("clean session: Settings button is missing");
  await first.evaluate(
    `document.querySelector('button[aria-label="Settings"]').click()`,
  );
  await first.waitFor(
    `(document.querySelector('[role="dialog"]')?.textContent ?? "").includes("Minecraft assets")`,
    30_000,
    "Settings dialog",
  );
  await first.waitFor(
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
    "Vanilla World readiness",
  );
  const settingsState = await first.evaluate(`(() => {
    const dialog = document.querySelector('[role="dialog"]');
    const text = dialog?.textContent ?? "";
    const readiness = document.querySelector('[aria-labelledby="vanilla-readiness-heading"]');
    return {
      title: dialog?.querySelector("h2")?.textContent ?? null,
      optionalForEmptyProject: text.includes("Optional for Empty Project"),
      readiness: text.includes("Vanilla World is ready")
        ? "ready"
        : text.includes("Setup required")
          ? "setup_required"
          : text.includes("Readiness check unavailable")
            ? "unavailable"
            : "missing",
      readinessText: readiness?.textContent?.slice(0, 900) ?? null,
      mentionsNeverDownloads: text.includes("It never downloads"),
    };
  })()`);
  await first.evaluate(`(() => {
    const dialog = document.querySelector('[role="dialog"]');
    const close = [...(dialog?.querySelectorAll("button") ?? [])].find(
      (button) => button.textContent.trim() === "Close",
    );
    close?.click();
  })()`);
  await first.waitFor(
    `document.querySelector('[role="dialog"]') === null`,
    15_000,
    "Settings close",
  );

  // Empty Project must be created, saved and reopened with no Java or Minecraft.
  const created = await first.storeCall(
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
    throw new Error(`clean session: Empty Project creation failed: ${JSON.stringify(created)}`);
  }
  const projectFolder = created.current.folder;
  await first.waitFor(
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
    "Empty Project world",
  );
  const emptyWorldInfo = await first.invoke("world_info");
  const emptyManifest = JSON.parse(
    await readFile(join(appData, "projects", projectFolder, "project.json"), "utf8"),
  );
  const saved = await first.storeCall(
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
  if (!saved?.ok) {
    throw new Error(`clean session: Empty Project save failed: ${JSON.stringify(saved)}`);
  }
  const closed = await first.storeCall(
    "/src/state/project-store.ts",
    `(async () => {
      const ok = await store.useProjectStore.getState().close(false);
      return { ok, error: store.useProjectStore.getState().error };
    })()`,
  );
  if (!closed?.ok) throw new Error(`clean session: Empty Project close failed`);
  await delay(1_000);
  const reopened = await first.storeCall(
    "/src/state/project-store.ts",
    `(async () => {
      const ok = await store.useProjectStore.getState().open(${JSON.stringify(projectFolder)});
      return { ok, error: store.useProjectStore.getState().error };
    })()`,
  );
  if (!reopened?.ok) {
    throw new Error(`clean session: Empty Project reopen failed: ${JSON.stringify(reopened)}`);
  }
  await first.waitFor(
    `(async () => {
      const project = await import("/src/state/project-store.ts");
      const world = await import("/src/state/world-store.ts");
      return Boolean(project.useProjectStore.getState().current?.folder) &&
        world.useWorldStore.getState().status === "ready";
    })()`,
    180_000,
    "reopened Empty Project",
  );
  const reopenedWorldInfo = await first.invoke("world_info");
  const reopenedManifest = JSON.parse(
    await readFile(join(appData, "projects", projectFolder, "project.json"), "utf8"),
  );
  await first.storeCall(
    "/src/state/project-store.ts",
    `store.useProjectStore.getState().close(false)`,
  );
  await delay(1_000);

  // Vanilla World wizard: offers explicit actions, downloads nothing by itself.
  await first.storeCall(
    "/src/state/project-store.ts",
    `store.useProjectStore.getState().setManagerOpen(true)`,
  );
  await first.waitFor(
    `document.querySelector('[role="dialog"]') !== null`,
    30_000,
    "project manager",
  );
  await first.waitFor(
    `(document.querySelector('[role="dialog"]')?.textContent ?? "").includes("New project")`,
    15_000,
    "project manager tabs",
  );
  const newProjectTab = await first.click('[role="tab"]', "New project");
  if (!newProjectTab) throw new Error("clean session: New project tab was not found");
  await first.waitFor(
    `[...document.querySelectorAll("button")].some(
      (button) => (button.textContent ?? "").includes("Vanilla World"),
    )`,
    15_000,
    "creation mode buttons",
  );
  const vanillaMode = await first.click("button", "Vanilla World");
  if (!vanillaMode) throw new Error("clean session: Vanilla World mode was not found");
  await first.waitFor(
    `(document.body.textContent ?? "").includes("Vanilla World setup")`,
    60_000,
    "Vanilla World wizard",
  );
  await first.waitFor(
    `(() => {
      const text = document.body.textContent ?? "";
      return text.includes("Setup is incomplete") || text.includes("Ready to generate");
    })()`,
    180_000,
    "Vanilla World setup result",
  );
  const wizardState = await first.evaluate(`(() => {
    const text = document.body.textContent ?? "";
    const generate = document.querySelector('button[aria-label="Start Vanilla World generation"]');
    const consent = document.querySelector('input[aria-label="Confirm local official server run"]');
    const eula = document.querySelector('input[aria-label="I have read and accept the Minecraft EULA"]');
    return {
      setupIncomplete: text.includes("Setup is incomplete"),
      ready: text.includes("Ready to generate"),
      noCompatibleJava: text.includes("No compatible Java candidate"),
      javaNotSelected: text.includes("Java not selected"),
      serverNotSelected: text.includes("JAR not selected"),
      eulaRequired: text.includes("EULA required"),
      generateDisabled: Boolean(generate?.disabled),
      consentPresent: Boolean(consent),
      consentChecked: Boolean(consent?.checked),
      eulaChecked: Boolean(eula?.checked),
      downloadServerOffered:
        document.querySelector('button[aria-label="Download official Minecraft server JAR"]') !== null,
      chooseJavaOffered:
        document.querySelector('button[aria-label="Choose Java executable"]') !== null,
      chooseServerOffered:
        document.querySelector('button[aria-label="Choose server JAR"]') !== null,
    };
  })()`);

  // Clean backend refusal: with no saved Java, no generation request can reach startup.
  const cleanGenerationAttempt = await first.evaluate(`(async () => {
    try {
      await window.__TAURI_INTERNALS__.invoke("server_start_generation", {
        request: {
          seed: "CleanSetup",
          preset: "flat",
          centerX: 0,
          centerZ: 0,
          radiusChunks: 1,
          timeLimitSeconds: 120,
          memoryMb: 2048,
          viewDistance: 3,
          cpuThreads: 2,
          generateStructures: false,
          consent: false,
          datapacks: [],
        },
      });
      return { rejected: false, code: null, message: null };
    } catch (error) {
      return {
        rejected: true,
        code: error?.code ?? null,
        message: String(error?.message ?? error),
      };
    }
  })()`);
  const generationStatus = await first.invoke("server_generation_status");
  const cleanDependenciesAfter = await dependencyDirectoryState();

  // Offline metadata: no automatic dependency downloads, no staging artifacts.
  const offlineRequests = await first.evaluate(
    `(async () => {
      const outcomes = await Promise.allSettled([
        window.__TAURI_INTERNALS__.invoke("server_release"),
        window.__TAURI_INTERNALS__.invoke("runtime_temurin_latest"),
      ]);
      return outcomes.map((outcome) =>
        outcome.status === "fulfilled"
          ? { ok: true }
          : {
              ok: false,
              message: String(outcome.reason?.message ?? outcome.reason),
            },
      );
    })()`,
    90_000,
  );
  const offlineDependenciesAfter = await dependencyDirectoryState();
  const firstClose = await first.close();
  firstSession = null;
  const firstConsoleErrors = firstClose.consoleErrors;

  // ---- Session 2: conventional launcher layouts and corrupted/stale caches. ----
  const launchers = await seedLauncherLayouts();
  await seedCorruptedCaches();
  const launcherBefore = {
    defaultRoot: await treeSummary(launchers.defaultRoot),
    instanceA: await treeSummary(launchers.instanceA),
    instanceB: await treeSummary(launchers.instanceB),
  };

  const profileTwo = join(workRoot, "webview-profile-2");
  secondSession = await openSession({ label: "launcher/cache session", profileRoot: profileTwo });
  const second = secondSession;

  const launcherAssets = await second.invoke("assets_discover");
  const brokenRuntime = await second.invoke("runtime_discover");
  const brokenJavaCache = await second.invoke("runtime_managed_list");
  const brokenServers = await second.invoke("server_discover");
  const brokenServerCache = await second.invoke("server_managed_list");
  const brokenSetup = await second.invoke("vanilla_setup_check");
  const javaSelection = await second.invoke("runtime_select_java", { path: javaPath });
  const consentFalseAttempt = await second.evaluate(`(async () => {
    try {
      await window.__TAURI_INTERNALS__.invoke("server_start_generation", {
        request: {
          seed: "ConsentGate",
          preset: "flat",
          centerX: 0,
          centerZ: 0,
          radiusChunks: 1,
          timeLimitSeconds: 120,
          memoryMb: 2048,
          viewDistance: 3,
          cpuThreads: 2,
          generateStructures: false,
          consent: false,
          datapacks: [],
        },
      });
      return { rejected: false, code: null, message: null };
    } catch (error) {
      return {
        rejected: true,
        code: error?.code ?? null,
        message: String(error?.message ?? error),
      };
    }
  })()`);
  const consentTrueAttempt = await second.evaluate(`(async () => {
    try {
      await window.__TAURI_INTERNALS__.invoke("server_start_generation", {
        request: {
          seed: "ConsentGate",
          preset: "flat",
          centerX: 0,
          centerZ: 0,
          radiusChunks: 1,
          timeLimitSeconds: 120,
          memoryMb: 2048,
          viewDistance: 3,
          cpuThreads: 2,
          generateStructures: false,
          consent: true,
          datapacks: [],
        },
      });
      return { rejected: false, code: null, message: null };
    } catch (error) {
      return {
        rejected: true,
        code: error?.code ?? null,
        message: String(error?.message ?? error),
      };
    }
  })()`);
  const consentGenerationStatus = await second.invoke("server_generation_status");
  const realThirdParty = thirdPartyRoot
    ? await second.invoke("assets_inspect_path", { path: thirdPartyRoot }).catch((error) => ({
        valid: false,
        error: sanitizeText(error?.message ?? error),
      }))
    : null;
  const launcherAfter = {
    defaultRoot: await treeSummary(launchers.defaultRoot),
    instanceA: await treeSummary(launchers.instanceA),
    instanceB: await treeSummary(launchers.instanceB),
  };
  const corruptedDependenciesAfter = await dependencyDirectoryState();
  const secondClose = await second.close();
  secondSession = null;
  const secondConsoleErrors = secondClose.consoleErrors;

  const secondStatus = await (async () => {
    // The app is closed; derive the expected cache state from the files on disk.
    return {
      javaFiles: corruptedDependenciesAfter.javaFiles,
      serverFiles: corruptedDependenciesAfter.serverFiles,
      runFiles: corruptedDependenciesAfter.runFiles,
    };
  })();

  const changedLauncherFiles = [
    ...treeChangedFiles(launcherBefore.defaultRoot, launcherAfter.defaultRoot),
    ...treeChangedFiles(launcherBefore.instanceA, launcherAfter.instanceA),
    ...treeChangedFiles(launcherBefore.instanceB, launcherAfter.instanceB),
  ];
  const cleanDownloads = [
    ...cleanDependenciesAfter.javaFiles,
    ...cleanDependenciesAfter.serverFiles,
    ...cleanDependenciesAfter.runFiles,
  ];
  const offlineDownloads = [
    ...offlineDependenciesAfter.javaFiles,
    ...offlineDependenciesAfter.serverFiles,
    ...offlineDependenciesAfter.runFiles,
  ];
  const corruptedDownloads = [
    ...corruptedDependenciesAfter.javaFiles,
    ...corruptedDependenciesAfter.serverFiles,
    ...corruptedDependenciesAfter.runFiles,
  ];

  const checks = {
    cleanDiscoveryFoundNoJava: cleanRuntime.candidates.length === 0,
    cleanDiscoveryFoundNoMinecraft:
      cleanAssets.defaultRootPresent === false && cleanAssets.installations.length === 0,
    cleanManagedCachesEmpty:
      cleanJavaCache.runtimes.length === 0 &&
      cleanServerCache.servers.length === 0 &&
      cleanDependenciesBefore.javaFiles.length === 0 &&
      cleanDependenciesBefore.serverFiles.length === 0,
    cleanVanillaSetupBlocked:
      cleanSetup.ready === false &&
      cleanSetup.javaReady === false &&
      cleanSetup.serverReady === false &&
      cleanSetup.eulaMatchesSelected === false &&
      cleanSetup.blockers.length >= 2,
    settingsSaysJavaOptionalForEmptyProject:
      settingsState.optionalForEmptyProject === true &&
      settingsState.readiness === "setup_required",
    emptyProjectCreatedWithoutJava:
      created.ok === true &&
      emptyManifest.creationMode === "empty" &&
      emptyManifest.targetMinecraftVersion === targetVersion &&
      emptyManifest.targetDataVersion === targetDataVersion &&
      emptyWorldInfo.revision >= 1,
    emptyProjectSavedAndReopened:
      saved.ok === true &&
      reopened.ok === true &&
      reopenedManifest.creationMode === "empty" &&
      reopenedWorldInfo.revision >= 1 &&
      reopenedWorldInfo.revision === saved.lastSave?.project?.summary?.worldRevision,
    vanillaWizardOffersButDoesNotAutoDownload:
      wizardState.setupIncomplete === true &&
      wizardState.noCompatibleJava === true &&
      wizardState.javaNotSelected === true &&
      wizardState.serverNotSelected === true &&
      wizardState.eulaRequired === true &&
      wizardState.consentPresent === true &&
      wizardState.consentChecked === false &&
      wizardState.downloadServerOffered === true &&
      wizardState.chooseJavaOffered === true &&
      wizardState.chooseServerOffered === true &&
      wizardState.generateDisabled === true &&
      cleanDependenciesAfter.javaFiles.length === 0 &&
      cleanDependenciesAfter.serverFiles.length === 0,
    cleanBackendRefusedWithoutJava:
      cleanGenerationAttempt.rejected === true &&
      cleanGenerationAttempt.message.includes("Select a compatible Java 21 runtime") &&
      generationStatus.active === null,
    noServerProcessOrRunDirectory:
      cleanDependenciesAfter.runsRootKind === "missing" &&
      !hasDownloadArtifacts(cleanDependenciesAfter.serverFiles),
    offlineMetadataFailed:
      offlineRequests.every((request) => request.ok === false),
    offlineLeftNoDownloadsOrStaging:
      offlineDownloads.length === 0 && !hasDownloadArtifacts(offlineDownloads),
    launcherDiscoveryFoundDefaultRoot:
      launcherAssets.defaultRootPresent === true &&
      launcherAssets.installations.length === 3,
    launcherDiscoveryFoundProfileRoots:
      launcherAssets.installations.filter(
        (installation) => installation.source === "launcher_profile",
      ).length === 2,
    launcherDiscoveryVerifiedTarget:
      launcherAssets.installations.every((installation) =>
        installation.compatibleVersions.includes(targetVersion),
      ) &&
      launcherAssets.installations.every((installation) =>
        installation.versions.some(
          (version) => version.id === targetVersion && version.status === "compatible",
        ),
      ),
    launcherRootsWereReadOnly: changedLauncherFiles.length === 0,
    corruptManagedJavaRejected:
      brokenRuntime.candidates.some(
        (candidate) => candidate.managed === true && candidate.status === "unreadable",
      ) &&
      compatibleCount(brokenRuntime.candidates) === 0 &&
      brokenRuntime.managed.length === 1 &&
      brokenRuntime.problems.some((problem) => problem.includes("no Java 21 runtime")),
    corruptManagedServerDetected:
      brokenServerCache.servers.length === 0 &&
      brokenServerCache.problems.some((problem) => problem.includes("changed on disk")),
    staleJavaSelectionReported:
      brokenRuntime.saved?.error !== null &&
      brokenRuntime.saved?.error !== undefined &&
      brokenRuntime.saved.status === "unreadable",
    staleServerSelectionReported:
      brokenServers.selected?.ready === false &&
      typeof brokenServers.selected?.error === "string" &&
      brokenServers.selected.error.length > 0,
    corruptedVanillaSetupStillBlocked:
      brokenSetup.ready === false &&
      brokenSetup.javaReady === false &&
      brokenSetup.serverReady === false,
    consentGateRejectedBeforeServerJar:
      javaSelection.status === "compatible" &&
      consentFalseAttempt.rejected === true &&
      consentFalseAttempt.message.includes("explicit generation consent is required") &&
      consentGenerationStatus.active === null,
    consentAloneDidNotStartMissingServer:
      consentTrueAttempt.rejected === true &&
      consentTrueAttempt.message.length > 0 &&
      consentGenerationStatus.active === null,
    corruptedCacheLeftNoDownloadsOrState:
      !hasDownloadArtifacts(corruptedDownloads) &&
      secondStatus.runFiles.length === 0,
    noConsoleErrors:
      firstConsoleErrors.length === 0 && secondConsoleErrors.length === 0,
  };
  const failedChecks = Object.entries(checks)
    .filter(([, value]) => value !== true)
    .map(([name]) => name);
  if (failedChecks.length > 0) {
    await writeFile(
      join(output, "failed-checks.json"),
      `${JSON.stringify(
        {
          failedChecks,
          runtime: {
            candidates: brokenRuntime.candidates,
            managed: brokenRuntime.managed,
            problems: brokenRuntime.problems.map(sanitizeText),
            saved: brokenRuntime.saved,
          },
          server: {
            selected: brokenServers.selected,
            managed: brokenServerCache.servers,
            problems: brokenServerCache.problems.map(sanitizeText),
          },
          setup: brokenSetup,
          checks,
        },
        null,
        2,
      )}\n`,
      "utf8",
    );
    throw new Error(`failed checks: ${failedChecks.join(", ")}`);
  }

  const evidence = {
    schemaVersion: 1,
    milestone: "M11",
    task: "clean-user-setup",
    recordedAt: new Date().toISOString(),
    pass: true,
    scope:
      "Isolated clean per-user environment on an existing Windows 11 machine, not a fresh OS install and not different launcher brands.",
    target: {
      minecraftVersion: targetVersion,
      dataVersion: targetDataVersion,
      javaMajor: 21,
    },
    environment: {
      platform: "Windows",
      appVersion: appInfo?.version ?? null,
      executable: "target/debug/furybuilder-desktop.exe",
      executableSha256,
      isolatedAppData: true,
      isolatedWebView2Profiles: 2,
      sanitizedPathVariable: true,
      javaHomeRemoved: true,
      launcherRootsInCleanSession: 0,
      networkReachable: network.reachable,
      networkProbeError: network.error ?? null,
      offlineMode: "closed loopback proxy (http://127.0.0.1:9) for managed dependency requests",
    },
    cleanStart: {
      assets: {
        defaultRootPresent: cleanAssets.defaultRootPresent,
        installations: cleanAssets.installations.length,
        problemCount: cleanAssets.problems.length,
        problemsMentionMissingLauncher: cleanAssets.problems.some((problem) =>
          problem.includes("no Minecraft Java launcher folder"),
        ),
      },
      java: {
        candidates: cleanRuntime.candidates.length,
        compatible: compatibleCount(cleanRuntime.candidates),
        managed: cleanRuntime.managed.length,
        managedBytes: cleanRuntime.managedBytes,
        problemCount: cleanRuntime.problems.length,
        problemsMentionNoJava: cleanRuntime.problems.some((problem) =>
          problem.includes("no Java 21 runtime"),
        ),
      },
      server: {
        selected: cleanServers.selected,
        managed: cleanServers.managed.length,
        problemCount: cleanServers.problems.length,
      },
      vanillaSetup: {
        ready: cleanSetup.ready,
        javaReady: cleanSetup.javaReady,
        serverReady: cleanSetup.serverReady,
        eulaMatchesSelected: cleanSetup.eulaMatchesSelected,
        blockerCount: cleanSetup.blockers.length,
      },
      managedCaches: {
        javaEntries: cleanJavaCache.runtimes.length,
        serverEntries: cleanServerCache.servers.length,
        javaFiles: cleanDependenciesBefore.javaFiles,
        serverFiles: cleanDependenciesBefore.serverFiles,
        runFiles: cleanDependenciesBefore.runFiles,
      },
    },
    settings: {
      ...settingsState,
      readinessText: settingsState.readinessText
        ? sanitizeText(settingsState.readinessText)
        : null,
    },
    emptyProject: {
      created: created.ok,
      folder: projectFolder,
      creationMode: emptyManifest.creationMode,
      targetMinecraftVersion: emptyManifest.targetMinecraftVersion,
      targetDataVersion: emptyManifest.targetDataVersion,
      worldRevision: reopenedWorldInfo.revision,
      chunks: reopenedWorldInfo.chunkCount,
      sections: reopenedWorldInfo.sectionCount,
      blocks: reopenedWorldInfo.blockCount,
      saved: saved.ok,
      reopened: reopened.ok,
      manifestAfterReopen: {
        creationMode: reopenedManifest.creationMode,
        worldRevision: reopenedManifest.worldRevision,
        lastSaveKind: reopenedManifest.lastSaveKind,
      },
    },
    vanillaConsent: {
      wizard: wizardState,
      cleanBackend: {
        rejected: cleanGenerationAttempt.rejected,
        code: cleanGenerationAttempt.code,
        message: sanitizeText(cleanGenerationAttempt.message),
        activeRun: generationStatus.active,
      },
      dependenciesAfterWizard: {
        javaFiles: cleanDependenciesAfter.javaFiles,
        serverFiles: cleanDependenciesAfter.serverFiles,
        runFiles: cleanDependenciesAfter.runFiles,
      },
    },
    offline: {
      networkReachable: network.reachable,
      mode: "closed loopback proxy",
      metadataRequests: offlineRequests.map((request) => ({
        ok: request.ok,
        message: request.ok ? null : sanitizeText(request.message),
      })),
      dependencyFilesAfter: offlineDownloads,
    },
    launcherLayouts: {
      synthetic: {
        defaultRoot: true,
        profileGameDirs: 2,
        installations: launcherAssets.installations.length,
        sources: launcherAssets.installations.map((installation) => installation.source),
        compatibleVersions: launcherAssets.installations.map(
          (installation) => installation.compatibleVersions,
        ),
        targetVersionsVerified: launcherAssets.installations.map((installation) =>
          installation.versions
            .filter((version) => version.id === targetVersion)
            .map((version) => version.status),
        ),
        launcherRootsUnchanged: changedLauncherFiles.length === 0,
        changedFiles: changedLauncherFiles,
      },
      realThirdPartyRoot: realThirdParty
        ? {
            present: true,
            valid: realThirdParty.valid ?? false,
            targetMatch: realThirdParty.targetMatch ?? false,
            kind: realThirdParty.kind ?? null,
            versionCount: realThirdParty.installation?.versions?.length ?? 0,
            error: realThirdParty.error ? sanitizeText(realThirdParty.error) : null,
          }
        : null,
    },
    corruptedCache: {
      managedJava: {
        entries: brokenRuntime.managed.length,
        candidateCount: brokenRuntime.candidates.length,
        statuses: countByStatus(brokenRuntime.candidates),
        compatible: compatibleCount(brokenRuntime.candidates),
        problemCount: brokenRuntime.problems.length,
      },
      managedServer: {
        entries: brokenServerCache.servers.length,
        problemCount: brokenServerCache.problems.length,
        problemsMentionChangedOnDisk: brokenServerCache.problems.some((problem) =>
          problem.includes("changed on disk"),
        ),
      },
      staleJavaSelection: {
        savedStatus: brokenRuntime.saved?.status ?? null,
        savedError: brokenRuntime.saved?.error ? sanitizeText(brokenRuntime.saved.error) : null,
      },
      staleServerSelection: {
        ready: brokenServers.selected?.ready ?? null,
        error: brokenServers.selected?.error
          ? sanitizeText(brokenServers.selected.error)
          : null,
      },
      vanillaSetupReady: brokenSetup.ready,
      consentGate: {
        selectedJavaStatus: javaSelection.status,
        withoutConsent: {
          rejected: consentFalseAttempt.rejected,
          code: consentFalseAttempt.code,
          message: sanitizeText(consentFalseAttempt.message),
        },
        withConsentButMissingServer: {
          rejected: consentTrueAttempt.rejected,
          code: consentTrueAttempt.code,
          message: sanitizeText(consentTrueAttempt.message),
        },
        activeRun: consentGenerationStatus.active,
      },
      dependencyFiles: {
        java: corruptedDependenciesAfter.javaFiles,
        server: corruptedDependenciesAfter.serverFiles,
        runs: corruptedDependenciesAfter.runFiles,
      },
      seededCacheKinds: [
        "managed Java runtime with a non-executable bin/java.exe",
        "managed server JAR whose size does not match its manifest",
        "stale runtime.json selection pointing to a missing Java path",
        "stale server.json selection pointing to a missing server JAR",
      ],
    },
    checks,
    consoleErrors: [...firstConsoleErrors, ...secondConsoleErrors].map((entry) => ({
      source: entry.source,
      text: sanitizeText(entry.text),
    })),
    limitations: [
      "This is not a fresh Windows installation. It is an isolated per-user environment on an existing Windows 11 machine; a clean VM without Minecraft or Java is still required for the full M11.7 claim.",
      "Launcher coverage is the conventional `.minecraft` root plus `launcher_profiles*.json` game directories with synthetic client JAR metadata. Different launcher brands are not emulated; a real third-party root is only inspected through manual selection when provided.",
      "The synthetic client JARs contain only version metadata; they prove discovery and validation, not rendering or Minecraft client compatibility.",
      "Offline is simulated by routing managed-dependency requests through a closed loopback proxy on a machine that may still have network access. It proves the failure/no-download path, not a physically disconnected network.",
      "Corrupted caches are seeded test directories under isolated app data. They prove rejection of a broken managed runtime, a size-mismatched managed server JAR and stale selected paths; they are not the user's real cache.",
      "No external beta user performed this setup; KL-43 stays open.",
      "The synthetic launcher files and corrupted cache seeds stay in the harness output and are not committed.",
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
        checks,
        cleanStart: {
          javaCandidates: cleanRuntime.candidates.length,
          assetInstallations: cleanAssets.installations.length,
          setupBlockers: cleanSetup.blockers.length,
        },
        launcherInstallations: launcherAssets.installations.length,
        corruptedJavaStatuses: countByStatus(brokenRuntime.candidates),
      },
      null,
      2,
    ),
  );
} catch (error) {
  console.error(
    `CLEAN_SETUP_FAILED: ${
      error instanceof Error ? `${error.message}\n${error.stack}` : String(error)
    }`,
  );
  process.exitCode = 1;
} finally {
  if (firstSession) {
    await firstSession.close().catch(() => {});
  }
  if (secondSession) {
    await secondSession.close().catch(() => {});
  }
  await delay(1_000);
  await rm(sandbox, { recursive: true, force: true, maxRetries: 3 }).catch(() => {});
}
