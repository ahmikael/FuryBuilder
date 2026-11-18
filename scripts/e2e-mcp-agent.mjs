#!/usr/bin/env node
// End-to-end probe of the AI edit loop through a real external MCP client.
//
// It drives OpenCode (a separate process) against the authenticated local endpoint of a
// running FuryBuilder desktop app, approves the real confirmation dialogs through the
// WebView, then verifies the result through the authoritative Application API. The app
// must be started with WebView2 remote debugging enabled:
//
//   $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=9229"
//   pnpm --filter @furybuilder/desktop tauri dev
//   node scripts/e2e-mcp-agent.mjs
//
// The script refuses to run against a non-empty world: it is a scratch-project test, not
// an editing tool for a user's project. It never changes user configuration; the OpenCode
// project lives in the temporary directory.

import { spawn, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync } from "node:fs";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { createConnection } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const bridgeRoot = join(repoRoot, "mcp", "bridge");
const defaultOpencode =
  "C:\\Users\\Mike\\AppData\\Local\\Programs\\ai.opencode.desktop\\resources\\opencode-cli.exe";
const opencode = process.env.FURYBUILDER_E2E_OPENCODE || defaultOpencode;
const model =
  process.env.FURYBUILDER_E2E_MODEL || "opencode/nemotron-3.5-super-vl-free";
const cdpBase = process.env.FURYBUILDER_E2E_CDP || "http://127.0.0.1:9229";
const evidencePath =
  process.env.FURYBUILDER_E2E_EVIDENCE ||
  join(repoRoot, "docs", "testing", "evidence", "2026-11-18-ai-edit-loop.json");
const runtimePath =
  process.env.FURYBUILDER_MCP_RUNTIME ||
  join(
    process.env.APPDATA ?? "",
    "com.furybuilder.desktop",
    "mcp",
    "runtime.json",
  );
const agentTimeoutMs = Number(
  process.env.FURYBUILDER_E2E_TIMEOUT_MS ?? 600_000,
);
const extraNotes = (() => {
  try {
    const parsed = JSON.parse(process.env.FURYBUILDER_E2E_NOTES ?? "[]");
    return Array.isArray(parsed) ? parsed.map(String) : [];
  } catch {
    return [];
  }
})();
const projectId = "local-project";
const dimension = "minecraft:overworld";

if (!process.env.APPDATA) {
  console.error("APPDATA is required on Windows");
  process.exit(2);
}
if (!existsSync(opencode)) {
  console.error(`OpenCode CLI not found: ${opencode}`);
  process.exit(2);
}

const sleep = (ms) => new Promise((done) => setTimeout(done, ms));

/** Node's `child.kill` leaves the CLI's private server alive on Windows. */
function killTree(pid) {
  if (!pid) return;
  if (process.platform === "win32") {
    spawnSync("taskkill", ["/PID", String(pid), "/T", "/F"], {
      windowsHide: true,
    });
    return;
  }
  try {
    process.kill(pid, "SIGKILL");
  } catch {
    // already gone
  }
}

function request(runtime, id, token, method, params = {}) {
  return new Promise((resolvePromise, reject) => {
    const socket = createConnection(runtime.endpoint);
    let buffer = "";
    const timer = setTimeout(() => {
      socket.destroy();
      reject(new Error(`timeout for ${method}`));
    }, 30_000);
    socket.setEncoding("utf8");
    socket.once("connect", () => {
      socket.write(
        `${JSON.stringify({
          protocol: 1,
          id,
          token,
          clientId: "e2e-mcp-agent",
          method,
          params,
        })}\n`,
      );
    });
    socket.on("data", (chunk) => {
      buffer += chunk;
      const newline = buffer.indexOf("\n");
      if (newline < 0) return;
      clearTimeout(timer);
      socket.destroy();
      resolvePromise(JSON.parse(buffer.slice(0, newline)));
    });
    socket.once("error", (error) => {
      clearTimeout(timer);
      reject(error);
    });
  });
}

async function waitForRuntime(timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      return JSON.parse(await readFile(runtimePath, "utf8"));
    } catch {
      await sleep(250);
    }
  }
  throw new Error(`runtime file did not appear: ${runtimePath}`);
}

