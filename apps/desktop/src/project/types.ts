import type { WorldInfo } from "@/lib/contracts";
import type { Bounds3 } from "@/renderer/types";
import type { Scene } from "@/project/scene-types";

export type CreationMode = "empty" | "flat" | "vanilla" | "import";
export type SaveKind = "manual" | "autosave" | "checkpoint";
export type SourceKind = "generated" | "imported";

/** The only project schema this build reads or writes. */
export const PROJECT_SCHEMA_VERSION = 2;

export interface WorkspaceLayer {
  id: string;
  name: string;
  visible: boolean;
  locked: boolean;
}

export interface WorkspaceRegion {
  id: string;
  name: string;
  dimension: string;
  bounds: Bounds3;
}

/** Editor-only policy persisted in project.json; not Minecraft world data. */
export interface WorkspacePolicy {
  schemaVersion: number;
  layers: WorkspaceLayer[];
  activeLayerId: string | null;
  masks: WorkspaceRegion[];
  activeMaskId: string | null;
  protectedRegions: WorkspaceRegion[];
}

export const WORKSPACE_SCHEMA_VERSION = 1;

export function defaultWorkspacePolicy(): WorkspacePolicy {
  return {
    schemaVersion: WORKSPACE_SCHEMA_VERSION,
    layers: [{ id: "base", name: "Base", visible: true, locked: false }],
    activeLayerId: "base",
    masks: [],
    activeMaskId: null,
    protectedRegions: [],
  };
}

export interface EditorState {
  selectedDimension: string;
  loadRadius: number;
  workspace?: WorkspacePolicy;
  scene?: Scene;
}

export interface SourceInfo {
  kind: SourceKind;
  immutable: boolean;
  reference?: string;
}

export interface VanillaDatapack {
  name: string;
  sha256: string;
  size: number;
  packFormat: number;
}

export interface VanillaProvenance {
  seed: string;
  preset: string;
  centerX: number;
  centerZ: number;
  radiusChunks: number;
  generateStructures: boolean;
  datapacks: VanillaDatapack[];
  minecraftVersion: string;
  dataVersion: number;
  javaVersion: string;
  serverJarSha1: string;
  runId: string;
  generatedAtMs: number;
  chunks: number;
  regionFiles: number;
  worldBytes: number;
}

export interface ProjectSummary {
  folder: string;
  projectId: string;
  name: string;
  creationMode: CreationMode;
  targetMinecraftVersion: string;
  targetDataVersion: number;
  worldRevision: number;
  updatedAtMs: number;
  lastSavedAtMs: number;
  lastSaveKind: SaveKind;
  bytes: number;
  schemaVersion: number;
  recoverable: boolean;
}

export type VanillaParityStatus = "matched" | "mismatched" | "refused";

export interface VanillaParityMismatch {
  chunkX: number;
  chunkZ: number;
  sectionY: number;
  x: number;
  y: number;
  z: number;
  reference: string;
  project: string;
}

export interface VanillaParityChunkRef {
  x: number;
  z: number;
}

export interface VanillaParityComparison {
  status: VanillaParityStatus;
  referenceRoot: string;
  dimension: string;
  requestedChunks: number;
  sampledChunks: number;
  checkedChunks: number;
  checkedSections: number;
  checkedBlocks: number;
  mismatchCount: number;
  mismatches: VanillaParityMismatch[];
  missingReferenceChunks: VanillaParityChunkRef[];
  missingProjectChunks: VanillaParityChunkRef[];
  refusals: string[];
  limitations: string[];
}

export interface VanillaParityReport {
  status: VanillaParityStatus;
  projectFolder: string;
  projectName: string;
  snapshotRevision: number;
  fromAutosave: boolean;
  referenceKind: "generatedRun" | "projectSource";
  referenceRoot: string;
  seed: string;
  preset: string;
  centerX: number;
  centerZ: number;
  radiusChunks: number;
  refusals: string[];
  comparison: VanillaParityComparison | null;
  limitations: string[];
}

export interface ProjectDetail {
  summary: ProjectSummary;
  editorState: EditorState;
  source: SourceInfo;
  vanilla?: VanillaProvenance;
  createdAtMs: number;
  warnings: string[];
}

