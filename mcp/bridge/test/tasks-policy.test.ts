import { describe, expect, it } from "vitest";

import { resolveTasksSupport } from "../src/capabilities.js";

describe("MCP Tasks option", () => {
  it("falls back when the client does not declare Tasks", () => {
    expect(
      resolveTasksSupport({
        clientCapabilities: { roots: {} },
        jobBackendConnected: true,
        bridgeImplementsTasks: true,
      }),
    ).toEqual({
      mode: "fallback",
      reason: "client_did_not_declare_tasks",
      extensionAdvertised: false,
    });
  });

  it("never advertises Tasks that the bridge has not implemented", () => {
    expect(
      resolveTasksSupport({
        clientCapabilities: { tasks: { cancel: {} } },
        jobBackendConnected: true,
        bridgeImplementsTasks: false,
      }),
    ).toEqual({
      mode: "fallback",
      reason: "tasks_not_implemented",
      extensionAdvertised: false,
    });
  });

  it("falls back when the authoritative Job API is unavailable", () => {
    expect(
      resolveTasksSupport({
        clientCapabilities: { tasks: { cancel: {} } },
        jobBackendConnected: false,
        bridgeImplementsTasks: true,
      }),
    ).toEqual({
      mode: "fallback",
      reason: "job_backend_unavailable",
      extensionAdvertised: false,
    });
  });

  it("enables Tasks only when the client, the bridge and the Job API all support it", () => {
    expect(
      resolveTasksSupport({
        clientCapabilities: { tasks: { cancel: {} } },
        jobBackendConnected: true,
        bridgeImplementsTasks: true,
      }),
    ).toEqual({
      mode: "tasks",
      reason: "tasks_negotiated",
      extensionAdvertised: true,
    });
  });
});
