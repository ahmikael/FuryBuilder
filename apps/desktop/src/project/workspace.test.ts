import { describe, expect, it } from "vitest";

import {
  withActiveLayer,
  withActiveMask,
  withLayerUpdate,
  withNewLayer,
  withNewRegion,
  withoutLayer,
  withoutRegion,
  workspaceSlug,
} from "@/project/workspace";
import { defaultWorkspacePolicy } from "@/project/types";

describe("workspace policy helpers", () => {
  it("creates stable unique ids without path characters", () => {
    expect(workspaceSlug("Details", [])).toBe("details");
    expect(workspaceSlug("Details", ["details"])).toBe("details-2");
    expect(workspaceSlug("  Roof / Trim  ", ["roof-trim"])).toBe("roof-trim-2");
    expect(workspaceSlug("!!!", [])).toBe("region");
    expect(workspaceSlug("1st floor", [])).toBe("1st-floor");
  });

  it("adds and activates a layer while keeping the old layers intact", () => {
    const policy = defaultWorkspacePolicy();
    const next = withNewLayer(policy, "Details");
    expect(next).not.toBeNull();
    expect(next?.layers.map((layer) => layer.id)).toEqual(["base", "details"]);
    expect(next?.activeLayerId).toBe("details");
    expect(policy.layers).toHaveLength(1);
  });

  it("updates visibility and lock without sharing array references", () => {
    const policy = defaultWorkspacePolicy();
    const locked = withLayerUpdate(policy, "base", { locked: true });
    expect(locked.layers[0].locked).toBe(true);
    expect(policy.layers[0].locked).toBe(false);
    expect(withActiveLayer(locked, "missing")).toBe(locked);
  });

  it("refuses to delete the last or active layer", () => {
    const policy = defaultWorkspacePolicy();
    expect(withoutLayer(policy, "base")).toBe(policy);
    const withDetails = withNewLayer(policy, "Details");
    expect(withDetails).not.toBeNull();
    const next = withDetails!;
    expect(withoutLayer(next, "details")).toBe(next);
    expect(withActiveLayer(next, "base").layers).toHaveLength(2);
    expect(withoutLayer(withActiveLayer(next, "base"), "details").layers).toHaveLength(1);
  });

  it("builds dimension-scoped masks and protected regions from a selection", () => {
    const policy = defaultWorkspacePolicy();
    const bounds = {
      min: { x: -4, y: 60, z: 2 },
      max: { x: 8, y: 72, z: 10 },
    };
    const masked = withNewRegion(policy, "mask", "Build area", "minecraft:overworld", bounds);
    expect(masked?.masks[0].bounds).toEqual(bounds);
    expect(masked?.activeMaskId).toBe("build-area");
    expect(withActiveMask(masked!, null).activeMaskId).toBeNull();
    expect(withActiveMask(masked!, "missing")).toBe(masked);

    const protectedPolicy = withNewRegion(
      policy,
      "protected",
      "Spawn",
      "minecraft:overworld",
      bounds,
    );
    expect(protectedPolicy?.protectedRegions[0].name).toBe("Spawn");
    expect(protectedPolicy?.activeMaskId).toBeNull();
    expect(withoutRegion(protectedPolicy!, "protected", "spawn").protectedRegions).toHaveLength(0);
  });

  it("rejects inverted bounds and an active mask that does not exist", () => {
    const policy = defaultWorkspacePolicy();
    const inverted = {
      min: { x: 1, y: 1, z: 1 },
      max: { x: 0, y: 0, z: 0 },
    };
    expect(
      withNewRegion(policy, "protected", "Broken", "minecraft:overworld", inverted),
    ).toBeNull();
    expect(
      withNewRegion(policy, "mask", "   ", "minecraft:overworld", {
        min: { x: 0, y: 0, z: 0 },
        max: { x: 1, y: 1, z: 1 },
      }),
    ).toBeNull();
  });
});
