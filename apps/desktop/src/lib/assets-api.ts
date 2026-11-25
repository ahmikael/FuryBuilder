import type { AssetBundle, BlockStateRequest } from "@/assets/render-types";
import type {
  AssetDiscoveryReport,
  AssetPathReport,
  AssetSourceKind,
  CatalogIconAtlas,
  CatalogSnapshot,
  SavedAssetSource,
} from "@/assets/types";
import { invokeCommand } from "@/lib/ipc";

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

/** Explicitly selects original placeholder textures instead of a client JAR. */
export function useMinecraftPlaceholder(): Promise<SavedAssetSource> {
  return invokeCommand<SavedAssetSource>("assets_use_placeholder");
}

export function clearMinecraftAssets(): Promise<SavedAssetSource> {
  return invokeCommand<SavedAssetSource>("assets_clear_source");
}

/** Saves the ordered pack stack, highest priority first. */
export function saveMinecraftPackStack(paths: string[]): Promise<SavedAssetSource> {
  return invokeCommand<SavedAssetSource>("assets_save_pack_stack", { paths });
}

export function clearMinecraftPackStack(): Promise<SavedAssetSource> {
  return invokeCommand<SavedAssetSource>("assets_clear_pack_stack");
}

/** Builds a read-only, verified render bundle for the requested block states. */
export function loadAssetBundle(
  blockStates: BlockStateRequest[],
  override?: { sourceKind: AssetSourceKind; sourcePath: string },
): Promise<AssetBundle> {
  return invokeCommand<AssetBundle>("assets_render_bundle", {
    blockStates,
    ...(override ? { sourceKind: override.sourceKind, sourcePath: override.sourcePath } : {}),
  });
}

export function clearAssetBundleCache(): Promise<void> {
  return invokeCommand<void>("assets_clear_cache");
}

/** Builds the full block/item catalog from the selected client JAR. */
export function loadAssetCatalog(override?: {
  sourceKind: AssetSourceKind;
  sourcePath: string;
}): Promise<CatalogSnapshot> {
  return invokeCommand<CatalogSnapshot>(
    "assets_catalog",
    override ? { sourceKind: override.sourceKind, sourcePath: override.sourcePath } : {},
  );
}

/**
 * Builds the palette icon atlas for the selected client JAR and the saved resource-pack
 * stack. The atlas is presentation only; it does not change the pinned catalog.
 */
export function loadAssetCatalogAtlas(override?: {
  sourceKind: AssetSourceKind;
  sourcePath: string;
}): Promise<CatalogIconAtlas> {
  return invokeCommand<CatalogIconAtlas>(
    "assets_catalog_atlas",
    override ? { sourceKind: override.sourceKind, sourcePath: override.sourcePath } : {},
  );
}

/**
 * Rebuilds the catalog pinned by the open project and verifies it against `project.json`.
 * Returns null for an explicit placeholder project, which has no client JAR catalog.
 */
export function loadProjectCatalog(): Promise<CatalogSnapshot | null> {
  return invokeCommand<CatalogSnapshot | null>("project_catalog");
}

/**
 * Builds the icon atlas for the catalog pinned by the open project and the saved resource-pack
 * stack. Returns null for an explicit placeholder project, matching `loadProjectCatalog`.
 */
export function loadProjectCatalogAtlas(): Promise<CatalogIconAtlas | null> {
  return invokeCommand<CatalogIconAtlas | null>("project_catalog_atlas");
}
