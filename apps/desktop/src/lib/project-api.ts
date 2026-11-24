import { invokeCommand } from "@/lib/ipc";
import type {
  CheckpointMeta,
  CreationMode,
  EditorState,
  JavaWorldAnalysis,
  OpenedProjectPayload,
  ProjectDefaults,
  ProjectDetail,
  ProjectList,
  SaveOutcome,
  SessionStatus,
  VanillaParityReport,
  WorkspacePolicy,
} from "@/project/types";

export function fetchProjects(): Promise<ProjectList> {
  return invokeCommand<ProjectList>("project_list");
}

export function fetchProjectDefaults(): Promise<ProjectDefaults> {
  return invokeCommand<ProjectDefaults>("project_defaults");
}

export function fetchProjectStatus(): Promise<SessionStatus> {
  return invokeCommand<SessionStatus>("project_status");
}

export function fetchWorkspacePolicy(): Promise<WorkspacePolicy> {
  return invokeCommand<WorkspacePolicy>("workspace_policy");
}

export function setWorkspacePolicy(policy: WorkspacePolicy): Promise<WorkspacePolicy> {
  return invokeCommand<WorkspacePolicy>("workspace_set_policy", { policy });
}

export function createProject(
  name: string,
  mode: CreationMode,
  editorState: EditorState,
  folder?: string,
): Promise<OpenedProjectPayload> {
  return invokeCommand<OpenedProjectPayload>("project_create", {
    name,
    mode,
    editorState,
    folder: folder ?? null,
  });
}

export function openProject(folder: string): Promise<OpenedProjectPayload> {
  return invokeCommand<OpenedProjectPayload>("project_open", { folder });
}

export function saveProject(editorState: EditorState): Promise<SaveOutcome> {
  return invokeCommand<SaveOutcome>("project_save", { editorState });
}

export function autosaveProject(editorState: EditorState, reason = ""): Promise<number> {
  return invokeCommand<number>("project_autosave", { editorState, reason });
}

export function checkpointProject(reason: string, label?: string): Promise<CheckpointMeta[]> {
  return invokeCommand<CheckpointMeta[]>("project_checkpoint", {
    reason,
    label: label ?? null,
  });
}

export function fetchCheckpoints(folder?: string): Promise<CheckpointMeta[]> {
  return invokeCommand<CheckpointMeta[]>("project_checkpoints", { folder: folder ?? null });
}

export function openCheckpoint(revision: number, folder?: string): Promise<unknown> {
  return invokeCommand("project_open_checkpoint", {
    folder: folder ?? null,
    revision,
  });
}

export function closeProject(save: boolean, editorState: EditorState): Promise<SessionStatus> {
  return invokeCommand<SessionStatus>("project_close", { save, editorState });
}

export function renameProject(folder: string, name: string): Promise<ProjectDetail> {
  return invokeCommand<ProjectDetail>("project_rename", { folder, name });
}

export function analyzeJavaWorld(folder: string): Promise<JavaWorldAnalysis> {
  return invokeCommand<JavaWorldAnalysis>("world_import_preview", { folder });
}

export function importJavaWorld(
  name: string,
  worldFolder: string,
  editorState: EditorState,
  folder?: string,
): Promise<OpenedProjectPayload> {
  return invokeCommand<OpenedProjectPayload>("project_import_world", {
    name,
    worldFolder,
    editorState,
    folder: folder ?? null,
  });
}

/**
 * Compares the saved Vanilla World base with the original generator's reference chunks.
 * The backend refuses edited snapshots, mismatched settings and unsupported references.
 */
export function checkVanillaParity(folder: string, runId?: string): Promise<VanillaParityReport> {
  return invokeCommand<VanillaParityReport>("project_verify_vanilla_parity", {
    folder,
    runId: runId ?? null,
  });
}

/** Imports a completed official-server generation run as an immutable-source project. */
export function importGeneratedWorld(
  name: string,
  runId: string,
  editorState: EditorState,
  folder?: string,
): Promise<OpenedProjectPayload> {
  return invokeCommand<OpenedProjectPayload>("project_import_generated_world", {
    name,
    runId,
    editorState,
    folder: folder ?? null,
  });
}
