import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { invokeCommand } from "@/lib/ipc";
import type {
  InstalledRuntime,
  JavaRuntimeReport,
  ManagedCacheReport,
  RuntimeDiscoveryReport,
  RuntimeProgress,
  RuntimeReleaseInfo,
} from "@/runtime/types";

export function discoverJavaRuntimes(): Promise<RuntimeDiscoveryReport> {
  return invokeCommand<RuntimeDiscoveryReport>("runtime_discover");
}

export function savedJavaRuntime(): Promise<JavaRuntimeReport | null> {
  return invokeCommand<JavaRuntimeReport | null>("runtime_saved");
}

/** Explicit manual selection. The backend probes java before persisting it. */
export function selectJavaRuntime(path: string): Promise<JavaRuntimeReport> {
  return invokeCommand<JavaRuntimeReport>("runtime_select_java", { path });
}

export function clearJavaRuntime(): Promise<void> {
  return invokeCommand<void>("runtime_clear_selection");
}

/** Adoptium metadata only; no download happens until installTemurin is called. */
export function latestTemurinRuntime(): Promise<RuntimeReleaseInfo> {
  return invokeCommand<RuntimeReleaseInfo>("runtime_temurin_latest");
}

/** Explicit opt-in download, checksum verification, safe extraction and installation. */
export function installTemurinRuntime(version: string): Promise<InstalledRuntime> {
  return invokeCommand<InstalledRuntime>("runtime_install_temurin", { version });
}

export function managedJavaRuntimes(): Promise<ManagedCacheReport> {
  return invokeCommand<ManagedCacheReport>("runtime_managed_list");
}

export function clearManagedJavaRuntimes(): Promise<ManagedCacheReport> {
  return invokeCommand<ManagedCacheReport>("runtime_clear_managed");
}

export function listenRuntimeDownload(
  handler: (progress: RuntimeProgress) => void,
): Promise<UnlistenFn> {
  return listen<RuntimeProgress>("furybuilder-runtime-download", (event) => handler(event.payload));
}

export function listenRuntimeChanged(handler: () => void): Promise<UnlistenFn> {
  return listen("furybuilder-runtime-changed", () => handler());
}

/** Windows canonical paths may start with the verbatim prefix, which is noise in the UI. */
export function displayRuntimePath(path: string): string {
  return path.replace(/^\\\\\?\\/, "");
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GiB`;
}
