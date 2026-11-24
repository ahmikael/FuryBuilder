import type { JavaRuntimeReport } from "@/runtime/types";

export type ServerSource = "managed" | "manual";

export interface ServerJarReport {
  path: string;
  source: ServerSource;
  managed: boolean;
  ready: boolean;
  version: string | null;
  dataVersion: number | null;
  javaMajor: number | null;
  protocolVersion: number | null;
  resourcePackFormat: number | null;
  sha1: string | null;
  size: number | null;
  matchesOfficial: boolean | null;
  warnings: string[];
  error: string | null;
}

export interface ManagedServerJar {
  version: string;
  path: string;
  jarPath: string;
  sha1: string;
  size: number;
  dataVersion: number;
  javaMajor: number;
  releaseTime: string;
  sourceUrl: string;
  versionJsonSha1: string;
  installedAt: number;
}

export interface ServerReleaseInfo {
  minecraftVersion: string;
  versionType: string;
  releaseTime: string;
  manifestUrl: string;
  versionJsonSha1: string;
  downloadUrl: string;
  sha1: string;
  size: number;
  javaMajor: number;
  expectedDataVersion: number;
  source: string;
}

export interface SavedEula {
  acceptedAt: number;
  minecraftVersion: string;
  jarSha1: string;
  eulaUrl: string;
}

export interface ServerDiscoveryReport {
  targetMinecraftVersion: string;
  targetDataVersion: number;
  targetJavaMajor: number;
  selected: ServerJarReport | null;
  eula: SavedEula | null;
  eulaMatchesSelected: boolean;
  managedRoot: string;
  managed: ManagedServerJar[];
  managedBytes: number;
  problems: string[];
  manualHint: string;
}

export interface ManagedServerCacheReport {
  root: string;
  servers: ManagedServerJar[];
  totalBytes: number;
  problems: string[];
}

export interface ServerProgress {
  phase: "metadata" | "downloading" | "verified" | "inspecting" | "installed" | "complete" | string;
  downloaded: number;
  total: number;
}

export interface InstalledServerJar {
  server: ManagedServerJar;
  report: ServerJarReport;
  selected: boolean;
}

export type WorldPreset = "normal" | "flat" | "large_biomes" | "amplified";

/** Backend-owned bounds for the supported generation form fields. */
export interface GenerationLimits {
  minRadiusChunks: number;
  maxRadiusChunks: number;
  minTimeLimitSeconds: number;
  maxTimeLimitSeconds: number;
  minMemoryMb: number;
  maxMemoryMb: number;
  minViewDistance: number;
  maxViewDistance: number;
  minCpuThreads: number;
  maxCpuThreads: number;
  maxDatapacks: number;
}

/** Bounded disk/area preflight for one prospective generation run. */
export interface GenerationEstimate {
  radiusChunks: number;
  timeLimitSeconds: number;
  expectedChunks: number;
  estimatedWorldBytes: number;
  requiredDiskBytes: number;
  availableDiskBytes: number;
  fitsOnDisk: boolean;
}

export interface DatapackSelection {
  path: string;
}

export interface GenerationDatapack {
  name: string;
  path: string;
  sha256: string;
  size: number;
  packFormat: number;
  warnings: string[];
}

export interface DatapackInspection {
  path: string;
  name: string;
  sha256: string | null;
  size: number | null;
  packFormat: number | null;
  valid: boolean;
  error: string | null;
  warnings: string[];
}

export interface GenerationRequest {
  seed: string;
  preset: WorldPreset;
  centerX: number;
  centerZ: number;
  radiusChunks: number;
  timeLimitSeconds: number;
  memoryMb: number;
  viewDistance: number;
  cpuThreads: number;
  generateStructures: boolean;
  consent: boolean;
  datapacks: DatapackSelection[];
}

export type GenerationPhase =
  | "preparing"
  | "starting"
  | "generating"
  | "saving"
  | "stopping"
  | "completed"
  | "cancelled"
  | "failed";

export type GenerationStopReason =
  | "completed"
  | "incomplete"
  | "time_limit"
  | "cancelled"
  | "crashed"
  | "startup_failed"
  | "isolation_failed";

export interface GenerationSnapshot {
  runId: string;
  running: boolean;
  phase: GenerationPhase;
  elapsedMs: number;
  expectedChunks: number;
  generatedChunks: number;
  regionFiles: number;
  worldBytes: number;
  message: string;
  error: string | null;
}

export interface GenerationReport {
  snapshot: GenerationSnapshot;
  stopReason: GenerationStopReason;
  worldComplete: boolean;
  exitCode: number | null;
  serverPid: number | null;
  seed: string;
  preset: WorldPreset;
  centerX: number;
  centerZ: number;
  radiusChunks: number;
  timeLimitSeconds: number;
  memoryMb: number;
  viewDistance: number;
  cpuThreads: number;
  generateStructures: boolean;
  javaPath: string;
  javaVersion: string;
  serverJar: string;
  serverSha1: string;
  eulaAcceptedAt: number;
  consentAt: number;
  datapacks: GenerationDatapack[];
  bindAddress: string | null;
  processTreeGuarded: boolean;
  runDir: string;
  worldDir: string;
  logPath: string;
  startedAt: number;
  finishedAt: number;
  logTail: string[];
}

export interface GenerationStatusReport {
  active: GenerationSnapshot | null;
  last: GenerationReport | null;
}

export interface GenerationRunInfo {
  runId: string;
  path: string;
  status: string;
  createdAt: number;
  finishedAt: number | null;
  chunks: number;
  expectedChunks: number;
  regionFiles: number;
  worldBytes: number;
  worldComplete: boolean | null;
  stopReason: GenerationStopReason | null;
  error: string | null;
}

export interface GenerationRunsReport {
  root: string;
  runs: GenerationRunInfo[];
  totalBytes: number;
  problems: string[];
}

/** Combined setup state for the Vanilla World wizard; it never starts or downloads anything. */
export interface VanillaSetupReport {
  targetMinecraftVersion: string;
  targetDataVersion: number;
  targetJavaMajor: number;
  serverDatapackFormat: number;
  generationLimits: GenerationLimits;
  java: JavaRuntimeReport | null;
  javaReady: boolean;
  server: ServerJarReport | null;
  serverReady: boolean;
  eulaAcceptedAt: number | null;
  eulaMatchesSelected: boolean;
  blockers: string[];
  ready: boolean;
}
