import { describe, expect, it } from "vitest";

import { evaluateContext } from "../src/context.js";

describe("explicit MCP request context", () => {
  it("reports desktop unavailability instead of inventing a project", () => {
    const evaluation = evaluateContext({ projectId: "project-1", worldRevision: 3 }, null);
    expect(evaluation).toMatchObject({
      state: "desktop_unavailable",
      valid: false,
      active: null,
    });
  });

  it("accepts a matching project and revision", () => {
    expect(
      evaluateContext(
        { projectId: "project-1", worldRevision: 3 },
        { projectId: "project-1", worldRevision: 3 },
      ),
    ).toMatchObject({ state: "ready", valid: true });
  });

  it("accepts an omitted revision for discovery reads", () => {
    expect(
      evaluateContext(
        { projectId: "project-1", worldRevision: null },
        { projectId: "project-1", worldRevision: 7 },
      ),
    ).toMatchObject({ state: "ready", valid: true });
  });

  it("rejects a different project without falling back to the transport session", () => {
    expect(
      evaluateContext(
        { projectId: "project-2", worldRevision: null },
        { projectId: "project-1", worldRevision: 7 },
      ),
    ).toMatchObject({
      state: "project_mismatch",
      valid: false,
      active: { projectId: "project-1", worldRevision: 7 },
    });
  });

  it("rejects a stale revision rather than reading newer data silently", () => {
    expect(
      evaluateContext(
        { projectId: "project-1", worldRevision: 6 },
        { projectId: "project-1", worldRevision: 7 },
      ),
    ).toMatchObject({
      state: "revision_mismatch",
      valid: false,
      active: { projectId: "project-1", worldRevision: 7 },
    });
  });
});
