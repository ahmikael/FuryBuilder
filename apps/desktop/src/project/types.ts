import type { WorldInfo } from "@/lib/contracts";
import type { Bounds3 } from "@/renderer/types";
import type { Scene } from "@/project/scene-types";

export type CreationMode = "empty" | "flat" | "vanilla" | "import";
export type SaveKind = "manual" | "autosave" | "checkpoint";
export type SourceKind = "generated" | "imported";

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

export interface RecoveryInfo {
  reason: string;
  recoveredRevision: number;
  recoveredAtMs: number;
  fromBackup: boolean;
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
  revision: number;
  checksum: string;
  bytes: number;
  savedAtMs: number;
  reason: string;
}

export interface ImportDimensionAnalysis {
  id: string;
  kind: "overworld" | "nether" | "end" | "custom" | string;
  minY: number;
  height: number;
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
  blockEntities: number;
  entities: number;
  warnings: string[];
  blockers: string[];
  supportMatrix: SupportMatrix;
}

export const CREATION_MODES: Array<{
  mode: CreationMode;
  label: string;
  description: string;
  available: boolean;
}> = [
  {
    mode: "empty",
    label: "Empty Project",
    description: "A blank world with only air. Every block is placed by you or a tool.",
    available: true,
  },
  {
    mode: "flat",
    label: "Flat base",
    description:
      "A 33×33 stone, dirt and grass platform around the origin. Not vanilla world generation.",
    available: true,
  },
  {
    mode: "vanilla",
    label: "Vanilla World",
    description:
      "Choose a seed, preset, supported world settings and optional local data packs. The official local Java server generates the requested area after you accept the EULA and explicitly confirm the run.",
    available: true,
  },
  {
    mode: "import",
    label: "Import World",
    description:
      "Open a supported Java 1.21.4 world read-only, keep a byte-for-byte source copy and edit its blocks through the project overlay.",
    available: true,
  },
];

export function creationModeLabel(mode: CreationMode): string {
  return CREATION_MODES.find((entry) => entry.mode === mode)?.label ?? mode;
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
