import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { invokeCommand } from "@/lib/ipc";
import type {
  GenerationRequest,
  GenerationRunsReport,
  GenerationSnapshot,
  GenerationStatusReport,
  InstalledServerJar,
  ManagedServerCacheReport,
  SavedEula,
  ServerDiscoveryReport,
  ServerJarReport,
  ServerProgress,
  ServerReleaseInfo,
} from "@/runtime/server-types";

export function discoverMinecraftServers(): Promise<ServerDiscoveryReport> {
  return invokeCommand<ServerDiscoveryReport>("server_discover");
}

export function savedMinecraftServer(): Promise<ServerJarReport | null> {
  return invokeCommand<ServerJarReport | null>("server_saved");
}

/** Explicit manual selection. The backend verifies version.json, DataVersion and the entry point. */
export function selectMinecraftServerJar(path: string): Promise<ServerJarReport> {
  return invokeCommand<ServerJarReport>("server_select_jar", { path });
}

export function clearMinecraftServerSelection(): Promise<void> {
  return invokeCommand<void>("server_clear_selection");
}

/** Mojang metadata only; no download happens until installMinecraftServer is called. */
export function latestMinecraftServerRelease(): Promise<ServerReleaseInfo> {
  return invokeCommand<ServerReleaseInfo>("server_release");
}

/** Explicit opt-in download, SHA-1/size verification and internal version inspection. */
export function installMinecraftServer(version: string): Promise<InstalledServerJar> {
  return invokeCommand<InstalledServerJar>("server_install", { version });
}

export function managedMinecraftServers(): Promise<ManagedServerCacheReport> {
  return invokeCommand<ManagedServerCacheReport>("server_managed_list");
}

export function clearManagedMinecraftServers(): Promise<ManagedServerCacheReport> {
  return invokeCommand<ManagedServerCacheReport>("server_clear_managed");
}

/** Explicit EULA acceptance for the currently selected JAR. `false` is rejected backend-side. */
export function acceptServerEula(accepted: boolean): Promise<SavedEula> {
  return invokeCommand<SavedEula>("server_accept_eula", { accepted });
}

export function clearServerEula(): Promise<void> {
  return invokeCommand<void>("server_clear_eula");
}

export function startServerGeneration(request: GenerationRequest): Promise<GenerationSnapshot> {
  return invokeCommand<GenerationSnapshot>("server_start_generation", { request });
}

export function serverGenerationStatus(): Promise<GenerationStatusReport> {
  return invokeCommand<GenerationStatusReport>("server_generation_status");
}

export function cancelServerGeneration(): Promise<boolean> {
  return invokeCommand<boolean>("server_cancel_generation");
}

export function serverGenerationRuns(): Promise<GenerationRunsReport> {
  return invokeCommand<GenerationRunsReport>("server_generation_runs");
}

export function clearServerGenerationRuns(): Promise<GenerationRunsReport> {
  return invokeCommand<GenerationRunsReport>("server_clear_generation_runs");
}

export function listenServerDownload(
  handler: (progress: ServerProgress) => void,
): Promise<UnlistenFn> {
  return listen<ServerProgress>("furybuilder-server-download", (event) => handler(event.payload));
}

export function listenServerChanged(handler: () => void): Promise<UnlistenFn> {
  return listen("furybuilder-server-changed", () => handler());
}
