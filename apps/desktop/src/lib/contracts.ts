/**
 * IPC contract exchanged with the Rust backend.
 * Keep field names in sync with serde(rename_all = "camelCase") DTOs.
 * Contract tests live on both sides (see docs/api/contracts.md).
 */

export interface AppInfo {
  name: string;
  version: string;
  targetMinecraftVersion: string;
  targetDataVersion: number;
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

export type BackendStatus = "connecting" | "online" | "offline";
