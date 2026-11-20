import { createServer, type Server, type Socket } from "node:net";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { randomUUID } from "node:crypto";
import { tmpdir } from "node:os";
import { join } from "node:path";

/** A local-socket stand-in for the Tauri endpoint, used by bridge contract tests. */
export interface MockDesktop {
  runtimePath: string;
  token: string;
  directory: string;
  close: () => Promise<void>;
}

export async function startMockDesktop(
  handler: (request: Record<string, unknown>) => unknown,
  options: { respond?: boolean } = {},
): Promise<MockDesktop> {
  const directory = await mkdtemp(join(tmpdir(), "furybuilder-desktop-"));
  const token = "a".repeat(64);
  const endpoint =
    process.platform === "win32"
      ? `\\\\.\\pipe\\furybuilder-test-${randomUUID()}`
      : join(directory, "bridge.sock");
  const sockets = new Set<Socket>();
  const server: Server = createServer((socket) => {
    sockets.add(socket);
    socket.once("close", () => sockets.delete(socket));
    let buffer = "";
    socket.setEncoding("utf8");
    socket.on("data", (chunk: string) => {
      buffer += chunk;
      const newline = buffer.indexOf("\n");
      if (newline < 0) return;
      const request = JSON.parse(buffer.slice(0, newline)) as Record<string, unknown>;
      if (options.respond === false) return;
      socket.end(`${JSON.stringify(handler(request))}\n`);
    });
  });
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(endpoint, () => resolve());
  });
  const runtimePath = join(directory, "runtime.json");
  await writeFile(
    runtimePath,
    JSON.stringify({
      schemaVersion: 1,
      transport: "local_socket",
      endpoint,
      token,
      pid: 4242,
      protocolVersion: 1,
      appVersion: "0.1.0",
      startedAtMs: 1_700_000_000_000,
    }),
  );
  return {
    runtimePath,
    token,
    directory,
    close: async () => {
      for (const socket of sockets) socket.destroy();
      await new Promise<void>((resolve) => server.close(() => resolve()));
      await rm(directory, { recursive: true, force: true });
    },
  };
}

export function okResult(request: Record<string, unknown>, result: unknown): unknown {
  return { protocol: 1, id: request.id, ok: true, result };
}
