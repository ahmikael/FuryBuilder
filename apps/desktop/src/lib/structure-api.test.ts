import { beforeEach, describe, expect, it, vi } from "vitest";
import { invokeCommand } from "@/lib/ipc";
import {
  exportStructure,
  insertStructure,
  previewStructureExport,
  previewStructureImport,
  readStructureFile,
} from "@/lib/structure-api";

vi.mock("@/lib/ipc", () => ({
  invokeCommand: vi.fn().mockResolvedValue(undefined),
}));

const invoke = vi.mocked(invokeCommand);

beforeEach(() => {
  invoke.mockClear();
});

describe("structure IPC contract", () => {
  it("uses the camelCase command payloads registered by the Rust shell", async () => {
    await previewStructureImport("cGF5bG9hZA==");
    expect(invoke).toHaveBeenLastCalledWith("structure_import_preview", {
      payload: "cGF5bG9hZA==",
    });

    await insertStructure({
      payload: "cGF5bG9hZA==",
      dimension: "minecraft:overworld",
      origin: [0, 64, 0],
      includeAir: false,
    });
    expect(invoke).toHaveBeenLastCalledWith("structure_insert", {
      payload: "cGF5bG9hZA==",
      dimension: "minecraft:overworld",
      origin: [0, 64, 0],
      includeAir: false,
    });

    const bounds = {
      min: { x: -2, y: 60, z: -2 },
      max: { x: 2, y: 66, z: 2 },
    };
    await previewStructureExport("minecraft:overworld", bounds);
    expect(invoke).toHaveBeenLastCalledWith("structure_export_preview", {
      dimension: "minecraft:overworld",
      bounds,
    });

    await exportStructure({
      dimension: "minecraft:overworld",
      bounds: null,
      fileName: "castle.nbt",
      expectedChecksum: "a".repeat(64),
    });
    expect(invoke).toHaveBeenLastCalledWith("structure_export", {
      dimension: "minecraft:overworld",
      bounds: null,
      fileName: "castle.nbt",
      expectedChecksum: "a".repeat(64),
    });
  });

  it("base64-encodes a chosen file without sending its path", async () => {
    const file = new File([new Uint8Array([0, 1, 2, 127, 255])], "castle.nbt");
    await expect(readStructureFile(file)).resolves.toBe("AAECf/8=");
  });

  it("rejects unsupported extensions and empty files before IPC", async () => {
    await expect(readStructureFile(new File([new Uint8Array([1])], "castle.txt"))).rejects.toThrow(
      /structure/i,
    );
    await expect(
      readStructureFile(new File([new Uint8Array([1])], "castle.schem")),
    ).rejects.toThrow(/not supported yet/i);
    await expect(readStructureFile(new File([], "empty.nbt"))).rejects.toThrow(/empty/i);
  });
});
