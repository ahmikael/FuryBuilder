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

export interface ServerDiscoveryReport {
  targetMinecraftVersion: string;
  targetDataVersion: number;
  targetJavaMajor: number;
  selected: ServerJarReport | null;
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
