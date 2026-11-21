import type { EditorOperation } from "@/editor/types";
import type { Bounds3 } from "@/renderer/types";

export type ValidationRuleId =
  | "floating_blocks"
  | "out_of_bounds"
  | "path_continuity"
  | "palette"
  | "export_compatibility"
  | "block_state_orientation"
  | "protected_region";

export type ValidationSeverity = "info" | "warning" | "error";
export type ValidationConfidence = "certain" | "heuristic";
export type FloatingFix = "none" | "remove" | "support";

export interface ValidationPathPoint {
  x: number;
  z: number;
}

export interface ValidationPath {
  from: ValidationPathPoint;
  to: ValidationPathPoint;
  baseY: number;
  width: number;
  material: { name: string; properties: Record<string, string> };
}

export interface ValidationQuery {
  dimension: string;
  bounds: Bounds3;
  rules?: ValidationRuleId[];
  allowedBounds?: Bounds3;
  path?: ValidationPath;
  maxSurfaceMaterials?: number;
  floatingFix?: FloatingFix;
}

export interface ValidationSuggestedFix {
  description: string;
  operation: EditorOperation;
}

export interface ValidationFinding {
  ruleId: ValidationRuleId;
  severity: ValidationSeverity;
  confidence: ValidationConfidence;
  message: string;
  bounds: Bounds3;
  positions: { x: number; y: number; z: number }[];
  positionCount: number;
  suggestedFix?: ValidationSuggestedFix;
}

export interface ValidationCounts {
  error: number;
  warning: number;
  info: number;
}

/** Read-only findings at an exact revision; fixes are ordinary operations, not writes. */
export interface ValidationReport {
  schemaVersion: number;
  dimension: string;
  worldRevision: number;
  bounds: Bounds3;
  rules: ValidationRuleId[];
  findings: ValidationFinding[];
  counts: ValidationCounts;
  truncated: boolean;
  scannedBlocks: number;
  warnings: string[];
  notes: string[];
}

export const VALIDATION_RULES: {
  value: ValidationRuleId;
  label: string;
  description: string;
  optional?: boolean;
}[] = [
  {
    value: "floating_blocks",
    label: "Floating blocks",
    description: "Structural blocks with air directly below (heuristic).",
  },
  {
    value: "palette",
    label: "Palette diversity",
    description: "Surface materials above the configured guideline.",
  },
  {
    value: "export_compatibility",
    label: "Export compatibility",
    description: "States outside the verified target catalog.",
  },
  {
    value: "block_state_orientation",
    label: "Block orientation",
    description: "Missing or invalid facing, half, hinge or axis properties.",
  },
  {
    value: "protected_region",
    label: "Protected regions",
    description: "Region conflicts enforced by the workspace policy.",
  },
  {
    value: "path_continuity",
    label: "Path corridor",
    description: "Gaps and obstructions along a declared surface path.",
    optional: true,
  },
];

export const DEFAULT_VALIDATION_RULES: ValidationRuleId[] = [
  "floating_blocks",
  "palette",
  "export_compatibility",
  "block_state_orientation",
  "protected_region",
];

export function validationRuleLabel(rule: ValidationRuleId): string {
  return VALIDATION_RULES.find((entry) => entry.value === rule)?.label ?? rule;
}

export function validationBoundsLabel(bounds: Bounds3): string {
  const { min, max } = bounds;
  return `(${min.x},${min.y},${min.z})..(${max.x},${max.y},${max.z})`;
}

export function severityClass(severity: ValidationSeverity): string {
  switch (severity) {
    case "error":
      return "text-status-error";
    case "warning":
      return "text-status-warn";
    default:
      return "text-muted-foreground";
  }
}