class CdpClient {
  constructor(webSocketDebuggerUrl) {
    this.socket = new WebSocket(webSocketDebuggerUrl);
    this.nextId = 0;
    this.pending = new Map();
  }

  async open() {
    await new Promise((resolvePromise, reject) => {
      this.socket.onopen = resolvePromise;
      this.socket.onerror = reject;
    });
    this.socket.onmessage = (event) => {
      const message = JSON.parse(event.data);
      if (!message.id || !this.pending.has(message.id)) return;
      const { resolvePromise, reject, timer } = this.pending.get(message.id);
      this.pending.delete(message.id);
      clearTimeout(timer);
      if (message.error) reject(new Error(JSON.stringify(message.error)));
      else resolvePromise(message.result);
    };
    this.socket.onclose = () => {
      for (const { reject, timer } of this.pending.values()) {
        clearTimeout(timer);
        reject(new Error("CDP socket closed"));
      }
      this.pending.clear();
    };
  }

  send(method, params = {}, timeoutMs = 10_000) {
    const id = ++this.nextId;
    this.socket.send(JSON.stringify({ id, method, params }));
    return new Promise((resolvePromise, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`CDP timeout for ${method}`));
      }, timeoutMs);
      this.pending.set(id, { resolvePromise, reject, timer });
    });
  }

  async evaluate(expression) {
    const result = await this.send("Runtime.evaluate", {
      expression,
      awaitPromise: true,
      returnByValue: true,
    });
    if (result.exceptionDetails) {
      throw new Error(JSON.stringify(result.exceptionDetails));
    }
    return result.result.value;
  }

  close() {
    this.socket.close();
  }
}

async function connectPage() {
  const response = await fetch(`${cdpBase}/json/list`);
  if (!response.ok) {
    throw new Error(
      `WebView2 debugging endpoint is not reachable at ${cdpBase}; start the app with WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9229`,
    );
  }
  const targets = await response.json();
  const page = targets.find(
    (target) => target.type === "page" && target.url.startsWith("http"),
  );
  if (!page)
    throw new Error("no FuryBuilder page target on the debugging endpoint");
  const client = new CdpClient(page.webSocketDebuggerUrl);
  await client.open();
  return client;
}

function invokeExpression(command, args) {
  return `window.__TAURI_INTERNALS__.invoke(${JSON.stringify(command)}, ${JSON.stringify(args)})`;
}

// A real confirmation dialog is rendered by McpConfirmationHost. Clicking its approve
// button is what a user does; the test does not call submit_mcp_confirmation directly.
const APPROVE_EXPRESSION = `(() => {
  const text = document.body?.innerText ?? "";
  if (!text.includes("Approve an AI edit?")) return { pending: false };
  const approve = [...document.querySelectorAll("button")].find(
    (button) => button.textContent?.trim() === "Approve and apply" && !button.disabled,
  );
  const dialog = approve?.closest('[role="dialog"]') ?? document.querySelector('[role="dialog"]');
  const dialogText = (dialog?.innerText ?? text).slice(0, 2000);
  if (!approve) return { pending: true, approved: false, text: dialogText };
  approve.click();
  return { pending: true, approved: true, text: dialogText };
})()`;

function change(x, y, z, name) {
  return {
    pos: { x, y, z },
    state: { name: `minecraft:${name}`, properties: {} },
  };
}

const platform = [];
for (let x = -1; x <= 1; x += 1) {
  for (let z = -1; z <= 1; z += 1) {
    platform.push(change(x, 64, z, "stone"));
  }
}
for (const [x, z] of [
  [-1, -1],
  [-1, 1],
  [1, -1],
  [1, 1],
]) {
  platform.push(change(x, 65, z, "stone"));
}