export interface ProjectList {
  projects: ProjectSummary[];
  warnings: string[];
}

/** One horizontal layer of the flat starting shape, listed bottom-up. */
export interface FlatLayer {
  block: string;
  thickness: number;
}

/** Version-neutral flat source definition validated by the backend before any write. */
export interface FlatPreset {
  baseY: number;
  layers: FlatLayer[];
  biome: string;
}

export interface FlatLimits {
  maxLayers: number;
  maxLayerThickness: number;
  maxTotalThickness: number;
  footprint: number;
}

/** Backend-owned flat defaults, limits and quick choices; the wizard never invents them. */
export interface FlatDefaults {
  preset: FlatPreset;
  limits: FlatLimits;
  blockChoices: string[];
  biomeChoices: string[];
}

/** Library location and flat-source parameters owned by the backend. */
export interface ProjectDefaults {
  projectsRoot: string;
  folderSuffix: string;
  flat: FlatDefaults;
}

export function flatTotalThickness(preset: FlatPreset): number {
  return preset.layers.reduce((sum, layer) => sum + layer.thickness, 0);
}

export function flatTopY(preset: FlatPreset): number {
  return preset.baseY + flatTotalThickness(preset) - 1;
}

/** Human label for a namespaced id while keeping the full id in tooltips and code. */
export function formatResourceId(id: string): string {
  const path = id.includes(":") ? id.slice(id.indexOf(":") + 1) : id;
  return path
    .split("_")
    .map((part) => (part.length > 0 ? part[0]!.toUpperCase() + part.slice(1) : part))
    .join(" ");
}

export interface RecoveryInfo {
  reason: string;
  recoveredRevision: number;
  recoveredAtMs: number;
  fromBackup: boolean;
  /** True only when the newer autosave passed verification and was loaded. */
  available: boolean;
  /** Revision recorded by the autosave metadata, even when it could not be applied. */
  autosaveRevision: number | null;
}

export interface SessionStatus {
  folder: string | null;
  name: string | null;
  savedRevision: number;
  autosavedRevision: number | null;
  revision: number;
  dirty: boolean;
  editorState: EditorState | null;
  availableCheckpoints: number;
}

export interface OpenedProjectPayload {
  detail: ProjectDetail;
  info: WorldInfo;
  recovery: RecoveryInfo | null;
  session: SessionStatus;
  blockStateWarning: string | null;
}

export interface SaveOutcome {
  project: ProjectDetail;
  checksum: string;
  snapshotFile: string;
  bytesWritten: number;
  backupCreated: boolean;
  prunedSnapshots: number;
}

export interface CheckpointMeta {
  checkpointSchemaVersion: number;
  profileId: string;
  revision: number;
  checksum: string;
  bytes: number;
  savedAtMs: number;
  label?: string;
  reason: string;
  editorState: EditorState;
}

export interface ImportDimensionAnalysis {
  id: string;
  kind: "overworld" | "nether" | "end" | "custom" | string;
  minY: number | null;
  height: number | null;
  regionFiles: number;
  chunks: number;
}

export interface SupportVersion {
  id: string;
  minecraftVersions: string;
  dataVersions: string;
  layout: string;
  status: string;
  read: string;
  edit: string;
  write: string;
  passthrough: string;
  note: string;
}

export interface SupportCategory {
  id: string;
  label: string;
  read: string;
  edit: string;
  write: string;
  passthrough: string;
  note: string;
}

export interface SupportMatrix {
  schemaVersion: number;
  versions: SupportVersion[];
  categories: SupportCategory[];
}

/** A world source is either a folder or a `.zip` world archive. */
export type ImportSourceKind = "folder" | "archive";

/**
 * Import policy. Conversion needs a verified converter, which this build does
 * not ship yet; the wizard exposes it as an explicit unavailable choice.
 */
export type ImportStrategy = "importAsIs" | "convert";

export interface ConfirmedImportVersion {
  minecraftVersion: string;
  dataVersion: number;
}

