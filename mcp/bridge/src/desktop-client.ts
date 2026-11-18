import { createConnection } from "node:net";
import { readFile } from "node:fs/promises";
import { homedir } from "node:os";
import { join } from "node:path";
import { randomUUID } from "node:crypto";

/**
 * Client for the authenticated local IPC endpoint owned by the running FuryBuilder
 * desktop process. It never opens project or world files: every call reaches the same
 * authoritative Application API as the UI.
 */

export const DESKTOP_PROTOCOL_VERSION = 1;
export const DESKTOP_DISCOVERY_SCHEMA_VERSION = 1;

const MAX_RUNTIME_FILE_BYTES = 16 * 1024;
// A PNG viewport capture is base64-encoded inside one JSON response. The Tauri endpoint
// caps the image at 12 MiB; leave room for metadata and JSON quoting.
const MAX_RESPONSE_BYTES = 16 * 1024 * 1024;
const DEFAULT_TIMEOUT_MS = 5_000;

export interface DesktopRuntime {
  schemaVersion: number;
  transport: string;
  endpoint: string;
  token: string;
  pid: number;
  protocolVersion: number;
  appVersion: string;
  startedAtMs: number;
}

export type DesktopUnavailableReason =
  | "runtime_file_missing"
  | "invalid_runtime_file"
  | "app_not_running"
  | "authentication_failed"
  | "protocol_mismatch"
  | "timeout"
  | "invalid_response"
  | "desktop_error";

export class DesktopUnavailableError extends Error {
  readonly reason: DesktopUnavailableReason;
  readonly code: string | null;

  constructor(
    reason: DesktopUnavailableReason,
    message: string,
    code: string | null = null,
  ) {
    super(message);
    this.name = "DesktopUnavailableError";
    this.reason = reason;
    this.code = code;
  }
}

export interface DesktopProbeConnected {
  connected: true;
  app: unknown;
  project: {
    projectId: string;
    name: string | null;
    worldRevision: number;
  };
  bridge: {
    protocolVersion: number;
    transport: string;
    pid: number;
  };
}

export interface DesktopProbeDisconnected {
  connected: false;
  reason: DesktopUnavailableReason;
  message: string;
}

export type DesktopProbe = DesktopProbeConnected | DesktopProbeDisconnected;

/** Resolves the discovery file without trusting an arbitrary user-provided path. */
export function runtimeFilePath(env: NodeJS.ProcessEnv = process.env): string {
  if (env.FURYBUILDER_MCP_RUNTIME) return env.FURYBUILDER_MCP_RUNTIME;

  const bundle = "com.furybuilder.desktop";
  if (process.platform === "win32") {
    const appData = env.APPDATA;
    if (!appData) {
      throw new DesktopUnavailableError(
        "runtime_file_missing",
        "APPDATA is not defined, so the FuryBuilder desktop endpoint cannot be discovered",
      );
    }
    return join(appData, bundle, "mcp", "runtime.json");
  }
  if (process.platform === "darwin") {
    return join(
      homedir(),
      "Library",
      "Application Support",
      bundle,
      "mcp",
      "runtime.json",
    );
  }
  const dataHome = env.XDG_DATA_HOME ?? join(homedir(), ".local", "share");
  return join(dataHome, bundle, "mcp", "runtime.json");
}

export async function readDesktopRuntime(
  path = runtimeFilePath(),
): Promise<DesktopRuntime> {
  let bytes: Buffer;
  try {
    bytes = await readFile(path);
  } catch (error) {
    const code = (error as NodeJS.ErrnoException).code;
    if (code === "ENOENT") {
      throw new DesktopUnavailableError(
        "runtime_file_missing",
        "FuryBuilder is not running: start the desktop app to enable MCP tools",
      );
    }
    throw new DesktopUnavailableError(
      "runtime_file_missing",
      `The FuryBuilder desktop endpoint could not be read (${code ?? "unknown error"})`,
    );
  }
  if (bytes.length > MAX_RUNTIME_FILE_BYTES) {
    throw new DesktopUnavailableError(
      "invalid_runtime_file",
      "The FuryBuilder desktop endpoint file is larger than expected",
    );
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(bytes.toString("utf8"));
  } catch {
    throw new DesktopUnavailableError(
      "invalid_runtime_file",
      "The FuryBuilder desktop endpoint file is not valid JSON; restart the app",
    );
  }
  return validateRuntime(parsed);
}

function validateRuntime(value: unknown): DesktopRuntime {
  if (typeof value !== "object" || value === null) {
    throw new DesktopUnavailableError(
      "invalid_runtime_file",
      "The FuryBuilder desktop endpoint file has no object payload",
    );
  }
  const record = value as Record<string, unknown>;
  const endpoint = record.endpoint;
  const token = record.token;
  if (
    record.schemaVersion !== DESKTOP_DISCOVERY_SCHEMA_VERSION ||
    record.protocolVersion !== DESKTOP_PROTOCOL_VERSION ||
    record.transport !== "local_socket" ||
    typeof endpoint !== "string" ||
    endpoint.length === 0 ||
    endpoint.length > 512 ||
    typeof token !== "string" ||
    token.length < 32 ||
    token.length > 256 ||
    typeof record.pid !== "number" ||
    !Number.isInteger(record.pid) ||
    record.pid <= 0 ||
    typeof record.appVersion !== "string" ||
    typeof record.startedAtMs !== "number"
  ) {
    throw new DesktopUnavailableError(
      "invalid_runtime_file",
      "The FuryBuilder desktop endpoint file does not match the supported schema; restart the app",
    );
  }
  return {
    schemaVersion: record.schemaVersion,
    transport: record.transport,
    endpoint,
    token,
    pid: record.pid,
    protocolVersion: record.protocolVersion,
    appVersion: record.appVersion,
    startedAtMs: record.startedAtMs,
  };
}

