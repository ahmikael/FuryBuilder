import { invokeCommand } from "@/lib/ipc";
import type {
  CheckpointMeta,
  CreationMode,
  EditorState,
  OpenedProjectPayload,
  ProjectDetail,
  ProjectList,
  SaveOutcome,
  SessionStatus,
} from "@/project/types";

export function fetchProjects(): Promise<ProjectList> {
  return invokeCommand<ProjectList>("project_list");
}

export function fetchProjectStatus(): Promise<SessionStatus> {
  return invokeCommand<SessionStatus>("project_status");
}

export function createProject(
  name: string,
  mode: CreationMode,
  editorState: EditorState,
): Promise<OpenedProjectPayload> {
  return invokeCommand<OpenedProjectPayload>("project_create", { name, mode, editorState });
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

export function checkpointProject(reason: string): Promise<CheckpointMeta[]> {
  return invokeCommand<CheckpointMeta[]>("project_checkpoint", { reason });
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