const prompt = `You are the controlled test agent for the FuryBuilder desktop editor. Use ONLY the furybuilder MCP tools (names start with furybuilder_). NEVER use shell, file, web or other tools. Do not ask questions; the user approves write dialogs when they appear. Copy the JSON arguments exactly and use project_id "${projectId}" and dimension "${dimension}" everywhere. Always read the newest world_revision from the preceding tool result; never guess it.

STAGE 1 - explore:
1. furybuilder_bridge_info {"project_id":"${projectId}","world_revision":0}. Confirm desktop.connected=true and that the context state is ready.
2. furybuilder_world_info {"project_id":"${projectId}"}.
3. furybuilder_inspect_region {"project_id":"${projectId}","dimension":"${dimension}","bounds":{"min":{"x":-2,"y":63,"z":-2},"max":{"x":2,"y":66,"z":2}}}.

STAGE 2 - preview and build a 3x3 stone platform at y=64 with four stone corner posts at y=65 (13 blocks total):
4. furybuilder_preview_operation {"project_id":"${projectId}","world_revision":0,"operation":{"type":"batch","dimension":"${dimension}","changes":${JSON.stringify(platform)}}}. Expect changedBlocks=13. Stop and report if it is an error.
5. furybuilder_apply_operation with the same operation plus "world_revision":0 and "idempotency_key":"e2e-build-1". A user approval dialog may appear; wait for the tool result. Record operationId A and the new worldRevision (expected 1).

STAGE 3 - observe:
6. furybuilder_get_camera {"project_id":"${projectId}","world_revision":<revision after the build>}.
7. furybuilder_capture_viewport {"project_id":"${projectId}","world_revision":<revision after the build>,"dimension":"${dimension}"}. The result has a PNG image and a metadata JSON text block with image.bytes, image.sha256, metadata.partial and verification.{renderRevision,authoritativeRevision,stale}. Record them and confirm stale=false.

STAGE 4 - fix:
8. furybuilder_preview_operation {"project_id":"${projectId}","world_revision":<revision after the build>,"operation":{"type":"batch","dimension":"${dimension}","changes":[{"pos":{"x":0,"y":64,"z":0},"state":{"name":"minecraft:glass","properties":{}}}]}}. Expect changedBlocks=1.
9. furybuilder_apply_operation with that operation, "world_revision":<revision after the build> and "idempotency_key":"e2e-fix-1". Approve the dialog. Record operationId B and the new worldRevision (expected 2).
10. furybuilder_capture_viewport {"project_id":"${projectId}","world_revision":<revision after the fix>,"dimension":"${dimension}"}. Record metadata again.

STAGE 5 - verify the fix:
11. furybuilder_get_block {"project_id":"${projectId}","dimension":"${dimension}","pos":{"x":0,"y":64,"z":0}}. Expect minecraft:glass.
12. furybuilder_get_block {"project_id":"${projectId}","dimension":"${dimension}","pos":{"x":1,"y":64,"z":1}}. Expect minecraft:stone.

STAGE 6 - undo the fix:
13. furybuilder_undo_operation {"project_id":"${projectId}","world_revision":<revision after the fix>,"operation_id":"<operationId B>"}. Approve the dialog. Record the new revision (expected 3).
14. furybuilder_get_block {"project_id":"${projectId}","dimension":"${dimension}","pos":{"x":0,"y":64,"z":0}}. Expect minecraft:stone again.

STAGE 7 - undo the build:
15. furybuilder_undo_operation {"project_id":"${projectId}","world_revision":<revision after undo of the fix>,"operation_id":"<operationId A>"}. Approve the dialog. Record the new revision (expected 4).
16. furybuilder_world_info {"project_id":"${projectId}"} and furybuilder_inspect_region for the same bounds as step 3. Expect an empty world.

Then reply with exactly one JSON object (no markdown fences) with fields: buildOperationId, fixOperationId, revisions {start, build, fix, undoFix, undoBuild}, screenshots [{revision, bytes, sha256, partial, stale}], final {revision, blockCount, nonAirBlocks}, checks {bridgeReady, previewCorrect, buildApplied, screenshotCaptured, fixApplied, blocksVerified, fixUndone, buildUndone, worldRestored}, notes. Do not call any more tools after that JSON.`;

function stripLongBase64(value) {
  return value.replace(
    /[A-Za-z0-9+/=\r\n]{2000,}/g,
    (match) => `<base64:${match.length}>`,
  );
}

function summarizeOutput(output) {
  if (typeof output !== "string") return { bytes: 0 };
  let text = output;
  let parsed = null;
  try {
    parsed = JSON.parse(output);
  } catch {
    parsed = null;
  }
  text = stripLongBase64(text);
  if (text.length > 3000) text = `${text.slice(0, 3000)}…`;
  return {
    bytes: Buffer.byteLength(output),
    parsed,
    text,
  };
}

