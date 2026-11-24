import { invokeCommand } from "@/lib/ipc";
import type { VersionCatalogReport } from "@/project/version-types";

/** Loads the official manifest and the verified profiles this build can offer. */
export function fetchVersionCatalog(): Promise<VersionCatalogReport> {
  return invokeCommand<VersionCatalogReport>("versions_catalog");
}
