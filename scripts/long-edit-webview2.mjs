/**
 * M10.8 Windows long-editing harness for the real desktop app.
 *
 * This starts its own Tauri process with an isolated app-data and WebView2 profile,
 * creates a flat project through the real frontend store, applies thousands of edits
 * through the real `applyEditorOperation` path (preview -> Rust apply -> dirty chunk
 * refresh), verifies the rendered block state and periodically probes undo/redo.
 *
 * It records frame metrics, JS heap, WebGL context state, Rust revisions, console
 * errors, project save/reopen equality and a final viewport screenshot.
 *
 * The machine used for the recorded evidence has a single physical GPU. The
 * `hardware-d3d11`, `software-warp` and `software-swiftshader` backends exercise
 * distinct Windows graphics paths; they do not claim integrated/discrete or
 * cross-vendor hardware coverage.
 *
 * Usage:
 *   node scripts/long-edit-webview2.mjs <new-output-directory> <backend> [duration-ms] [interval-ms]
 *   node scripts/long-edit-webview2.mjs <new-output-directory> <backend> --probe
 *
 * Backends:
 *   hardware-d3d11       default ANGLE D3D11 on the NVIDIA adapter
 *   software-warp        ANGLE OpenGL request, observed as Microsoft WARP fallback
 *   software-swiftshader ANGLE SwiftShader software rasterizer
 *
 * Prerequisites:
 *   - `pnpm --filter @furybuilder/desktop dev` at http://127.0.0.1:1420
 *   - a debug `target/debug/furybuilder-desktop.exe`
 */