/** Never confirm stale analysis from a different source or an incomplete version. */
export function importVersionForSource(
  analysis: JavaWorldAnalysis | null,
  source: string,
): ConfirmedImportVersion | null {
  if (
    !analysis?.supported ||
    analysis.sourcePath !== source.trim() ||
    !analysis.versionName?.trim() ||
    analysis.dataVersion === null
  ) {
    return null;
  }
  return { minecraftVersion: analysis.versionName, dataVersion: analysis.dataVersion };
}

export interface WorldArchiveReport {
  entries: number;
  bytes: number;
}

export interface JavaWorldAnalysis {
  root: string;
  layout: string;
  selectionKind: string;
  completeSave: boolean;
  supported: boolean;
  dataVersion: number | null;
  versionId: number | null;
  versionName: string | null;
  levelName: string | null;
  declaredDimensions: string[];
  dimensions: ImportDimensionAnalysis[];
  regions: number;
  chunks: number;
  blockEntities: number | null;
  entities: number | null;
  sourceBytes: number;
  sourcePath: string;
  sourceKind: ImportSourceKind;
  archive: WorldArchiveReport | null;
  warnings: string[];
  blockers: string[];
  supportMatrix: SupportMatrix;
}

export const CREATION_MODES: Array<{
  mode: CreationMode;
  label: string;
  description: string;
  requirements: string[];
  available: boolean;
}> = [
  {
    mode: "empty",
    label: "Empty Project",
    description: "A blank world with only air. Every block is placed by you or a tool.",
    requirements: [
      "A project name and folder.",
      "No Java runtime, client assets or server are used at creation.",
    ],
    available: true,
  },
  {
    mode: "flat",
    label: "Flat base",
    description:
      "An editable flat platform around the origin with your own layers, height and biome. Not vanilla world generation.",
    requirements: [
      "A project name and folder.",
      "No world generator runs; the platform is created as FuryBuilder world data.",
      "Layers, height and biome must fit the selected version's overworld height.",
    ],
    available: true,
  },
  {
    mode: "vanilla",
    label: "Vanilla World",
    description:
      "Choose a seed, preset, supported world settings and optional local data packs. The official local Java server generates the requested area after you accept the EULA and explicitly confirm the run.",
    requirements: [
      "A compatible local Java runtime and the exact official server JAR for the selected version.",
      "Explicit EULA consent, free disk space and time for generation.",
      "The seed names the generated project; the folder is still chosen here.",
    ],
    available: true,
  },
  {
    mode: "import",
    label: "Import World",
    description:
      "Open a supported Java world read-only, keep a byte-for-byte source copy and edit its blocks through the project overlay.",
    requirements: [
      "A complete Java world folder or .zip world archive with level.dat and region data.",
      "Read-only analysis must pass before the project can be created.",
      "A project name and folder; the source is copied byte for byte and never written.",
    ],
    available: true,
  },
];

export function creationModeLabel(mode: CreationMode): string {
  return CREATION_MODES.find((entry) => entry.mode === mode)?.label ?? mode;
}

/** Honest, user-facing problems with a project summary from the backend. */
export function projectSummaryWarnings(project: ProjectSummary): string[] {
  const warnings: string[] = [];
  if (project.schemaVersion > PROJECT_SCHEMA_VERSION) {
    warnings.push("Newer project schema; update FuryBuilder");
  } else if (project.schemaVersion !== PROJECT_SCHEMA_VERSION) {
    warnings.push("Unsupported project schema; this build cannot open it");
  }
  if (project.bytes === 0) {
    warnings.push("Manifest has no snapshot size");
  }
  return warnings;
}

export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KiB", "MiB", "GiB"] as const;
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(unit === 0 ? 0 : 1)} ${units[unit]}`;
}

export function formatRelativeTime(timestampMs: number, now = Date.now()): string {
  if (!Number.isFinite(timestampMs) || timestampMs <= 0) return "never";
  const seconds = Math.max(0, Math.round((now - timestampMs) / 1000));
  if (seconds < 45) return "just now";
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours} h ago`;
  const days = Math.round(hours / 24);
  return `${days} d ago`;
}
