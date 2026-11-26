/**
 * Mirrors the Rust `asset-source` reports (M9.1). All fields are camelCase because
 * the Rust structs use `serde(rename_all = "camelCase")`.
 */

export type AssetSelectionKind =
  | "minecraft_root"
  | "version_directory"
  | "client_jar"
  | "resource_pack"
  | "not_found"
  | "unsafe_link"
  | "unknown";

export type InstallationSource = "default_root" | "launcher_profile" | "manual";

export type VersionStatus =
  | "compatible"
  | "compatible_unverified"
  | "wrong_version"
  | "modded"
  | "missing_metadata"
  | "missing_client_jar"
  | "checksum_mismatch"
  | "world_version_mismatch"
  | "invalid_metadata"
  | "invalid_client_jar";

export type ResourcePackKind = "zip" | "folder";

export type ResourcePackStatus =
  | "compatible"
  | "incompatible_format"
  | "missing_metadata"
  | "invalid_metadata"
  | "unsafe_contents"
  | "unreadable";

export type AssetSourceKind =
  | "minecraft_root"
  | "version_directory"
  | "client_jar"
  | "resource_pack";

export interface VersionReport {
  id: string;
  versionType: string | null;
  releaseTime: string | null;
  javaMajor: number | null;
  inheritsFrom: string | null;
  metadataPath: string;
  metadataPresent: boolean;
  jarPath: string;
  jarPresent: boolean;
  jarBytes: number | null;
  expectedSha1: string | null;
  actualSha1: string | null;
  checksumVerified: boolean;
  internalVersionId: string | null;
  worldVersion: number | null;
  resourcePackFormat: number | null;
  javaVersionFromJar: number | null;
  assetIndex: string | null;
  assetIndexPresent: boolean;
  assetIndexChecksumVerified: boolean;
  status: VersionStatus;
  notes: string[];
}

export interface ResourcePackReport {
  path: string;
  name: string;
  kind: ResourcePackKind;
  bytes: number;
  packFormat: number | null;
  supportedFormats: string | null;
  description: string | null;
  compatible: boolean;
  hasBlockstates: boolean;
  hasTextures: boolean;
  entryCount: number;
  status: ResourcePackStatus;
  errors: string[];
  warnings: string[];
}

export interface InstallationReport {
  root: string;
  source: InstallationSource;
  versionsDirPresent: boolean;
  assetsDirPresent: boolean;
  assetIndexes: number;
  versions: VersionReport[];
  compatibleVersions: string[];
  resourcePacks: ResourcePackReport[];
  warnings: string[];
}

export interface AssetDiscoveryReport {
  targetMinecraftVersion: string;
  targetDataVersion: number;
  targetResourcePackFormat: number;
  defaultRootPresent: boolean;
  searchedRoots: string[];
  installations: InstallationReport[];
  problems: string[];
  manualHint: string;
}

export interface AssetPathReport {
  path: string;
  kind: AssetSelectionKind;
  valid: boolean;
  targetMatch: boolean;
  installation: InstallationReport | null;
  version: VersionReport | null;
  resourcePack: ResourcePackReport | null;
  errors: string[];
  warnings: string[];
}

export interface AssetSourceSetting {
  kind: string;
  path: string;
}

export interface SavedAssetSource {
  source: AssetSourceSetting | null;
  report: AssetPathReport | null;
  /** True when the user explicitly chose original placeholder textures. */
  placeholder: boolean;
  loadWarning: string | null;
}

export interface ProjectPackStatus {
  path: string;
  sha1: string;
  name: string;
  state: "ready" | "missing" | "changed" | "incompatible" | "invalid";
  message: string | null;
  packFormat: number | null;
  supportedFormats: string | null;
}

export interface ProjectPackStack {
  minecraftVersion: string;
  targetFormat: string | null;
  enabled: boolean;
  maxPacks: number;
  /** Highest priority first, including unavailable pins. */
  packs: ProjectPackStatus[];
}

export type ProjectPackEdit =
  | { kind: "add"; path: string }
  | { kind: "remove"; sha1: string }
  | { kind: "move"; sha1: string; earlier: boolean };

/** Numeric id used by pre-flattening catalog versions. */
export interface CatalogLegacyId {
  id: number;
  metadata: number | null;
}

