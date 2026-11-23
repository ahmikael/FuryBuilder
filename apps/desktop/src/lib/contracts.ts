/**
 * IPC contract exchanged with the Rust backend.
 * Keep field names in sync with serde(rename_all = "camelCase") DTOs.
 * Contract tests live on both sides (see docs/architecture.md).
 */

export interface AppInfo {
  name: string;
  version: string;
}

export interface PingResponse {
  message: string;
  appVersion: string;
  timestampMs: number;
}

export interface AppErrorShape {
  code: string;
  message: string;
  details?: unknown;
}

export interface DimensionInfo {
  id: string;
  kind: "overworld" | "nether" | "end" | "custom";
  minY: number;
  maxY: number;
  height: number;
  chunkCount: number;
  sectionCount: number;
  blockCount: number;
}

export interface WorldInfo {
  targetMinecraftVersion: string;
  targetDataVersion: number;
  revision: number;
  dimensions: DimensionInfo[];
  chunkCount: number;
  sectionCount: number;
  blockCount: number;
}

export type BackendStatus = "connecting" | "online" | "offline";
