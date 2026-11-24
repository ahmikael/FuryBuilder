import { beforeEach, describe, expect, it, vi } from "vitest";
import { invokeCommand } from "@/lib/ipc";
import {
  analyzeJavaWorld,
  autosaveProject,
  cancelProjectPrepare,
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
    await createProject("job-1", "Castle", "flat", editorState);
    expect(invoke).toHaveBeenLastCalledWith("project_create", {
      jobId: "job-1",
      request: {
        name: "Castle",
        mode: "flat",
        editorState,
        folder: null,
        profileId: null,
        flat: null,
      },
    });

    await createProject("job-2", "Castle", "flat", editorState, "Chosen.furybuilder");
    expect(invoke).toHaveBeenLastCalledWith("project_create", {
      jobId: "job-2",
      request: {
        name: "Castle",
        mode: "flat",
        editorState,
        folder: "Chosen.furybuilder",
        profileId: null,
        flat: null,
      },
    });

    const flat = {
      baseY: 70,
      layers: [{ block: "minecraft:stone", thickness: 2 }],
      biome: "minecraft:desert",
    };
    await createProject("job-3", "Castle", "flat", editorState, "Chosen.furybuilder", {
      profileId: "java-classic-test",
      flat,
    });
    expect(invoke).toHaveBeenLastCalledWith("project_create", {
      jobId: "job-3",
      request: {
        name: "Castle",
        mode: "flat",
        editorState,
        folder: "Chosen.furybuilder",
        profileId: "java-classic-test",
        flat,
      },
    });

    await analyzeJavaWorld("C:\\saves\\Castle.zip");
    expect(invoke).toHaveBeenLastCalledWith("world_import_preview", {
      source: "C:\\saves\\Castle.zip",
    });

    await importJavaWorld(
      "job-4",
      "Castle",
      "C:\\saves\\Castle",
      "importAsIs",
      editorState,
      "Imported.furybuilder",
    );
    expect(invoke).toHaveBeenLastCalledWith("project_import_world", {
      jobId: "job-4",
      name: "Castle",
      source: "C:\\saves\\Castle",
      strategy: "importAsIs",
      editorState,
      folder: "Imported.furybuilder",
    });

    await importGeneratedWorld("job-5", "Castle", "run-1", editorState, "Vanilla.furybuilder");
    expect(invoke).toHaveBeenLastCalledWith("project_import_generated_world", {
      jobId: "job-5",
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

    await cancelProjectPrepare("job-6");
    expect(invoke).toHaveBeenLastCalledWith("project_prepare_cancel", { jobId: "job-6" });
  });
});
