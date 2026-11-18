export type RuntimeSource = "managed" | "java_home" | "path" | "common" | "manual";

export type RuntimeStatus =
  "compatible" | "outdated" | "newer" | "unsupported_arch" | "not_java" | "unreadable";

export interface JavaRuntimeReport {
  path: string;
  source: RuntimeSource;
  status: RuntimeStatus;
  version: string | null;
  major: number | null;
  vendor: string | null;
  arch: string | null;
  vm: string | null;
  managed: boolean;
  warnings: string[];
  error: string | null;
}

export interface ManagedRuntime {
  id: string;
  path: string;
  javaPath: string;
  version: string;
  releaseName: string;
  sha256: string;
  sourceUrl: string;
  vendor: string;
  os: string;
  arch: string;
  installedAt: number;
  bytes: number;
}

export interface RuntimeReleaseInfo {
  version: string;
  releaseName: string;
  downloadUrl: string;
  sha256: string;
  size: number;
  vendor: string;
  os: string;
  arch: string;
  imageType: string;
  source: string;
}

export interface RuntimeDiscoveryReport {
  targetJavaMajor: number;
  targetMinecraftVersion: string;
  candidates: JavaRuntimeReport[];
  saved: JavaRuntimeReport | null;
  managedRoot: string;
  managed: ManagedRuntime[];
  managedBytes: number;
  problems: string[];
  manualHint: string;
}

export interface InstalledRuntime {
  runtime: ManagedRuntime;
  report: JavaRuntimeReport;
  selected: boolean;
}

export interface RuntimeProgress {
  phase:
    | "metadata"
    | "downloading"
    | "verified"
    | "extracting"
    | "extracted"
    | "probing"
    | "complete"
    | string;
  downloaded: number;
  total: number;
}

export interface ManagedCacheReport {
  root: string;
  runtimes: ManagedRuntime[];
  totalBytes: number;
  problems: string[];
}