/** Every block state property key and value declared by the blockstate file. */
export interface CatalogStates {
  properties: Record<string, string[]>;
  /** True when the blockstate file enumerates every value of every property it declares. */
  propertyValuesComplete: boolean;
  /** The only declared state; null when several variants exist and the game default applies. */
  defaultProperties: Record<string, string> | null;
  /** Number of explicit `variants` entries; zero for multipart blocks. */
  variantCount: number;
  /** Every explicit variant in file order, including its exact property key. */
  variants: CatalogVariant[];
  multipart: boolean;
  model: string | null;
}

/** One explicit entry from the blockstate `variants` object. */
export interface CatalogVariant {
  properties: Record<string, string>;
  /** Weighted model choices declared by the variant. */
  models: CatalogVariantModel[];
}

/** One model choice inside a variant, including rotation and weight from the JAR. */
export interface CatalogVariantModel {
  model: string | null;
  xRotation: number;
  yRotation: number;
  uvlock: boolean;
  weight: number;
}

/** The item form of one catalog id. */
export interface CatalogItemForm {
  /** True when the entry came from `assets/<namespace>/items/<item>.json`. */
  itemDefinition: boolean;
  /** Top-level item definition model type, for example `minecraft:select`. */
  modelType: string | null;
  model: string | null;
}

/** Facts used by palette rules and placement; null means the fact is not known yet. */
export interface CatalogFlags {
  placeable: boolean | null;
  hasItem: boolean | null;
  obtainable: boolean | null;
  creativeOnly: boolean | null;
  technical: boolean | null;
  fluid: boolean | null;
  airlike: boolean | null;
  blockEntity: boolean | null;
  transparent: boolean | null;
  fullCube: boolean | null;
  gravity: boolean | null;
  waterloggable: boolean | null;
  tinted: boolean | null;
  hasVariants: boolean | null;
}

/** One block and/or item from the exact client JAR. */
export interface CatalogEntry {
  id: string;
  /** Numeric id and metadata for legacy versions; null for named registries. */
  legacyId: CatalogLegacyId | null;
  displayName: string | null;
  category: string | null;
  tags: string[];
  iconTexture: string | null;
  /** Older names that resolve to this id. */
  aliases: string[];
  /** Block states declared by the JAR; null when the id has no block form. */
  states: CatalogStates | null;
  /** Item form declared by the JAR; null for block-only technical entries. */
  itemForm: CatalogItemForm | null;
  flags: CatalogFlags;
}

export interface CatalogAtlasSource {
  kind: string;
  source: string | null;
  prefix: string | null;
  resource: string | null;
}

export interface CatalogAtlas {
  id: string;
  sources: CatalogAtlasSource[];
}

export interface CatalogCounts {
  blocks: number;
  items: number;
  blocksWithItems: number;
  itemOnly: number;
  blockstateFiles: number;
  blockModels: number;
  itemModels: number;
  itemDefinitions: number;
  textures: number;
  languages: number;
  langEntries: number;
  blockTags: number;
  itemTags: number;
  atlases: number;
  atlasSources: number;
}

/** Deterministic catalog built from one exact client JAR. */
export interface CatalogSnapshot {
  schemaVersion: number;
  catalogId: string;
  catalogHash: string;
  minecraftVersion: string;
  dataVersion: number;
  clientJar: string;
  clientJarSha1: string | null;
  resourcePackFormat: number | null;
  datapackFormat: number | null;
  entries: CatalogEntry[];
  atlases: CatalogAtlas[];
  counts: CatalogCounts;
  warnings: string[];
  cacheHit: boolean;
}

/** One decoded palette icon placed in a catalog icon atlas. */
export interface CatalogAtlasTexture {
  key: string;
  path: string | null;
  x: number;
  y: number;
  width: number;
  height: number;
  sourceWidth: number;
  sourceHeight: number;
  missing: boolean;
  hasAlpha: boolean;
}

/**
 * Packed PNG atlas for palette icons. The cache key is
 * `minecraftVersion|clientJarSha1|packStackHash|catalogSchema.atlasSchema`.
 */
export interface CatalogIconAtlas {
  schemaVersion: number;
  cacheKey: string;
  cacheHit: boolean;
  catalogId: string;
  catalogHash: string;
  catalogSchemaVersion: number;
  minecraftVersion: string;
  clientJarSha1: string;
  packStackHash: string;
  width: number;
  height: number;
  pngBase64: string;
  textures: CatalogAtlasTexture[];
  warnings: string[];
  packIssues: string[];
}
