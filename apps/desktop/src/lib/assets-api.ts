import { invokeCommand } from "@/lib/ipc";
import type {
  AssetDiscoveryReport,
  AssetPathReport,
  AssetSourceKind,
  SavedAssetSource,
} from "@/assets/types";

export function discoverMinecraftAssets(): Promise<AssetDiscoveryReport> {
  return invokeCommand<AssetDiscoveryReport>("assets_discover");
}

export function inspectAssetPath(path: string): Promise<AssetPathReport> {
  return invokeCommand<AssetPathReport>("assets_inspect_path", { path });
}

export function savedMinecraftAssets(): Promise<SavedAssetSource> {
  return invokeCommand<SavedAssetSource>("assets_saved_source");
}

export function saveMinecraftAssets(
  kind: AssetSourceKind,
  path: string,
): Promise<SavedAssetSource> {
  return invokeCommand<SavedAssetSource>("assets_save_source", { kind, path });
}

export function clearMinecraftAssets(): Promise<SavedAssetSource> {
  return invokeCommand<SavedAssetSource>("assets_clear_source");
}