import { execFileSync, spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { cpus, release, tmpdir, totalmem } from "node:os";
import { dirname, join, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const appPath = join(root, "target", "debug", "furybuilder-desktop.exe");
const origin = "http://127.0.0.1:1420";
const dimension = "minecraft:overworld";

const BACKENDS = {
  "hardware-d3d11": {
    label: "hardware / ANGLE D3D11",
    kind: "hardware",
    args: [],
    expectation:
      "The default WebView2 hardware path on the installed NVIDIA adapter.",
  },
  "software-warp": {
    label: "software / Microsoft WARP",
    kind: "software",
    args: ["--use-angle=gl"],
    expectation:
      "This WebView2 build falls back to Microsoft Basic Render Driver (WARP) when the ANGLE OpenGL backend is requested. The recorded renderer string is authoritative.",
  },
  "software-swiftshader": {
    label: "software / ANGLE SwiftShader",
    kind: "software",
    args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader"],
    expectation:
      "A non-hardware Windows rasterizer path used as the software adapter type.",
  },
};

const commonBrowserArguments = [
  "--remote-debugging-port=0",
  "--disable-background-timer-throttling",
  "--disable-renderer-backgrounding",
  "--disable-backgrounding-occluded-windows",
  "--disable-features=CalculateNativeWinOcclusion",
  "--enable-precise-memory-info",
];

const STONE = { name: "minecraft:stone", properties: {} };
const AIR = { name: "minecraft:air", properties: {} };
const MATERIALS = [
  STONE,
  { name: "minecraft:oak_planks", properties: {} },
  { name: "minecraft:glass", properties: {} },
  { name: "minecraft:red_wool", properties: {} },
  { name: "minecraft:gold_block", properties: {} },
  { name: "minecraft:dirt", properties: {} },
];

const output = process.argv[2] ? resolve(process.argv[2]) : null;
const backendId = process.argv[3] ?? "hardware-d3d11";
const probeOnly = process.argv.includes("--probe");
const durationMs = probeOnly
  ? 0
  : Number(process.env.FURY_EDIT_DURATION_MS ?? process.argv[4] ?? 600_000);
const intervalMs = Number(
  process.env.FURY_EDIT_INTERVAL_MS ?? process.argv[5] ?? 250,
);
const sampleEveryMs = 10_000;
const verifyTimeoutMs = 8_000;
const probeEveryMs = 60_000;

if (process.platform !== "win32") {
  throw new Error("This harness is Windows-only.");
}
if (!output) {
  throw new Error(
    "Usage: node scripts/long-edit-webview2.mjs <new-output-directory> <backend> [duration-ms] [interval-ms] [--probe]",
  );
}
const backend = BACKENDS[backendId];
if (!backend) {
  throw new Error(
    `Unknown backend "${backendId}". Use: ${Object.keys(BACKENDS).join(", ")}`,
  );
}
if (!probeOnly && (!Number.isFinite(durationMs) || durationMs < 10_000)) {
  throw new Error("duration-ms must be at least 10000");
}
if (!Number.isFinite(intervalMs) || intervalMs < 50) {
  throw new Error("interval-ms must be at least 50");
}

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

function finite(values) {
  return values.filter(
    (value) => typeof value === "number" && Number.isFinite(value),
  );
}

function percentile(sorted, fraction) {
  if (sorted.length === 0) return null;
  const index = Math.min(
    sorted.length - 1,
    Math.floor((sorted.length - 1) * fraction),
  );
  return sorted[index];
}

function collectSystemInfo() {
  const script = `
$ErrorActionPreference = 'SilentlyContinue'
$ProgressPreference = 'SilentlyContinue'
$adapters = @(Get-CimInstance Win32_VideoController | Select-Object Name, DriverVersion, AdapterRAM, PNPDeviceID)
$cpu = @(Get-CimInstance Win32_Processor | Select-Object -First 1 -Property Name, NumberOfCores, NumberOfLogicalProcessors)
$os = @(Get-CimInstance Win32_OperatingSystem | Select-Object -First 1 -Property Caption, Version, BuildNumber)
$webview2 = @()
Get-ChildItem 'HKLM:\\SOFTWARE\\WOW6432Node\\Microsoft\\EdgeUpdate\\Clients' | ForEach-Object {
  $item = Get-ItemProperty $_.PSPath
  if ($item.name -like '*WebView2*') {
    $webview2 += [pscustomobject]@{ name = $item.name; version = $item.pv }
  }
}
[pscustomobject]@{ adapters = $adapters; cpu = $cpu; os = $os; webview2 = $webview2 } | ConvertTo-Json -Depth 6 -Compress
`;
  try {
    const text = execFileSync(
      "powershell.exe",
      ["-NoProfile", "-NonInteractive", "-Command", script],
      {
        encoding: "utf8",
        windowsHide: true,
        timeout: 30_000,
      },
    ).trim();
    return JSON.parse(text);
  } catch {
    return null;
  }
}

function physicalGpuLimitation(system) {
  const raw = system?.adapters;
  const adapters = Array.isArray(raw) ? raw : raw ? [raw] : [];
  const physical = adapters.filter(
    (adapter) =>
      !/basic|remote|virtual|swiftshader|warp|render driver/i.test(
        adapter?.Name ?? "",
      ),
  );
  if (physical.length > 1) return null;
  const names =
    physical
      .map((adapter) => adapter?.Name)
      .filter(Boolean)
      .join(", ") || "none";
  return `Only ${physical.length} physical display adapter(s) detected (${names}). No integrated or second discrete GPU is available in this environment. This run covers the "${backend.label}" path only; cross-vendor and integrated/discrete hardware coverage is not claimed.`;
}

function box(x, y, z, sizeX, sizeY, sizeZ) {
  const x0 = x - Math.floor((sizeX - 1) / 2);
  const z0 = z - Math.floor((sizeZ - 1) / 2);
  return {
    min: { x: x0, y, z: z0 },
    max: { x: x0 + sizeX - 1, y: y + sizeY - 1, z: z0 + sizeZ - 1 },
  };
}

function gridPosition(cycle, spread) {
  const span = spread * 2 + 1;
  return {
    x: ((cycle * 7) % span) - spread,
    z: ((cycle * 11) % span) - spread,
  };
}

function operationFor(cycle) {
  switch (cycle % 8) {
    case 0: {
      const position = gridPosition(cycle, 40);
      const state = MATERIALS[cycle % MATERIALS.length];
      return {
        operation: {
          type: "fill_cuboid",
          dimension,
          bounds: box(position.x, 65, position.z, 6, 3, 6),
          state,
        },
        sample: {
          pos: { x: position.x, y: 66, z: position.z },
          expected: state,
        },
      };
    }
    case 1: {
      const position = gridPosition(cycle, 40);
      const state = MATERIALS[(cycle + 1) % MATERIALS.length];
      const changes = [];
      for (let dx = -8; dx <= 7; dx += 1) {
        for (let dz = -8; dz <= 7; dz += 1) {
          const useAir = (dx + dz + cycle) % 3 === 0;
          changes.push({
            pos: { x: position.x + dx, y: 66, z: position.z + dz },
            state: useAir ? AIR : state,
          });
        }
      }
      const sampleChange =
        changes.find((change) => change.state.name !== AIR.name) ?? changes[0];
      return {
        operation: { type: "batch", dimension, changes },
        sample: { pos: sampleChange.pos, expected: sampleChange.state },
      };
    }
    case 2: {
      const position = gridPosition(cycle, 10);
      return {
        operation: {
          type: "terrain_brush",
          dimension,
          bounds: box(position.x, 62, position.z, 10, 6, 10),
          kind: cycle % 16 < 8 ? "raise" : "lower",
          material: STONE,
          amount: 1,
          seed: 1000 + cycle,
          step: 2,
          targetY: null,
        },
        sample: null,
      };
    }
    case 3: {
      const position = gridPosition(cycle - 3, 40);
      return {
        operation: {
          type: "fill_cuboid",
          dimension,
          bounds: box(position.x, 65, position.z, 6, 3, 6),
          state: AIR,
        },
        sample: { pos: { x: position.x, y: 66, z: position.z }, expected: AIR },
      };
    }
    case 4: {
      const position = gridPosition(cycle, 40);
      const state = MATERIALS[(cycle + 2) % MATERIALS.length];
      return {
        operation: {
          type: "fill_cuboid",
          dimension,
          bounds: box(position.x, 64, position.z, 8, 1, 8),
          state,
        },
        sample: {
          pos: { x: position.x, y: 64, z: position.z },
          expected: state,
        },
      };
    }
    case 5: {
      const position = gridPosition(cycle, 40);
      const state = MATERIALS[(cycle + 3) % MATERIALS.length];
      return {
        operation: {
          type: "fill_cuboid",
          dimension,
          bounds: box(position.x, 67, position.z, 4, 2, 4),
          state,
        },
        sample: {
          pos: { x: position.x, y: 67, z: position.z },
          expected: state,
        },
      };
    }
    case 6: {
      const position = gridPosition(cycle, 24);
      return {
        operation: {
          type: "replace",
          dimension,
          bounds: box(position.x, 62, position.z, 16, 6, 16),
          from: STONE,
          to:
            cycle % 16 === 6
              ? { name: "minecraft:cobblestone", properties: {} }
              : { name: "minecraft:stone_bricks", properties: {} },
        },
        sample: null,
      };
    }
    default: {
      const position = gridPosition(cycle, 40);
      const state = MATERIALS[(cycle + 4) % MATERIALS.length];
      return {
        operation: {
          type: "fill_cuboid",
          dimension,
          bounds: box(position.x, 65, position.z, 10, 2, 10),
          state,
        },
        sample: {
          pos: { x: position.x, y: 65, z: position.z },
          expected: state,
        },
      };
    }
  }
}

const bootstrapExpression = `(async () => {
  const [project, world, tool, commands] = await Promise.all([
    import("/src/state/project-store.ts"),
    import("/src/state/world-store.ts"),
    import("/src/state/tool-store.ts"),
    import("/src/editor/commands.ts"),
  ]);
  const observers = { longTasks: [], errors: [], contextLosses: [] };
  try {
    new PerformanceObserver((list) => {
      for (const entry of list.getEntries()) {
        observers.longTasks.push({ startTime: entry.startTime, duration: entry.duration });
      }
    }).observe({ type: "longtask", buffered: true });
  } catch {
    // Long-task entries are not available in every WebView2 build.
  }
  window.addEventListener("error", (event) => {
    observers.errors.push({ kind: "error", message: String(event.message ?? "") });
  });
  window.addEventListener("unhandledrejection", (event) => {
    const reason = event.reason;
    observers.errors.push({
      kind: "rejection",
      message: String(reason?.message ?? reason ?? ""),
    });
  });
  const renderer = window.__furyViewport;
  renderer?.renderer?.domElement?.addEventListener("webglcontextlost", () => {
    observers.contextLosses.push(performance.now());
  });
  window.__furyLongEdit = {
    project,
    world,
    tool,
    commands,
    observers,
    renderer,
    startedAt: performance.now(),
    startedWallMs: Date.now(),
  };
  return {
    tauri: Boolean(window.__TAURI_INTERNALS__),
    viewport: Boolean(window.__furyViewport),
    webgl2: Boolean(renderer?.renderer?.capabilities?.isWebGL2),
    status: world.useWorldStore.getState().status,
  };
})()`;

const glInfoExpression = `(() => {
  const renderer = window.__furyViewport;
  if (!renderer) return null;
  const gl = renderer.renderer.getContext();
  const debug = gl.getExtension("WEBGL_debug_renderer_info");
  const timer = gl.getExtension("EXT_disjoint_timer_query_webgl2");
  return {
    vendor: debug ? gl.getParameter(debug.UNMASKED_VENDOR_WEBGL) : gl.getParameter(gl.VENDOR),
    renderer: debug ? gl.getParameter(debug.UNMASKED_RENDERER_WEBGL) : gl.getParameter(gl.RENDERER),
    webglVersion: gl.getParameter(gl.VERSION),
    shadingLanguage: gl.getParameter(gl.SHADING_LANGUAGE_VERSION),
    timerAvailable: Boolean(timer),
    contextLost: gl.isContextLost(),
    attributes: gl.getContextAttributes(),
    error: gl.getError(),
  };
})()`;

const sampleExpression = `(() => {
  const world = window.__furyLongEdit.world.useWorldStore.getState();
  const renderer = window.__furyViewport;
  const gl = renderer?.renderer?.getContext?.() ?? null;
  const info = renderer?.renderer?.info ?? null;
  const memory =
    typeof performance.memory !== "undefined"
      ? {
          usedBytes: performance.memory.usedJSHeapSize,
          totalBytes: performance.memory.totalJSHeapSize,
          limitBytes: performance.memory.jsHeapSizeLimit,
        }
      : null;
  const observers = window.__furyLongEdit.observers;
  const longTasks = observers.longTasks;
  return {
    atMs: performance.now() - window.__furyLongEdit.startedAt,
    wallElapsedMs: Date.now() - window.__furyLongEdit.startedWallMs,
    visibility: document.visibilityState,
    world: {
      revision: world.revision,
      blockCount: world.blockCount,
      sectionCount: world.sectionCount,
      chunkCount: world.chunkCount,
      status: world.status,
      error: world.error,
    },
    renderer: {
      present: Boolean(renderer),
      sameInstance: renderer === window.__furyLongEdit.renderer,
      revision: renderer?.world?.revision ?? null,
      loadedChunks: renderer?.world?.loadedChunkCount?.() ?? null,
      loadedSections: renderer?.world?.sectionCount?.() ?? null,
      contextLost: gl ? gl.isContextLost() : null,
      calls: info?.render?.calls ?? null,
      triangles: info?.render?.triangles ?? null,
      geometries: info?.memory?.geometries ?? null,
      textures: info?.memory?.textures ?? null,
      metrics: world.metrics ?? null,
    },
    memory,
    observers: {
      longTaskCount: longTasks.length,
      longTaskMaxMs: longTasks.reduce((max, entry) => Math.max(max, entry.duration), 0),
      longTaskTotalMs: longTasks.reduce((sum, entry) => sum + entry.duration, 0),
      errorCount: observers.errors.length,
      contextLossCount: observers.contextLosses.length,
    },
  };
})()`;

function applyExpression(operation) {
  return `(async () => {
    const started = performance.now();
    try {
      const result = await window.__furyLongEdit.commands.applyEditorOperation(
        ${JSON.stringify(operation)},
        { silentSuccess: true },
      );
      const world = window.__furyLongEdit.world.useWorldStore.getState();
      const renderer = window.__furyViewport;
      const gl = renderer?.renderer?.getContext?.() ?? null;
      const toasts = result
        ? []
        : window.__furyLongEdit.tool.useToastStore
          ? window.__furyLongEdit.tool.useToastStore
              .getState()
              .toasts.filter((toast) => toast.kind === "error")
              .slice(-4)
          : [];
      return {
        ok: Boolean(result),
        result,
        elapsedMs: performance.now() - started,
        world: {
          revision: world.revision,
          blockCount: world.blockCount,
          sectionCount: world.sectionCount,
          chunkCount: world.chunkCount,
          status: world.status,
          error: world.error,
        },
        renderer: {
          present: Boolean(renderer),
          revision: renderer?.world?.revision ?? null,
          loadedChunks: renderer?.world?.loadedChunkCount?.() ?? null,
          contextLost: gl ? gl.isContextLost() : null,
        },
        toasts,
      };
    } catch (error) {
      return {
        ok: false,
        elapsedMs: performance.now() - started,
        error: {
          name: error?.name ?? "Error",
          message: String(error?.message ?? error),
          stack: String(error?.stack ?? ""),
        },
      };
    }
  })()`;
}

function verifyExpression(sample, expectedRevision) {
  return `(async () => {
    const started = performance.now();
    const targetRevision = ${JSON.stringify(expectedRevision)};
    const expected = ${JSON.stringify(sample?.expected ?? null)};
    const position = ${JSON.stringify(sample?.pos ?? null)};
    let lastBlock = null;
    let lastRevision = null;
    let lastContextLost = null;
    while (performance.now() - started < ${verifyTimeoutMs}) {
      const renderer = window.__furyViewport;
      if (!renderer) {
        return {
          ok: false,
          reason: "viewport disposed",
          waitedMs: performance.now() - started,
          lastBlock,
          lastRevision,
          lastContextLost,
        };
      }
      const gl = renderer.renderer.getContext();
      lastContextLost = gl.isContextLost();
      if (lastContextLost) {
        return {
          ok: false,
          reason: "webgl context lost",
          waitedMs: performance.now() - started,
          lastBlock,
          lastRevision,
          lastContextLost,
        };
      }
      lastRevision = renderer.world.revision;
      lastBlock = expected && position ? renderer.world.blockAt(position) : null;
      const blockOk =
        !expected ||
        (expected.name === "minecraft:air"
          ? lastBlock === null || lastBlock.name === "minecraft:air"
          : Boolean(lastBlock && lastBlock.name === expected.name));
      if (lastRevision >= targetRevision && blockOk) {
        return {
          ok: true,
          waitedMs: performance.now() - started,
          lastBlock,
          lastRevision,
          lastContextLost,
        };
      }
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
    return {
      ok: false,
      reason: "renderer did not converge",
      waitedMs: performance.now() - started,
      lastBlock,
      lastRevision,
      lastContextLost,
    };
  })()`;
}

const finalizeExpression = `(async () => {
  const store = window.__furyLongEdit.project.useProjectStore;
  const folder = store.getState().current?.folder ?? null;
  const before = await window.__TAURI_INTERNALS__.invoke("world_info");
  const saveStarted = performance.now();
  const saved = await store.getState().save();
  const saveMs = performance.now() - saveStarted;
  const lastSave = store.getState().lastSave;
  const saveError = store.getState().error;
  const closeStarted = performance.now();
  const closed = await store.getState().close(false);
  const closeMs = performance.now() - closeStarted;
  let opened = false;
  let openMs = null;
  let openError = null;
  if (closed && folder) {
    const openStarted = performance.now();
    opened = await store.getState().open(folder);
    openMs = performance.now() - openStarted;
    openError = store.getState().error;
  }
  const after = await window.__TAURI_INTERNALS__.invoke("world_info");
  let probe = null;
  let probeError = null;
  try {
    probe = await window.__TAURI_INTERNALS__.invoke("get_block", {
      dimension: "minecraft:overworld",
      pos: { x: 0, y: 72, z: 0 },
    });
  } catch (error) {
    probeError = String(error?.message ?? error);
  }
  return {
    folder,
    before,
    saved,
    saveMs,
    lastSave,
    saveError,
    closed,
    closeMs,
    opened,
    openMs,
    openError,
    after,
    probe,
    probeError,
    current: store.getState().current,
    recovery: store.getState().recovery,
  };
})()`;

await mkdir(output, { recursive: false });
const startedAt = Date.now();
const system = collectSystemInfo();
const executableSha256 = sha256(await readFile(appPath));
const physicalLimitation = physicalGpuLimitation(system);
const browserArguments = [...commonBrowserArguments, ...backend.args];
const run = {
  kind: "long-edit-webview2",
  schemaVersion: 1,
  milestone: "M10.8",
  startedAt: new Date(startedAt).toISOString(),
  durationMs: null,
  schedule: {
    requestedDurationMs: durationMs,
    intervalMs,
    verifyTimeoutMs,
    probeEveryMs,
  },
  backend: {
    id: backendId,
    label: backend.label,
    kind: backend.kind,
    expectation: backend.expectation,
    browserArguments: backend.args,
    commonBrowserArguments,
  },
  environment: {
    platform: process.platform,
    osRelease: release(),
    node: process.version,
    cpu: cpus()[0]?.model ?? null,
    logicalCpus: cpus().length,
    totalMemoryBytes: totalmem(),
    system,
    physicalGpuLimitation: physicalLimitation,
  },
  app: {
    executable: appPath,
    sha256: executableSha256,
    webgl: null,
    initialInfo: null,
    initialHeap: null,
    finalHeap: null,
  },
  project: null,
  operations: {
    attempted: 0,
    succeeded: 0,
    failed: 0,
    zeroChange: 0,
    changedBlocks: 0,
    byKind: {},
    undoRedoProbes: [],
    verificationFailures: [],
  },
  revisionMismatches: 0,
  contextLossCount: 0,
  rendererStable: true,
  samples: [],
  console: { errors: [], warnings: [], exceptionCount: 0 },
  saveReopen: null,
  capture: null,
  failure: null,
  pass: false,
  summary: null,
};

let app = null;
let ws = null;
let appData = null;
let profile = null;
let appStderr = "";

try {
  const vite = await fetch(`${origin}/src/features/viewport/Viewport.tsx`, {
    signal: AbortSignal.timeout(5_000),
  }).catch(() => null);
  if (!vite?.ok) {
    throw new Error(`Start this checkout's Vite server at ${origin}`);
  }

  appData = await mkdtemp(join(tmpdir(), `fury-long-edit-data-${backendId}-`));
  profile = await mkdtemp(
    join(tmpdir(), `fury-long-edit-webview-${backendId}-`),
  );
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

  let nextId = 0;
  const pending = new Map();
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
      run.console.exceptionCount += 1;
      const details = message.params?.exceptionDetails ?? {};
      pushConsoleError({
        source: "exception",
        text:
          details.exception?.description ??
          details.text ??
          "uncaught exception",
        url: details.url ?? null,
        line: details.lineNumber ?? null,
      });
    } else if (message.method === "Log.entryAdded") {
      const entry = message.params?.entry ?? {};
      if (entry.level === "error") {
        pushConsoleError({
          source: "log",
          text: entry.text ?? "",
          url: entry.url ?? null,
        });
      } else if (
        entry.level === "warning" &&
        run.console.warnings.length < 100
      ) {
        run.console.warnings.push({
          source: "log",
          text: entry.text ?? "",
          url: entry.url ?? null,
        });
      }
    } else if (message.method === "Runtime.consoleAPICalled") {
      const type = message.params?.type;
      const text = (message.params?.args ?? [])
        .map((arg) => arg.value ?? arg.description ?? arg.type ?? "")
        .join(" ");
      if (type === "error" || type === "assert") {
        pushConsoleError({ source: `console.${type}`, text, url: null });
      } else if (type === "warning" && run.console.warnings.length < 100) {
        run.console.warnings.push({
          source: `console.${type}`,
          text,
          url: null,
        });
      }
    }
  };

  function pushConsoleError(entry) {
    if (
      (entry.text ?? "").includes("favicon.ico") ||
      (entry.url ?? "").includes("favicon.ico")
    ) {
      if (run.console.warnings.length < 100) {
        run.console.warnings.push({ ...entry, benign: true });
      }
      return;
    }
    if (run.console.errors.length < 200) run.console.errors.push(entry);
  }

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

  async function invoke(command, args = {}) {
    return evaluate(
      `window.__TAURI_INTERNALS__.invoke(${JSON.stringify(command)}, ${JSON.stringify(args)})`,
    );
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

  async function collectGarbage() {
    try {
      await send("HeapProfiler.collectGarbage");
    } catch {
      // HeapProfiler is best-effort; the recorded heap remains a Chromium sample.
    }
    await delay(750);
  }

  async function runUndoRedoProbe(cycle) {
    const gold = { name: "minecraft:gold_block", properties: {} };
    const diamond = { name: "minecraft:diamond_block", properties: {} };
    const position = { x: 0, y: 70, z: 0 };
    // Always choose a state different from the current one so the fill is a real edit.
    // Otherwise a repeated gold probe would be a no-op and undo would target stale history.
    let previous = null;
    try {
      previous = await evaluate(
        `(() => {
          const block = window.__furyViewport?.world?.blockAt(${JSON.stringify(position)});
          return block ?? null;
        })()`,
        30_000,
      );
    } catch {
      previous = null;
    }
    const target = previous?.name === gold.name ? diamond : gold;
    const applied = await evaluate(
      applyExpression({
        type: "fill_cuboid",
        dimension,
        bounds: box(position.x, position.y, position.z, 1, 1, 1),
        state: target,
      }),
      120_000,
    );
    if (!applied.ok || !applied.result || applied.result.changedBlocks === 0) {
      run.operations.failed += 1;
      run.operations.undoRedoProbes.push({
        cycle,
        ok: false,
        stage: "apply",
        previous,
        target,
        detail: applied,
      });
      return;
    }
    const targetShown = await evaluate(
      verifyExpression(
        { pos: position, expected: target },
        applied.result.revision,
      ),
      30_000,
    );
    await evaluate("window.__furyLongEdit.commands.undoWorld()", 120_000);
    const restored = await evaluate(
      verifyExpression(
        { pos: position, expected: previous ?? AIR },
        applied.result.revision + 1,
      ),
      30_000,
    );
    await evaluate("window.__furyLongEdit.commands.redoWorld()", 120_000);
    const targetRestored = await evaluate(
      verifyExpression(
        { pos: position, expected: target },
        applied.result.revision + 2,
      ),
      30_000,
    );
    const ok = targetShown.ok && restored.ok && targetRestored.ok;
    run.operations.undoRedoProbes.push({
      cycle,
      ok,
      previous,
      target,
      targetShown,
      restored,
      targetRestored,
      revision: applied.result.revision,
    });
    if (!ok) {
      run.console.errors.push({
        source: "undo-redo",
        cycle,
        targetShown,
        restored,
        targetRestored,
      });
    }
  }

  async function takeSample(label) {
    const sample = await evaluate(sampleExpression, 60_000);
    sample.label = label;
    sample.at = new Date().toISOString();
    try {
      sample.rust = await invoke("world_info");
      if (sample.rust?.revision !== sample.world?.revision) {
        run.revisionMismatches += 1;
      }
    } catch (error) {
      sample.rustError = String(error?.message ?? error);
    }
    if (sample.renderer?.contextLost) run.contextLossCount += 1;
    if (
      sample.renderer?.present === false ||
      sample.renderer?.sameInstance === false
    ) {
      run.rendererStable = false;
    }
    run.samples.push(sample);
    return sample;
  }

  await send("Page.enable");
  await send("Runtime.enable");
  await send("Log.enable");
  await send("HeapProfiler.enable");

  let loaded = false;
  for (let attempt = 0; attempt < 160 && !loaded; attempt += 1) {
    try {
      if (attempt % 20 === 0) {
        await send("Page.navigate", { url: `${origin}/` });
      }
      const state = await evaluate(
        "({ href: location.href, origin: location.origin, state: document.readyState })",
        10_000,
      );
      loaded = state.origin === origin && state.state === "complete";
    } catch {
      // The execution context may still be navigating.
    }
    if (!loaded) await delay(250);
  }
  if (!loaded) throw new Error("WebView2 did not load the local Vite frontend");

  await waitFor(
    "Boolean(window.__TAURI_INTERNALS__?.invoke)",
    60_000,
    "Tauri IPC",
  );
  await waitFor("Boolean(window.__furyViewport)", 60_000, "viewport renderer");

  run.app.webgl = await evaluate(glInfoExpression);
  if (!run.app.webgl)
    throw new Error("renderer did not expose a WebGL context");
  if (run.app.webgl.contextLost)
    throw new Error("WebGL context was already lost at startup");

  if (probeOnly) {
    run.durationMs = Date.now() - startedAt;
    run.pass =
      run.app.webgl.webglVersion.startsWith("WebGL 2.0") &&
      run.app.webgl.error === 0;
    run.summary = {
      probeOnly: true,
      renderer: run.app.webgl.renderer,
      vendor: run.app.webgl.vendor,
      pass: run.pass,
    };
  } else {
    const bootstrap = await evaluate(bootstrapExpression, 60_000);
    if (!bootstrap.tauri || !bootstrap.viewport) {
      throw new Error(
        `unexpected bootstrap state: ${JSON.stringify(bootstrap)}`,
      );
    }

    const projectName = `M10.8 ${backendId} ${Date.now()}`;
    const created = await evaluate(
      `(async () => {
        const project = window.__furyLongEdit.project.useProjectStore;
        const world = window.__furyLongEdit.world.useWorldStore;
        world.getState().setLoadRadius(4);
        const ok = await project.getState().create(${JSON.stringify(projectName)}, "flat");
        return {
          ok,
          error: project.getState().error,
          current: project.getState().current,
          revision: world.getState().revision,
        };
      })()`,
      120_000,
    );
    if (!created.ok) {
      throw new Error(
        `project creation failed: ${created.error ?? "unknown error"}`,
      );
    }
    run.project = {
      name: projectName,
      folder: created.current?.folder ?? null,
      initialRevision: created.revision,
    };

    await waitFor(
      `(() => {
        const world = window.__furyLongEdit.world.useWorldStore.getState();
        const renderer = window.__furyViewport;
        return (
          world.status === "ready" &&
          world.metrics &&
          world.metrics.loadedChunks >= 9 &&
          (world.metrics.pendingMeshJobs ?? 0) === 0 &&
          renderer &&
          renderer.world.loadedChunkCount() >= 9 &&
          renderer.world.revision === world.revision
        );
      })()`,
      120_000,
      "flat project renderer",
    );

    await evaluate(
      `(() => {
        const renderer = window.__furyViewport;
        renderer.setCameraPose([64, 88, 64], [0, 65, 0]);
        return true;
      })()`,
    );

    run.app.initialInfo = await invoke("world_info");
    await collectGarbage();
    run.app.initialHeap = await evaluate(
      `(performance.memory ? { usedBytes: performance.memory.usedJSHeapSize, totalBytes: performance.memory.totalJSHeapSize, limitBytes: performance.memory.jsHeapSizeLimit } : null)`,
    );
    await takeSample("initial");

    const deadline = Date.now() + durationMs;
    let cycle = 0;
    let lastSampleAt = Date.now();
    let nextProbeAt = Date.now() + probeEveryMs;

    while (Date.now() < deadline) {
      if (app.exitCode !== null) {
        throw new Error(
          `Tauri exited with ${app.exitCode}: ${appStderr.slice(-2000)}`,
        );
      }
      const plan = operationFor(cycle);
      run.operations.attempted += 1;
      const applied = await evaluate(applyExpression(plan.operation), 180_000);
      if (!applied.ok || !applied.result) {
        run.operations.failed += 1;
        run.console.errors.push({
          source: "operation",
          cycle,
          kind: plan.operation.type,
          detail: applied.error ?? applied.toasts ?? null,
          elapsedMs: applied.elapsedMs,
        });
        if (run.operations.failed >= 5) {
          throw new Error(
            `five editing operations failed; last: ${JSON.stringify(applied)}`,
          );
        }
        await delay(500);
        cycle += 1;
        continue;
      }

      run.operations.succeeded += 1;
      run.operations.changedBlocks += applied.result.changedBlocks;
      run.operations.byKind[plan.operation.type] =
        (run.operations.byKind[plan.operation.type] ?? 0) + 1;
      if (applied.result.changedBlocks === 0) run.operations.zeroChange += 1;
      if (applied.world.revision !== applied.result.revision) {
        run.revisionMismatches += 1;
      }
      if (applied.renderer.contextLost) {
        run.contextLossCount += 1;
        throw new Error(`WebGL context lost during ${plan.operation.type}`);
      }
      if (!applied.renderer.present) {
        run.rendererStable = false;
        throw new Error("viewport disappeared during editing");
      }

      const verification = await evaluate(
        verifyExpression(plan.sample, applied.result.revision),
        30_000,
      );
      if (!verification.ok) {
        run.operations.verificationFailures.push({
          cycle,
          kind: plan.operation.type,
          sample: plan.sample,
          ...verification,
        });
        if (run.operations.verificationFailures.length >= 5) {
          throw new Error(
            `five renderer verifications failed; last: ${JSON.stringify(verification)}`,
          );
        }
      }

      if (Date.now() >= nextProbeAt) {
        await runUndoRedoProbe(cycle);
        nextProbeAt = Date.now() + probeEveryMs;
      }
      if (Date.now() - lastSampleAt >= sampleEveryMs) {
        await takeSample(`cycle-${cycle}`);
        lastSampleAt = Date.now();
      }
      cycle += 1;
      await delay(intervalMs);
    }

    // A final unique block proves that the last edits survive save and reopen.
    const finalOperation = {
      type: "fill_cuboid",
      dimension,
      bounds: box(0, 72, 0, 1, 1, 1),
      state: { name: "minecraft:diamond_block", properties: {} },
    };
    const finalApplied = await evaluate(
      applyExpression(finalOperation),
      180_000,
    );
    if (!finalApplied.ok || !finalApplied.result) {
      throw new Error(
        `final marker edit failed: ${JSON.stringify(finalApplied)}`,
      );
    }
    const finalVerification = await evaluate(
      verifyExpression(
        {
          pos: { x: 0, y: 72, z: 0 },
          expected: { name: "minecraft:diamond_block", properties: {} },
        },
        finalApplied.result.revision,
      ),
      30_000,
    );
    if (!finalVerification.ok) {
      throw new Error(
        `final marker did not render: ${JSON.stringify(finalVerification)}`,
      );
    }

    await delay(750);
    await collectGarbage();
    run.app.finalHeap = await evaluate(
      `(performance.memory ? { usedBytes: performance.memory.usedJSHeapSize, totalBytes: performance.memory.totalJSHeapSize, limitBytes: performance.memory.jsHeapSizeLimit } : null)`,
    );
    await takeSample("before-save");

    const finalized = await evaluate(finalizeExpression, 300_000);
    run.saveReopen = finalized;
    if (finalized.opened) {
      await waitFor(
        `(() => {
          const world = window.__furyLongEdit.world.useWorldStore.getState();
          const renderer = window.__furyViewport;
          return (
            world.status === "ready" &&
            world.metrics &&
            (world.metrics.pendingMeshJobs ?? 0) === 0 &&
            renderer &&
            renderer.world.loadedChunkCount() >= 9 &&
            renderer.world.revision === world.revision
          );
        })()`,
        120_000,
        "renderer after reopen",
      );
    }
    const saveReopenMatches = Boolean(
      finalized.opened &&
      finalized.before &&
      finalized.after &&
      finalized.before.revision === finalized.after.revision &&
      finalized.before.blockCount === finalized.after.blockCount &&
      finalized.before.chunkCount === finalized.after.chunkCount &&
      finalized.probe?.name === "minecraft:diamond_block",
    );
    run.saveReopen.matches = saveReopenMatches;
    if (!saveReopenMatches) {
      run.console.errors.push({
        source: "save-reopen",
        detail: {
          before: finalized.before,
          after: finalized.after,
          probe: finalized.probe,
          opened: finalized.opened,
          error: finalized.openError ?? finalized.saveError,
        },
      });
    }

    await evaluate(
      `(() => {
        const renderer = window.__furyViewport;
        renderer.setCameraPose([64, 88, 64], [0, 65, 0]);
        return true;
      })()`,
    );
    await delay(1_000);
    const capture = await evaluate(
      `(() => {
        const renderer = window.__furyViewport;
        const result = renderer.capture();
        return { dataUrl: result.dataUrl, metadata: result.metadata };
      })()`,
      60_000,
    );
    const pngPrefix = "data:image/png;base64,";
    if (!capture?.dataUrl?.startsWith(pngPrefix)) {
      throw new Error("viewport capture did not return a PNG data URL");
    }
    const viewportPng = Buffer.from(
      capture.dataUrl.slice(pngPrefix.length),
      "base64",
    );
    if (
      !viewportPng.subarray(0, 8).equals(Buffer.from("89504e470d0a1a0a", "hex"))
    ) {
      throw new Error("invalid viewport PNG signature");
    }
    await writeFile(join(output, "viewport.png"), viewportPng, { flag: "wx" });
    const appScreenshot = await send("Page.captureScreenshot", {
      format: "png",
      captureBeyondViewport: false,
      fromSurface: true,
    });
    const appPng = Buffer.from(appScreenshot.data, "base64");
    if (!appPng.subarray(0, 8).equals(Buffer.from("89504e470d0a1a0a", "hex"))) {
      throw new Error("invalid application PNG signature");
    }
    await writeFile(join(output, "app.png"), appPng, { flag: "wx" });
    run.capture = {
      viewportPngBytes: viewportPng.length,
      viewportPngSha256: sha256(viewportPng),
      appPngBytes: appPng.length,
      appPngSha256: sha256(appPng),
      metadata: capture.metadata,
    };

    await takeSample("final");
  }
} catch (error) {
  run.failure = {
    name: error?.name ?? "Error",
    message: String(error?.message ?? error),
    stack: String(error?.stack ?? ""),
  };
} finally {
  run.durationMs = Date.now() - startedAt;
  run.stderr = appStderr.slice(-8000);
  const fps = finite(
    run.samples.map((sample) => sample.renderer?.metrics?.fps),
  ).sort((a, b) => a - b);
  const frameMs = finite(
    run.samples.map((sample) => sample.renderer?.metrics?.frameMs),
  ).sort((a, b) => a - b);
  const heaps = finite(run.samples.map((sample) => sample.memory?.usedBytes));
  const initialHeap = run.app.initialHeap?.usedBytes ?? heaps[0] ?? null;
  const finalHeap = run.app.finalHeap?.usedBytes ?? heaps.at(-1) ?? null;
  const heapGrowthBytes =
    initialHeap !== null && finalHeap !== null ? finalHeap - initialHeap : null;
  const observedContextLoss = run.samples.reduce(
    (sum, sample) => Math.max(sum, sample.observers?.contextLossCount ?? 0),
    0,
  );
  run.contextLossCount = Math.max(run.contextLossCount, observedContextLoss);
  const hasHandshake =
    run.operations.succeeded > 0 ||
    run.saveReopen !== null ||
    run.app.webgl !== null;
  const saveReopenMatches = run.saveReopen?.matches === true;
  const assertions = {
    handshake: hasHandshake,
    longSessionCompleted:
      run.durationMs !== null &&
      (probeOnly || run.operations.attempted > 0) &&
      (probeOnly || run.operations.succeeded > 0),
    noFailedOperations: run.operations.failed === 0,
    noFailedUndoRedoProbes: run.operations.undoRedoProbes.every(
      (probe) => probe.ok,
    ),
    noRendererVerificationFailures:
      run.operations.verificationFailures.length === 0,
    noRevisionMismatch: run.revisionMismatches === 0,
    noContextLoss: run.contextLossCount === 0,
    rendererStable: run.rendererStable,
    saveReopenMatches: probeOnly || saveReopenMatches,
    hardwareFrameRate:
      backend.kind === "software" ||
      probeOnly ||
      (percentile(fps, 0.5) ?? 0) >= 20,
    heapGrowthBounded:
      probeOnly ||
      heapGrowthBytes === null ||
      heapGrowthBytes < 256 * 1024 * 1024,
  };
  run.summary = {
    probeOnly,
    backend: run.backend,
    durationMs: run.durationMs,
    operations: run.operations,
    fps: {
      samples: fps.length,
      min: fps[0] ?? null,
      median: percentile(fps, 0.5),
      p95: percentile(fps, 0.95),
      max: fps.at(-1) ?? null,
    },
    frameMs: {
      samples: frameMs.length,
      median: percentile(frameMs, 0.5),
      p95: percentile(frameMs, 0.95),
      max: frameMs.at(-1) ?? null,
    },
    heap: {
      initialUsedBytes: initialHeap,
      finalUsedBytes: finalHeap,
      growthBytes: heapGrowthBytes,
      maxUsedBytes: heaps.length ? Math.max(...heaps) : null,
    },
    contextLossCount: run.contextLossCount,
    revisionMismatches: run.revisionMismatches,
    verificationFailures: run.operations.verificationFailures.length,
    consoleErrors: run.console.errors.length,
    consoleWarnings: run.console.warnings.length,
    saveReopen: run.saveReopen
      ? {
          saved: run.saveReopen.saved,
          closed: run.saveReopen.closed,
          opened: run.saveReopen.opened,
          matches: saveReopenMatches,
          before: run.saveReopen.before,
          after: run.saveReopen.after,
          probe: run.saveReopen.probe,
          saveMs: run.saveReopen.saveMs,
          closeMs: run.saveReopen.closeMs,
          openMs: run.saveReopen.openMs,
          checksum: run.saveReopen.lastSave?.checksum ?? null,
          snapshotFile: run.saveReopen.lastSave?.snapshotFile ?? null,
          bytesWritten: run.saveReopen.lastSave?.bytesWritten ?? null,
        }
      : null,
    assertions,
    physicalGpuLimitation: physicalLimitation,
  };
  run.pass = Object.values(assertions).every(Boolean) && run.failure === null;

  try {
    await writeFile(
      join(output, "metrics.json"),
      `${JSON.stringify(run, null, 2)}\n`,
      { flag: "wx" },
    );
  } catch (writeError) {
    console.error(`Could not write metrics.json: ${writeError.message}`);
  }

  if (probeOnly) {
    console.log(
      JSON.stringify(
        {
          backend: backendId,
          pass: run.pass,
          webgl: run.app.webgl,
          limitation: physicalLimitation,
        },
        null,
        2,
      ),
    );
  } else {
    console.log(
      JSON.stringify(
        {
          backend: backendId,
          pass: run.pass,
          durationMs: run.durationMs,
          operations: {
            attempted: run.operations.attempted,
            succeeded: run.operations.succeeded,
            failed: run.operations.failed,
            changedBlocks: run.operations.changedBlocks,
          },
          fps: run.summary.fps,
          heap: run.summary.heap,
          saveReopen: run.summary.saveReopen?.matches ?? null,
          failure: run.failure,
          assertions: run.summary.assertions,
          output,
        },
        null,
        2,
      ),
    );
  }

  ws?.close();
  if (app && app.exitCode === null) app.kill();
  await delay(1_500);
  if (appData)
    await rm(appData, { recursive: true, force: true, maxRetries: 3 }).catch(
      () => {},
    );
  if (profile)
    await rm(profile, { recursive: true, force: true, maxRetries: 3 }).catch(
      () => {},
    );
}

if (!run.pass) process.exitCode = 1;
