import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

interface Capability {
  identifier: string;
  permissions: string[];
  windows?: string[];
}

function readCapability(): Capability {
  const path = resolve(process.cwd(), "src-tauri/capabilities/default.json");
  return JSON.parse(readFileSync(path, "utf8")) as Capability;
}

describe("tauri capability policy", () => {
  it("grants core defaults, the native folder picker and only required update permissions", () => {
    const capability = readCapability();
    expect(capability.windows).toEqual(["main"]);
    expect(capability.permissions).toEqual([
      "core:default",
      "dialog:allow-open",
      "process:allow-restart",
      "updater:default",
    ]);
    expect(
      capability.permissions.some((permission) => permission.startsWith("dialog:allow-save")),
    ).toBe(false);
    expect(capability.permissions.some((permission) => permission.startsWith("fs:"))).toBe(false);
    expect(capability.permissions.some((permission) => permission.startsWith("shell:"))).toBe(
      false,
    );
    expect(capability.permissions.some((permission) => permission.startsWith("http:"))).toBe(false);
  });

  it("declares a restrictive production CSP", () => {
    const path = resolve(process.cwd(), "src-tauri/tauri.conf.json");
    const config = JSON.parse(readFileSync(path, "utf8")) as {
      app: { security: { csp: string } };
    };
    const csp = config.app.security.csp;
    expect(csp).toContain("default-src 'self'");
    expect(csp).toContain("connect-src ipc: http://ipc.localhost");
    expect(csp).not.toContain("0.0.0.0");
    expect(csp).not.toContain("unsafe-eval");
  });
});
