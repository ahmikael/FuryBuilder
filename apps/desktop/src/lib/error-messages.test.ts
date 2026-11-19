import { describe, expect, it } from "vitest";
import { presentError, describeError } from "@/lib/error-messages";
import { BackendError } from "@/lib/ipc";

describe("error presentation", () => {
  it("adds actionable hints without replacing the backend message", () => {
    const presented = presentError(
      new BackendError("out_of_bounds", "Y=512 is outside the world height"),
    );
    expect(presented.code).toBe("out_of_bounds");
    expect(presented.message).toContain("Y=512");
    expect(presented.hint).toContain("world height");
  });

  it("explains the browser fallback for backend_unavailable", () => {
    const presented = presentError(
      new BackendError("backend_unavailable", "The Rust backend is not available"),
    );
    expect(presented.hint).toContain("pnpm tauri dev");
  });

  it("keeps unknown failures safe and honest", () => {
    const presented = presentError("plain failure");
    expect(presented.code).toBe("unknown");
    expect(presented.hint).toContain("diagnostics");
    expect(describeError("plain failure")).toContain("plain failure");
  });
});
