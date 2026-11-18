import { act, render, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { McpWorldChangeHost } from "@/features/mcp/McpWorldChangeHost";
import { applyExternalWorldChange } from "@/editor/commands";
import { listenForMcpWorldChanges, type McpWorldChange } from "@/lib/mcp-access";

import type * as McpAccessModule from "@/lib/mcp-access";

vi.mock("@/editor/commands", () => ({
  applyExternalWorldChange: vi.fn(),
}));

vi.mock("@/lib/mcp-access", async (importOriginal) => {
  const actual = await importOriginal<typeof McpAccessModule>();
  return {
    ...actual,
    listenForMcpWorldChanges: vi.fn(),
  };
});

const listen = vi.mocked(listenForMcpWorldChanges);
const apply = vi.mocked(applyExternalWorldChange);

const change: McpWorldChange = {
  source: "mcp",
  action: "apply",
  projectId: "project-1",
  jobId: null,
  operationId: "op-1",
  worldRevision: 4,
  changedBlocks: 2,
  dirtyChunks: [{ x: 0, z: 0 }],
  reload: false,
};

let handler: ((change: McpWorldChange) => void) | null = null;

beforeEach(() => {
  handler = null;
  apply.mockClear();
  listen.mockReset();
  listen.mockImplementation(async (callback) => {
    handler = callback;
    return () => undefined;
  });
});

describe("MCP world change host", () => {
  it("applies a committed MCP write exactly once", async () => {
    render(<McpWorldChangeHost />);
    await waitFor(() => expect(listen).toHaveBeenCalledTimes(1));

    act(() => handler?.(change));

    expect(apply).toHaveBeenCalledTimes(1);
    expect(apply).toHaveBeenCalledWith(change);
  });

  it("stops listening when the editor unmounts", async () => {
    const stop = vi.fn();
    listen.mockImplementationOnce(async () => stop);

    const { unmount } = render(<McpWorldChangeHost />);
    await waitFor(() => expect(listen).toHaveBeenCalledTimes(1));
    unmount();

    expect(stop).toHaveBeenCalledTimes(1);
  });
});
