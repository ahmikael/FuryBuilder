import {
  SUPPORT_LEVEL_LABELS,
  type SupportLevel,
  type VersionCapabilities,
} from "@/project/version-types";

export type SupportBadgeVariant = "default" | "secondary" | "outline" | "destructive";

/** One badge treatment per support level; the badge is a fact, not a decoration. */
export const SUPPORT_BADGE_VARIANTS: Record<SupportLevel, SupportBadgeVariant> = {
  full: "default",
  "read-write-subset": "secondary",
  "read-only": "secondary",
  convert: "secondary",
  unverified: "outline",
  "out-of-scope": "destructive",
};

/** Plain-language meaning of each support level, shared by cards and project settings. */
export const SUPPORT_LEVEL_DESCRIPTIONS: Record<SupportLevel, string> = {
  full: "Create, read, edit, save, export and generate with verified codecs and fixtures.",
  "read-write-subset":
    "Read, edit and save a verified subset; untouched data is preserved byte for byte.",
  "read-only": "Analyze and copy worlds of this version without editing or saving them.",
  convert: "Read the world and convert it to a supported target; the original is preserved.",
  unverified:
    "Exact release metadata is known, but this build has no verified world adapter for it.",
  "out-of-scope": "The format is outside FuryBuilder's supported Java world scope.",
};

export const VERSION_ACTIONS = ["read", "edit", "write", "export", "generate", "assets"] as const;

export type VersionAction = (typeof VERSION_ACTIONS)[number];

export const VERSION_ACTION_LABELS: Record<VersionAction, string> = {
  read: "Read",
  edit: "Edit",
  write: "Save",
  export: "Export",
  generate: "Generate",
  assets: "Assets",
};

export function supportBadgeVariant(level: SupportLevel | null | undefined): SupportBadgeVariant {
  return level ? SUPPORT_BADGE_VARIANTS[level] : "outline";
}

export function supportBadgeLabel(level: SupportLevel | null | undefined): string {
  return level ? SUPPORT_LEVEL_LABELS[level] : "No verified profile";
}

/**
 * Why an action is unavailable, or `null` when the profile allows it. Reasons depend on
 * the support level so a read-only version never gets the same explanation as an
 * unverified one.
 */
export function capabilityDisabledReason(
  capabilities: VersionCapabilities | null,
  action: VersionAction,
): string | null {
  if (!capabilities) {
    return "This build has no verified profile for this project version.";
  }
  if (capabilities[action]) return null;

  if (capabilities.supportLevel === "unverified" || capabilities.supportLevel === "out-of-scope") {
    return "No verified world adapter exists for this version, so this action is unavailable.";
  }

  switch (action) {
    case "read":
      return "World data of this version cannot be read by this build.";
    case "edit":
      return capabilities.supportLevel === "read-only"
        ? "Read-only: the world can be viewed and copied, but blocks and biomes cannot be edited."
        : capabilities.supportLevel === "convert"
          ? "Convert only: the world is read and converted to a supported target instead of being edited here."
          : "Editing is not verified for this version.";
    case "write":
      return capabilities.supportLevel === "read-only"
        ? "Read-only: saving a modified project is disabled for this version."
        : capabilities.supportLevel === "convert"
          ? "Convert only: saving directly in this version is disabled; the original stays unchanged."
          : "Saving is not verified for this version.";
    case "export":
      return "Same-version world export is not verified for this version.";
    case "generate":
      return "Vanilla generation is not available for this version; create an Empty, Flat or Imported project instead.";
    case "assets":
      return "A verified client-asset catalog is not available for this version.";
  }
}

export interface UnavailableCapability {
  action: VersionAction;
  label: string;
  reason: string;
}

/** Every disabled capability with the reason shown in the UI. */
export function unavailableCapabilities(
  capabilities: VersionCapabilities | null,
): UnavailableCapability[] {
  return VERSION_ACTIONS.flatMap((action) => {
    const reason = capabilityDisabledReason(capabilities, action);
    return reason ? [{ action, label: VERSION_ACTION_LABELS[action], reason }] : [];
  });
}

/**
 * Blocks project creation before the backend is asked. Imported, Empty and Flat projects
 * need read/edit/write; Vanilla projects additionally need verified generation.
 */
export function projectCreationDisabledReason(
  capabilities: VersionCapabilities | null,
  mode: "empty" | "flat" | "vanilla" | "import",
): string | null {
  if (!capabilities) {
    return "This build has no verified profile for the selected Minecraft version.";
  }
  for (const action of ["read", "edit", "write"] as const) {
    const reason = capabilityDisabledReason(capabilities, action);
    if (reason) return reason;
  }
  if (mode === "vanilla") {
    return capabilityDisabledReason(capabilities, "generate");
  }
  return null;
}
