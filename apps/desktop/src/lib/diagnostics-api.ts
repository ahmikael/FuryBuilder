import { invokeCommand } from "@/lib/ipc";
export { recordUiError } from "@/lib/diagnostics-client";

export interface DiagnosticLogFile {
  name: string;
  bytes: number;
  modifiedAtMs: number;
}

export interface DiagnosticsInfo {
  appVersion: string;
  logDir: string;
  fileName: string;
  files: DiagnosticLogFile[];
  totalBytes: number;
  maxBytesPerFile: number;
  maxFiles: number;
}

export function fetchDiagnostics(): Promise<DiagnosticsInfo> {
  return invokeCommand<DiagnosticsInfo>("diagnostics_info");
}

export function clearDiagnostics(): Promise<DiagnosticsInfo> {
  return invokeCommand<DiagnosticsInfo>("diagnostics_clear");
}

export function openDiagnosticsFolder(): Promise<void> {
  return invokeCommand<void>("diagnostics_open_folder");
}
