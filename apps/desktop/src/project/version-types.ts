/** Stable support levels exposed by the backend version profile. */
export type SupportLevel = "full" | "read-write-subset" | "read-only" | "convert" | "out-of-scope";

export type VersionType = "release" | "snapshot" | "old_beta" | "old_alpha";

export type WorldLayout = "classic-anvil" | "split-anvil" | "mc-region" | "unknown";

export interface VersionDimension {
  id: string;
  kind: "overworld" | "nether" | "end" | "custom";
  minY: number;
  height: number;
  folder: string;
}

export interface VersionJavaRequirement {
  minimumMajor: number;
  recommendedMajor: number;
}

export interface VersionContentCatalog {
  registryFormat: "named" | "numeric-ids" | "legacy" | "unknown";
  blockstateFormat: "json" | "legacy-metadata" | "none" | "unknown";
  itemModelFormat: "item-definitions" | "item-models" | "legacy-metadata" | "none" | "unknown";
  creativeCategories: boolean;
  dataComponents: boolean;
}

export interface VersionCapabilities {
  supportLevel: SupportLevel;
  read: boolean;
  edit: boolean;
  write: boolean;
  export: boolean;
  generate: boolean;
  assets: boolean;
}

/** The verified facts the backend can promise for one exact version. */
export interface VersionProfile {
  id: string;
  minecraftVersion: string;
  dataVersion: number;
  releaseType: string;
  family: string;
  worldLayout: WorldLayout;
  chunkFamily: string;
  biomeFormat: string;
  dimensionProfiles: VersionDimension[];
  java: VersionJavaRequirement;
  resourcePackFormat: number | null;
  datapackFormat: number | null;
  contentCatalog: VersionContentCatalog;
  capabilities: VersionCapabilities;
  notes: string[];
}

export interface VersionCatalogEntry {
  id: string;
  versionType: VersionType;
  releaseTime: string;
  latestRelease: boolean;
  latestSnapshot: boolean;
  recommended: boolean;
  profile: VersionProfile | null;
  unavailableReason: string | null;
}

export interface VersionCatalogReport {
  source: "network" | "cache";
  fetchedAtUnix: number;
  latestRelease: string;
  latestSnapshot: string;
  recommended: string;
  versions: VersionCatalogEntry[];
}

export const SUPPORT_LEVEL_LABELS: Record<SupportLevel, string> = {
  full: "Full support",
  "read-write-subset": "Read + write subset",
  "read-only": "Read-only",
  convert: "Convert only",
  "out-of-scope": "Out of scope",
};

export function supportLevelLabel(level: SupportLevel | null | undefined): string {
  return level ? SUPPORT_LEVEL_LABELS[level] : "No verified profile";
}

/** Groups patch releases under their minor series; ids without a numeric series go to `Snapshots`. */
export function versionSeries(id: string): string {
  const match = /^(\d+)\.(\d+)/.exec(id);
  return match ? `${match[1]}.${match[2]}` : "Snapshots";
}
