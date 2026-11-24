import { beforeEach, describe, expect, it, vi } from "vitest";
import { invokeCommand } from "@/lib/ipc";
import {
  autosaveProject,
  checkpointProject,
  closeProject,
  createProject,
  fetchCheckpoints,
  fetchProjectDefaults,
  fetchProjectStatus,
  fetchProjects,
  importGeneratedWorld,
  importJavaWorld,
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

    await fetchProjectDefaults();
    expect(invoke).toHaveBeenLastCalledWith("project_defaults");

    const editorState = { selectedDimension: "minecraft:overworld", loadRadius: 2 };
    await createProject("Castle", "flat", editorState);
    expect(invoke).toHaveBeenLastCalledWith("project_create", {
      name: "Castle",
      mode: "flat",
      editorState,
      folder: null,
    });

    await createProject("Castle", "flat", editorState, "Chosen.furybuilder");
    expect(invoke).toHaveBeenLastCalledWith("project_create", {
      name: "Castle",
      mode: "flat",
      editorState,
      folder: "Chosen.furybuilder",
    });

    await importJavaWorld("Castle", "C:\\saves\\Castle", editorState, "Imported.furybuilder");
    expect(invoke).toHaveBeenLastCalledWith("project_import_world", {
      name: "Castle",
      worldFolder: "C:\\saves\\Castle",
      editorState,
      folder: "Imported.furybuilder",
    });

    await importGeneratedWorld("Castle", "run-1", editorState, "Vanilla.furybuilder");
    expect(invoke).toHaveBeenLastCalledWith("project_import_generated_world", {
      name: "Castle",
      runId: "run-1",
      editorState,
      folder: "Vanilla.furybuilder",
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
      label: null,
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
