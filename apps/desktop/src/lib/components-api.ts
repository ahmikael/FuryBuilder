import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { invokeCommand } from "@/lib/ipc";
import type {
  CancelOutcome,
  ClientJarReleaseInfo,
  ComponentChangedEvent,
  ComponentErrorEvent,
  ComponentInstallRequest,
  ComponentInventory,
  ComponentKind,
  ComponentProgress,
  ComponentRemoval,
  ComponentReport,
  ComponentRequirements,
  ComponentStatus,
  ComponentTarget,
  ComponentVerification,
  ResolvedComponent,
} from "@/lib/component-types";
import type { RuntimeReleaseInfo } from "@/runtime/types";
import type { ServerReleaseInfo } from "@/runtime/server-types";

/**
 * The only component API the UI should use. It unifies Java, client assets and the
 * official server JAR behind one status vocabulary and one install/cancel path.
 */

export function componentStatus(
  target: ComponentTarget,
  requirements: ComponentRequirements,
): Promise<ComponentReport> {
  return invokeCommand<ComponentReport>("components_status", { target, requirements });
}

export function resolveComponent(
  kind: ComponentKind,
  target: ComponentTarget,
  requirements: ComponentRequirements,
): Promise<ResolvedComponent> {
  return invokeCommand<ResolvedComponent>("components_resolve", {
    kind,
    target,
    requirements,
  });
}

export function componentInventory(): Promise<ComponentInventory> {
  return invokeCommand<ComponentInventory>("components_inventory");
}

/** Adoptium metadata only; no download happens until installComponent is called. */
export function componentJavaRelease(target: ComponentTarget): Promise<RuntimeReleaseInfo> {
  return invokeCommand<RuntimeReleaseInfo>("components_java_release", { target });
}

/** Official client JAR metadata only; no download happens until installComponent is called. */
export function componentAssetsRelease(target: ComponentTarget): Promise<ClientJarReleaseInfo> {
  return invokeCommand<ClientJarReleaseInfo>("components_assets_release", { target });
}

/** Official server JAR metadata only; no download happens until installComponent is called. */
export function componentServerRelease(target: ComponentTarget): Promise<ServerReleaseInfo> {
  return invokeCommand<ServerReleaseInfo>("components_server_release", { target });
}

/** Explicit opt-in install. Progress arrives on `furybuilder-component-progress`. */
export function installComponent(request: ComponentInstallRequest): Promise<ResolvedComponent> {
  return invokeCommand<ResolvedComponent>("components_install", { request });
}

export function cancelComponentInstall(kind: ComponentKind): Promise<CancelOutcome> {
  return invokeCommand<CancelOutcome>("components_cancel", { kind });
}

/** Validates and records a manual source; the user's file is never copied or edited. */
export function relinkComponent(
  kind: ComponentKind,
  path: string,
  target: ComponentTarget,
): Promise<ComponentStatus> {
  return invokeCommand<ComponentStatus>("components_relink", { kind, path, target });
}

/** Explicitly selects original placeholder textures; no client JAR or catalog is claimed. */
export function selectPlaceholderComponent(target: ComponentTarget): Promise<ComponentStatus> {
  return invokeCommand<ComponentStatus>("components_use_placeholder", { target });
}

/** Removes FuryBuilder-owned data or clears a manual pointer, never a user file. */
export function removeComponent(kind: ComponentKind, id: string): Promise<ComponentRemoval> {
  return invokeCommand<ComponentRemoval>("components_remove", { kind, id });
}

/** Re-inspects the selected component; a stale id is refused instead of verified. */
export function verifyComponent(
  kind: ComponentKind,
  id: string,
  target: ComponentTarget,
): Promise<ComponentVerification> {
  return invokeCommand<ComponentVerification>("components_verify", { kind, id, target });
}

/** Re-inspects a managed artifact from the Settings inventory without a project target. */
export function verifyComponentInventory(
  kind: ComponentKind,
  id: string,
): Promise<ComponentVerification> {
  return invokeCommand<ComponentVerification>("components_verify_inventory", { kind, id });
}

/** Opens one FuryBuilder-owned component cache directory in the platform file manager. */
export function openComponentFolder(kind: ComponentKind): Promise<void> {
  return invokeCommand<void>("components_open_folder", { kind });
}

export function listenComponentProgress(
  handler: (progress: ComponentProgress) => void,
): Promise<UnlistenFn> {
  return listen<ComponentProgress>("furybuilder-component-progress", (event) =>
    handler(event.payload),
  );
}

export function listenComponentChanged(
  handler: (event: ComponentChangedEvent) => void,
): Promise<UnlistenFn> {
  return listen<ComponentChangedEvent>("furybuilder-component-changed", (event) =>
    handler(event.payload),
  );
}

/** Emitted for a failed install, relink or remove before the command rejects. */
export function listenComponentError(
  handler: (event: ComponentErrorEvent) => void,
): Promise<UnlistenFn> {
  return listen<ComponentErrorEvent>("furybuilder-component-error", (event) =>
    handler(event.payload),
  );
}