interface WireResponse {
  protocol: number;
  id: string;
  ok: boolean;
  result?: unknown;
  error?: { code?: string; message?: string };
}

export interface CallDesktopOptions {
  runtimePath?: string;
  runtime?: DesktopRuntime;
  timeoutMs?: number;
}

/** Sends one request over a fresh local connection and awaits exactly one response. */
export async function callDesktop(
  method: string,
  params: unknown = {},
  options: CallDesktopOptions = {},
): Promise<unknown> {
  const runtime =
    options.runtime ?? (await readDesktopRuntime(options.runtimePath));
  const timeoutMs = options.timeoutMs ?? DEFAULT_TIMEOUT_MS;
  const id = randomUUID();
  const request = JSON.stringify({
    protocol: DESKTOP_PROTOCOL_VERSION,
    id,
    token: runtime.token,
    method,
    params,
  });

  return new Promise<unknown>((resolve, reject) => {
    let settled = false;
    let received = Buffer.alloc(0);
    const socket = createConnection(runtime.endpoint);

    const finish = (action: () => void) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      socket.destroy();
      action();
    };
    const fail = (
      reason: DesktopUnavailableReason,
      message: string,
      code: string | null = null,
    ) =>
      finish(() => reject(new DesktopUnavailableError(reason, message, code)));

    const timer = setTimeout(
      () =>
        fail("timeout", `FuryBuilder did not answer within ${timeoutMs} ms`),
      timeoutMs,
    );

    socket.once("connect", () => {
      socket.write(request);
      socket.write("\n");
    });
    socket.on("data", (chunk: Buffer) => {
      received = Buffer.concat([received, chunk]);
      if (received.length > MAX_RESPONSE_BYTES) {
        fail("invalid_response", "FuryBuilder returned an oversized response");
        return;
      }
      const newline = received.indexOf(0x0a);
      if (newline < 0) return;
      const line = received.subarray(0, newline).toString("utf8");
      let response: WireResponse;
      try {
        response = JSON.parse(line) as WireResponse;
      } catch {
        fail("invalid_response", "FuryBuilder returned malformed JSON");
        return;
      }
      if (
        typeof response !== "object" ||
        response === null ||
        response.protocol !== DESKTOP_PROTOCOL_VERSION ||
        response.id !== id ||
        typeof response.ok !== "boolean"
      ) {
        fail(
          "invalid_response",
          "FuryBuilder returned an unexpected bridge response",
        );
        return;
      }
      if (!response.ok) {
        const code =
          typeof response.error?.code === "string" ? response.error.code : null;
        if (code === "unauthorized") {
          fail(
            "authentication_failed",
            "The FuryBuilder desktop endpoint rejected the bridge token; restart both",
            code,
          );
          return;
        }
        if (code === "unsupported_version") {
          fail(
            "protocol_mismatch",
            "The desktop app and MCP bridge protocol versions do not match",
            code,
          );
          return;
        }
        const message =
          response.error?.message ?? "FuryBuilder rejected the bridge request";
        fail("desktop_error", message, code);
        return;
      }
      finish(() => resolve(response.result));
    });
    socket.once("error", (error: NodeJS.ErrnoException) => {
      if (error.code === "ENOENT" || error.code === "ECONNREFUSED") {
        fail(
          "app_not_running",
          "FuryBuilder is not running or its local endpoint has closed; start the desktop app",
          error.code,
        );
      } else {
        fail(
          "desktop_error",
          `The FuryBuilder desktop endpoint failed (${error.code ?? "unknown error"})`,
          error.code ?? null,
        );
      }
    });
    socket.once("end", () => {
      if (!settled) {
        fail(
          "invalid_response",
          "FuryBuilder closed the connection without a response",
        );
      }
    });
  });
}

/** Non-failing diagnostic used by `bridge_info`; no token or local path is exposed. */
export async function probeDesktop(
  options: CallDesktopOptions = {},
): Promise<DesktopProbe> {
  try {
    const result = (await callDesktop(
      "ping",
      {},
      { ...options, timeoutMs: 2_000 },
    )) as {
      app?: unknown;
      project?: {
        projectId?: unknown;
        name?: unknown;
        worldRevision?: unknown;
      };
      bridge?: {
        protocolVersion?: unknown;
        transport?: unknown;
        pid?: unknown;
      };
    };
    if (
      typeof result !== "object" ||
      result === null ||
      typeof result.project?.projectId !== "string" ||
      typeof result.project.worldRevision !== "number" ||
      typeof result.bridge?.protocolVersion !== "number"
    ) {
      return {
        connected: false,
        reason: "invalid_response",
        message: "FuryBuilder returned an incomplete ping response",
      };
    }
    return {
      connected: true,
      app: result.app ?? null,
      project: {
        projectId: result.project.projectId,
        name:
          typeof result.project.name === "string" ? result.project.name : null,
        worldRevision: result.project.worldRevision,
      },
      bridge: {
        protocolVersion: result.bridge.protocolVersion,
        transport:
          typeof result.bridge.transport === "string"
            ? result.bridge.transport
            : "unknown",
        pid: typeof result.bridge.pid === "number" ? result.bridge.pid : 0,
      },
    };
  } catch (error) {
    if (error instanceof DesktopUnavailableError) {
      return { connected: false, reason: error.reason, message: error.message };
    }
    return {
      connected: false,
      reason: "desktop_error",
      message: "The FuryBuilder desktop endpoint could not be reached",
    };
  }
}