function parseTranscript(raw) {
  const events = [];
  const toolCalls = [];
  const assistantText = [];
  const errors = [];
  for (const line of raw.split(/\r?\n/)) {
    if (!line.trim()) continue;
    let event;
    try {
      event = JSON.parse(line);
    } catch {
      events.push({
        type: "unparsed",
        line: stripLongBase64(line).slice(0, 500),
      });
      continue;
    }
    events.push({ type: event.type });
    if (event.type === "error") {
      errors.push(event.error ?? event);
    }
    if (event.type === "tool_use" && event.part?.tool) {
      toolCalls.push({
        tool: event.part.tool,
        status: event.part.state?.status ?? null,
        input: event.part.state?.input ?? null,
        inputTruncated: stripLongBase64(
          JSON.stringify(event.part.state?.input ?? null),
        ).slice(0, 4000),
        output: summarizeOutput(event.part.state?.output),
      });
    }
    if (event.type === "text" && event.part?.text) {
      assistantText.push(event.part.text);
    }
  }
  return { events, toolCalls, assistantText, errors };
}

function findTool(toolCalls, name) {
  return toolCalls.filter((call) => call.tool === `furybuilder_${name}`);
}

function parseToolJson(call) {
  if (!call) return null;
  if (call.output.parsed && typeof call.output.parsed === "object") {
    return call.output.parsed;
  }
  try {
    return JSON.parse(call.output.text);
  } catch {
    return null;
  }
}

function extractCaptureMetadata(text) {
  try {
    const parsed = JSON.parse(text);
    return {
      bytes: parsed.image?.bytes ?? null,
      sha256: parsed.image?.sha256 ?? null,
      mimeType: parsed.image?.mimeType ?? null,
      partial: parsed.metadata?.partial ?? null,
      stale: parsed.verification?.stale ?? null,
      renderRevision: parsed.verification?.renderRevision ?? null,
      authoritativeRevision:
        parsed.verification?.authoritativeRevision ??
        parsed.metadata?.worldRevision ??
        null,
      loadedChunks: Array.isArray(parsed.metadata?.loadedChunks)
        ? parsed.metadata.loadedChunks.length
        : null,
      missingChunks: Array.isArray(parsed.metadata?.missingChunks)
        ? parsed.metadata.missingChunks.length
        : null,
      hasBuildChunk: Array.isArray(parsed.metadata?.loadedChunks)
        ? parsed.metadata.loadedChunks.some(
            (chunk) => chunk.x === 0 && chunk.z === 0,
          )
        : false,
      legend: Array.isArray(parsed.metadata?.legend)
        ? parsed.metadata.legend.map((entry) => ({
            state: entry.state,
            count: entry.count,
          }))
        : [],
      verifiedPartial: parsed.verification?.partial ?? null,
    };
  } catch {
    return null;
  }
}

function legendCount(metadata, state) {
  return (metadata?.legend ?? [])
    .filter((entry) => entry.state === state)
    .reduce((sum, entry) => sum + entry.count, 0);
}

