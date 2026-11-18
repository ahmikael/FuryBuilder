import type { WorldInfo } from "@/lib/contracts";

export type CreationMode = "empty" | "flat" | "vanilla" | "import";
export type SaveKind = "manual" | "autosave" | "checkpoint";
export type SourceKind = "generated" | "imported";

export interface EditorState {
  selectedDimension: string;
  loadRadius: number;
}

export interface SourceInfo {
  kind: SourceKind;
  immutable: boolean;
  reference?: string;
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

export interface ProjectDetail {
  summary: ProjectSummary;
  editorState: EditorState;
  source: SourceInfo;
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
      "Future mode. Will require a user-provided Java runtime and official server JAR; the EULA is never accepted for you.",
    available: false,
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
