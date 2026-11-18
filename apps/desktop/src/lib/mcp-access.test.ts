import { beforeEach, describe, expect, it, vi } from "vitest";

import { invokeCommand } from "@/lib/ipc";
import {
  describeAccessPolicy,
  getMcpAccessStatus,
  MCP_ACCESS_STATUS_COMMAND,
  SET_MCP_ACCESS_COMMAND,
  setMcpAccess,
  SUBMIT_MCP_CONFIRMATION_COMMAND,
  submitMcpConfirmation,
  type McpAccessPolicy,
} from "@/lib/mcp-access";

vi.mock("@/lib/ipc", () => ({
  invokeCommand: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => undefined),
}));

const invoke = vi.mocked(invokeCommand);

const readOnlyPolicy: McpAccessPolicy = {
  mode: "read_only",
  selection: null,
  maxBlocks: 50000,
  maxBatchChanges: 4096,
  requireConfirmation: true,
  writesEnabled: false,
};

beforeEach(() => {
  invoke.mockClear();
});

describe("MCP access client", () => {
  it("sends the full policy as a single command argument", async () => {
    invoke.mockResolvedValueOnce(readOnlyPolicy);
    const policy = await setMcpAccess({
      mode: "edit_in_selection",
      selection: {
        dimension: "minecraft:overworld",
        bounds: {
          min: { x: 0, y: 60, z: 0 },
          max: { x: 15, y: 75, z: 15 },
        },
      },
      maxBlocks: 4000,
      maxBatchChanges: 100,
      requireConfirmation: true,
    });

    expect(invoke).toHaveBeenCalledWith(SET_MCP_ACCESS_COMMAND, {
      input: {
        mode: "edit_in_selection",
        selection: {
          dimension: "minecraft:overworld",
          bounds: {
            min: { x: 0, y: 60, z: 0 },
            max: { x: 15, y: 75, z: 15 },
          },
        },
        maxBlocks: 4000,
        maxBatchChanges: 100,
        requireConfirmation: true,
      },
    });
    expect(policy).toEqual(readOnlyPolicy);
  });

  it("reads the policy and pending confirmations", async () => {
    invoke.mockResolvedValueOnce({
      policy: readOnlyPolicy,
      pendingConfirmations: [],
    });
    const status = await getMcpAccessStatus();
    expect(invoke).toHaveBeenCalledWith(MCP_ACCESS_STATUS_COMMAND);
    expect(status.policy.writesEnabled).toBe(false);
  });

  it("submits an explicit approve or deny answer", async () => {
    await submitMcpConfirmation("request-1", true);
    expect(invoke).toHaveBeenCalledWith(SUBMIT_MCP_CONFIRMATION_COMMAND, {
      requestId: "request-1",
      approved: true,
    });
  });

  it("describes read-only, selection and full-project policies", () => {
    expect(describeAccessPolicy(readOnlyPolicy)).toBe("MCP read-only");
    expect(
      describeAccessPolicy({
        ...readOnlyPolicy,
        mode: "edit_in_selection",
        selection: {
          dimension: "minecraft:overworld",
          bounds: {
            min: { x: 0, y: 60, z: 0 },
            max: { x: 15, y: 75, z: 15 },
          },
        },
        writesEnabled: true,
      }),
    ).toContain("minecraft:overworld");
    expect(
      describeAccessPolicy({
        ...readOnlyPolicy,
        mode: "full_project",
        writesEnabled: true,
      }),
    ).toBe("MCP edit · full project");
  });
});
