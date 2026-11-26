import { describe, expect, it } from "vitest";
import {
  capabilityDisabledReason,
  projectCreationDisabledReason,
  supportBadgeLabel,
  supportBadgeVariant,
  unavailableCapabilities,
} from "@/project/version-support";
import type { SupportLevel, VersionCapabilities } from "@/project/version-types";

const full: VersionCapabilities = {
  supportLevel: "full",
  read: true,
  edit: true,
  write: true,
  export: true,
  generate: true,
  assets: true,
};

const readOnly: VersionCapabilities = {
  supportLevel: "read-only",
  read: true,
  edit: false,
  write: false,
  export: false,
  generate: false,
  assets: false,
};

const convert: VersionCapabilities = {
  supportLevel: "convert",
  read: true,
  edit: false,
  write: false,
  export: false,
  generate: false,
  assets: false,
};

const unverified: VersionCapabilities = {
  supportLevel: "unverified",
  read: false,
  edit: false,
  write: false,
  export: false,
  generate: false,
  assets: false,
};

describe("version support badges", () => {
  it("labels every backend support level, including unverified profiles", () => {
    const levels: SupportLevel[] = [
      "full",
      "read-write-subset",
      "read-only",
      "convert",
      "unverified",
      "out-of-scope",
    ];
    expect(levels.map(supportBadgeLabel)).toEqual([
      "Full support",
      "Read + write subset",
      "Read-only",
      "Convert only",
      "Unverified",
      "Out of scope",
    ]);
    expect(supportBadgeLabel(null)).toBe("No verified profile");
  });

  it("uses a distinct badge treatment for every level", () => {
    expect(supportBadgeVariant("full")).toBe("default");
    expect(supportBadgeVariant("read-write-subset")).toBe("secondary");
    expect(supportBadgeVariant("read-only")).toBe("secondary");
    expect(supportBadgeVariant("convert")).toBe("secondary");
    expect(supportBadgeVariant("unverified")).toBe("outline");
    expect(supportBadgeVariant("out-of-scope")).toBe("destructive");
    expect(supportBadgeVariant(null)).toBe("outline");
  });
});

describe("capability disabled reasons", () => {
  it("allows every action when the profile verifies it", () => {
    for (const action of ["read", "edit", "write", "export", "generate", "assets"] as const) {
      expect(capabilityDisabledReason(full, action)).toBeNull();
    }
    expect(unavailableCapabilities(full)).toEqual([]);
  });

  it("explains read-only editing and saving without implying a missing adapter", () => {
    expect(capabilityDisabledReason(readOnly, "read")).toBeNull();
    expect(capabilityDisabledReason(readOnly, "edit")).toContain("Read-only");
    expect(capabilityDisabledReason(readOnly, "write")).toContain("Read-only");
    expect(capabilityDisabledReason(readOnly, "generate")).toContain("Vanilla generation");
    expect(unavailableCapabilities(readOnly).map((entry) => entry.label)).toEqual([
      "Edit",
      "Save",
      "Export",
      "Generate",
      "Assets",
    ]);
  });

  it("explains that convert-only profiles cannot be edited or saved in place", () => {
    expect(capabilityDisabledReason(convert, "read")).toBeNull();
    expect(capabilityDisabledReason(convert, "edit")).toContain("Convert only");
    expect(capabilityDisabledReason(convert, "write")).toContain("Convert only");
  });

  it("never promises an operation without a verified profile", () => {
    expect(capabilityDisabledReason(null, "read")).toContain("no verified profile");
    expect(capabilityDisabledReason(unverified, "read")).toContain("No verified world adapter");
    expect(unavailableCapabilities(unverified)).toHaveLength(6);
  });

  it("blocks project creation before the backend is asked", () => {
    expect(projectCreationDisabledReason(full, "empty")).toBeNull();
    expect(projectCreationDisabledReason(full, "vanilla")).toBeNull();
    expect(projectCreationDisabledReason(readOnly, "empty")).toContain("Read-only");
    expect(projectCreationDisabledReason(unverified, "flat")).toContain(
      "No verified world adapter",
    );
    expect(projectCreationDisabledReason({ ...full, generate: false }, "vanilla")).toContain(
      "Vanilla generation",
    );
    expect(projectCreationDisabledReason(null, "import")).toContain("no verified profile");
  });
});
