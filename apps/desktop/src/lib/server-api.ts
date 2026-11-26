import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { invokeCommand } from "@/lib/ipc";
import type {
  DatapackInspection,
  GenerationEstimate,
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
  VanillaSetupReport,
} from "@/runtime/server-types";

export function discoverMinecraftServers(profileId: string): Promise<ServerDiscoveryReport> {
  return invokeCommand<ServerDiscoveryReport>("server_discover", { profileId });
}

export function savedMinecraftServer(profileId: string): Promise<ServerJarReport | null> {
  return invokeCommand<ServerJarReport | null>("server_saved", { profileId });
}

/** Explicit manual selection. The backend verifies version.json, DataVersion and the entry point. */
export function selectMinecraftServerJar(
  profileId: string,
  path: string,
): Promise<ServerJarReport> {
  return invokeCommand<ServerJarReport>("server_select_jar", { profileId, path });
}

export function clearMinecraftServerSelection(profileId: string): Promise<void> {
  return invokeCommand<void>("server_clear_selection", { profileId });
}

/** Mojang metadata only; no download happens until installMinecraftServer is called. */
export function latestMinecraftServerRelease(profileId: string): Promise<ServerReleaseInfo> {
  return invokeCommand<ServerReleaseInfo>("server_release", { profileId });
}

/** Explicit opt-in download, SHA-1/size verification and internal version inspection. */
export function installMinecraftServer(
  profileId: string,
  version: string,
): Promise<InstalledServerJar> {
  return invokeCommand<InstalledServerJar>("server_install", { profileId, version });
}

export function managedMinecraftServers(): Promise<ManagedServerCacheReport> {
  return invokeCommand<ManagedServerCacheReport>("server_managed_list");
}

export function clearManagedMinecraftServers(): Promise<ManagedServerCacheReport> {
  return invokeCommand<ManagedServerCacheReport>("server_clear_managed");
}

/** Explicit EULA acceptance for the currently selected JAR. `false` is rejected backend-side. */
export function acceptServerEula(profileId: string, accepted: boolean): Promise<SavedEula> {
  return invokeCommand<SavedEula>("server_accept_eula", { profileId, accepted });
}

export function clearServerEula(profileId: string): Promise<void> {
  return invokeCommand<void>("server_clear_eula", { profileId });
}

export function startServerGeneration(
  profileId: string,
  request: GenerationRequest,
): Promise<GenerationSnapshot> {
  return invokeCommand<GenerationSnapshot>("server_start_generation", { profileId, request });
}

export function serverGenerationStatus(): Promise<GenerationStatusReport> {
  return invokeCommand<GenerationStatusReport>("server_generation_status");
}

/** Bounded preflight for the Source step; it never creates a run directory or starts Java. */
export function serverGenerationEstimate(
  radiusChunks: number,
  timeLimitSeconds: number,
): Promise<GenerationEstimate> {
  return invokeCommand<GenerationEstimate>("server_generation_estimate", {
    radiusChunks,
    timeLimitSeconds,
  });
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

/** Validates local `.zip` data packs against the selected profile; invalid entries are returned individually. */
export function inspectServerDatapacks(
  profileId: string,
  paths: string[],
): Promise<DatapackInspection[]> {
  return invokeCommand<DatapackInspection[]>("server_inspect_datapacks", { profileId, paths });
}

/** Reads the verified Java/server/EULA state for the selected profile without starting anything. */
export function vanillaSetupCheck(profileId: string): Promise<VanillaSetupReport> {
  return invokeCommand<VanillaSetupReport>("vanilla_setup_check", { profileId });
}

export function listenServerDownload(
  handler: (progress: ServerProgress) => void,
): Promise<UnlistenFn> {
  return listen<ServerProgress>("furybuilder-server-download", (event) => handler(event.payload));
}

export function listenServerChanged(handler: () => void): Promise<UnlistenFn> {
  return listen("furybuilder-server-changed", () => handler());
}
