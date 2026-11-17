import { beforeEach, describe, expect, it, vi } from "vitest";
import { invokeCommand } from "@/lib/ipc";
import {
  autosaveProject,
  checkpointProject,
  closeProject,
  createProject,
  fetchCheckpoints,
  fetchProjectStatus,
  fetchProjects,
  openCheckpoint,
  openProject,
  renameProject,
  saveProject,
} from "@/lib/project-api";

vi.mock("@/lib/ipc", () => ({
  invokeCommand: vi.fn().mockResolvedValue(undefined),
}));

const invoke = vi.mocked(invokeCommand);

beforeEach(() => {
  invoke.mockClear();
});

describe("project IPC contract", () => {
  it("uses the camelCase command payloads registered by the Rust shell", async () => {
    await fetchProjects();
    expect(invoke).toHaveBeenLastCalledWith("project_list");

    await fetchProjectStatus();
    expect(invoke).toHaveBeenLastCalledWith("project_status");

    const editorState = { selectedDimension: "minecraft:overworld", loadRadius: 2 };
    await createProject("Castle", "flat", editorState);
    expect(invoke).toHaveBeenLastCalledWith("project_create", {
      name: "Castle",
      mode: "flat",
      editorState,
    });

    await openProject("Castle.furybuilder");
    expect(invoke).toHaveBeenLastCalledWith("project_open", {
      folder: "Castle.furybuilder",
    });

    await saveProject(editorState);
    expect(invoke).toHaveBeenLastCalledWith("project_save", { editorState });

    await autosaveProject(editorState, "interval");
    expect(invoke).toHaveBeenLastCalledWith("project_autosave", {
      editorState,
      reason: "interval",
    });

    await checkpointProject("before test");
    expect(invoke).toHaveBeenLastCalledWith("project_checkpoint", {
      reason: "before test",
    });

    await fetchCheckpoints("Castle.furybuilder");
    expect(invoke).toHaveBeenLastCalledWith("project_checkpoints", {
      folder: "Castle.furybuilder",
    });

    await openCheckpoint(7, "Castle.furybuilder");
    expect(invoke).toHaveBeenLastCalledWith("project_open_checkpoint", {
      folder: "Castle.furybuilder",
      revision: 7,
    });

    await closeProject(true, editorState);
    expect(invoke).toHaveBeenLastCalledWith("project_close", {
      save: true,
      editorState,
    });

    await renameProject("Castle.furybuilder", "Keep");
    expect(invoke).toHaveBeenLastCalledWith("project_rename", {
      folder: "Castle.furybuilder",
      name: "Keep",
    });
  });
});
