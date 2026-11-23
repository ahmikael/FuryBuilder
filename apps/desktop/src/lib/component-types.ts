/**
 * Unified component contract. Mirrors `crates/component-manager` and
 * `crates/component-model`; every struct uses camelCase on the wire.
 */

import type { ManagedCacheReport } from "@/runtime/types";
import type { ManagedServerCacheReport } from "@/runtime/server-types";

export type ComponentKind = "java" | "assets" | "server";
export type ComponentMode = "managed" | "manual" | "placeholder";
export type ComponentState =
  | "ready"
  | "missing"
  | "unverified"
  | "incompatible"
  | "error"
  | "not_required";
export type HashAlgorithm = "sha1" | "sha256";

export interface ComponentHash {
  algorithm: HashAlgorithm;
  value: string;
}

/** Facts from the project target; never a global UI setting. */
export interface ComponentTarget {
  profileId: string;
  minecraftVersion: string;
  dataVersion: number;
  javaMajor: number;
}

/** Which components a project actually needs for its creation mode. */
export interface ComponentRequirements {
  java: boolean;
  assets: boolean;
  server: boolean;
  assetsPlaceholder: boolean;
}

export interface ComponentStatus {
  kind: ComponentKind;
  required: boolean;
  mode?: ComponentMode;
  state: ComponentState;
  id?: string;
  version?: string;
  path?: string;
  hash?: ComponentHash;
  bytes?: number;
  notes?: string[];
  error?: string;
}

export interface ComponentReport {
  profileId: string;
  minecraftVersion: string;
  dataVersion: number;
  javaMajor: number;
  components: ComponentStatus[];
  ready: boolean;
  warnings: string[];
}

export interface ResolvedComponent {
  kind: ComponentKind;
  mode: ComponentMode;
  id: string;
  version?: string;
  path: string;
  hash: ComponentHash;
  bytes: number;
}

export interface ComponentVerification {
  kind: ComponentKind;
  id: string;
  verified: boolean;
  state: ComponentState;
  notes?: string[];
  error?: string;
}

export interface ComponentRemoval {
  kind: ComponentKind;
  id: string;
  removedManaged: boolean;
  clearedSelection: boolean;
}

export interface ComponentProgress {
  kind: ComponentKind;
  phase: string;
  downloaded: number;
  total: number;
}

/** The reviewed version and hash the user approved before a download starts. */
export interface ComponentInstallRequest {
  kind: ComponentKind;
  target: ComponentTarget;
  expectedVersion?: string;
  expectedHash?: ComponentHash;
}

export type CancelOutcome = "not_running" | "requested" | "already_requested";

export interface ManagedClientJar {
  id: string;
  version: string;
  path: string;
  jarPath: string;
  sha1: string;
  size: number;
  dataVersion: number;
  resourcePackFormat: number | null;
  datapackFormat: number | null;
  sourceUrl: string;
  installedAt: number;
}

export interface ManagedClientJarReport {
  root: string;
  clients: ManagedClientJar[];
  totalBytes: number;
  problems: string[];
}

/** FuryBuilder-owned artifacts on disk, grouped by kind. */
export interface ComponentInventory {
  java: ManagedCacheReport;
  assets: ManagedClientJarReport;
  servers: ManagedServerCacheReport;
}

/** Official client JAR metadata shown before a download. */
export interface ClientJarReleaseInfo {
  minecraftVersion: string;
  versionType: string;
  releaseTime: string;
  versionJsonUrl: string;
  versionJsonSha1: string;
  downloadUrl: string;
  sha1: string;
  size: number;
  source: string;
}

export interface ComponentChangedEvent {
  kind: ComponentKind;
}

export interface ComponentErrorEvent {
  kind: ComponentKind;
  code: string;
  message: string;
}
