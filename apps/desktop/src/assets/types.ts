/**
 * Mirrors the Rust `asset-source` reports (M9.1). All fields are camelCase because
 * the Rust structs use `serde(rename_all = "camelCase")`.
 */

export type AssetSelectionKind =
  | "minecraft_root"
  | "version_directory"
  | "client_jar"
  | "resource_pack"
  | "not_found"
  | "unsafe_link"
  | "unknown";

export type InstallationSource = "default_root" | "launcher_profile" | "manual";

export type VersionStatus =
  | "compatible"
  | "compatible_unverified"
  | "wrong_version"
  | "modded"
  | "missing_metadata"
  | "missing_client_jar"
  | "checksum_mismatch"
  | "world_version_mismatch"
  | "invalid_metadata"
  | "invalid_client_jar";

export type ResourcePackKind = "zip" | "folder";

export type ResourcePackStatus =
  | "compatible"
  | "incompatible_format"
  | "missing_metadata"
  | "invalid_metadata"
  | "unsafe_contents"
  | "unreadable";

export type AssetSourceKind = "minecraft_root" | "client_jar" | "resource_pack";

export interface VersionReport {
  id: string;
  versionType: string | null;
  releaseTime: string | null;
  javaMajor: number | null;
  inheritsFrom: string | null;
  metadataPath: string;
  metadataPresent: boolean;
  jarPath: string;
  jarPresent: boolean;
  jarBytes: number | null;
  expectedSha1: string | null;
  actualSha1: string | null;
  checksumVerified: boolean;
  internalVersionId: string | null;
  worldVersion: number | null;
  resourcePackFormat: number | null;
  javaVersionFromJar: number | null;
  assetIndex: string | null;
  assetIndexPresent: boolean;
  assetIndexChecksumVerified: boolean;
  status: VersionStatus;
  notes: string[];
}

export interface ResourcePackReport {
  path: string;
  name: string;
  kind: ResourcePackKind;
  bytes: number;
  packFormat: number | null;
  supportedFormats: string | null;
  description: string | null;
  compatible: boolean;
  hasBlockstates: boolean;
  hasTextures: boolean;
  entryCount: number;
  status: ResourcePackStatus;
  errors: string[];
  warnings: string[];
}

export interface InstallationReport {
  root: string;
  source: InstallationSource;
  versionsDirPresent: boolean;
  assetsDirPresent: boolean;
  assetIndexes: number;
  versions: VersionReport[];
  compatibleVersions: string[];
  resourcePacks: ResourcePackReport[];
  warnings: string[];
}

export interface AssetDiscoveryReport {
  targetMinecraftVersion: string;
  targetDataVersion: number;
  targetResourcePackFormat: number;
  defaultRootPresent: boolean;
  searchedRoots: string[];
  installations: InstallationReport[];
  problems: string[];
  manualHint: string;
}

export interface AssetPathReport {
  path: string;
  kind: AssetSelectionKind;
  valid: boolean;
  targetMatch: boolean;
  installation: InstallationReport | null;
  version: VersionReport | null;
  resourcePack: ResourcePackReport | null;
  errors: string[];
  warnings: string[];
}

export interface AssetSourceSetting {
  kind: string;
  path: string;
}

export interface SavedAssetSource {
  source: AssetSourceSetting | null;
  report: AssetPathReport | null;
  loadWarning: string | null;
  /** Saved packs in priority order, highest priority first. */
  packs: ResourcePackReport[];
}
