import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { invokeCommand } from "@/lib/ipc";
import type {
  InstalledServerJar,
  ManagedServerCacheReport,
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

export function listenServerDownload(
  handler: (progress: ServerProgress) => void,
): Promise<UnlistenFn> {
  return listen<ServerProgress>("furybuilder-server-download", (event) => handler(event.payload));
}

export function listenServerChanged(handler: () => void): Promise<UnlistenFn> {
  return listen("furybuilder-server-changed", () => handler());
}
