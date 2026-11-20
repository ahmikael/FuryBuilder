import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { McpConfirmationHost } from "@/features/mcp/McpConfirmationHost";
import {
  getMcpAccessStatus,
  listenForMcpConfirmations,
  submitMcpConfirmation,
  type McpConfirmationRequest,
} from "@/lib/mcp-access";

import type * as McpAccessModule from "@/lib/mcp-access";
import type * as IpcModule from "@/lib/ipc";

vi.mock("@/lib/ipc", async (importOriginal) => {
  const actual = await importOriginal<typeof IpcModule>();
  return { ...actual, isTauriRuntime: () => true };
});

vi.mock("@/lib/mcp-access", async (importOriginal) => {
  const actual = await importOriginal<typeof McpAccessModule>();
  return {
    ...actual,
    getMcpAccessStatus: vi.fn(),
    listenForMcpConfirmations: vi.fn(),
    submitMcpConfirmation: vi.fn(),
  };
});

const listen = vi.mocked(listenForMcpConfirmations);
const submit = vi.mocked(submitMcpConfirmation);
const status = vi.mocked(getMcpAccessStatus);

const request: McpConfirmationRequest = {
  requestId: "request-1",
  expiresAtMs: Date.now() + 60_000,
  summary: {
    kind: "apply",
    projectId: "project-1",
    worldRevision: 4,
    accessMode: "edit_in_selection",
    operation: {
      type: "set_block",
      dimension: "minecraft:overworld",
      bounds: {
        min: { x: 1, y: 64, z: 1 },
        max: { x: 1, y: 64, z: 1 },
      },
      pos: { x: 1, y: 64, z: 1 },
      state: "minecraft:stone",
    },
    affectedBlocks: 1,
    changedBlocks: 1,
    dirtyChunks: 1,
    warnings: [],
    idempotencyKey: "edit-1",
  },
};

let handler: ((request: McpConfirmationRequest) => void) | null = null;

beforeEach(() => {
  handler = null;
  listen.mockImplementation(async (callback) => {
    handler = callback;
    return () => undefined;
  });
  submit.mockResolvedValue(undefined);
  status.mockResolvedValue({
    policy: {
      mode: "read_only",
      selection: null,
      maxBlocks: 50000,
      maxBatchChanges: 4096,
      requireConfirmation: true,
      writesEnabled: false,
    },
    pendingConfirmations: [],
  });
});

describe("MCP confirmation host", () => {
  it("shows the real operation and approves only after a click", async () => {
    const user = userEvent.setup();
    render(<McpConfirmationHost />);
    await waitFor(() => expect(listen).toHaveBeenCalled());

    expect(screen.queryByText("Approve an AI edit?")).not.toBeInTheDocument();
    act(() => handler?.(request));

    expect(await screen.findByText("Approve an AI edit?")).toBeInTheDocument();
    expect(screen.getByText("minecraft:stone")).toBeInTheDocument();
    expect(screen.getByText("1, 64, 1")).toBeInTheDocument();
    expect(submit).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "Approve and apply" }));
    expect(submit).toHaveBeenCalledWith("request-1", true);
  });

  it("sends a deny answer without approving the write", async () => {
    const user = userEvent.setup();
    render(<McpConfirmationHost />);
    await waitFor(() => expect(listen).toHaveBeenCalled());

    act(() => handler?.(request));
    await user.click(await screen.findByRole("button", { name: "Deny" }));

    expect(submit).toHaveBeenCalledWith("request-1", false);
  });

  it("restores a pending confirmation after the UI reloads", async () => {
    status.mockResolvedValueOnce({
      policy: {
        mode: "edit_in_selection",
        selection: null,
        maxBlocks: 50000,
        maxBatchChanges: 4096,
        requireConfirmation: true,
        writesEnabled: true,
      },
      pendingConfirmations: [request],
    });

    render(<McpConfirmationHost />);

    expect(await screen.findByText("Approve an AI edit?")).toBeInTheDocument();
    expect(screen.getByText("request-1", { exact: false })).toBeInTheDocument();
  });
});