async function main() {
  const runtime = await waitForRuntime(120_000);
  const call = (id, method, params) =>
    request(runtime, id, runtime.token, method, params);

  const startedAt = new Date();
  const cdp = await connectPage();
  const checks = [];
  const approvals = [];
  const approvalTimeline = [];
  const check = (name, passed, detail = null) => {
    checks.push({ name, passed: Boolean(passed), detail });
  };

  let policy = null;
  let restoredPolicy = null;
  let agent = null;
  let transcript = null;
  let verification = null;
  let captureMetadata = [];
  let pendingBeforeRestore = null;
  let errorMessage = null;

  try {
    const ping = await call("e2e-ping", "ping", { projectId });
    if (!ping.ok) throw new Error(`ping failed: ${JSON.stringify(ping.error)}`);
    const initialWorld = await call("e2e-world-initial", "world_info", {
      projectId,
    });
    if (!initialWorld.ok) {
      throw new Error(
        `world_info failed: ${JSON.stringify(initialWorld.error)}`,
      );
    }
    const initial = initialWorld.result;
    if (initial.revision !== 0 || initial.blockCount !== 0) {
      throw new Error(
        `refusing to run against a non-empty world: revision ${initial.revision}, blocks ${initial.blockCount}`,
      );
    }
    check("scratch world is empty", true, {
      revision: initial.revision,
      blockCount: initial.blockCount,
    });

    // Same command the access dialog calls after the user chooses "Full project".
    policy = await cdp.evaluate(
      invokeExpression("set_mcp_access", {
        input: {
          mode: "full_project",
          requireConfirmation: true,
          maxBlocks: 50_000,
          maxBatchChanges: 4096,
        },
      }),
    );
    check(
      "write policy enabled with confirmation",
      policy.mode === "full_project" && policy.requireConfirmation === true,
      policy,
    );

    const projectDir = await mkdtemp(join(tmpdir(), "fury-e2e-"));
    const config = {
      $schema: "https://opencode.ai/config.json",
      model,
      mcp: {
        servers: {
          furybuilder: {
            type: "local",
            command: [process.execPath, "--import", "tsx", "src/main.ts"],
            cwd: bridgeRoot,
            environment: { FURYBUILDER_MCP_RUNTIME: runtimePath },
            codemode: false,
            protocol: "auto",
            timeout: { startup: 20_000, catalog: 20_000, execution: 120_000 },
          },
        },
      },
    };
    await writeFile(
      join(projectDir, "opencode.json"),
      `${JSON.stringify(config, null, 2)}\n`,
      "utf8",
    );

    const child = spawn(
      opencode,
      [
        "run",
        "--standalone",
        "--auto",
        "--model",
        model,
        "--format",
        "json",
        "--title",
        "FuryBuilder AI edit loop E2E",
        prompt,
      ],
      {
        cwd: projectDir,
        env: { ...process.env, FURYBUILDER_MCP_RUNTIME: runtimePath },
        // A pipe on stdin stays open forever. OpenCode probes stdin at startup, so an
        // ignored stdin is what lets a scripted `run` proceed like an interactive one.
        stdio: ["ignore", "pipe", "pipe"],
        windowsHide: true,
      },
    );
    const stdout = [];
    const stderr = [];
    child.stdout.on("data", (chunk) => stdout.push(chunk));
    child.stderr.on("data", (chunk) => {
      stderr.push(chunk);
      const text = stripLongBase64(chunk.toString("utf8"));
      for (const line of text.split(/\r?\n/)) {
        if (line.trim()) console.error(`[agent] ${line}`);
      }
    });
    const exit = new Promise((resolveExit) => {
      child.once("close", (code, signal) =>
        resolveExit({ code, signal, finishedAt: new Date() }),
      );
      child.once("error", (error) =>
        resolveExit({ code: null, signal: null, error: String(error) }),
      );
    });
    let timedOutFlag = false;
    const watchdog = setTimeout(() => {
      timedOutFlag = true;
      console.error(
        `[agent] timed out after ${agentTimeoutMs} ms; killing the process tree`,
      );
      killTree(child.pid);
    }, agentTimeoutMs);

    let exited = false;
    const exitInfo = exit.then((info) => {
      exited = true;
      clearTimeout(watchdog);
      return info;
    });
    const deadline = Date.now() + agentTimeoutMs;
    let lastApprovalAt = 0;
    while (!exited && Date.now() < deadline) {
      if (Date.now() - lastApprovalAt > 800) {
        try {
          const result = await cdp.evaluate(APPROVE_EXPRESSION);
          if (result.pending) {
            approvalTimeline.push({
              atMs: Date.now(),
              approved: result.approved,
              text: String(result.text).slice(0, 1500),
            });
            if (result.approved) {
              approvals.push(String(result.text).slice(0, 1500));
              lastApprovalAt = Date.now();
            }
          }
        } catch (error) {
          approvalTimeline.push({
            atMs: Date.now(),
            approved: false,
            error: String(error),
          });
        }
      }
      await sleep(250);
    }
    let run = await Promise.race([exitInfo, sleep(5_000).then(() => null)]);
    if (run === null) {
      timedOutFlag = true;
      errorMessage = `agent did not finish within ${agentTimeoutMs} ms`;
      killTree(child.pid);
      run = await Promise.race([
        exitInfo,
        sleep(15_000).then(() => ({
          code: null,
          signal: "kill_timeout",
          finishedAt: new Date(),
        })),
      ]);
    }
    const timedOut = timedOutFlag;
    // The CLI's private server is a child process; a failed start or an early exit must
    // not leave it behind to hold the shared session database.
    killTree(child.pid);
    agent = {
      command: opencode,
      model,
      exitCode: run.code,
      signal: run.signal,
      timedOut,
      startedAt: startedAt.toISOString(),
      finishedAt: run.finishedAt?.toISOString() ?? null,
      stderrTail: stripLongBase64(Buffer.concat(stderr).toString("utf8"))
        .split(/\r?\n/)
        .filter(Boolean)
        .slice(-20),
      transcriptPath: null,
    };
    transcript = parseTranscript(Buffer.concat(stdout).toString("utf8"));
    agent.errors = transcript.errors;
    const rawTranscriptPath = join(projectDir, "transcript.jsonl");
    await writeFile(rawTranscriptPath, Buffer.concat(stdout), "utf8");
    agent.transcriptPath = rawTranscriptPath;

    check("agent exited cleanly", run.code === 0 && !agent.timedOut, {
      code: run.code,
      signal: run.signal,
    });

    pendingBeforeRestore = await cdp.evaluate(
      invokeExpression("mcp_access_status", undefined),
    );
    const pending = pendingBeforeRestore?.pendingConfirmations ?? [];
    for (const request of pending) {
      await cdp.evaluate(
        invokeExpression("submit_mcp_confirmation", {
          requestId: request.requestId,
          approved: false,
        }),
      );
    }

    // Authoritative verification through the same endpoint the bridge used.
    const finalWorld = await call("e2e-world-final", "world_info", {
      projectId,
    });
    const center = await call("e2e-block-center", "query", {
      projectId,
      query: {
        type: "get_block",
        dimension,
        pos: { x: 0, y: 64, z: 0 },
      },
    });
    const corner = await call("e2e-block-corner", "query", {
      projectId,
      query: {
        type: "get_block",
        dimension,
        pos: { x: 1, y: 64, z: 1 },
      },
    });
    const region = await call("e2e-region-final", "query", {
      projectId,
      query: {
        type: "region_summary",
        dimension,
        bounds: {
          min: { x: -2, y: 63, z: -2 },
          max: { x: 2, y: 66, z: 2 },
        },
      },
    });
    const activity = await call("e2e-activity", "mcp_activity", {
      projectId,
      limit: 100,
    });
    const jobs = await call("e2e-jobs", "list_jobs", { projectId, limit: 10 });

    verification = {
      finalWorld: finalWorld.result,
      center: center.result,
      corner: corner.result,
      region: region.result,
      activity: activity.result,
      jobs: jobs.result,
    };

    const tools = transcript.toolCalls.map((entry) => entry.tool);
    const requiredTools = [
      "furybuilder_bridge_info",
      "furybuilder_world_info",
      "furybuilder_inspect_region",
      "furybuilder_preview_operation",
      "furybuilder_apply_operation",
      "furybuilder_get_camera",
      "furybuilder_capture_viewport",
      "furybuilder_get_block",
      "furybuilder_undo_operation",
    ];
    const missing = requiredTools.filter((name) => !tools.includes(name));
    check("required MCP tools were called", missing.length === 0, {
      missing,
      called: tools,
    });
    const nonMcpTools = transcript.toolCalls
      .map((entry) => entry.tool)
      .filter((name) => !name.startsWith("furybuilder_"));
    check("no non-MCP tools were used", nonMcpTools.length === 0, {
      nonMcpTools,
    });

    check(
      "four real confirmation dialogs were approved",
      approvals.length === 4,
      { count: approvals.length },
    );

    const previews = findTool(transcript.toolCalls, "preview_operation");
    const applies = findTool(transcript.toolCalls, "apply_operation");
    const undos = findTool(transcript.toolCalls, "undo_operation");
    const captures = findTool(transcript.toolCalls, "capture_viewport");
    const previewResults = previews.map(parseToolJson);
    const applyResults = applies.map(parseToolJson);
    const undoResults = undos.map(parseToolJson);
    captureMetadata = captures
      .map((entry) => extractCaptureMetadata(entry.output.text))
      .filter(Boolean);

    check(
      "previews matched the planned changes",
      previewResults.length === 2 &&
        previewResults[0]?.changedBlocks === platform.length &&
        previewResults[1]?.changedBlocks === 1,
      {
        previewChangedBlocks: previewResults.map(
          (entry) => entry?.changedBlocks,
        ),
      },
    );
    check(
      "build applied the 13-block platform",
      applyResults.length === 2 &&
        applyResults[0]?.changedBlocks === platform.length &&
        applyResults[0]?.worldRevision === 1 &&
        typeof applyResults[0]?.operationId === "string",
      {
        changedBlocks: applyResults[0]?.changedBlocks,
        worldRevision: applyResults[0]?.worldRevision,
        operationId: applyResults[0]?.operationId,
      },
    );
    check(
      "fix applied the glass block",
      applyResults.length === 2 &&
        applyResults[1]?.changedBlocks === 1 &&
        applyResults[1]?.worldRevision === 2 &&
        typeof applyResults[1]?.operationId === "string",
      {
        changedBlocks: applyResults[1]?.changedBlocks,
        worldRevision: applyResults[1]?.worldRevision,
        operationId: applyResults[1]?.operationId,
      },
    );
    check(
      "two viewport screenshots reflect the build and the fix",
      captureMetadata.length === 2 &&
        captureMetadata.every(
          (entry) =>
            typeof entry.bytes === "number" &&
            entry.bytes > 0 &&
            typeof entry.sha256 === "string" &&
            /^[0-9a-f]{64}$/.test(entry.sha256) &&
            typeof entry.partial === "boolean" &&
            entry.partial === entry.missingChunks > 0 &&
            entry.verifiedPartial === entry.partial &&
            entry.stale === false &&
            entry.renderRevision === entry.authoritativeRevision &&
            entry.hasBuildChunk === true,
        ) &&
        captureMetadata[0].authoritativeRevision === 1 &&
        captureMetadata[1].authoritativeRevision === 2 &&
        legendCount(captureMetadata[0], "minecraft:stone") ===
          platform.length &&
        legendCount(captureMetadata[1], "minecraft:stone") ===
          platform.length - 1 &&
        legendCount(captureMetadata[1], "minecraft:glass") === 1,
      captureMetadata,
    );
    check(
      "undo restored the fix and then the build",
      undoResults.length === 2 &&
        undoResults[0]?.changedBlocks === 1 &&
        undoResults[0]?.worldRevision === 3 &&
        undoResults[1]?.changedBlocks === platform.length &&
        undoResults[1]?.worldRevision === 4,
      undoResults.map((entry) => ({
        changedBlocks: entry?.changedBlocks,
        worldRevision: entry?.worldRevision,
        operationId: entry?.operationId,
      })),
    );

    const final = finalWorld.result;
    check(
      "authoritative world returned to revision 4 and zero blocks",
      final.revision === 4 && final.blockCount === 0,
      { revision: final.revision, blockCount: final.blockCount },
    );
    check(
      "final center and corner blocks are air",
      center.result?.data?.state?.name === "minecraft:air" &&
        corner.result?.data?.state?.name === "minecraft:air",
      {
        center: center.result?.data?.state,
        corner: corner.result?.data?.state,
      },
    );
    check(
      "final region summary contains no non-air blocks",
      (region.result?.data?.nonAirBlocks ?? region.result?.nonAirBlocks) === 0,
      region.result,
    );

    const entries = activity.result?.entries ?? [];
    const writes = entries.filter((entry) =>
      ["apply_operation", "undo_operation"].includes(entry.method),
    );
    const approvedWrites = writes.filter(
      (entry) => entry.status === "ok" && entry.confirmation === "approved",
    );
    check(
      "activity journal links four approved writes to operations",
      writes.length === 4 &&
        approvedWrites.length === 4 &&
        approvedWrites.every(
          (entry) =>
            typeof entry.operationId === "string" &&
            typeof entry.changedBlocks === "number" &&
            entry.projectId === projectId,
        ),
      writes.map((entry) => ({
        method: entry.method,
        status: entry.status,
        confirmation: entry.confirmation,
        operationId: entry.operationId,
        changedBlocks: entry.changedBlocks,
        requestId: entry.requestId,
      })),
    );
    const capturedEntries = entries.filter(
      (entry) => entry.method === "capture_viewport" && entry.status === "ok",
    );
    check(
      "activity journal records both captures",
      capturedEntries.length >= 2,
      { count: capturedEntries.length },
    );
    check(
      "sync tools did not create background jobs",
      Array.isArray(jobs.result?.jobs) && jobs.result.jobs.length === 0,
      jobs.result?.summary ?? null,
    );

    const finalAssistant = transcript.assistantText.join("\n\n");
    const reported = (() => {
      const match = finalAssistant.match(/\{[\s\S]*\}\s*$/);
      if (!match) return null;
      try {
        return JSON.parse(match[0]);
      } catch {
        return null;
      }
    })();
    check(
      "agent reported a complete result",
      reported !== null &&
        reported.buildOperationId === applyResults[0]?.operationId &&
        reported.fixOperationId === applyResults[1]?.operationId &&
        reported.final?.revision === 4 &&
        reported.final?.nonAirBlocks === 0,
      reported ?? { finalText: finalAssistant.slice(-1200) },
    );
  } catch (error) {
    errorMessage = String(error?.stack ?? error);
    check("e2e harness completed", false, errorMessage);
  } finally {
    if (!verification) {
      try {
        verification = {
          finalWorld: (
            await call("e2e-final-fallback", "world_info", { projectId })
          ).result,
          activity: (
            await call("e2e-activity-fallback", "mcp_activity", {
              projectId,
              limit: 100,
            })
          ).result,
        };
      } catch (error) {
        verification = { error: String(error) };
      }
    }
    try {
      restoredPolicy = await cdp.evaluate(
        invokeExpression("set_mcp_access", {
          input: {
            mode: "read_only",
            requireConfirmation: true,
            maxBlocks: 50_000,
            maxBatchChanges: 4096,
          },
        }),
      );
      check(
        "write policy restored to read-only",
        restoredPolicy.mode === "read_only" &&
          restoredPolicy.writesEnabled === false,
        restoredPolicy,
      );
    } catch (error) {
      check("write policy restored to read-only", false, String(error));
    }
    cdp.close();
  }

  const failed = checks.filter((entry) => !entry.passed);
  const evidence = {
    date: new Date().toISOString().slice(0, 10),
    result: failed.length === 0 ? "passed" : "failed",
    agent,
    notes: [
      ...extraNotes,
      ...(captureMetadata.some((entry) => entry?.partial)
        ? [
            "A viewport screenshot reports partial=true when empty neighbour chunks are not loaded. The edited chunk (0,0) was loaded and its legend matched the intended build and fix.",
          ]
        : []),
    ],
    access: {
      requested: policy,
      restored: restoredPolicy,
      pendingBeforeRestore:
        pendingBeforeRestore?.pendingConfirmations?.length ?? null,
    },
    approvals: {
      count: approvals.length,
      dialogs: approvals,
      timeline: approvalTimeline.slice(0, 20),
    },
    toolCalls: transcript
      ? transcript.toolCalls.map((entry) => ({
          tool: entry.tool,
          status: entry.status,
          input: entry.inputTruncated,
          outputBytes: entry.output.bytes,
          outputText: entry.output.text,
        }))
      : [],
    verification,
    checks,
    error: errorMessage,
    promptSha256: createHash("sha256").update(prompt).digest("hex"),
  };
  await writeFile(
    evidencePath,
    `${JSON.stringify(evidence, null, 2)}\n`,
    "utf8",
  );
  console.log(
    JSON.stringify(
      {
        result: evidence.result,
        evidence: evidencePath,
        approvals: approvals.length,
        toolCalls: evidence.toolCalls.length,
        checks: checks.map((entry) => ({
          name: entry.name,
          passed: entry.passed,
        })),
        error: errorMessage,
      },
      null,
      2,
    ),
  );
  process.exit(failed.length === 0 ? 0 : 1);
}

await main();
